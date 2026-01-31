# OpenSTT Project Plan

## 1. Executive Summary

**Project Name:** OpenSTT

**Vision:** 建立本地 AI 语音转文字服务的行业标准接口。

**Problem:** 当前 AI 生态系统存在"模型孤岛"问题——每个支持语音的应用都捆绑自己庞大（数 GB）的模型，浪费磁盘空间、内存和 GPU 资源。

**Solution:** 一个统一、轻量级的本地"基础设施即服务"应用。OpenSTT 静默运行在后台，托管单一共享的高性能语音模型实例（Whisper、GLM 等），通过标准的 OpenAI 兼容 API（`/v1/audio/transcriptions`）对外提供服务。

**Target Audience:** 需要高效、私密、离线语音能力的 macOS 高级用户、开发者和 AI 应用创作者（未来将支持其他平台）。

---

## 2. 设计参考：CodexMonitor UI 设计分析与借鉴

### 2.1 设计哲学概述

CodexMonitor 是一个成熟的 Tauri 2.0 + React 桌面应用，其 UI 设计展现了现代 macOS 原生应用的标杆水准。经过对其代码的深入分析，我们总结出以下核心设计理念：

| 设计维度 | 特点 | 适用性评估 |
|---------|------|-----------|
| 视觉风格 | 玻璃拟态 (Glassmorphism) + 深色模式优先 | **高度适用** - 与 OpenSTT 的系统托盘定位契合 |
| 布局架构 | 三栏响应式布局（侧边栏-主内容-右面板） | **部分适用** - OpenSTT 可简化为单栏 + 设置面板 |
| 交互系统 | 可拖拽调整面板、脉冲状态指示器 | **完全适用** - 服务状态展示的理想选择 |
| 配色系统 | CSS 变量驱动的分层透明度设计 | **完全适用** - 可直接复用设计令牌 |

### 2.2 核心设计要素详解

#### 玻璃拟态 (Glassmorphism)

CodexMonitor 大量运用 `backdrop-filter` 创造现代感：

```css
/* 来自 CodexMonitor/src/styles/base.css */
.sidebar {
  background: rgba(18, 18, 18, 0.35);
  backdrop-filter: blur(32px) saturate(1.35);
  border-right: 1px solid rgba(255, 255, 255, 0.08);
}
```

**对 OpenSTT 的启示：**

作为系统托盘常驻应用，OpenSTT 的窗口应该轻盈、不突兀。玻璃拟态效果能让 UI 与 macOS 系统界面自然融合，符合用户对"后台服务"的心理预期。建议主窗口使用 `blur(24px)` 级别的背景模糊，设置面板使用 `blur(12px)` 以区分层级。

#### CSS 变量设计系统

CodexMonitor 建立了完整的文本层级和表面层级系统：

```css
/* 文本层级 - 从强到弱 */
--text-strong: #ffffff;
--text-emphasis: rgba(255, 255, 255, 0.9);
--text-primary: #e6e7ea;
--text-muted: rgba(255, 255, 255, 0.7);
--text-subtle: rgba(255, 255, 255, 0.6);
--text-faint: rgba(255, 255, 255, 0.5);

/* 表面层级 - 玻璃效果 */
--surface-sidebar: rgba(18, 18, 18, 0.35);
--surface-topbar: rgba(10, 14, 20, 0.45);
--surface-card: rgba(255, 255, 255, 0.04);

/* 强调色 */
--border-accent: rgba(100, 200, 255, 0.6);
--shadow-accent: rgba(92, 168, 255, 0.28);
```

**对 OpenSTT 的启示：**

这种分层透明度设计非常适合 OpenSTT 的信息密度需求：
- 服务器运行状态 → `--text-strong` 白色
- 当前模型名称 → `--text-primary` 主文本色
- 次要统计信息（请求次数、延迟）→ `--text-muted`
- 占位提示文字 → `--text-subtle`

