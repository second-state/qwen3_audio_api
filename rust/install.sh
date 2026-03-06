#!/bin/bash
# Installer for qwen3-audio-api — downloads binary (with bundled libtorch), models, and tokenizers
# Usage: curl -sSf https://raw.githubusercontent.com/second-state/qwen3_audio_api/main/rust/install.sh | bash

set -e

REPO="second-state/qwen3_audio_api"
INSTALL_DIR="./qwen3_audio_api"

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
NC='\033[0m'

info()  { echo -e "${CYAN}[info]${NC}  $*"; }
ok()    { echo -e "${GREEN}[ok]${NC}    $*"; }
warn()  { echo -e "${YELLOW}[warn]${NC}  $*"; }
err()   { echo -e "${RED}[error]${NC} $*" >&2; }

# ---------------------------------------------------------------------------
# 1. Detect platform
# ---------------------------------------------------------------------------
detect_platform() {
    case "$(uname -s)" in
        Linux*)  OS="linux" ;;
        Darwin*) OS="darwin" ;;
        *)
            err "Unsupported operating system: $(uname -s)"
            exit 1
            ;;
    esac

    case "$(uname -m)" in
        x86_64|amd64)    ARCH="x86_64" ;;
        aarch64|arm64)   ARCH="aarch64" ;;
        *)
            err "Unsupported architecture: $(uname -m)"
            exit 1
            ;;
    esac

    # CUDA detection (Linux only — macOS uses Metal via MLX)
    CUDA_DRIVER=""
    if [ "$OS" = "linux" ]; then
        if command -v nvidia-smi &>/dev/null && nvidia-smi --query-gpu=driver_version --format=csv,noheader &>/dev/null; then
            CUDA_DRIVER=$(nvidia-smi --query-gpu=driver_version --format=csv,noheader 2>/dev/null | head -1)
        fi
    fi

    info "Platform: ${OS} ${ARCH}${CUDA_DRIVER:+ (NVIDIA driver ${CUDA_DRIVER})}"
}

# ---------------------------------------------------------------------------
# 2. Resolve release asset name
# ---------------------------------------------------------------------------
resolve_asset() {
    case "${OS}-${ARCH}" in
        darwin-aarch64)  ASSET_NAME="qwen3-audio-api-macos-arm64" ;;
        linux-x86_64)
            if [ -n "$CUDA_DRIVER" ]; then
                info "NVIDIA GPU detected. Choose build variant:"
                echo "  1) CUDA  (recommended for GPU)"
                echo "  2) CPU only"
                printf "Select variant [1]: "
                read -r variant </dev/tty
                variant="${variant:-1}"
                case "$variant" in
                    1) ASSET_NAME="qwen3-audio-api-linux-x86_64-cuda" ;;
                    2) ASSET_NAME="qwen3-audio-api-linux-x86_64" ;;
                    *) warn "Invalid choice, defaulting to CUDA."
                       ASSET_NAME="qwen3-audio-api-linux-x86_64-cuda" ;;
                esac
            else
                ASSET_NAME="qwen3-audio-api-linux-x86_64"
            fi
            ;;
        linux-aarch64)
            if [ -n "$CUDA_DRIVER" ]; then
                info "NVIDIA GPU detected on ARM64 (Jetson). Choose build variant:"
                echo "  1) CUDA  (recommended for Jetson)"
                echo "  2) CPU only"
                printf "Select variant [1]: "
                read -r variant </dev/tty
                variant="${variant:-1}"
                case "$variant" in
                    1) ASSET_NAME="qwen3-audio-api-linux-aarch64-cuda" ;;
                    2) ASSET_NAME="qwen3-audio-api-linux-aarch64" ;;
                    *) warn "Invalid choice, defaulting to CUDA."
                       ASSET_NAME="qwen3-audio-api-linux-aarch64-cuda" ;;
                esac
            else
                ASSET_NAME="qwen3-audio-api-linux-aarch64"
            fi
            ;;
        *)
            err "Unsupported platform: ${OS}-${ARCH}"
            exit 1
            ;;
    esac
    info "Release asset: ${ASSET_NAME}"
}

# ---------------------------------------------------------------------------
# 3. Download & extract release (libtorch is bundled for all Linux builds)
# ---------------------------------------------------------------------------
download_release() {
    local tarball="${ASSET_NAME}.tar.gz"
    local url="https://github.com/${REPO}/releases/latest/download/${tarball}"

    info "Downloading release..."
    mkdir -p "${INSTALL_DIR}"

    local temp_dir
    temp_dir=$(mktemp -d)

    curl -fSL -o "${temp_dir}/${tarball}" "$url"
    info "Extracting release..."
    tar -xzf "${temp_dir}/${tarball}" -C "${temp_dir}"

    cp -r "${temp_dir}/${ASSET_NAME}/"* "${INSTALL_DIR}/"
    chmod +x "${INSTALL_DIR}/qwen3-audio-api"

    rm -rf "$temp_dir"
    ok "Binary installed to ${INSTALL_DIR}/"
}

