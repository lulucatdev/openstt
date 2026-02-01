# OpenSTT

Local-first speech-to-text hub with multiple local engines.

A native macOS app that unifies Whisper and MLX models behind a single OpenAI-compatible API endpoint — plus system-wide dictation with a global hotkey.

> **Requirements:** macOS on Apple Silicon (M1 / M2 / M3 / M4). Intel Macs are not supported.

## Features

- **Multiple engines** — Local Whisper (whisper.cpp with Metal), local MLX models
- **OpenAI-compatible API** — `POST /v1/audio/transcriptions` on localhost, drop-in replacement
- **System-wide dictation** — Hold a global shortcut to record, release to transcribe, auto-paste into any app
- **Model management** — Download, switch, and delete models from the GUI
- **Playground** — Built-in record-and-transcribe for quick testing

## Tech Stack

- **Frontend**: React + TypeScript + Vite
- **Backend**: Rust + Tauri v2
- **STT**: whisper.cpp (via whisper-rs with Metal), MLX Audio sidecar
- **Platform**: macOS Apple Silicon only

## Development

```bash
# Install dependencies
npm install

# Run in development mode
npm run tauri dev

# Build for production
npm run tauri build
```

## Roadmap

- [ ] Migrate macOS MLX inference from Python sidecar to [mlx-audio-swift](https://github.com/Blaizzy/mlx-audio-swift) for native performance and zero Python dependency

## License

MIT

---

# OpenSTT

聚合多种本地引擎的语音转文字 Hub。

一个原生 macOS 应用，将 Whisper 和 MLX 模型统一在一个 OpenAI 兼容的 API 端点之后，同时提供全局快捷键系统级听写。

> **系统要求：** macOS Apple Silicon（M1 / M2 / M3 / M4）。不支持 Intel Mac。

## 功能

- **多引擎支持** — 本地 Whisper (whisper.cpp，Metal 加速)、本地 MLX 模型
- **OpenAI 兼容 API** — 本地 `POST /v1/audio/transcriptions`，可直接替换
- **系统级听写** — 按住全局快捷键录音，松开转写，自动粘贴到当前应用
- **模型管理** — 在界面中下载、切换、删除模型
- **试听台** — 内置录音转写，便于快速测试

## 技术栈

- **前端**: React + TypeScript + Vite
- **后端**: Rust + Tauri v2
- **STT 引擎**: whisper.cpp (通过 whisper-rs，Metal 加速)、MLX Audio 侧车
- **平台**: 仅支持 macOS Apple Silicon

## 开发

```bash
# 安装依赖
npm install

# 开发模式运行
npm run tauri dev

# 生产构建
npm run tauri build
```

## 路线图

- [ ] 将 macOS MLX 推理从 Python 侧车迁移到 [mlx-audio-swift](https://github.com/Blaizzy/mlx-audio-swift)，实现原生性能、无需 Python 依赖

## 许可证

MIT