**建议直接移植这套变量系统**，因其已经过精心调校，且与 Apple 的 Human Interface Guidelines 保持一致。

#### 状态指示器设计

CodexMonitor 使用脉冲动画指示处理状态：

```css
/* 来自 CodexMonitor/src/styles/sidebar.css */
.thread-status.processing {
  background: #ff9f43;
  box-shadow: 0 0 8px rgba(255, 159, 67, 0.8);
  animation: pulse 1.2s ease-in-out infinite;
}
```

**对 OpenSTT 的启示：**

OpenSTT 作为服务应用，状态可视化是核心 UX：
- **待机状态**：柔和的绿色呼吸灯效果
- **转录中**：快速的橙色脉冲，配合波形可视化
- **加载模型**：进度环 + 百分比文字
- **错误状态**：红色闪烁 + 托盘图标变化

这种非语言的状态传达对用户至关重要，因为 OpenSTT 大部分时间运行在后台，用户需要通过一瞥就能了解系统状态。

#### 响应式布局策略

CodexMonitor 实现了三种布局变体：
- **DesktopLayout**: 三栏布局（侧边栏 + 主内容 + 右面板）
- **TabletLayout**: 侧边栏可折叠的双栏布局
- **PhoneLayout**: 标签页切换的单栏布局

**对 OpenSTT 的启示：**

OpenSTT 的功能相对集中，不需要三栏复杂度。建议采用简化策略：
- **主窗口**：单栏仪表盘，显示服务状态、模型选择、实时日志
- **设置窗口**：模态对话框，参考 CodexMonitor 的设置面板设计
- **系统托盘菜单**：原生 macOS 菜单，快速开关和状态查看

#### 设置面板模式

CodexMonitor 的设置面板采用模态对话框设计：

```css
/* 来自 CodexMonitor/src/styles/settings.css */
.settings-overlay {
  position: fixed;
  inset: 0;
  background: rgba(6, 8, 12, 0.55);
  backdrop-filter: blur(8px);
}

.settings-window {
  width: min(860px, 92vw);
  height: min(560px, 84vh);
  border-radius: 18px;
  background: rgba(22, 24, 30, 0.85);
  border: 1px solid rgba(255, 255, 255, 0.12);
  box-shadow: 0 24px 60px rgba(0, 0, 0, 0.35);
}
```

**对 OpenSTT 的启示：**

设置面板是 OpenSTT 与用户的主要交互界面。建议：
- 使用 720x480px 的模态窗口（比 CodexMonitor 更紧凑，符合 OpenSTT 的简单定位）
- 侧边导航组织设置项：通用、模型、API、日志
- 复用 CodexMonitor 的开关组件样式

### 2.3 OpenSTT 专属设计决策

#### 品牌色彩建议

CodexMonitor 使用蓝紫渐变作为强调色。OpenSTT 作为语音服务，建议采用**蓝绿渐变**，传达：
- 声音/声波的联想
- 本地/自然的隐喻（绿色代表本地运行、环保资源利用）
- 与 CodexMonitor 保持视觉关联，同时有独特识别度

```css
--brand-gradient: linear-gradient(135deg, #62b7ff, #4fe3a3);
--brand-glow: rgba(79, 227, 163, 0.28);
```

#### 波形可视化

参考 CodexMonitor 的语音输入波形：

```css
.composer-waveform-bar {
  flex: 1;
  min-width: 2px;
  border-radius: 999px;
  background: rgba(180, 220, 255, 0.7);
  transition: height 0.12s ease;
}
```

OpenSTT 应在主界面展示实时音频处理可视化，让用户直观感知服务正在工作。

#### 可访问性考虑

CodexMonitor 提供 `reduced-transparency` 模式。OpenSTT 必须继承这一设计：

```css
.app.reduced-transparency .glass-panel {
  background: rgba(30, 30, 30, 0.95);
  backdrop-filter: none;
}
```

