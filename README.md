# OpenSTT

Local-first speech-to-text hub with local and cloud engines.

A native macOS app that unifies Whisper, GLM-4-Voice (via MLX), and cloud providers (BigModel, ElevenLabs) behind a single OpenAI-compatible API endpoint — plus system-wide dictation with a global hotkey.

## Features

- **Multiple engines** — Local Whisper (whisper.cpp), local GLM-4-Voice (MLX on Apple Silicon), cloud BigModel & ElevenLabs
- **OpenAI-compatible API** — `POST /v1/audio/transcriptions` on localhost, drop-in replacement
- **System-wide dictation** — Hold a global shortcut to record, release to transcribe, auto-paste into any app
- **Model management** — Download, switch, and delete models from the GUI
- **Playground** — Built-in record-and-transcribe for quick testing

## Tech Stack

- **Frontend**: React + TypeScript + Vite
- **Backend**: Rust + Tauri v2
- **STT**: whisper.cpp (via whisper-rs with Metal), MLX Audio sidecar

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

聚合本地与云端引擎的语音转文字 Hub。

一个原生 macOS 应用，将 Whisper、GLM-4-Voice（通过 MLX）和云端服务商（BigModel、ElevenLabs）统一在一个 OpenAI 兼容的 API 端点之后，同时提供全局快捷键系统级听写。

## 功能

- **多引擎支持** — 本地 Whisper (whisper.cpp)、本地 GLM-4-Voice (Apple Silicon MLX)、云端 BigModel 和 ElevenLabs
- **OpenAI 兼容 API** — 本地 `POST /v1/audio/transcriptions`，可直接替换
- **系统级听写** — 按住全局快捷键录音，松开转写，自动粘贴到当前应用
- **模型管理** — 在界面中下载、切换、删除模型
- **试听台** — 内置录音转写，便于快速测试

## 技术栈

- **前端**: React + TypeScript + Vite
- **后端**: Rust + Tauri v2
- **STT 引擎**: whisper.cpp (通过 whisper-rs，Metal 加速)、MLX Audio 侧车

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
