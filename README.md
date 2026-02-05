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

## Known Issues

### ElevenLabs Scribe V2 Realtime — text injection unreliable (critical)

The `elevenlabs:scribe_v2_realtime` model streams `partial_transcript` and `committed_transcript` over WebSocket. The client must inject these into the focused app in real time.

**Problem:** macOS CGEvent keyboard events (backspace + `set_string`) are unreliable at high frequency — the OS and/or active IME silently drops events, causing characters to be lost or garbled. The ElevenLabs API also frequently revises partial transcripts (not just appending), which requires deleting previously typed text.

**Approaches tried (all insufficient):**

| Approach | Result |
|----------|--------|
| Full-draft rewrite (backspace N + type N) | Events dropped at high frequency → garbled text |
| Incremental diff (common-prefix, only touch suffix) | Helps for monotonic growth, but API revisions still cause large deletions |
| Clipboard paste (Cmd+V) for insertion | Insertion is reliable, but backspace deletion still drops events |
| Commit-only mode (skip partials, paste on commit) | Text is correct but loses real-time capability; also timing issue where typing task exits before final flush arrives |

**Current state:** Commit-only mode is implemented as a stopgap. Partials are skipped; only committed transcripts are pasted via clipboard. The real-time typing experience is lost.

**Needs investigation:** How other macOS dictation tools (Superwhisper, Whisper Flow, macOS built-in dictation, Talon Voice) solve reliable text replacement — likely via Input Method Kit (IMK) or Accessibility API (AXUIElement) rather than raw CGEvent key injection.

## Roadmap

- [ ] Migrate macOS MLX inference from Python sidecar to [mlx-audio-swift](https://github.com/Blaizzy/mlx-audio-swift) for native performance and zero Python dependency
- [ ] Fix real-time text injection for ElevenLabs Scribe V2 Realtime (see Known Issues)

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

## 已知问题

### ElevenLabs Scribe V2 Realtime — 实时文本注入不可靠（严重）

`elevenlabs:scribe_v2_realtime` 通过 WebSocket 流式返回 `partial_transcript` 和 `committed_transcript`，客户端需要将其实时注入到当前焦点应用。

**问题：** macOS CGEvent 键盘事件（退格 + `set_string`）在高频下不可靠——系统或输入法会静默丢弃事件，导致吃字或乱码。ElevenLabs API 还会频繁全量修订 partial（而非仅追加），需要删除已输入的文本。

**已尝试的方案（均不足）：**

| 方案 | 结果 |
|------|------|
| 全量重写（退格 N + 输入 N） | 高频下事件丢失 → 乱码 |
| 增量 diff（公共前缀，只改后缀） | 单调增长时有效，但 API 修订仍需大量退格 |
| 剪贴板粘贴（Cmd+V）插入 | 插入可靠，但退格删除仍丢事件 |
| Commit-only 模式（跳过 partial，仅粘贴 committed） | 文本正确但失去实时能力；且存在 typing task 先于 flush 退出的时序问题 |

**当前状态：** 已实现 commit-only 模式作为临时方案。跳过 partial，仅在 committed 时通过剪贴板粘贴。实时打字体验丧失。

**待调研：** 其他 macOS 听写工具（Superwhisper、Whisper Flow、macOS 原生听写、Talon Voice）如何实现可靠的文本替换——可能通过 Input Method Kit (IMK) 或 Accessibility API (AXUIElement)，而非原始 CGEvent 键盘注入。

## 路线图

- [ ] 将 macOS MLX 推理从 Python 侧车迁移到 [mlx-audio-swift](https://github.com/Blaizzy/mlx-audio-swift)，实现原生性能、无需 Python 依赖
- [ ] 修复 ElevenLabs Scribe V2 Realtime 实时文本注入（见已知问题）

## 许可证

MIT
