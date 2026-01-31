# OpenSTT

> **Warning**: This project is currently under active development and is not yet functional. macOS only for now.

OpenSTT is a local speech-to-text gateway application built with Tauri. It provides a native macOS interface for managing local and cloud-based speech recognition services.

## Status

- **Platform**: macOS only (for now)
- **Stage**: Early development, not yet runnable
- **API**: Local HTTP server for speech-to-text

## Features (Planned)

- Local Whisper model support
- Cloud STT providers (BigModel, ElevenLabs)
- System-wide dictation with global hotkey
- OpenAI-compatible API endpoint

## Tech Stack

- **Frontend**: React + TypeScript + Vite
- **Backend**: Rust + Tauri v2
- **STT Engine**: whisper.cpp via whisper-rs

## Development

```bash
# Install dependencies
npm install

# Run in development mode
npm run tauri dev

# Build for production
npm run tauri build
```

## License

MIT

---

# OpenSTT

> **注意**: 本项目正在积极开发中，尚未完成，暂时无法运行。目前仅支持 macOS。

OpenSTT 是一个使用 Tauri 构建的本地语音转文字网关应用。它提供了一个原生的 macOS 界面，用于管理本地和云端的语音识别服务。

## 状态

- **平台**: 仅 macOS（暂时）
- **阶段**: 早期开发中，尚未可运行
- **API**: 本地 HTTP 服务器用于语音转文字

## 功能（计划中）

- 本地 Whisper 模型支持
- 云端 STT 服务商（BigModel、ElevenLabs）
- 全局快捷键系统级听写
- OpenAI 兼容的 API 端点

## 技术栈

- **前端**: React + TypeScript + Vite
- **后端**: Rust + Tauri v2
- **STT 引擎**: whisper.cpp (通过 whisper-rs)

## 开发

```bash
# 安装依赖
npm install

# 开发模式运行
npm run tauri dev

# 生产构建
npm run tauri build
```

## 许可证

MIT