# ---------------------------------------------------------------------------
# 4. Model selection & download
# ---------------------------------------------------------------------------
download_models() {
    echo ""
    info "Select model size:"
    echo "  1) 0.6B (Recommended — smaller, faster)"
    echo "  2) 1.7B (Higher quality, needs more RAM/VRAM)"
    printf "Select model [1]: "
    read -r model_choice </dev/tty
    model_choice="${model_choice:-1}"

    if [ "$model_choice" = "2" ]; then
        MODEL_SIZE="1.7B"
    else
        MODEL_SIZE="0.6B"
    fi

    ASR_MODEL="Qwen3-ASR-${MODEL_SIZE}"
    TTS_BASE_MODEL="Qwen3-TTS-12Hz-${MODEL_SIZE}-Base"
    TTS_CV_MODEL="Qwen3-TTS-12Hz-${MODEL_SIZE}-CustomVoice"

    info "Selected ${MODEL_SIZE} models: ${ASR_MODEL}, ${TTS_BASE_MODEL}, ${TTS_CV_MODEL}"

    local models_dir="${INSTALL_DIR}/models"
    mkdir -p "$models_dir"

    for model in "$ASR_MODEL" "$TTS_BASE_MODEL" "$TTS_CV_MODEL"; do
        local model_dir="${models_dir}/${model}"
        if [ -d "$model_dir" ] && [ -f "$model_dir/model.safetensors" ]; then
            ok "${model} already downloaded, skipping."
        else
            info "Downloading ${model}..."
            mkdir -p "$model_dir"

            local api_url="https://huggingface.co/api/models/Qwen/${model}"
            local hf_url="https://huggingface.co/Qwen/${model}/resolve/main"
            local files
            files=$(curl -fSL "$api_url" | grep -o '"rfilename":"[^"]*"' | sed 's/"rfilename":"//;s/"//')

            for file in $files; do
                case "$file" in
                    .gitattributes|README.md) continue ;;
                esac
                info "  ${file}..."
                mkdir -p "${model_dir}/$(dirname "$file")"
                curl -fSL -o "${model_dir}/${file}" "${hf_url}/${file}"
            done
            ok "${model} downloaded."
        fi
    done
}

# ---------------------------------------------------------------------------
# 5. Download tokenizers from release assets
# ---------------------------------------------------------------------------
download_tokenizers() {
    info "Downloading tokenizers..."
    for model in "$ASR_MODEL" "$TTS_BASE_MODEL" "$TTS_CV_MODEL"; do
        local tokenizer_url="https://github.com/${REPO}/releases/latest/download/tokenizer-${model}.json"
        local model_dir="${INSTALL_DIR}/models/${model}"
        info "  tokenizer for ${model}..."
        curl -fSL -o "${model_dir}/tokenizer.json" "$tokenizer_url"
    done
    ok "Tokenizers installed."
}

# ---------------------------------------------------------------------------
# 6. Done — print sample commands
# ---------------------------------------------------------------------------
print_usage() {
    local cv_path="models/${TTS_CV_MODEL}"
    local base_path="models/${TTS_BASE_MODEL}"
    local asr_path="models/${ASR_MODEL}"

    echo ""
    echo -e "${GREEN}======================================${NC}"
    echo -e "${GREEN} Installation complete!${NC}"
    echo -e "${GREEN}======================================${NC}"
    echo ""

    echo "Start the server:"
    echo ""
    echo "  cd ${INSTALL_DIR}"
    echo "  TTS_CUSTOMVOICE_MODEL_PATH=./${cv_path} \\"
    echo "    TTS_BASE_MODEL_PATH=./${base_path} \\"
    echo "    ASR_MODEL_PATH=./${asr_path} \\"
    echo "    ./qwen3-audio-api"
    echo ""

    echo "Text-to-Speech (after server starts):"
    echo ""
    echo "  curl -X POST http://localhost:8000/v1/audio/speech \\"
    echo "    -H 'Content-Type: application/json' \\"
    echo "    -d '{\"model\": \"qwen3-tts\", \"input\": \"Hello world!\", \"voice\": \"alloy\"}' \\"
    echo "    --output speech.mp3"
    echo ""

    echo "Speech-to-Text:"
    echo ""
    echo "  curl -X POST http://localhost:8000/v1/audio/transcriptions \\"
    echo "    -F file=@audio.wav -F model=qwen3-asr"
    echo ""
}

# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------
main() {
    echo ""
    info "Qwen3 Audio API Installer"
    echo ""

    detect_platform
    resolve_asset
    download_release
    download_models
    download_tokenizers
    print_usage
}

main "$@"
