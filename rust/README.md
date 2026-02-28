# Qwen3-TTS/ASR OpenAI-Compatible API (Rust)

A high-performance Rust server that wraps [Qwen3-TTS](https://github.com/QwenLM/Qwen3-TTS) and [Qwen3-ASR](https://github.com/QwenLM/Qwen3-ASR) behind OpenAI-compatible endpoints. Any client that speaks the OpenAI audio API can point at this server and get text-to-speech and speech-to-text from Qwen3 models.

**Backends:**

- **libtorch** (Linux) — PyTorch C++ runtime, CPU or CUDA GPU
- **MLX** (macOS Apple Silicon) — Apple Metal GPU acceleration

The release binaries are self-contained — ffmpeg is statically linked for audio format conversion (MP3, Opus, AAC, FLAC encoding and decoding of all common audio input formats). No external ffmpeg installation is required.

## Quick start with pre-built binaries

Pre-built binaries are available from [GitHub Releases](https://github.com/second-state/qwen3_audio_api/releases).

### Linux (x86_64)

```bash
# Download and extract
curl -LO https://github.com/second-state/qwen3_audio_api/releases/latest/download/qwen3-audio-api-linux-x86_64.tar.gz
tar xzf qwen3-audio-api-linux-x86_64.tar.gz
cd qwen3-audio-api-linux-x86_64

# Set libtorch library path (bundled in the archive)
export LD_LIBRARY_PATH=$(pwd)/libtorch/lib:$LD_LIBRARY_PATH

# Download models (see "Download models" section below)
# ...

# Run the server with TTS + ASR
TTS_CUSTOMVOICE_MODEL_PATH=/path/to/models/Qwen3-TTS-12Hz-0.6B-CustomVoice \
  TTS_BASE_MODEL_PATH=/path/to/models/Qwen3-TTS-12Hz-0.6B-Base \
  ASR_MODEL_PATH=/path/to/models/Qwen3-ASR-0.6B \
  ./qwen3-audio-api
```

### macOS (Apple Silicon)

```bash
# Download and extract
curl -LO https://github.com/second-state/qwen3_audio_api/releases/latest/download/qwen3-audio-api-macos-arm64.tar.gz
tar xzf qwen3-audio-api-macos-arm64.tar.gz
cd qwen3-audio-api-macos-arm64

# Download models (see "Download models" section below)
# ...

# Run the server with TTS + ASR
# mlx.metallib is included next to the binary — no extra setup needed
TTS_CUSTOMVOICE_MODEL_PATH=/path/to/models/Qwen3-TTS-12Hz-0.6B-CustomVoice \
  TTS_BASE_MODEL_PATH=/path/to/models/Qwen3-TTS-12Hz-0.6B-Base \
  ASR_MODEL_PATH=/path/to/models/Qwen3-ASR-0.6B \
  ./qwen3-audio-api
```

## Download models

Download model weights before starting the server. At least one of the three model paths must be set.

| Model | Parameters | Type | Use case |
|-------|-----------|------|----------|
| `Qwen3-TTS-12Hz-0.6B-CustomVoice` | 0.6B | CustomVoice | Built-in voice presets via `voice` parameter |
| `Qwen3-TTS-12Hz-0.6B-Base` | 0.6B | Base | Voice cloning via `audio_sample` parameter |
| `Qwen3-ASR-0.6B` | 0.6B | ASR | Speech-to-text transcription |

```bash
pip install huggingface_hub transformers
mkdir -p models

# CustomVoice TTS
huggingface-cli download Qwen/Qwen3-TTS-12Hz-0.6B-CustomVoice \
  --local-dir ./models/Qwen3-TTS-12Hz-0.6B-CustomVoice

# Base TTS (voice cloning)
huggingface-cli download Qwen/Qwen3-TTS-12Hz-0.6B-Base \
  --local-dir ./models/Qwen3-TTS-12Hz-0.6B-Base

# ASR
huggingface-cli download Qwen/Qwen3-ASR-0.6B \
  --local-dir ./models/Qwen3-ASR-0.6B
```

### Generate tokenizer.json

The Rust tokenizer crate requires `tokenizer.json` files that are not included in the HuggingFace model downloads. Generate them with:

```bash
python3 -c "
from transformers import AutoTokenizer
import os
for model in ['Qwen3-TTS-12Hz-0.6B-CustomVoice', 'Qwen3-TTS-12Hz-0.6B-Base', 'Qwen3-ASR-0.6B']:
    path = f'models/{model}'
    if os.path.isdir(path):
        tok = AutoTokenizer.from_pretrained(path, trust_remote_code=True)
        tok.backend_tokenizer.save(f'{path}/tokenizer.json')
        print(f'Generated {path}/tokenizer.json')
"
```

## Configuration

| Variable | Default | Description |
|----------|---------|-------------|
| `TTS_CUSTOMVOICE_MODEL_PATH` | -- | Path to CustomVoice model directory (enables `voice`/`instructions` parameters) |
| `TTS_BASE_MODEL_PATH` | -- | Path to Base model directory (enables `audio_sample` voice cloning) |
| `ASR_MODEL_PATH` | -- | Path to ASR model directory (enables `/v1/audio/transcriptions`) |
| `HOST` | `0.0.0.0` | Server bind address |
| `PORT` | `8000` | Server port |
| `RUST_LOG` | `info` | Log level (`trace`, `debug`, `info`, `warn`, `error`) |

At least one of `TTS_CUSTOMVOICE_MODEL_PATH`, `TTS_BASE_MODEL_PATH`, or `ASR_MODEL_PATH` must be set.

**Example — all models loaded:**

```bash
TTS_CUSTOMVOICE_MODEL_PATH=./models/Qwen3-TTS-12Hz-0.6B-CustomVoice \
  TTS_BASE_MODEL_PATH=./models/Qwen3-TTS-12Hz-0.6B-Base \
  ASR_MODEL_PATH=./models/Qwen3-ASR-0.6B \
  ./qwen3-audio-api
```

## API reference

### `POST /v1/audio/speech`

Generate speech from text. Compatible with the [OpenAI audio speech API](https://platform.openai.com/docs/api-reference/audio/createSpeech).

**Request body (JSON):**

| Field | Type | Required | Default | Description | Requires model |
|-------|------|----------|---------|-------------|----------------|
| `model` | string | yes | -- | Model identifier (accepted for compatibility; the loaded model is always used) | -- |
| `input` | string | yes | -- | Text to synthesize (max 4096 characters) | -- |
| `voice` | string | no | `alloy` | Voice name (see table below) | CustomVoice |
| `response_format` | string | no | `mp3` | `mp3`, `opus`, `aac`, `flac`, `wav`, or `pcm` | -- |
| `speed` | number | no | `1.0` | Playback speed, `0.25` to `4.0` | -- |
| `language` | string | no | `Auto` | Language of the input text (`Auto`, `English`, `Chinese`, `Japanese`, `Korean`, `French`, `German`, `Spanish`, `Italian`, `Portuguese`, `Russian`) | -- |
| `instructions` | string | no | -- | Style/emotion instruction passed to the model | CustomVoice |
| `audio_sample` | string/file | no | -- | Reference audio for voice cloning (file upload via multipart, or base64 string via JSON) | Base |
| `audio_sample_text` | string | no | -- | Transcript of the reference audio; enables in-context learning mode for higher quality cloning | Base |

> **Note:** The endpoint accepts both JSON and multipart/form-data. Use multipart (`curl -F`) to upload `audio_sample` as a binary file — this avoids base64 encoding. JSON requests can pass `audio_sample` as a base64-encoded string.
>
> When `audio_sample` is provided the request uses the **Base** model for voice cloning and `voice`/`instructions` are ignored. When `audio_sample` is omitted the request uses the **CustomVoice** model and requires a valid `voice`. If the required model is not loaded the server returns HTTP 400.

**Response:** The raw audio bytes with the appropriate `Content-Type` header.

**Example — predefined voice (CustomVoice model):**

```bash
curl -X POST http://localhost:8000/v1/audio/speech \
  -H "Content-Type: application/json" \
  -d '{
    "model": "qwen3-tts",
    "input": "Hello, welcome to the Qwen text-to-speech API.",
    "voice": "alloy",
    "language": "English",
    "response_format": "wav"
  }' \
  --output speech.wav
```

**Example — voice cloning (Base model):**

```bash
curl -X POST http://localhost:8000/v1/audio/speech \
  -F model=qwen3-tts \
  -F "input=This sentence will be spoken in the cloned voice." \
  -F audio_sample=@reference.wav \
  -F "audio_sample_text=Transcript of the reference audio." \
  -F language=English \
  -F response_format=wav \
  --output cloned.wav
```

### `POST /v1/audio/transcriptions`

Transcribe audio to text. Compatible with the [OpenAI audio transcriptions API](https://platform.openai.com/docs/api-reference/audio/createTranscription).

**Request body (multipart/form-data):**

| Field | Type | Required | Default | Description |
|-------|------|----------|---------|-------------|
| `file` | file | yes | -- | The audio file to transcribe (mp3, mp4, mpeg, mpga, m4a, wav, webm) |
| `model` | string | no | `qwen3-asr` | Model identifier (accepted for compatibility; the loaded model is always used) |
| `language` | string | no | -- | Language of the audio (auto-detected if not specified). Supports 30+ languages including English, Chinese, Japanese, Korean, French, German, Spanish, etc. |
| `prompt` | string | no | -- | Optional context hint (not currently used) |
| `response_format` | string | no | `json` | `json` or `text` |
| `temperature` | number | no | `0.0` | Sampling temperature (not currently used) |

**Response (JSON):**

```json
{
  "text": "The transcribed text content."
}
```

**Example:**

```bash
curl -X POST http://localhost:8000/v1/audio/transcriptions \
  -F file=@audio.wav \
  -F model=qwen3-asr
```

**Example with language hint:**

```bash
curl -X POST http://localhost:8000/v1/audio/transcriptions \
  -F file=@audio.wav \
  -F model=qwen3-asr \
  -F language=English \
  -F response_format=text
```

### `GET /v1/models`

Returns the list of available models.

```bash
curl http://localhost:8000/v1/models
```

### `GET /health`

Returns `{"status": "ok"}` when the server is ready.

```bash
curl http://localhost:8000/health
```

## Voices

The `voice` field accepts OpenAI voice names (mapped to Qwen3-TTS speakers) or Qwen3-TTS speaker names directly.

**OpenAI voice mapping:**

| OpenAI voice | Qwen3-TTS speaker |
|--------------|-------------------|
| `alloy` | Vivian |
| `ash` | Serena |
| `ballad` | Uncle_Fu |
| `coral` | Dylan |
| `echo` | Eric |
| `fable` | Ryan |
| `onyx` | Aiden |
| `nova` | Ono_Anna |
| `sage` | Sohee |
| `shimmer` | Vivian |
| `verse` | Ryan |
| `marin` | Serena |
| `cedar` | Aiden |

**Qwen3-TTS speakers** can also be used directly as the voice value: `Vivian`, `Serena`, `Uncle_Fu`, `Dylan`, `Eric`, `Ryan`, `Aiden`, `Ono_Anna`, `Sohee`.

## Output formats

| Format | Content-Type |
|--------|-------------|
| `wav` | `audio/wav` |
| `pcm` | `audio/pcm` |
| `mp3` | `audio/mpeg` |
| `opus` | `audio/opus` |
| `aac` | `audio/aac` |
| `flac` | `audio/flac` |

All formats are handled natively by the statically-linked ffmpeg library. No external tools are needed.

## Building from source

ffmpeg is built from source and statically linked by default (via the `build-ffmpeg` feature). You do **not** need ffmpeg installed.

### Linux (libtorch backend)

```bash
# Install build dependencies
sudo apt-get install -y cmake pkg-config nasm libclang-dev libmp3lame-dev libopus-dev

# Download libtorch (CPU)
wget -q "https://download.pytorch.org/libtorch/cpu/libtorch-cxx11-abi-shared-with-deps-2.7.0%2Bcpu.zip" -O libtorch.zip
unzip -q libtorch.zip && rm libtorch.zip
export LIBTORCH=$(pwd)/libtorch
export LD_LIBRARY_PATH=$LIBTORCH/lib:$LD_LIBRARY_PATH

# For CUDA 12.8 instead, download:
# wget -q "https://download.pytorch.org/libtorch/cu128/libtorch-cxx11-abi-shared-with-deps-2.7.0%2Bcu128.zip" -O libtorch.zip

# Build (ffmpeg is compiled from source and statically linked)
cd rust
cargo build --release
# Binary at: target/release/qwen3-audio-api
```

### macOS Apple Silicon (MLX backend)

```bash
# Install build dependencies
brew install cmake lame opus

# Build with MLX (ffmpeg is compiled from source and statically linked)
cd rust
cargo build --release --no-default-features --features "mlx build-ffmpeg"

# Copy mlx.metallib next to the binary for redistribution
cp target/release/build/qwen3_tts-*/out/lib/mlx.metallib target/release/
# Binary at: target/release/qwen3-audio-api
```

## License

[Apache-2.0](../LICENSE)