这对于长时间运行的后台应用尤为重要。

### 2.4 技术实现参考

CodexMonitor 的关键 UI 文件可直接作为实现参考：

| 功能 | 参考文件 | 借鉴程度 |
|-----|---------|---------|
| CSS 变量系统 | `src/styles/base.css` | 直接移植 |
| 玻璃效果 | `src/styles/main.css` | 调整透明度后使用 |
| 设置面板 | `src/features/settings/components/SettingsView.tsx` | 结构参考 |
| 响应式布局 | `src/features/layout/components/DesktopLayout.tsx` | 简化为单栏 |
| 状态指示器 | `src/styles/sidebar.css` (.thread-status) | 直接移植动画 |

---

## 3. Technical Architecture: "Micro-Kernel Gateway"

OpenSTT 采用 **Hub & Spoke** 架构，在性能（Rust）与灵活性（Python）之间取得平衡。

### The Hub: Rust Gateway (Tauri Main Process)

- **角色**：系统的控制中心
- **职责**：
  - 在 `localhost:port` 托管 HTTP 服务器（Axum）
  - 实现严格的 OpenAI API 规范
  - 管理系统托盘 UI、配置和安全性（API Keys）
  - 将请求路由到适当的引擎（Native 或 Python）
- **技术栈**：Rust, Tauri 2.0, Axum

### Spoke A: Native Engine（默认，高性能）

- **角色**：90% 任务的"日常引擎"
- **实现**：直接集成到 Rust 二进制文件中
- **技术栈**：`whisper-rs`（Whisper.cpp 绑定）
- **优势**：零依赖、瞬间启动、极低内存占用、Apple Silicon 优化
- **适用场景**：使用 OpenAI Whisper 模型的标准英语/多语言转录

### Spoke B: Extension Engine（专业，高灵活性）

- **角色**：专业需求的"重型引擎"
- **实现**：托管的 Python 附属进程
- **技术栈**：PyTorch, Transformers, FastAPI（内部）
- **优势**：访问需要复杂 Python 生态的最新研究模型（GLM-4-Voice、Microsoft VibeVoice）
- **适用场景**：专业中文方言识别、长音频说话人分离、实验性模型

---

## 4. Development Roadmap

### Phase 1: Foundation & Gateway（"Hub"）

**目标**：建立 Tauri 2.0 应用外壳和核心 Rust HTTP Gateway，提供标准 OpenAI 兼容 API。

1. **项目初始化**
   - [ ] 在 `openstt-app` 中初始化 Tauri 2.0 项目（React + Vite + TypeScript）
   - [ ] 配置 `tauri.conf.json` 的能力、权限和应用标识
   - [ ] **移植 CodexMonitor 的 CSS 基础系统**：复制 `base.css` 变量系统，建立 OpenSTT 的品牌色
   - [ ] 实现基本的玻璃拟态主窗口布局

2. **核心 Gateway 实现（Rust）**
   - [ ] 使用 `Axum` 在 Tauri 主进程中实现 HTTP 服务器
   - [ ] 创建严格遵循 OpenAI API 规范的 `/v1/audio/transcriptions` 端点
   - [ ] 实现虚拟响应器以验证外部工具（如 `curl`）的连通性

3. **基础 UI - 服务控制**
   - [ ] **仪表盘设计**：单栏布局，参考 CodexMonitor 的卡片设计
     - 服务状态指示器（移植 CodexMonitor 的脉冲动画）
     - 端口配置输入框
     - 实时日志流（深色背景，等宽字体，彩色级别标签）
   - [ ] 实现从前端启动/停止 Axum 服务器的逻辑
   - [ ] **系统托盘集成**：
     - 托盘图标状态变化（待机/运行/错误）
     - 右键菜单：显示窗口、启动/停止服务、退出

### Phase 2: Native Engine Integration（"Speed"）

