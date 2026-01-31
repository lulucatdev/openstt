import { useEffect, useMemo, useRef, useState } from "react";
import {
  Database,
  FileText,
  LayoutDashboard,
  Menu,
  Mic,
  Settings,
  Trash2,
  X,
} from "lucide-react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { openUrl } from "@tauri-apps/plugin-opener";
import "./App.css";

type ServerStatus = {
  running: boolean;
  port: number;
  url: string | null;
  startedAt: number | null;
  requests: number;
};

type UiSettings = {
  reducedTransparency: boolean;
  language: "en" | "zh";
  bigmodelApiKey: string;
  bigmodelApiEndpoint: string;
  elevenlabsApiKey: string;
  elevenlabsApiEndpoint: string;
  dictationShortcut: DictationShortcut;
  dictationAutoPaste: boolean;
};

type DictationShortcut = {
  key: string;
  modifiers: string[];
};

type ModelInfo = {
  id: string;
  name: string;
  size: string;
  description: string;
  downloadUrl: string;
  downloaded: boolean;
  localPath: string | null;
  engine: string;
  provider?: string | null;
};

type CloudProvider = "bigmodel" | "elevenlabs";

type CloudUsage = {
  requests: number;
  lastLatencyMs: number | null;
  lastError: string | null;
  lastProvider: string | null;
};

type CloudTestResult = {
  ok: boolean;
  latencyMs: number | null;
  message: string;
};

type CloudTestState = {
  status: "idle" | "testing" | "success" | "error";
  latencyMs?: number | null;
  message?: string;
};

type DictationShortcutEvent = {
  state: "pressed" | "released";
};

type DictationRecorder = {
  context: AudioContext;
  source: MediaStreamAudioSourceNode;
  processor: ScriptProcessorNode;
  stream: MediaStream;
  chunks: Float32Array[];
};

type GlmDependencyStatus = {
  supported: boolean;
  ready: boolean;
  python: string | null;
  venv: boolean;
  mlxAudio: boolean;
  message?: string | null;
};

type LogEntry = {
  id: number;
  timestamp: number;
  level: string;
  message: string;
};

const fallbackPort = 8787;

const formatTimestamp = (value?: number | null) => {
  if (!value) {
    return "n/a";
  }
  return new Date(value).toLocaleTimeString(undefined, {
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  });
};

const formatLatency = (value?: number | null) => {
  if (value === null || value === undefined) {
    return "n/a";
  }
  return `${value} ms`;
};

const defaultDictationShortcut: DictationShortcut = {
  key: "AltLeft",
  modifiers: [],
};

const isModifierKey = (code: string) =>
  [
    "AltLeft",
    "AltRight",
    "ShiftLeft",
    "ShiftRight",
    "ControlLeft",
    "ControlRight",
    "MetaLeft",
    "MetaRight",
  ].includes(code);

const normalizeDictationShortcut = (
  shortcut: DictationShortcut,
): DictationShortcut => {
  if (isModifierKey(shortcut.key)) {
    return { key: shortcut.key, modifiers: [] };
  }
  const normalized = new Set(
    shortcut.modifiers
      .map((value) => value.trim().toLowerCase())
      .filter(Boolean),
  );
  const ordered = ["command", "control", "alt", "shift"].filter((value) =>
    normalized.has(value),
  );
  return { key: shortcut.key, modifiers: ordered };
};

const formatShortcutLabel = (shortcut: DictationShortcut) => {
  const modifierLabels: Record<string, string> = {
    command: "Command",
    control: "Control",
    alt: "Option",
    shift: "Shift",
  };
  const keyLabel = (() => {
    if (shortcut.key.startsWith("Key")) {
      return shortcut.key.slice(3).toUpperCase();
    }
    if (shortcut.key.startsWith("Digit")) {
      return shortcut.key.slice(5);
    }
    switch (shortcut.key) {
      case "Space":
        return "Space";
      case "Enter":
        return "Enter";
      case "Escape":
        return "Escape";
      case "AltLeft":
        return "Option (Left)";
      case "AltRight":
        return "Option (Right)";
      case "ShiftLeft":
        return "Shift (Left)";
      case "ShiftRight":
        return "Shift (Right)";
      case "ControlLeft":
        return "Control (Left)";
      case "ControlRight":
        return "Control (Right)";
      case "MetaLeft":
        return "Command (Left)";
      case "MetaRight":
        return "Command (Right)";
      default:
        return shortcut.key;
    }
  })();
  if (isModifierKey(shortcut.key)) {
    return keyLabel;
  }
  const modifiers = shortcut.modifiers
    .map((value) => modifierLabels[value] || value)
    .filter(Boolean);
  return [...modifiers, keyLabel].join(" + ");
};

const mergeFloat32 = (chunks: Float32Array[]) => {
  const length = chunks.reduce((total, chunk) => total + chunk.length, 0);
  const result = new Float32Array(length);
  let offset = 0;
  chunks.forEach((chunk) => {
    result.set(chunk, offset);
    offset += chunk.length;
  });
  return result;
};

const encodeWav = (samples: Float32Array, sampleRate: number) => {
  const buffer = new ArrayBuffer(44 + samples.length * 2);
  const view = new DataView(buffer);
  const writeString = (offset: number, value: string) => {
    for (let i = 0; i < value.length; i += 1) {
      view.setUint8(offset + i, value.charCodeAt(i));
    }
  };
  writeString(0, "RIFF");
  view.setUint32(4, 36 + samples.length * 2, true);
  writeString(8, "WAVE");
  writeString(12, "fmt ");
  view.setUint32(16, 16, true);
  view.setUint16(20, 1, true);
  view.setUint16(22, 1, true);
  view.setUint32(24, sampleRate, true);
  view.setUint32(28, sampleRate * 2, true);
  view.setUint16(32, 2, true);
  view.setUint16(34, 16, true);
  writeString(36, "data");
  view.setUint32(40, samples.length * 2, true);
  let offset = 44;
  for (let i = 0; i < samples.length; i += 1) {
    const sample = Math.max(-1, Math.min(1, samples[i]));
    view.setInt16(offset, sample < 0 ? sample * 0x8000 : sample * 0x7fff, true);
    offset += 2;
  }
  return new Uint8Array(buffer);
};


