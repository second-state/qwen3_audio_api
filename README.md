# Qwen3 Audio API

OpenAI-compatible API servers for [Qwen3-TTS](https://github.com/QwenLM/Qwen3-TTS) and [Qwen3-ASR](https://github.com/QwenLM/Qwen3-ASR), enabling self-hosted text-to-speech and speech-to-text via the standard OpenAI audio endpoints:

- `/v1/audio/speech` — Text-to-speech (TTS)
- `/v1/audio/transcriptions` — Speech-to-text (ASR)

## Implementations

| Language | Directory | Status |
|----------|-----------|--------|
| Python | [python/](python/) | Available |
| Rust | [rust/](rust/) | Available |

The **Python** implementation is a FastAPI server built on the `qwen-tts` and `qwen-asr` Python packages. See [python/README.md](python/README.md) for setup, Docker images, API reference, and usage examples.

The **Rust** implementation is a high-performance axum/tokio server built on the [qwen3_tts](https://github.com/second-state/qwen3_tts_rs) and [qwen3_asr](https://github.com/second-state/qwen3_asr_rs) Rust crates, with libtorch (Linux) and MLX (macOS Apple Silicon) backends. See [rust/README.md](rust/README.md) for setup, pre-built binaries, API reference, and usage examples.

## Features

- **Text-to-Speech (TTS)**: Generate natural speech from text using Qwen3-TTS models
  - Multiple voice presets (Vivian, Ryan, Serena, etc.)
  - Voice cloning from audio samples
  - Multiple languages (English, Chinese, Japanese, Korean, and more)
  - Multiple output formats (WAV, MP3, FLAC, Opus, AAC)

- **Speech-to-Text (ASR)**: Transcribe audio to text using Qwen3-ASR models
  - 30+ language support with auto-detection
  - Accepts various audio formats (WAV, MP3, M4A, etc.)

## Why

The purpose of these API servers is to provide self-hosted, free backend audio services for projects such as:

- [OpenClaw](https://github.com/openclaw/openclaw) — AI agent
- [EchoKit](https://github.com/second-state/echokit_server) — Voice AI device
- [Olares](https://github.com/beclab/Olares) — Personal AI cloud OS
- [GaiaNet](https://github.com/GaiaNet-AI/gaianet-node) — Incentivized AI agent network and marketplace

Any application that speaks the OpenAI audio API can swap in this server as a drop-in replacement.

## License

See [LICENSE](LICENSE).