**目标**：使用 Rust 原生绑定实现默认的零依赖推理引擎，以获得即时性能和低资源占用。

1. **引擎架构**
   - [ ] 在 Rust 中定义 `InferenceEngine` trait/接口（加载、转录、卸载）
   - [ ] 实现处理模型文件路径和下载的"模型管理器"

2. **Whisper Native Implementation（参考：CodexMonitor）**
   - [ ] **移植 `dictation.rs` 逻辑**：从 `CodexMonitor/src-tauri/src/dictation.rs` 调整 `whisper-rs` 实现
     - 使用 `whisper-rs` 进行 Apple Silicon 优化推理
     - 如需直接麦克风输入，使用 `cpal` 进行原生音频捕获
     - 复用模型目录逻辑（从 HuggingFace 下载 `ggml` 模型）
   - [ ] **为服务器模式重构**：
     - 与 CodexMonitor（从麦克风流式传输音频）不同，OpenSTT 主要通过 HTTP 处理*文件*
     - 调整 `transcribe_audio` 函数以接受文件路径/缓冲区，而不仅仅是来自麦克风的 `Vec<f32>` 样本
   - [ ] **依赖**：向 `Cargo.toml` 添加 `whisper-rs`、`cpal`、`hound`（WAV 处理）和 `reqwest`

3. **UI - 模型管理**
   - [ ] **模型列表视图**：参考 CodexMonitor 的侧边栏列表设计
     - 本地可用模型列表（带图标区分大小和语言）
     - 当前激活模型高亮显示
   - [ ] **模型下载界面**：
     - 进度环 + 百分比（参考 CodexMonitor 的进度指示器）
     - 取消下载按钮
   - [ ] 允许为 `/v1` 端点选择"激活模型"

### Phase 3: Python Extension Engine（"Flexibility"）

**目标**：将现有的 Python 服务器作为"插件"或"Sidecar"重新集成，以支持高级/特定模型（GLM、VibeVoice）。

1. **Sidecar 架构（参考：CodexMonitor `codex.rs`）**
   - [ ] 创建 `SidecarManager` 结构体（类似于 CodexMonitor 中的 `WorkspaceSession`）
   - [ ] 使用 `tokio::process::Command` 生成和管理 Python 服务器进程
   - [ ] 实现健康检查（`/health`）以确保 Python sidecar 在转发请求前已就绪

2. **Python 打包**
   - [ ] 将现有的 `server/` Python 代码重构为独立可执行文件或托管子进程
   - [ ] 确保 Python 环境隔离（分发时可能使用 `pyinstaller` 或便携 Python zip，开发时依赖用户的 venv）

3. **Gateway 路由**
   - [ ] 更新 Rust Gateway Router，将特定请求（或配置时）转发到 Python 后端的内部端口
   - [ ] 实现协议转换：`OpenAI Format (Rust)` -> `Custom JSON (Python)` -> `OpenAI Format (Rust)`

4. **进程管理**
   - [ ] Tauri 管理 Python 进程的生命周期（启动/停止/重启）
   - [ ] 通过 `AppHandle::emit` 将 Python 日志流式传输到 Tauri 前端控制台

### Phase 4: Polish & Standardization（"Product"）

**目标**：完善 UX 并确保公开发布的健壮性。

1. **系统托盘 & 后台运行**
   - [ ] 确保应用最小化到托盘并保持服务器运行
   - [ ] **托盘菜单快捷操作**：启动/停止、切换模型（参考 CodexMonitor 的托盘交互模式）
   - [ ] 点击托盘图标显示/隐藏主窗口（带淡入淡出动画，参考 CodexMonitor 的过渡效果）

2. **API 安全 & 配置**
   - [ ] **设置面板实现**：
     - 参考 CodexMonitor 的 `SettingsView.tsx` 模态对话框设计
     - 使用 720x480px 紧凑尺寸
     - 侧边导航：通用、模型、API、日志
     - 开关组件复用 CodexMonitor 的 CSS
   - [ ] 实现 Gateway 的可选 API Key 认证
   - [ ] 添加 CORS、Host 绑定（localhost vs LAN）配置