const translations = {
  en: {
    appName: "OpenSTT",
    appSubtitle: "Local gateway",
    searchPlaceholder: "Search",
    pageOverview: "Overview",
    pageModels: "Models",
    pageLogs: "Logs",
    pageSettings: "Settings",
    pageOverviewDesc: "Service control and API surface",
    pageModelsDesc: "Download and activate Whisper models",
    pageLogsDesc: "Recent gateway activity",
    pageSettingsDesc: "Preferences and accessibility",
    running: "Running",
    stopped: "Stopped",
    requests: "Requests",
    port: "Port",
    model: "Model",
    serviceControl: "Service Control",
    serviceControlDesc: "Start or stop the local gateway",
    start: "Start",
    starting: "Starting...",
    stop: "Stop",
    stopping: "Stopping...",
    url: "URL",
    started: "Started",
    health: "Health",
    listening: "Listening on localhost",
    listeningOn: "Listening on {url}",
    offline: "Offline",
    notRunning: "Not running",
    endpoints: "Endpoints",
    endpointsDesc: "OpenAI compatible surface for transcription",
    openai: "OpenAI /v1",
    transcriptions: "POST /v1/audio/transcriptions",
    transcriptionsDesc: "Multipart form with file and model",
    healthEndpoint: "GET /health",
    healthEndpointDesc: "Service readiness probe",
    copied: "Copied",
    copy: "Copy",
    modelsTitle: "Models",
    modelsDesc: "Manage local and cloud models",
    noModels: "No models available yet.",
    active: "Active",
    activate: "Activate",
    activating: "Activating...",
    download: "Download",
    downloading: "Downloading...",
    logsTitle: "Gateway Logs",
    logsDesc: "Recent events from the local server",
    clear: "Clear",
    noLogs: "No logs yet. Start the server to see activity.",
    logsSettings: "Logs",
    clearLogFile: "Clear log file",
    clearLogFileHint: "Remove all stored log entries",
    clearLogFileAction: "Clear logs",
    settings: "Settings",
    edit: "Edit",
    done: "Done",
    remove: "Remove",
    deleteConfirm: "Remove {name}? This deletes the local model file.",
    runtimeTitle: "MLX Runtime",
    runtimeDesc: "Apple Silicon sidecar dependencies",
    runtimeReady: "Ready",
    runtimeMissing: "Missing",
    runtimeUnsupported: "Unsupported",
    runtimeInstall: "Install runtime",
    runtimeInstalling: "Installing...",
    runtimeReset: "Reset runtime",
    runtimeResetting: "Resetting...",
    runtimeClearCache: "Clear cache",
    runtimeClearing: "Clearing...",
    runtimeRequired: "Install MLX runtime before downloading this model.",
    setup: "Set up",
    settingUp: "Setting up...",
    viewLogs: "View logs",
    live: "Live",
    localModels: "Local Models",
    cloudModels: "Cloud Models",
    whisperModels: "Whisper Models",
    pythonModels: "Python Models",
    cloudSettings: "Cloud",
    apiKey: "API Key",
    apiKeyHint: "Stored locally in ~/.openstt/settings.json",
    apiEndpoint: "API Endpoint",
    apiEndpointHint: "BigModel transcription endpoint",
    bigmodelKeyHint: "BigModel API key",
    bigmodelEndpointHint: "BigModel transcription endpoint",
    elevenlabsKeyHint: "ElevenLabs API key",
    elevenlabsEndpointHint: "ElevenLabs speech-to-text endpoint",
    save: "Save",
    saved: "Saved",
    configure: "Configure",
    all: "All",
    test: "Test",
    testKey: "Test API key",
    testKeyHint: "Send a short request to validate",
    testing: "Testing...",
    testSuccess: "Valid",
    testSuccessLatency: "Valid ({ms} ms)",
    testFailed: "Test failed",
    apiKeyRequired: "API key required",
    cloudKeyRequired: "Cloud API key is required.",
    cloudKeyRequiredProvider: "{provider} API key is required.",
    bigmodel: "BigModel",
    elevenlabs: "ElevenLabs",
    cloudUsageTitle: "Cloud Usage",
    cloudUsageDesc: "Recent cloud request activity",
    cloudRequests: "Cloud requests",
    cloudLatency: "Last latency",
    cloudLastError: "Last error",
    cloudNoError: "No errors",
    appearance: "Appearance",
    reducedTransparency: "Reduced transparency",
    reducedTransparencyHint: "Use solid surfaces for better contrast",
    language: "Language",
    languageHint: "Display language",
    dictationTitle: "Dictation",
    dictationDesc: "Hold the shortcut to record, release to transcribe",
    dictationShortcutLabel: "Hold shortcut",
    dictationShortcutHint: "Hold to record, release to transcribe",
    dictationShortcutCaptureHint: "Press new shortcut (Esc to cancel)",
    dictationShortcutChange: "Change",
    dictationShortcutListening: "Listening...",
    dictationAutoPaste: "Auto paste",
    dictationAutoPasteHint: "Paste transcript to the active app after copying",
    dictationStatus: "Status",
    dictationStatusHint: "Listening and transcription state",
    dictationIdle: "Idle",
    dictationListening: "Listening",
    dictationProcessing: "Processing",
    permissionsTitle: "Permissions",
    permissionsDesc: "Manage microphone and accessibility access",
    microphonePermission: "Microphone",
    microphonePermissionHint: "Required to record audio",
    microphonePermissionDenied: "Microphone access denied",
    accessibilityPermission: "Accessibility",
    accessibilityPermissionHint: "Required for auto paste",
    requestPermission: "Request",
    openMicrophoneSettings: "Open Microphone Settings",
    openAccessibilitySettings: "Open Accessibility Settings",
    dictationListeningBadge: "Listening",
    dictationProcessingBadge: "Transcribing",
    service: "Service",
    gateway: "Gateway",
    online: "Online",
    modelsSettings: "Models",
    activeModel: "Active model",
    available: "Available",
    modelsCount: "{count} models",
    about: "About",
    aboutDesc: "Local speech to text gateway",
    build: "Build",
    portHint: "Port must be 1-65535",
    pagePlayground: "Playground",
    pagePlaygroundDesc: "Record and transcribe",
    playgroundRecord: "Record",
    playgroundStop: "Stop & Transcribe",
    playgroundTranscribing: "Transcribing...",
    playgroundPlaceholder: "Transcription will appear here",
    playgroundRecording: "Recording...",
  },
  zh: {
    appName: "OpenSTT",
    appSubtitle: "本地网关",
    searchPlaceholder: "搜索",
    pageOverview: "概览",
    pageModels: "模型",
    pageLogs: "日志",
    pageSettings: "设置",
    pageOverviewDesc: "服务控制与 API 概览",
    pageModelsDesc: "下载并激活 Whisper 模型",
    pageLogsDesc: "网关最近活动",
    pageSettingsDesc: "偏好设置与辅助选项",
    running: "运行中",
    stopped: "已停止",
    requests: "请求",
    port: "端口",
    model: "模型",
    serviceControl: "服务控制",
    serviceControlDesc: "启动或停止本地网关",
    start: "启动",
    starting: "启动中...",
    stop: "停止",
    stopping: "停止中...",
    url: "地址",
    started: "启动时间",
    health: "状态",
    listening: "监听本地地址",
    listeningOn: "监听 {url}",
    offline: "离线",
    notRunning: "未运行",
    endpoints: "端点",
    endpointsDesc: "兼容 OpenAI 的转写接口",
    openai: "OpenAI /v1",
    transcriptions: "POST /v1/audio/transcriptions",
    transcriptionsDesc: "包含音频文件与模型的表单",
    healthEndpoint: "GET /health",
    healthEndpointDesc: "服务健康检查",
    copied: "已复制",
    copy: "复制",
    modelsTitle: "模型",
    modelsDesc: "管理本地与云端模型",
    noModels: "暂无可用模型。",
    active: "已启用",
    activate: "启用",
    activating: "启用中...",
    download: "下载",
    downloading: "下载中...",
    logsTitle: "网关日志",
    logsDesc: "本地服务最新事件",
    clear: "清空",
    noLogs: "暂无日志。启动服务后将显示。",
    logsSettings: "日志",
    clearLogFile: "清理日志文件",
    clearLogFileHint: "删除已保存的日志记录",
    clearLogFileAction: "清理日志",
    settings: "设置",
    edit: "编辑",
    done: "完成",
    remove: "移除",
    deleteConfirm: "移除 {name}？这将删除本地模型文件。",
    runtimeTitle: "MLX 运行时",
    runtimeDesc: "Apple Silicon 侧车依赖",
    runtimeReady: "已就绪",
    runtimeMissing: "未安装",
    runtimeUnsupported: "不支持",
    runtimeInstall: "安装运行时",
    runtimeInstalling: "安装中...",
    runtimeReset: "重置运行时",
    runtimeResetting: "重置中...",
    runtimeClearCache: "清理缓存",
    runtimeClearing: "清理中...",
    runtimeRequired: "请先安装 MLX 运行时再下载该模型。",
    setup: "配置",
    settingUp: "配置中...",
    viewLogs: "查看日志",
    live: "实时",
    localModels: "本地模型",
    cloudModels: "云端模型",
    whisperModels: "Whisper 模型",
    pythonModels: "Python 模型",
    cloudSettings: "云端",
    apiKey: "API Key",
    apiKeyHint: "本地保存于 ~/.openstt/settings.json",
    apiEndpoint: "API 接口",
    apiEndpointHint: "BigModel 语音转写接口",
    bigmodelKeyHint: "BigModel API Key",
    bigmodelEndpointHint: "BigModel 语音转写接口",
    elevenlabsKeyHint: "ElevenLabs API Key",
    elevenlabsEndpointHint: "ElevenLabs 语音转写接口",
    save: "保存",
    saved: "已保存",
    configure: "配置",
    all: "全部",
    test: "测试",
    testKey: "测试 API Key",
    testKeyHint: "发送一次简短请求验证",
    testing: "测试中...",
    testSuccess: "已验证",
    testSuccessLatency: "已验证（{ms} ms）",
    testFailed: "测试失败",
    apiKeyRequired: "需要 API Key",
    cloudKeyRequired: "需要先配置云端 API Key。",
    cloudKeyRequiredProvider: "需要先配置 {provider} API Key。",
    bigmodel: "BigModel",
    elevenlabs: "ElevenLabs",
    cloudUsageTitle: "云端使用情况",
    cloudUsageDesc: "最近的云端请求概览",
    cloudRequests: "云端请求",
    cloudLatency: "最近延迟",
    cloudLastError: "最近错误",
    cloudNoError: "暂无错误",
    appearance: "外观",
    reducedTransparency: "减少透明度",
    reducedTransparencyHint: "使用更实的表面以提升对比度",
    language: "语言",
    languageHint: "界面显示语言",
    dictationTitle: "语音输入",
    dictationDesc: "按住快捷键录音，松开开始转写",
    dictationShortcutLabel: "按住快捷键",
    dictationShortcutHint: "按住录音，松开转写",
    dictationShortcutCaptureHint: "请按下新的快捷键（Esc 取消）",
    dictationShortcutChange: "修改",
    dictationShortcutListening: "监听中...",
    dictationAutoPaste: "自动粘贴",
    dictationAutoPasteHint: "复制后尝试粘贴到当前应用",
    dictationStatus: "状态",
    dictationStatusHint: "录音与转写状态",
    dictationIdle: "空闲",
    dictationListening: "录音中",
    dictationProcessing: "转写中",
    permissionsTitle: "权限",
    permissionsDesc: "管理麦克风与辅助功能权限",
    microphonePermission: "麦克风",
    microphonePermissionHint: "录音需要此权限",
    microphonePermissionDenied: "麦克风权限被拒绝",
    accessibilityPermission: "辅助功能",
    accessibilityPermissionHint: "自动粘贴需要此权限",
    requestPermission: "请求",
    openMicrophoneSettings: "打开麦克风设置",
    openAccessibilitySettings: "打开辅助功能设置",
    dictationListeningBadge: "录音中",
    dictationProcessingBadge: "转写中",
    service: "服务",
    gateway: "网关",
    online: "在线",
    modelsSettings: "模型",
    activeModel: "当前模型",
    available: "可用",
    modelsCount: "{count} 个模型",
    about: "关于",
    aboutDesc: "本地语音转文字网关",
    build: "版本",
    portHint: "端口必须在 1-65535 之间",
    pagePlayground: "试听",
    pagePlaygroundDesc: "录音并转写",
    playgroundRecord: "录音",
    playgroundStop: "停止并转写",
    playgroundTranscribing: "转写中...",
    playgroundPlaceholder: "转写结果将显示在这里",
    playgroundRecording: "录音中...",
  },
} as const;

function App() {
  const [status, setStatus] = useState<ServerStatus | null>(null);
  const [logs, setLogs] = useState<LogEntry[]>([]);
  const [cloudUsage, setCloudUsage] = useState<CloudUsage | null>(null);
  const [portInput, setPortInput] = useState(String(fallbackPort));
  const [error, setError] = useState<string | null>(null);
  const [permissionError, setPermissionError] = useState<"microphone" | null>(null);
  const [action, setAction] = useState<"start" | "stop" | null>(null);
  const [copied, setCopied] = useState(false);
  const [initialized, setInitialized] = useState(false);
  const [uiSettings, setUiSettings] = useState<UiSettings>({
    reducedTransparency: false,
    language: "en",
    bigmodelApiKey: "",
    bigmodelApiEndpoint: "https://open.bigmodel.cn/api/paas/v4/audio/transcriptions",
    elevenlabsApiKey: "",
    elevenlabsApiEndpoint: "https://api.elevenlabs.io/v1/speech-to-text",
    dictationShortcut: defaultDictationShortcut,
    dictationAutoPaste: true,
  });
  const [bigmodelKeyInput, setBigmodelKeyInput] = useState("");
  const [bigmodelEndpointInput, setBigmodelEndpointInput] = useState("");
  const [elevenlabsKeyInput, setElevenlabsKeyInput] = useState("");
  const [elevenlabsEndpointInput, setElevenlabsEndpointInput] = useState("");
  const [cloudDirty, setCloudDirty] = useState(false);
  const [cloudFilter, setCloudFilter] = useState<"all" | CloudProvider>("all");
  const [cloudTests, setCloudTests] = useState<
    Record<CloudProvider, CloudTestState>
  >({
    bigmodel: { status: "idle" },
    elevenlabs: { status: "idle" },
  });
  const [dictationCapture, setDictationCapture] = useState(false);
  const [dictationState, setDictationState] = useState<
    "idle" | "listening" | "processing"
  >("idle");
  const [dictationQueueCount, setDictationQueueCount] = useState(0);
  const dictationRecorderRef = useRef<DictationRecorder | null>(null);
  const dictationQueueRef = useRef<Uint8Array[]>([]);
  const dictationListeningRef = useRef(false);
  const dictationProcessingRef = useRef(false);
  const [playgroundStatus, setPlaygroundStatus] = useState<
    "idle" | "recording" | "transcribing"
  >("idle");
  const [playgroundText, setPlaygroundText] = useState("");
  const playgroundRecorderRef = useRef<DictationRecorder | null>(null);
  const [activePage, setActivePage] = useState<
    "overview" | "models" | "logs" | "settings" | "playground"
  >("overview");
  const [drawerOpen, setDrawerOpen] = useState(false);
  const [modelsEditing, setModelsEditing] = useState(false);
  const [modelsTab, setModelsTab] = useState<"whisper" | "python" | "cloud">(
    "whisper",
  );
  const [glmDeps, setGlmDeps] = useState<GlmDependencyStatus | null>(null);
  const [glmAction, setGlmAction] = useState<
    "install" | "setup" | "reset" | "clear" | null
  >(null);
  const [models, setModels] = useState<ModelInfo[]>([]);
  const [activeModelId, setActiveModelId] = useState<string | null>(null);
  const [modelAction, setModelAction] = useState<{
    id: string;
    type: "download" | "activate";
  } | null>(null);
  const language = uiSettings.language ?? "en";
  const t = (key: keyof (typeof translations)["en"], params?: Record<string, number | string>) => {
    const template =
      translations[language]?.[key] ?? translations.en[key] ?? String(key);
    if (!params) {
      return template;
    }
    return template.replace(/\{(\w+)\}/g, (_, name) =>
      String(params[name] ?? ""),
    );
  };
  const pages = [
    {
      id: "overview",
      label: t("pageOverview"),
      description: t("pageOverviewDesc"),
      icon: LayoutDashboard,
    },
    {
      id: "models",
      label: t("pageModels"),
      description: t("pageModelsDesc"),
      icon: Database,
    },
    {
      id: "logs",
      label: t("pageLogs"),
      description: t("pageLogsDesc"),
      icon: FileText,
    },
    {
      id: "playground",
      label: t("pagePlayground"),
      description: t("pagePlaygroundDesc"),
      icon: Mic,
    },
    {
      id: "settings",
      label: t("pageSettings"),
      description: t("pageSettingsDesc"),
      icon: Settings,
    },
  ] as const;

  const parsedPort = Number(portInput);
  const portValid =
    Number.isInteger(parsedPort) && parsedPort > 0 && parsedPort < 65536;
  const isRunning = status?.running ?? false;
  const currentModelId = activeModelId ?? "base";
  const endpointBase = status?.url
    ? status.url
    : `http://127.0.0.1:${portValid ? parsedPort : fallbackPort}`;
  const curlCommand = useMemo(() => {
    return `curl -X POST ${endpointBase}/v1/audio/transcriptions -F "file=@/path/to/audio.m4a" -F "model=${currentModelId}"`;
  }, [endpointBase, currentModelId]);

  const refreshStatus = async () => {
    try {
      const next = await invoke<ServerStatus>("get_server_status");
      setStatus(next);
      if (!initialized) {
        setPortInput(String(next.port));
        setInitialized(true);
      }
    } catch (err) {
      setError(String(err));
    }
  };

  const refreshLogs = async () => {
    try {
      const next = await invoke<LogEntry[]>("get_logs");
      setLogs(next);
    } catch (err) {
      setError(String(err));
    }
  };

  const refreshCloudUsage = async () => {
    try {
      const next = await invoke<CloudUsage>("get_cloud_usage");
      setCloudUsage(next);
    } catch (err) {
      setError(String(err));
    }
  };

  const refreshModels = async () => {
    try {
      const [list, active] = await Promise.all([
        invoke<ModelInfo[]>("list_models"),
        invoke<string>("get_active_model"),
      ]);
      setModels(list);
      setActiveModelId(active);
    } catch (err) {
      setError(String(err));
    }
  };

  const refreshGlmDeps = async () => {
    try {
      const status = await invoke<GlmDependencyStatus>(
        "glm_dependency_status",
      );
      setGlmDeps(status);
    } catch (err) {
      setError(String(err));
    }
  };

  useEffect(() => {
    void refreshStatus();
    void refreshLogs();
    void refreshCloudUsage();
    void refreshModels();
    void refreshGlmDeps();
    void (async () => {
      try {
        const settings = await invoke<UiSettings>("get_ui_settings");
        setUiSettings({
          ...settings,
          dictationShortcut:
            settings.dictationShortcut ?? defaultDictationShortcut,
          dictationAutoPaste: settings.dictationAutoPaste ?? true,
        });
      } catch (err) {
        setError(String(err));
      }
    })();
    const timer = setInterval(() => {
      void refreshStatus();
      void refreshCloudUsage();
    }, 2000);
    return () => clearInterval(timer);
  }, []);

  // Window dragging via mousedown on drag regions
  useEffect(() => {
    const appWindow = getCurrentWindow();
    const handleMouseDown = (e: MouseEvent) => {
      if (e.button !== 0) return;
      const target = e.target as HTMLElement | null;
      if (!target) return;
      // Check if target or any ancestor is interactive (no-drag)
      const noDrag = target.closest(
        'button, input, select, textarea, a, [data-tauri-drag-region="false"]',
      );
      if (noDrag) return;
      // Check if we're in a drag region (attribute present and not "false")
      const dragRegion = target.closest(
        "[data-tauri-drag-region]:not([data-tauri-drag-region='false'])",
      );
      if (!dragRegion) return;
      // Prevent default to avoid text selection
      e.preventDefault();
      // Double-click to maximize, single-click to drag
      if (e.detail === 2) {
        void appWindow.toggleMaximize();
      } else {
        void appWindow.startDragging();
      }
    };
    document.addEventListener("mousedown", handleMouseDown);
    return () => document.removeEventListener("mousedown", handleMouseDown);
  }, []);

  const currentPage =
    pages.find((page) => page.id === activePage) ?? pages[0];
  const glmSupported = glmDeps?.supported ?? true;
  const glmReady = glmDeps?.ready ?? false;
  const logsStreaming =
    activePage === "logs" || glmAction === "install" || glmAction === "setup";
  const whisperModels = models.filter((model) => model.engine === "whisper");
  const pythonModels = models.filter(
    (model) => model.engine !== "whisper" && model.engine !== "cloud",
  );
  const cloudModels = models.filter((model) => model.engine === "cloud");
  const cloudProviderCounts = useMemo(() => {
    const counts = {
      all: cloudModels.length,
      bigmodel: 0,
      elevenlabs: 0,
    };
    for (const model of cloudModels) {
      if (model.provider === "elevenlabs") {
        counts.elevenlabs += 1;
      } else if (model.provider === "bigmodel") {
        counts.bigmodel += 1;
      }
    }
    return counts;
  }, [cloudModels]);
  const filteredCloudModels = cloudModels.filter((model) => {
    if (cloudFilter === "all") {
      return true;
    }
    return model.provider === cloudFilter;
  });

  const providerLabel = (provider?: string | null) =>
    provider === "elevenlabs" ? t("elevenlabs") : t("bigmodel");

  const providerKey = (provider?: string | null) =>
    provider === "elevenlabs"
      ? uiSettings.elevenlabsApiKey
      : uiSettings.bigmodelApiKey;

  const providerInputKey = (provider: CloudProvider) =>
    provider === "elevenlabs" ? elevenlabsKeyInput : bigmodelKeyInput;

  const providerInputEndpoint = (provider: CloudProvider) =>
    provider === "elevenlabs"
      ? elevenlabsEndpointInput
      : bigmodelEndpointInput;
  const bigmodelKeyMissing = !bigmodelKeyInput.trim();
  const elevenlabsKeyMissing = !elevenlabsKeyInput.trim();

  useEffect(() => {
    setBigmodelKeyInput(uiSettings.bigmodelApiKey ?? "");
    setBigmodelEndpointInput(
      uiSettings.bigmodelApiEndpoint ||
        "https://open.bigmodel.cn/api/paas/v4/audio/transcriptions",
    );
    setElevenlabsKeyInput(uiSettings.elevenlabsApiKey ?? "");
    setElevenlabsEndpointInput(
      uiSettings.elevenlabsApiEndpoint ||
        "https://api.elevenlabs.io/v1/speech-to-text",
    );
    setCloudDirty(false);
    setCloudTests({
      bigmodel: { status: "idle" },
      elevenlabs: { status: "idle" },
    });
  }, [
    uiSettings.bigmodelApiKey,
    uiSettings.bigmodelApiEndpoint,
    uiSettings.elevenlabsApiKey,
    uiSettings.elevenlabsApiEndpoint,
  ]);

  useEffect(() => {
    if (activePage !== "models") {
      setModelsEditing(false);
    }
  }, [activePage]);

  useEffect(() => {
    if (modelsTab === "cloud") {
      setModelsEditing(false);
    }
  }, [modelsTab]);

  useEffect(() => {
    const interval = logsStreaming ? 700 : 2000;
    const timer = setInterval(() => {
      void refreshLogs();
    }, interval);
    return () => clearInterval(timer);
  }, [logsStreaming]);

  useEffect(() => {
    if (!dictationCapture) {
      return;
    }
    const handler = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        event.stopPropagation();
        setDictationCapture(false);
        return;
      }
      event.preventDefault();
      event.stopPropagation();
      const nextShortcut = buildDictationShortcut(event);
      void persistSettings({
        ...uiSettings,
        dictationShortcut: nextShortcut,
      });
      setDictationCapture(false);
    };
    window.addEventListener("keydown", handler, true);
    return () => window.removeEventListener("keydown", handler, true);
  }, [dictationCapture, uiSettings]);

  useEffect(() => {
    let unlisten: (() => void) | null = null;
    void (async () => {
      unlisten = await listen<string>("open-page", (event) => {
        const page = event.payload;
        if (
          page === "overview" ||
          page === "models" ||
          page === "logs" ||
          page === "settings" ||
          page === "playground"
        ) {
          setActivePage(page);
          setDrawerOpen(false);
        }
      });
    })();
    return () => {
      if (unlisten) {
        unlisten();
      }
    };
  }, []);

  useEffect(() => {
    let unlisten: (() => void) | null = null;
    void (async () => {
      unlisten = await listen<DictationShortcutEvent>(
        "dictation-shortcut",
        (event) => {
          if (dictationCapture) {
            return;
          }
          if (event.payload.state === "pressed") {
            void startDictationRecording();
            return;
          }
          void (async () => {
            const wav = await stopDictationRecording();
            if (wav) {
              enqueueDictation(wav);
            }
          })();
        },
      );
    })();
    return () => {
      if (unlisten) {
        unlisten();
      }
    };
  }, [dictationCapture]);

  useEffect(() => {
    if (!drawerOpen) {
      return;
    }
    const handler = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        setDrawerOpen(false);
      }
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [drawerOpen]);

  useEffect(() => {
    void (async () => {
      try {
        await invoke("set_dictation_tray_state", {
          update: {
            state: dictationState,
            queueLen: dictationQueueCount,
          },
        });
      } catch (err) {
        setError(String(err));
      }
    })();
  }, [dictationState, dictationQueueCount]);

  const handleStart = async () => {
    setError(null);
    if (!portValid) {
      setError("Port must be between 1 and 65535");
      return;
    }
    setAction("start");
    try {
      const next = await invoke<ServerStatus>("start_server", {
        port: parsedPort,
      });
      setStatus(next);
      if (!initialized) {
        setPortInput(String(next.port));
        setInitialized(true);
      }
    } catch (err) {
      setError(String(err));
    } finally {
      setAction(null);
      void refreshLogs();
    }
  };

  const handleStop = async () => {
    setError(null);
    setAction("stop");
    try {
      const next = await invoke<ServerStatus>("stop_server");
      setStatus(next);
    } catch (err) {
      setError(String(err));
    } finally {
      setAction(null);
      void refreshLogs();
    }
  };

  const handleClearLogs = async () => {
    setError(null);
    try {
      await invoke("clear_logs");
      setLogs([]);
    } catch (err) {
      setError(String(err));
    }
  };

  const handleCopy = async () => {
    try {
      await navigator.clipboard.writeText(curlCommand);
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    } catch (err) {
      setError(String(err));
    }
  };

  const persistSettings = async (next: UiSettings) => {
    setUiSettings(next);
    try {
      const saved = await invoke<UiSettings>("set_ui_settings", {
        settings: next,
      });
      setUiSettings(saved);
      return saved;
    } catch (err) {
      setError(String(err));
      return null;
    }
  };

  const handleToggleTransparency = async (next: boolean) => {
    await persistSettings({ ...uiSettings, reducedTransparency: next });
  };

  const handleLanguageChange = async (next: UiSettings["language"]) => {
    await persistSettings({ ...uiSettings, language: next });
  };

  const handleSaveCloudSettings = async () => {
    const bigmodelEndpoint = bigmodelEndpointInput.trim();
    const bigmodelKey = bigmodelKeyInput.trim();
    const elevenlabsEndpoint = elevenlabsEndpointInput.trim();
    const elevenlabsKey = elevenlabsKeyInput.trim();
    const updated = await persistSettings({
      ...uiSettings,
      bigmodelApiKey: bigmodelKey,
      bigmodelApiEndpoint: bigmodelEndpoint,
      elevenlabsApiKey: elevenlabsKey,
      elevenlabsApiEndpoint: elevenlabsEndpoint,
    });
    if (updated) {
      setCloudDirty(false);
    }
  };

  const handleRequestMicrophone = async () => {
    try {
      const stream = await navigator.mediaDevices.getUserMedia({ audio: true });
      stream.getTracks().forEach((track) => track.stop());
    } catch (err) {
      setError(String(err));
    }
  };

  const handleOpenPermissionSettings = async (
    target: "microphone" | "accessibility",
  ) => {
    const url =
      target === "microphone"
        ? "x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone"
        : "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility";
    try {
      await openUrl(url);
    } catch (err) {
      setError(String(err));
    }
  };


  const syncDictationState = () => {
    if (dictationListeningRef.current) {
      setDictationState("listening");
      return;
    }
    if (dictationProcessingRef.current) {
      setDictationState("processing");
      return;
    }
    setDictationState("idle");
  };

  const syncDictationQueueCount = () => {
    setDictationQueueCount(dictationQueueRef.current.length);
  };

  const startDictationRecording = async () => {
    if (dictationListeningRef.current) {
      return;
    }
    try {
      const stream = await navigator.mediaDevices.getUserMedia({ audio: true });
      const context = new AudioContext();
      await context.resume();
      const source = context.createMediaStreamSource(stream);
      const processor = context.createScriptProcessor(4096, 1, 1);
      const chunks: Float32Array[] = [];
      processor.onaudioprocess = (event) => {
        if (!dictationListeningRef.current) {
          return;
        }
        const input = event.inputBuffer.getChannelData(0);
        chunks.push(new Float32Array(input));
      };
      source.connect(processor);
      processor.connect(context.destination);
      dictationRecorderRef.current = {
        context,
        source,
        processor,
        stream,
        chunks,
      };
      dictationListeningRef.current = true;
      syncDictationState();
    } catch (err) {
      const errStr = String(err);
      if (
        errStr.includes("NotAllowedError") ||
        errStr.includes("not allowed") ||
        errStr.includes("denied permission")
      ) {
        setPermissionError("microphone");
      } else {
        setError(errStr);
      }
      dictationListeningRef.current = false;
      syncDictationState();
    }
  };

  const stopDictationRecording = async () => {
    const recorder = dictationRecorderRef.current;
    dictationListeningRef.current = false;
    if (!recorder) {
      syncDictationState();
      return null;
    }
    const { context, source, processor, stream, chunks } = recorder;
    processor.disconnect();
    source.disconnect();
    processor.onaudioprocess = null;
    stream.getTracks().forEach((track) => track.stop());
    const sampleRate = context.sampleRate;
    await context.close();
    dictationRecorderRef.current = null;
    syncDictationState();
    const merged = mergeFloat32(chunks);
    if (merged.length < sampleRate * 0.15) {
      return null;
    }
    return encodeWav(merged, sampleRate);
  };

  const processDictationQueue = async () => {
    if (dictationProcessingRef.current) {
      return;
    }
    const next = dictationQueueRef.current.shift();
    syncDictationQueueCount();
    if (!next) {
      syncDictationState();
      return;
    }
    dictationProcessingRef.current = true;
    syncDictationState();
    try {
      const text = await invoke<string>("transcribe_audio", {
        request: {
          bytes: Array.from(next),
          fileName: "dictation.wav",
          modelId: activeModelId ?? "base",
        },
      });
      const trimmed = text.trim();
      if (trimmed) {
        await navigator.clipboard.writeText(trimmed);
        if (uiSettings.dictationAutoPaste) {
          await new Promise((resolve) => setTimeout(resolve, 80));
          await invoke("paste_clipboard");
        }
      }
    } catch (err) {
      setError(String(err));
    } finally {
      dictationProcessingRef.current = false;
      syncDictationState();
      void processDictationQueue();
    }
  };

  const enqueueDictation = (wavBytes: Uint8Array) => {
    dictationQueueRef.current.push(wavBytes);
    syncDictationQueueCount();
    void processDictationQueue();
  };

  const startPlaygroundRecording = async () => {
    if (playgroundStatus === "recording") {
      return;
    }
    try {
      const stream = await navigator.mediaDevices.getUserMedia({ audio: true });
      const context = new AudioContext();
      await context.resume();
      const source = context.createMediaStreamSource(stream);
      const processor = context.createScriptProcessor(4096, 1, 1);
      const chunks: Float32Array[] = [];
      processor.onaudioprocess = (event) => {
        if (playgroundRecorderRef.current) {
          const input = event.inputBuffer.getChannelData(0);
          chunks.push(new Float32Array(input));
        }
      };
      source.connect(processor);
      processor.connect(context.destination);
      playgroundRecorderRef.current = {
        context,
        source,
        processor,
        stream,
        chunks,
      };
      setPlaygroundStatus("recording");
    } catch (err) {
      const errStr = String(err);
      if (
        errStr.includes("NotAllowedError") ||
        errStr.includes("not allowed") ||
        errStr.includes("denied permission")
      ) {
        setPermissionError("microphone");
      } else {
        setError(errStr);
      }
    }
  };

  const stopPlaygroundAndTranscribe = async () => {
    const recorder = playgroundRecorderRef.current;
    if (!recorder) {
      setPlaygroundStatus("idle");
      return;
    }
    const { context, source, processor, stream, chunks } = recorder;
    processor.disconnect();
    source.disconnect();
    processor.onaudioprocess = null;
    stream.getTracks().forEach((track) => track.stop());
    const sampleRate = context.sampleRate;
    await context.close();
    playgroundRecorderRef.current = null;
    const merged = mergeFloat32(chunks);
    if (merged.length < sampleRate * 0.15) {
      setPlaygroundStatus("idle");
      return;
    }
    const wav = encodeWav(merged, sampleRate);
    setPlaygroundStatus("transcribing");
    try {
      const text = await invoke<string>("transcribe_audio", {
        request: {
          bytes: Array.from(wav),
          fileName: "playground.wav",
          modelId: activeModelId ?? "base",
        },
      });
      const trimmed = text.trim();
      if (trimmed) {
        setPlaygroundText((prev) => (prev ? prev + "\n" + trimmed : trimmed));
      }
    } catch (err) {
      setError(String(err));
    } finally {
      setPlaygroundStatus("idle");
    }
  };

  const buildDictationShortcut = (event: KeyboardEvent) => {
    const key = event.code;
    const modifiers: string[] = [];
    if (event.metaKey && !key.startsWith("Meta")) {
      modifiers.push("command");
    }
    if (event.ctrlKey && !key.startsWith("Control")) {
      modifiers.push("control");
    }
    if (event.altKey && !key.startsWith("Alt")) {
      modifiers.push("alt");
    }
    if (event.shiftKey && !key.startsWith("Shift")) {
      modifiers.push("shift");
    }
    return normalizeDictationShortcut({ key, modifiers });
  };

  const cloudTestHint = (provider: CloudProvider) => {
    const state = cloudTests[provider];
    if (state.status === "testing") {
      return t("testing");
    }
    if (state.status === "success") {
      if (state.latencyMs !== undefined && state.latencyMs !== null) {
        return t("testSuccessLatency", { ms: state.latencyMs });
      }
      return t("testSuccess");
    }
    if (state.status === "error") {
      return state.message || t("testFailed");
    }
    return t("testKeyHint");
  };

  const handleTestCloudKey = async (provider: CloudProvider) => {
    const apiKey = providerInputKey(provider).trim();
    const endpoint = providerInputEndpoint(provider).trim();
    if (!apiKey) {
      setCloudTests((prev) => ({
        ...prev,
        [provider]: { status: "error", message: t("apiKeyRequired") },
      }));
      return;
    }
    setCloudTests((prev) => ({
      ...prev,
      [provider]: { status: "testing" },
    }));
    try {
      const result = await invoke<CloudTestResult>("test_cloud_api_key", {
        provider,
        apiKey,
        endpoint,
      });
      setCloudTests((prev) => ({
        ...prev,
        [provider]: {
          status: result.ok ? "success" : "error",
          latencyMs: result.latencyMs ?? null,
          message: result.ok ? undefined : result.message || t("testFailed"),
        },
      }));
    } catch (err) {
      setCloudTests((prev) => ({
        ...prev,
        [provider]: { status: "error", message: String(err) },
      }));
    }
  };

  const handleDownloadModel = async (modelId: string) => {
    setError(null);
    const model = models.find((item) => item.id === modelId);
    if (model?.engine === "cloud") {
      const key = providerKey(model.provider);
      if (!key.trim()) {
        setError(
          t("cloudKeyRequiredProvider", {
            provider: providerLabel(model.provider),
          }),
        );
        setActivePage("settings");
        return;
      }
    }
    if (model?.engine === "glm-mlx" && !(glmDeps?.ready ?? false)) {
      setError(t("runtimeRequired"));
      return;
    }
    setModelAction({ id: modelId, type: "download" });
    try {
      await invoke<ModelInfo>("download_model", { modelId });
      await refreshModels();
    } catch (err) {
      setError(String(err));
    } finally {
      setModelAction(null);
    }
  };

  const handleInstallGlm = async () => {
    setError(null);
    setGlmAction("install");
    try {
      const status = await invoke<GlmDependencyStatus>(
        "glm_install_dependencies",
      );
      setGlmDeps(status);
    } catch (err) {
      setError(String(err));
    } finally {
      setGlmAction(null);
      void refreshGlmDeps();
    }
  };

  const handleSetupGlmModel = async (modelId: string) => {
    if (glmDeps && !glmDeps.supported) {
      setError(t("runtimeUnsupported"));
      return;
    }
    setError(null);
    setGlmAction("setup");
    setModelAction({ id: modelId, type: "download" });
    try {
      if (!(glmDeps?.ready ?? false)) {
        await invoke<GlmDependencyStatus>("glm_install_dependencies");
        await refreshGlmDeps();
      }
      await invoke<ModelInfo>("download_model", { modelId });
      await refreshModels();
    } catch (err) {
      setError(String(err));
    } finally {
      setGlmAction(null);
      setModelAction(null);
    }
  };

  const handleResetGlmRuntime = async () => {
    setError(null);
    setGlmAction("reset");
    try {
      await invoke("glm_reset_runtime");
      await refreshGlmDeps();
    } catch (err) {
      setError(String(err));
    } finally {
      setGlmAction(null);
    }
  };

  const handleClearGlmCache = async () => {
    setError(null);
    setGlmAction("clear");
    try {
      await invoke("glm_clear_cache");
      await refreshModels();
    } catch (err) {
      setError(String(err));
    } finally {
      setGlmAction(null);
    }
  };

  const handleActivateModel = async (modelId: string) => {
    setError(null);
    const model = models.find((item) => item.id === modelId);
    if (model?.engine === "cloud") {
      const key = providerKey(model.provider);
      if (!key.trim()) {
        setError(
          t("cloudKeyRequiredProvider", {
            provider: providerLabel(model.provider),
          }),
        );
        setActivePage("settings");
        return;
      }
    }
    setModelAction({ id: modelId, type: "activate" });
    try {
      const next = await invoke<string>("set_active_model", { modelId });
      setActiveModelId(next);
    } catch (err) {
      setError(String(err));
    } finally {
      setModelAction(null);
    }
  };

  const handleDeleteModel = async (modelId: string, name: string) => {
    if (!window.confirm(t("deleteConfirm", { name }))) {
      return;
    }
    setError(null);
    try {
      await invoke("delete_model", { modelId });
      await refreshModels();
    } catch (err) {
      setError(String(err));
    }
  };

  return (
    <div
      className={`app ${
        uiSettings.reducedTransparency ? "reduced-transparency" : ""
      } ${drawerOpen ? "drawer-open" : ""}`}
    >
      <div className="layout">
        <aside className="sidebar">
          <div className="sidebar-top" data-tauri-drag-region>
            <div className="sidebar-header">
              <div className="brand">
                <div className="brand-mark">
                  <div className="mark-core" />
                  <div className="mark-ring" />
                </div>
                <div>
                  <div className="brand-title">{t("appName")}</div>
                  <div className="brand-subtitle">{t("appSubtitle")}</div>
                </div>
              </div>
              <button
                className="drawer-close"
                onClick={() => setDrawerOpen(false)}
                aria-label="Close menu"
                data-tauri-drag-region="false"
              >
                <X size={16} strokeWidth={1.6} aria-hidden />
              </button>
            </div>
          </div>
          <div className="sidebar-body">
            <nav className="sidebar-nav" data-tauri-drag-region="false">
              {pages.map((page) => {
                const Icon = page.icon;
                return (
                  <button
                    key={page.id}
                    className={`nav-item ${
                      activePage === page.id ? "is-active" : ""
                    }`}
                    onClick={() => {
                      setActivePage(page.id);
                      setDrawerOpen(false);
                    }}
                  >
                    <span className="nav-icon">
                      <Icon size={14} strokeWidth={1.6} />
                    </span>
                    <span className="nav-label">{page.label}</span>
                  </button>
                );
              })}
            </nav>
            <div className="sidebar-footer">
              <div className="sidebar-status">
                <div
                  className={`status-pill ${isRunning ? "is-online" : "is-offline"}`}
                >
                  <span className="status-dot" />
                  {isRunning ? t("running") : t("stopped")}
                </div>
              </div>
              <div className="sidebar-meta">
                <span>{t("port")}</span>
                <span>{status?.port ?? fallbackPort}</span>
              </div>
              <div className="sidebar-meta">
                <span>{t("model")}</span>
                <span>{currentModelId}</span>
              </div>
            </div>
          </div>
        </aside>

        <main className="content">
          <div className="content-header-wrapper" data-tauri-drag-region>
            <div className="drag-strip" />
            <header className="content-header">
              <div className="content-title-row">
                <button
                  className="drawer-toggle"
                  onClick={() => setDrawerOpen(true)}
                  aria-label="Open menu"
                  data-tauri-drag-region="false"
                >
                  <Menu size={16} strokeWidth={1.6} aria-hidden />
                </button>
                <div>
                  <div className="content-title">{currentPage.label}</div>
                  <div className="content-subtitle">
                    {currentPage.description}
                  </div>
                </div>
              </div>
              <div className="content-actions" data-tauri-drag-region="false">
                {dictationState !== "idle" && (
                  <div className="dictation-badge">
                    <span className="dictation-dot" />
                    {dictationState === "listening"
                      ? t("dictationListeningBadge")
                      : t("dictationProcessingBadge")}
                  </div>
                )}
                {activePage === "models" && modelsTab !== "cloud" ? (
                  <button
                    className="button tiny"
                    onClick={() => setModelsEditing((value) => !value)}
                  >
                    {modelsEditing ? t("done") : t("edit")}
                  </button>
                ) : (
                  activePage !== "settings" && (
                    <button
                      className="button tiny"
                      onClick={() => setActivePage("settings")}
                    >
                      {t("settings")}
                    </button>
                  )
                )}
              </div>
            </header>
          </div>

          <div className="content-body">
            {activePage === "overview" && (
              <>
                <div className="card">
                  <div className="card-header">
                    <div>
                      <h2>{t("serviceControl")}</h2>
                      <p className="muted">{t("serviceControlDesc")}</p>
                    </div>
                    <div className="badge-row">
                      <span className="badge">
                        {t("requests")} {status?.requests ?? 0}
                      </span>
                      <span className="badge">
                        {t("port")} {status?.port ?? fallbackPort}
                      </span>
                    </div>
                  </div>

                  <div className="server-controls">
                    <div className="field">
                      <label htmlFor="port">{t("port")}</label>
                      <input
                        id="port"
                        type="text"
                        inputMode="numeric"
                        value={portInput}
                        onChange={(event) => setPortInput(event.target.value)}
                        disabled={isRunning}
                      />
                    {!portValid && (
                      <span className="input-hint">{t("portHint")}</span>
                    )}
                  </div>
                  <button
                    className="button primary"
                    onClick={handleStart}
                    disabled={isRunning || action === "start"}
                  >
                    {action === "start" ? t("starting") : t("start")}
                  </button>
                  <button
                    className="button ghost"
                    onClick={handleStop}
                    disabled={!isRunning || action === "stop"}
                  >
                    {action === "stop" ? t("stopping") : t("stop")}
                  </button>
                </div>

                  <div className="server-meta">
                    <div className="meta-row">
                      <span className="meta-label">{t("url")}</span>
                      <span className="meta-value">
                        {status?.url ?? t("notRunning")}
                      </span>
                    </div>
                    <div className="meta-row">
                      <span className="meta-label">{t("started")}</span>
                      <span className="meta-value">
                        {formatTimestamp(status?.startedAt)}
                      </span>
                    </div>
                    <div className="meta-row">
                      <span className="meta-label">{t("health")}</span>
                      <span className="meta-value">
                        {isRunning ? t("listening") : t("offline")}
                      </span>
                    </div>
                  </div>

                  {error && <div className="error-banner">{error}</div>}
                  {permissionError === "microphone" && (
                    <div className="error-banner permission-error">
                      <span>{t("microphonePermissionDenied")}</span>
                      <button
                        className="small"
                        onClick={() => {
                          setPermissionError(null);
                          handleOpenPermissionSettings("microphone");
                        }}
                      >
                        {t("openMicrophoneSettings")}
                      </button>
                    </div>
                  )}
                </div>

                <div className="card">
                  <div className="card-header">
                    <div>
                      <h2>{t("cloudUsageTitle")}</h2>
                      <p className="muted">{t("cloudUsageDesc")}</p>
                    </div>
                  </div>
                  <div className="cloud-meta">
                    <div className="cloud-meta-row">
                      <span className="cloud-meta-label">
                        {t("cloudRequests")}
                      </span>
                      <span className="cloud-meta-value">
                        {cloudUsage?.requests ?? 0}
                      </span>
                    </div>
                    <div className="cloud-meta-row">
                      <span className="cloud-meta-label">
                        {t("cloudLatency")}
                      </span>
                      <span className="cloud-meta-value">
                        {formatLatency(cloudUsage?.lastLatencyMs)}
                      </span>
                    </div>
                    <div className="cloud-meta-row">
                      <span className="cloud-meta-label">
                        {t("cloudLastError")}
                      </span>
                      <span
                        className={`cloud-meta-value ${
                          cloudUsage?.lastError ? "is-error" : ""
                        }`}
                      >
                        {cloudUsage?.lastError ?? t("cloudNoError")}
                      </span>
                    </div>
                  </div>
                </div>

                <div className="card">
                  <div className="card-header">
                    <div>
                      <h2>{t("endpoints")}</h2>
                      <p className="muted">{t("endpointsDesc")}</p>
                    </div>
                    <div className="pill">{t("openai")}</div>
                  </div>

                  <div className="endpoint-list">
                    <div className="endpoint">
                      <div className="endpoint-title">{t("transcriptions")}</div>
                      <div className="endpoint-desc">
                        {t("transcriptionsDesc")}
                      </div>
                      <div className="endpoint-actions">
                        <code className="endpoint-code">{curlCommand}</code>
                        <button className="button tiny" onClick={handleCopy}>
                          {copied ? t("copied") : t("copy")}
                        </button>
                      </div>
                    </div>
                    <div className="endpoint">
                      <div className="endpoint-title">{t("healthEndpoint")}</div>
                      <div className="endpoint-desc">
                        {t("healthEndpointDesc")}
                      </div>
                    </div>
                  </div>
                </div>
              </>
            )}

            {activePage === "models" && (
              <div className="card models-card">
                <div className="card-header">
                  <div>
                    <h2>{t("modelsTitle")}</h2>
                    <p className="muted">{t("modelsDesc")}</p>
                  </div>
                  <div className="badge-row">
                    <span className="badge badge-accent">Native</span>
                  </div>
                </div>

                <div className="model-tabs">
                  <button
                    className={`model-tab ${
                      modelsTab === "whisper" ? "is-active" : ""
                    }`}
                    onClick={() => setModelsTab("whisper")}
                  >
                    {t("whisperModels")}
                  </button>
                  <button
                    className={`model-tab ${
                      modelsTab === "python" ? "is-active" : ""
                    }`}
                    onClick={() => setModelsTab("python")}
                  >
                    {t("pythonModels")}
                  </button>
                  <button
                    className={`model-tab ${
                      modelsTab === "cloud" ? "is-active" : ""
                    }`}
                    onClick={() => setModelsTab("cloud")}
                  >
                    {t("cloudModels")}
                  </button>
                </div>

                {modelsTab === "cloud" && (
                  <div className="cloud-filters">
                    <button
                      className={`filter-chip ${
                        cloudFilter === "all" ? "is-active" : ""
                      }`}
                      onClick={() => setCloudFilter("all")}
                    >
                      {t("all")}
                      <span className="filter-count">
                        {cloudProviderCounts.all}
                      </span>
                    </button>
                    <button
                      className={`filter-chip ${
                        cloudFilter === "bigmodel" ? "is-active" : ""
                      }`}
                      onClick={() => setCloudFilter("bigmodel")}
                    >
                      {t("bigmodel")}
                      <span className="filter-count">
                        {cloudProviderCounts.bigmodel}
                      </span>
                    </button>
                    <button
                      className={`filter-chip ${
                        cloudFilter === "elevenlabs" ? "is-active" : ""
                      }`}
                      onClick={() => setCloudFilter("elevenlabs")}
                    >
                      {t("elevenlabs")}
                      <span className="filter-count">
                        {cloudProviderCounts.elevenlabs}
                      </span>
                    </button>
                  </div>
                )}

                {modelsTab === "python" && pythonModels.length > 0 && (
                  <div className="runtime-row">
                    <div>
                      <div className="runtime-title">{t("runtimeTitle")}</div>
                      <div className="muted">{t("runtimeDesc")}</div>
                    </div>
                    <div className="runtime-actions">
                      <span
                        className={`runtime-status ${
                          glmReady
                            ? "is-ready"
                            : glmSupported
                              ? "is-missing"
                              : "is-unsupported"
                        }`}
                      >
                        {glmReady
                          ? t("runtimeReady")
                          : glmSupported
                            ? t("runtimeMissing")
                            : t("runtimeUnsupported")}
                      </span>
                      {!glmReady && glmSupported && (
                        <button
                          className="button tiny"
                          onClick={handleInstallGlm}
                          disabled={glmAction === "install"}
                        >
                          {glmAction === "install"
                            ? t("runtimeInstalling")
                            : t("runtimeInstall")}
                        </button>
                      )}
                      {(glmAction === "install" || glmAction === "setup") && (
                        <button
                          className="button tiny ghost"
                          onClick={() => setActivePage("logs")}
                        >
                          {t("viewLogs")}
                        </button>
                      )}
                    </div>
                  </div>
                )}

                {modelsTab === "cloud" ? (
                  <div className="model-section">
                    <div className="section-title">{t("cloudModels")}</div>
                    <div className="model-list">
                      {filteredCloudModels.length === 0 ? (
                        <div className="empty">{t("noModels")}</div>
                      ) : (
                        filteredCloudModels.map((model) => {
                          const isActive = model.id === activeModelId;
                          const isActivating =
                            modelAction?.id === model.id &&
                            modelAction.type === "activate";
                          const missingKey = !providerKey(model.provider).trim();
                          const providerName = providerLabel(model.provider);
                          return (
                            <div
                              key={model.id}
                              className={`model-row ${isActive ? "is-active" : ""}`}
                            >
                              <div className="model-info">
                                <div className="model-title">
                                  <span>{model.name}</span>
                                  <span className="model-size">{model.size}</span>
                                  <span className="model-tag">{providerName}</span>
                                </div>
                                <div className="model-desc">{model.description}</div>
                                {missingKey && (
                                  <div className="model-note warning">
                                    {t("apiKeyRequired")}
                                  </div>
                                )}
                              </div>
                              <div className="model-actions">
                                {isActive ? (
                                  <span className="pill">{t("active")}</span>
                                ) : missingKey ? (
                                  <button
                                    className="button tiny ghost"
                                    onClick={() => setActivePage("settings")}
                                  >
                                    {t("configure")}
                                  </button>
                                ) : (
                                  <button
                                    className="button tiny"
                                    onClick={() => handleActivateModel(model.id)}
                                    disabled={Boolean(modelAction)}
                                  >
                                    {isActivating ? t("activating") : t("activate")}
                                  </button>
                                )}
                              </div>
                            </div>
                          );
                        })
                      )}
                    </div>
                  </div>
                ) : (
                  <div className="model-section">
                    <div className="section-title">
                      {modelsTab === "whisper"
                        ? t("whisperModels")
                        : t("pythonModels")}
                    </div>
                    <div className="model-list">
                      {(modelsTab === "whisper" ? whisperModels : pythonModels)
                        .length === 0 ? (
                        <div className="empty">{t("noModels")}</div>
                      ) : (
                        (modelsTab === "whisper" ? whisperModels : pythonModels).map(
                          (model) => {
                            const isActive = model.id === activeModelId;
                            const isDownloading =
                              modelAction?.id === model.id &&
                              modelAction.type === "download";
                            const isActivating =
                              modelAction?.id === model.id &&
                              modelAction.type === "activate";
                            const isGlm = model.engine === "glm-mlx";
                            return (
                              <div
                                key={model.id}
                                className={`model-row ${
                                  isActive ? "is-active" : ""
                                }`}
                              >
                                <div className="model-info">
                                  <div className="model-title">
                                    <span>{model.name}</span>
                                    <span className="model-size">{model.size}</span>
                                  </div>
                                  <div className="model-desc">
                                    {model.description}
                                  </div>
                                </div>
                                <div className="model-actions">
                                  {model.downloaded ? (
                                    modelsEditing ? (
                                      isActive ? (
                                        <span className="pill">{t("active")}</span>
                                      ) : (
                                        <button
                                          className="button tiny danger"
                                          onClick={() =>
                                            handleDeleteModel(model.id, model.name)
                                          }
                                          disabled={Boolean(modelAction)}
                                        >
                                          <Trash2 size={12} strokeWidth={1.6} />
                                          {t("remove")}
                                        </button>
                                      )
                                    ) : isActive ? (
                                      <span className="pill">{t("active")}</span>
                                    ) : (
                                      <button
                                        className="button tiny"
                                        onClick={() =>
                                          handleActivateModel(model.id)
                                        }
                                        disabled={Boolean(modelAction)}
                                      >
                                        {isActivating ? t("activating") : t("activate")}
                                      </button>
                                    )
                                  ) : (
                                    <button
                                      className="button tiny"
                                      onClick={() =>
                                        isGlm && !(glmDeps?.ready ?? false)
                                          ? handleSetupGlmModel(model.id)
                                          : handleDownloadModel(model.id)
                                      }
                                      disabled={
                                        Boolean(modelAction) || glmAction === "setup"
                                      }
                                    >
                                      {isGlm && !(glmDeps?.ready ?? false)
                                        ? glmAction === "setup"
                                          ? t("settingUp")
                                          : t("setup")
                                        : isDownloading
                                          ? t("downloading")
                                          : t("download")}
                                    </button>
                                  )}
                                </div>
                              </div>
                            );
                          },
                        )
                      )}
                    </div>
                  </div>
                )}
              </div>
            )}

            {activePage === "logs" && (
              <div className="card logs-card">
                <div className="card-header">
                  <div>
                    <h2>{t("logsTitle")}</h2>
                    <p className="muted">{t("logsDesc")}</p>
                  </div>
                  <div className="badge-row">
                    {logsStreaming && (
                      <span className="stream-indicator">{t("live")}</span>
                    )}
                    <button
                      className="button ghost tiny"
                      onClick={handleClearLogs}
                      disabled={logs.length === 0}
                    >
                      {t("clear")}
                    </button>
                  </div>
                </div>

                <div className="logs">
                  {logs.length === 0 ? (
                    <div className="empty">{t("noLogs")}</div>
                  ) : (
                    logs.map((entry) => (
                      <div
                        key={entry.id}
                        className={`log-row level-${entry.level}`}
                      >
                        <span className="log-time">
                          {formatTimestamp(entry.timestamp)}
                        </span>
                        <span className="log-level">{entry.level}</span>
                        <span className="log-message">{entry.message}</span>
                      </div>
                    ))
                  )}
                </div>
              </div>
            )}

            {activePage === "playground" && (
              <div className="card">
                <div className="playground-area">
                  <button
                    className={`playground-record-btn ${
                      playgroundStatus === "recording" ? "is-recording" : ""
                    }`}
                    onClick={
                      playgroundStatus === "idle"
                        ? startPlaygroundRecording
                        : playgroundStatus === "recording"
                          ? stopPlaygroundAndTranscribe
                          : undefined
                    }
                    disabled={playgroundStatus === "transcribing"}
                  >
                    <Mic size={28} />
                  </button>
                  <div className="playground-status">
                    {playgroundStatus === "recording"
                      ? t("playgroundRecording")
                      : playgroundStatus === "transcribing"
                        ? t("playgroundTranscribing")
                        : t("playgroundRecord")}
                  </div>
                  <div className="playground-transcript">
                    {playgroundText || (
                      <span className="playground-placeholder">
                        {t("playgroundPlaceholder")}
                      </span>
                    )}
                  </div>
                </div>
              </div>
            )}

            {activePage === "settings" && (
              <div className="settings-stack">
                <div className="card settings-section">
                  <h3>{t("appearance")}</h3>
                  <div className="settings-group">
                    <div className="settings-row">
                      <div>
                        <div className="settings-label">
                          {t("reducedTransparency")}
                        </div>
                        <div className="settings-hint">
                          {t("reducedTransparencyHint")}
                        </div>
                      </div>
                      <button
                        className={`switch ${
                          uiSettings.reducedTransparency ? "is-on" : ""
                        }`}
                        onClick={() =>
                          handleToggleTransparency(!uiSettings.reducedTransparency)
                        }
                        aria-pressed={uiSettings.reducedTransparency}
                      >
                        <span className="switch-thumb" />
                      </button>
                    </div>
                    <div className="settings-row">
                      <div>
                        <div className="settings-label">{t("language")}</div>
                        <div className="settings-hint">{t("languageHint")}</div>
                      </div>
                      <select
                        value={uiSettings.language}
                        onChange={(event) =>
                          handleLanguageChange(
                            event.target.value as UiSettings["language"],
                          )
                        }
                      >
                        <option value="en">English</option>
                        <option value="zh">中文</option>
                      </select>
                    </div>
                  </div>
                </div>

                <div className="card settings-section">
                  <h3>{t("dictationTitle")}</h3>
                  <div className="settings-group">
                    <div className="settings-row">
                      <div>
                        <div className="settings-label">{t("dictationStatus")}</div>
                        <div className="settings-hint">
                          {t("dictationStatusHint")}
                        </div>
                      </div>
                      <span className="status-chip">
                        {dictationState === "listening"
                          ? t("dictationListening")
                          : dictationState === "processing"
                            ? t("dictationProcessing")
                            : t("dictationIdle")}
                      </span>
                    </div>
                    <div className="settings-row">
                      <div>
                        <div className="settings-label">
                          {t("dictationShortcutLabel")}
                        </div>
                        <div className="settings-hint">
                          {dictationCapture
                            ? t("dictationShortcutCaptureHint")
                            : t("dictationShortcutHint")}
                        </div>
                      </div>
                      <div className="shortcut-control">
                        <div
                          className={`shortcut-display ${
                            dictationCapture ? "is-capturing" : ""
                          }`}
                        >
                          {dictationCapture
                            ? t("dictationShortcutListening")
                            : formatShortcutLabel(uiSettings.dictationShortcut)}
                        </div>
                        <button
                          className="button tiny"
                          onClick={() => setDictationCapture(true)}
                          disabled={dictationCapture}
                        >
                          {dictationCapture
                            ? t("dictationShortcutListening")
                            : t("dictationShortcutChange")}
                        </button>
                      </div>
                    </div>
                    <div className="settings-row">
                      <div>
                        <div className="settings-label">
                          {t("dictationAutoPaste")}
                        </div>
                        <div className="settings-hint">
                          {t("dictationAutoPasteHint")}
                        </div>
                      </div>
                      <button
                        className={`switch ${
                          uiSettings.dictationAutoPaste ? "is-on" : ""
                        }`}
                        onClick={() =>
                          persistSettings({
                            ...uiSettings,
                            dictationAutoPaste: !uiSettings.dictationAutoPaste,
                          })
                        }
                        aria-pressed={uiSettings.dictationAutoPaste}
                      >
                        <span className="switch-thumb" />
                      </button>
                    </div>
                  </div>
                </div>

                <div className="card settings-section">
                  <h3>{t("permissionsTitle")}</h3>
                  <div className="settings-group">
                    <div className="settings-row">
                      <div>
                        <div className="settings-label">
                          {t("microphonePermission")}
                        </div>
                        <div className="settings-hint">
                          {t("microphonePermissionHint")}
                        </div>
                      </div>
                      <div className="settings-actions">
                        <button
                          className="button tiny"
                          onClick={handleRequestMicrophone}
                        >
                          {t("requestPermission")}
                        </button>
                        <button
                          className="button tiny ghost"
                          onClick={() =>
                            handleOpenPermissionSettings("microphone")
                          }
                        >
                          {t("openMicrophoneSettings")}
                        </button>
                      </div>
                    </div>
                    <div className="settings-row">
                      <div>
                        <div className="settings-label">
                          {t("accessibilityPermission")}
                        </div>
                        <div className="settings-hint">
                          {t("accessibilityPermissionHint")}
                        </div>
                      </div>
                      <div className="settings-actions">
                        <button
                          className="button tiny ghost"
                          onClick={() =>
                            handleOpenPermissionSettings("accessibility")
                          }
                        >
                          {t("openAccessibilitySettings")}
                        </button>
                      </div>
                    </div>
                  </div>
                </div>

                <div className="card settings-section">
                  <h3>{t("service")}</h3>
                  <div className="settings-group">
                    <div className="settings-row">
                      <div>
                        <div className="settings-label">{t("gateway")}</div>
                        <div className="settings-hint">
                          {isRunning
                            ? t("listeningOn", {
                                url: status?.url ?? "localhost",
                              })
                            : t("stopped")}
                        </div>
                      </div>
                      <span
                        className={`status-chip ${
                          isRunning ? "is-online" : "is-offline"
                        }`}
                      >
                        {isRunning ? t("online") : t("offline")}
                      </span>
                    </div>
                    <div className="settings-row">
                      <div>
                        <div className="settings-label">{t("port")}</div>
                        <div className="settings-hint">
                          {status?.port ?? fallbackPort}
                        </div>
                      </div>
                    </div>
                  </div>
                </div>

                <div className="card settings-section">
                  <h3>{t("logsSettings")}</h3>
                  <div className="settings-group">
                    <div className="settings-row">
                      <div>
                        <div className="settings-label">{t("clearLogFile")}</div>
                        <div className="settings-hint">
                          {t("clearLogFileHint")}
                        </div>
                      </div>
                      <button
                        className="button tiny"
                        onClick={handleClearLogs}
                        disabled={logs.length === 0}
                      >
                        {t("clearLogFileAction")}
                      </button>
                    </div>
                  </div>
                </div>

                <div className="card settings-section">
                  <h3>{t("modelsSettings")}</h3>
                  <div className="settings-group">
                    <div className="settings-row">
                      <div>
                        <div className="settings-label">{t("activeModel")}</div>
                        <div className="settings-hint">{currentModelId}</div>
                      </div>
                    </div>
                    <div className="settings-row">
                      <div>
                        <div className="settings-label">{t("available")}</div>
                        <div className="settings-hint">
                          {t("modelsCount", { count: models.length })}
                        </div>
                      </div>
                    </div>
                  </div>
                  <div className="settings-group">
                    <div className="settings-row">
                      <div>
                        <div className="settings-label">{t("runtimeTitle")}</div>
                        <div className="settings-hint">{t("runtimeDesc")}</div>
                      </div>
                      <span
                        className={`status-chip ${glmReady ? "is-online" : ""}`}
                      >
                        {glmReady
                          ? t("runtimeReady")
                          : glmSupported
                            ? t("runtimeMissing")
                            : t("runtimeUnsupported")}
                      </span>
                    </div>
                    <div className="settings-row">
                      <div>
                        <div className="settings-label">{t("runtimeInstall")}</div>
                        <div className="settings-hint">
                          {glmDeps?.python ?? "python3"}
                        </div>
                      </div>
                      <button
                        className="button tiny"
                        onClick={handleInstallGlm}
                        disabled={glmAction === "install" || !glmSupported}
                      >
                        {glmAction === "install"
                          ? t("runtimeInstalling")
                          : t("runtimeInstall")}
                      </button>
                    </div>
                    <div className="settings-row">
                      <div>
                        <div className="settings-label">{t("runtimeReset")}</div>
                        <div className="settings-hint">~/.openstt/venv</div>
                      </div>
                      <button
                        className="button tiny"
                        onClick={handleResetGlmRuntime}
                        disabled={glmAction === "reset" || !glmSupported}
                      >
                        {glmAction === "reset"
                          ? t("runtimeResetting")
                          : t("runtimeReset")}
                      </button>
                    </div>
                    <div className="settings-row">
                      <div>
                        <div className="settings-label">{t("runtimeClearCache")}</div>
                        <div className="settings-hint">~/.openstt/models/glm/cache</div>
                      </div>
                      <button
                        className="button tiny"
                        onClick={handleClearGlmCache}
                        disabled={glmAction === "clear" || !glmSupported}
                      >
                        {glmAction === "clear"
                          ? t("runtimeClearing")
                          : t("runtimeClearCache")}
                      </button>
                    </div>
                  </div>
                </div>

                <div className="card settings-section">
                  <h3>{t("cloudSettings")}</h3>
                  <div className="settings-group">
                    <div className="settings-row">
                      <div>
                        <div className="settings-label">{t("bigmodel")}</div>
                        <div className="settings-hint">{t("bigmodelKeyHint")}</div>
                      </div>
                      <input
                        type="password"
                        value={bigmodelKeyInput}
                        onChange={(event) => {
                          setBigmodelKeyInput(event.target.value);
                          setCloudDirty(true);
                          setCloudTests((prev) => ({
                            ...prev,
                            bigmodel: { status: "idle" },
                          }));
                        }}
                        placeholder="sk-..."
                        className="settings-input"
                      />
                    </div>
                    <div className="settings-row">
                      <div>
                        <div className="settings-label">{t("apiEndpoint")}</div>
                        <div className="settings-hint">{t("bigmodelEndpointHint")}</div>
                      </div>
                      <input
                        type="text"
                        value={bigmodelEndpointInput}
                        onChange={(event) => {
                          setBigmodelEndpointInput(event.target.value);
                          setCloudDirty(true);
                          setCloudTests((prev) => ({
                            ...prev,
                            bigmodel: { status: "idle" },
                          }));
                        }}
                        className="settings-input"
                      />
                    </div>
                    <div className="settings-row">
                      <div>
                        <div className="settings-label">{t("testKey")}</div>
                        <div
                          className={`settings-hint ${
                            cloudTests.bigmodel.status === "error"
                              ? "is-error"
                              : cloudTests.bigmodel.status === "success"
                                ? "is-success"
                                : ""
                          }`}
                        >
                          {cloudTestHint("bigmodel")}
                        </div>
                      </div>
                      <button
                        className="button tiny"
                        onClick={() => handleTestCloudKey("bigmodel")}
                        disabled={
                          cloudTests.bigmodel.status === "testing" ||
                          bigmodelKeyMissing
                        }
                      >
                        {cloudTests.bigmodel.status === "testing"
                          ? t("testing")
                          : t("test")}
                      </button>
                    </div>
                  </div>

                  <div className="settings-group">
                    <div className="settings-row">
                      <div>
                        <div className="settings-label">{t("elevenlabs")}</div>
                        <div className="settings-hint">{t("elevenlabsKeyHint")}</div>
                      </div>
                      <input
                        type="password"
                        value={elevenlabsKeyInput}
                        onChange={(event) => {
                          setElevenlabsKeyInput(event.target.value);
                          setCloudDirty(true);
                          setCloudTests((prev) => ({
                            ...prev,
                            elevenlabs: { status: "idle" },
                          }));
                        }}
                        placeholder="xi-..."
                        className="settings-input"
                      />
                    </div>
                    <div className="settings-row">
                      <div>
                        <div className="settings-label">{t("apiEndpoint")}</div>
                        <div className="settings-hint">{t("elevenlabsEndpointHint")}</div>
                      </div>
                      <input
                        type="text"
                        value={elevenlabsEndpointInput}
                        onChange={(event) => {
                          setElevenlabsEndpointInput(event.target.value);
                          setCloudDirty(true);
                          setCloudTests((prev) => ({
                            ...prev,
                            elevenlabs: { status: "idle" },
                          }));
                        }}
                        className="settings-input"
                      />
                    </div>
                    <div className="settings-row">
                      <div>
                        <div className="settings-label">{t("testKey")}</div>
                        <div
                          className={`settings-hint ${
                            cloudTests.elevenlabs.status === "error"
                              ? "is-error"
                              : cloudTests.elevenlabs.status === "success"
                                ? "is-success"
                                : ""
                          }`}
                        >
                          {cloudTestHint("elevenlabs")}
                        </div>
                      </div>
                      <button
                        className="button tiny"
                        onClick={() => handleTestCloudKey("elevenlabs")}
                        disabled={
                          cloudTests.elevenlabs.status === "testing" ||
                          elevenlabsKeyMissing
                        }
                      >
                        {cloudTests.elevenlabs.status === "testing"
                          ? t("testing")
                          : t("test")}
                      </button>
                    </div>
                  </div>

                  <div className="settings-group">
                    <div className="settings-row">
                      <div>
                        <div className="settings-label">{t("save")}</div>
                        <div className="settings-hint">
                          {cloudDirty ? "" : t("saved")}
                        </div>
                      </div>
                      <button
                        className="button tiny"
                        onClick={handleSaveCloudSettings}
                        disabled={!cloudDirty}
                      >
                        {t("save")}
                      </button>
                    </div>
                  </div>
                </div>

                <div className="card settings-section">
                  <h3>{t("about")}</h3>
                  <div className="settings-group">
                    <div className="settings-row">
                      <div>
                        <div className="settings-label">OpenSTT</div>
                        <div className="settings-hint">{t("aboutDesc")}</div>
                      </div>
                    </div>
                    <div className="settings-row">
                      <div>
                        <div className="settings-label">{t("build")}</div>
                        <div className="settings-hint">0.1.0</div>
                      </div>
                    </div>
                  </div>
                </div>
              </div>
            )}
          </div>
        </main>
      </div>
      <div
        className="drawer-scrim"
        onClick={() => setDrawerOpen(false)}
      />
    </div>
  );
}

export default App;