3. **动画 & 过渡优化**
   - [ ] 窗口显示/隐藏：200ms ease-out 淡入淡出
   - [ ] 状态变化：脉冲动画过渡
   - [ ] 设置面板：从中心缩放展开（参考 CodexMonitor 的模态动画）

4. **分发**
   - [ ] 优化包大小（去除未使用资源）
   - [ ] 设置 CI/CD 工作流用于构建 macOS DMG
   - [ ] 代码签名和公证

---

## 5. 设计实现检查清单

基于 CodexMonitor 的 UI 分析，以下是视觉实现的具体检查点：

### CSS 基础
- [ ] 移植完整的 CSS 变量系统（文本层级、表面层级、强调色）
- [ ] 实现 `reduced-transparency` 辅助功能模式
- [ ] 定义 OpenSTT 品牌渐变（蓝绿）

### 玻璃拟态
- [ ] 主窗口背景：`blur(24px)` 级别
- [ ] 设置面板背景：`blur(12px)` 级别
- [ ] 卡片/面板：`rgba(255, 255, 255, 0.04)` 表面色

### 状态指示
- [ ] 待机：柔和绿色呼吸灯（2s 周期）
- [ ] 运行中：快速橙色脉冲（1s 周期）
- [ ] 错误：红色闪烁（0.5s 周期）
- [ ] 模型加载中：进度环动画

### 布局
- [ ] 主窗口：单栏仪表盘，最大宽度 960px
- [ ] 设置面板：模态对话框，720x480px
- [ ] 响应式：适配从 640px 到 1920px 的宽度

### 动画
- [ ] 窗口过渡：200ms ease-out
- [ ] 按钮悬停：`transform: translateY(-1px)`
- [ ] 状态脉冲：`animation: pulse 1.2s ease-in-out infinite`

---

## 6. 关键参考文件索引

| 组件/功能 | CodexMonitor 参考路径 | 备注 |
|----------|----------------------|------|
| CSS 变量系统 | `src/styles/base.css` | 设计令牌，直接移植 |
| 玻璃效果 | `src/styles/main.css` | 背景模糊实现 |
| 侧边栏 | `src/styles/sidebar.css` | 状态指示器动画 |
| 设置面板 | `src/styles/settings.css` | 模态对话框样式 |
| 桌面布局 | `src/features/layout/DesktopLayout.tsx` | 布局结构参考 |
| 设置组件 | `src/features/settings/SettingsView.tsx` | 组件结构参考 |
| Dictation 引擎 | `src-tauri/src/dictation.rs` | whisper-rs 使用参考 |
| Sidecar 管理 | `src-tauri/src/codex.rs` | 进程管理参考 |

---

## 7. 风险与缓解策略

| 风险 | 影响 | 缓解策略 |
|-----|------|---------|
| Python sidecar 分发复杂 | 高 | 提供 Homebrew 公式；优先使用 Native Engine |
| 内存占用过高 | 中 | 模型 LRU 缓存；空闲时自动卸载 |
| UI 与 CodexMonitor 过于相似 | 低 | 使用差异化品牌色；简化布局 |
| API 兼容性不完整 | 高 | 严格遵循 OpenAI API 规范；提供测试套件 |

---

## 8. 成功指标

- [ ] 安装包大小 < 50MB（不含模型）
- [ ] 内存占用 < 200MB（待机状态）
- [ ] 冷启动到首次转录 < 3 秒
- [ ] 与 OpenAI API 100% 兼容（核心端点）
- [ ] 通过 CodexMonitor 的 CSS 可访问性标准

---

*最后更新：2026-01-30*
*设计参考版本：CodexMonitor main branch*
