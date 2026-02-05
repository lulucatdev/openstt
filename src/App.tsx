import { useEffect, useMemo, useState } from "react";
import {
  Check,
  Database,
  FileText,
  LayoutDashboard,
  Menu,
  Settings,
  Shield,
  Square,
  Trash2,
  X,
} from "lucide-react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";

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
  dictationShortcut: DictationShortcut;
  dictationAutoPaste: boolean;
  elevenlabsApiKey: string;
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
};

type DictationStateEvent = {
  state: "idle" | "listening" | "processing";
  queueLen: number;
};

type PlaygroundTranscriptionResult = {
  text: string;
  error: string | null;
};

type DownloadProgressEvent = {
  modelId: string;
  percent: number;
  done: boolean;
  error: string | null;
};

type MlxDependencyStatus = {
  supported: boolean;
  ready: boolean;
  python: string | null;
  venv: boolean;
  mlxAudio: boolean;
  message?: string | null;
};

type LegacyModelsInfo = {
  found: boolean;
  path: string;
  sizeBytes: number;
};

type PermissionStatus = {
  accessibility: boolean;
  microphone: string;
  inputMonitoring: boolean;
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

const detectSystemLanguage = (): "en" | "zh" => {
  try {
    const locale = new Intl.Locale(navigator.language).maximize();
    if (locale.script === "Hans") return "zh";
  } catch {
    // fall through
  }
  return "en";
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
    pageModelsDesc: "Manage speech-to-text models",
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
    modelsDesc: "Manage speech-to-text models",
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
    runtimeRequired: "Install MLX runtime before downloading this model.",
    setup: "Set up",
    settingUp: "Setting up...",
    viewLogs: "View logs",
    live: "Live",
    localModels: "Local Models",
    whisperModels: "Whisper Models",
    mlxModels: "MLX Models",
    mlxRecommendation: "MLX models run faster on Apple Silicon. Switch to MLX Models tab for better performance.",
    switchToMlx: "Switch to MLX",
    cloudModels: "Cloud Models",
    elevenlabsApiKey: "ElevenLabs API Key",
    elevenlabsApiKeyHint: "Required for cloud transcription",
    elevenlabsApiKeyPlaceholder: "Enter your API key",
    testApiKey: "Test",
    testingApiKey: "Testing...",
    apiKeyValid: "Valid",
    apiKeyInvalid: "Invalid",
    apiKeyNotSet: "Not configured",
    apiKeyNotVerified: "Not verified",
    getApiKey: "Get API Key",
    scribeV2: "Scribe v2",
    scribeV2Desc: "Best quality batch transcription, 90+ languages",
    scribeV2Realtime: "Scribe v2 Realtime",
    scribeV2RealtimeDesc: "Real-time dictation, ~150ms latency, 90+ languages",
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
    overviewModelTitle: "Model",
    overviewModelDesc: "Active transcription model",
    noActiveModel: "No model activated",
    noActiveModelHint: "Go to Models to download and activate one",
    manageModels: "Manage",
    playgroundTitle: "Test Transcription",
    playgroundDesc: "Record and transcribe to verify your setup",
    playgroundRecord: "Record",
    playgroundStop: "Stop & Transcribe",
    playgroundTranscribing: "Transcribing...",
    playgroundPlaceholder: "Transcription will appear here",
    playgroundRecording: "Recording...",
    status_stopped: "Stopped",
    status_loading: "Loading Model",
    status_ready: "Ready",
    status_listening: "Listening",
    status_transcribing: "Transcribing",
    legacyTitle: "Legacy Cleanup",
    legacyDesc: "Remove old model files from previous versions",
    legacyFound: "Found {size} of legacy files",
    legacyNotFound: "No legacy files found",
    legacyClean: "Clean up",
    legacyCleaning: "Cleaning...",
    onboardingTitle: "Welcome to OpenSTT",
    onboardingSubtitle: "Grant permissions to enable all features",
    onboardingInputMonitoring: "Input Monitoring",
    onboardingInputMonitoringDesc: "Required for global keyboard shortcut",
    onboardingMicrophone: "Microphone",
    onboardingMicrophoneDesc: "Required to record audio for transcription",
    onboardingAccessibility: "Accessibility",
    onboardingAccessibilityDesc: "Required for auto-paste after transcription",
    onboardingOpenSettings: "Open Settings",
    onboardingGrantAccess: "Grant Access",
    onboardingCheckPermission: "Check",
    onboardingGranted: "Granted",
    onboardingRestartApp: "Restart App",
    onboardingSkip: "Set up later",
    onboardingAllDone: "All permissions granted!",
    stopDictation: "Stop",
    runtimeInstallSuccess: "MLX runtime installed successfully",
    pagePermissions: "Permissions",
    pagePermissionsDesc: "Manage system permissions",
    permissionGranted: "Granted",
    permissionNotGranted: "Not Granted",
    permissionRefreshAll: "Refresh All",
    inputMonitoringPermission: "Input Monitoring",
    inputMonitoringPermissionHint: "Required for global keyboard shortcut",
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
    pageModelsDesc: "管理语音转文字模型",
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
    modelsDesc: "管理语音转文字模型",
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
    runtimeRequired: "请先安装 MLX 运行时再下载该模型。",
    setup: "配置",
    settingUp: "配置中...",
    viewLogs: "查看日志",
    live: "实时",
    localModels: "本地模型",
    whisperModels: "Whisper 模型",
    mlxModels: "MLX 模型",
    mlxRecommendation: "MLX 模型在 Apple Silicon 上运行更快。切换到 MLX 模型以获得更好的性能。",
    switchToMlx: "切换到 MLX",
    cloudModels: "云端模型",
    elevenlabsApiKey: "ElevenLabs API 密钥",
    elevenlabsApiKeyHint: "云端转写需要此密钥",
    elevenlabsApiKeyPlaceholder: "输入您的 API 密钥",
    testApiKey: "测试",
    testingApiKey: "测试中...",
    apiKeyValid: "有效",
    apiKeyInvalid: "无效",
    apiKeyNotSet: "未配置",
    apiKeyNotVerified: "未验证",
    getApiKey: "获取密钥",
    scribeV2: "Scribe v2",
    scribeV2Desc: "最佳质量批量转写，支持90+语言",
    scribeV2Realtime: "Scribe v2 实时",
    scribeV2RealtimeDesc: "实时听写，约150ms延迟，支持90+语言",
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
    overviewModelTitle: "模型",
    overviewModelDesc: "当前转写模型",
    noActiveModel: "未启用任何模型",
    noActiveModelHint: "前往模型页下载并启用",
    manageModels: "管理",
    playgroundTitle: "测试转写",
    playgroundDesc: "录音并转写以验证配置",
    playgroundRecord: "录音",
    playgroundStop: "停止并转写",
    playgroundTranscribing: "转写中...",
    playgroundPlaceholder: "转写结果将显示在这里",
    playgroundRecording: "录音中...",
    status_stopped: "已停止",
    status_loading: "加载模型中",
    status_ready: "就绪",
    status_listening: "录音中",
    status_transcribing: "转写中",
    legacyTitle: "历史遗留清理",
    legacyDesc: "删除旧版本遗留的模型文件",
    legacyFound: "发现 {size} 遗留文件",
    legacyNotFound: "未发现遗留文件",
    legacyClean: "清理",
    legacyCleaning: "清理中...",
    onboardingTitle: "欢迎使用 OpenSTT",
    onboardingSubtitle: "授予权限以启用所有功能",
    onboardingInputMonitoring: "输入监控",
    onboardingInputMonitoringDesc: "全局快捷键需要此权限",
    onboardingMicrophone: "麦克风",
    onboardingMicrophoneDesc: "录音转写需要此权限",
    onboardingAccessibility: "辅助功能",
    onboardingAccessibilityDesc: "转写后自动粘贴需要此权限",
    onboardingOpenSettings: "打开设置",
    onboardingGrantAccess: "授予权限",
    onboardingCheckPermission: "检查",
    onboardingGranted: "已授权",
    onboardingRestartApp: "重启应用",
    onboardingSkip: "稍后设置",
    onboardingAllDone: "所有权限已授予！",
    stopDictation: "停止",
    runtimeInstallSuccess: "MLX 运行时安装成功",
    pagePermissions: "权限",
    pagePermissionsDesc: "管理系统权限",
    permissionGranted: "已授权",
    permissionNotGranted: "未授权",
    permissionRefreshAll: "刷新全部",
    inputMonitoringPermission: "输入监控",
    inputMonitoringPermissionHint: "全局快捷键需要此权限",
  },
} as const;

function App() {
  const [appStatus, setAppStatus] = useState<
    "stopped" | "loading" | "ready" | "listening" | "transcribing"
  >("stopped");
  const [status, setStatus] = useState<ServerStatus | null>(null);
  const [logs, setLogs] = useState<LogEntry[]>([]);
  const [portInput, setPortInput] = useState(String(fallbackPort));
  const [error, setError] = useState<string | null>(null);
  const [permissionError, setPermissionError] = useState<"microphone" | null>(null);
  const [action, setAction] = useState<"start" | "stop" | null>(null);
  const [copied, setCopied] = useState(false);
  const [initialized, setInitialized] = useState(false);
  const [uiSettings, setUiSettings] = useState<UiSettings>({
    reducedTransparency: false,
    language: detectSystemLanguage(),
    dictationShortcut: defaultDictationShortcut,
    dictationAutoPaste: true,
    elevenlabsApiKey: "",
  });
  const [dictationCapture, setDictationCapture] = useState(false);
  const [dictationState, setDictationState] = useState<
    "idle" | "listening" | "processing"
  >("idle");
  const [, setDictationQueueCount] = useState(0);
  const [playgroundStatus, setPlaygroundStatus] = useState<
    "idle" | "recording" | "transcribing"
  >("idle");
  const [playgroundText, setPlaygroundText] = useState("");
  const [successMessage, setSuccessMessage] = useState<string | null>(null);
  const [activePage, setActivePage] = useState<
    "overview" | "models" | "permissions" | "logs" | "settings"
  >("overview");
  const [drawerOpen, setDrawerOpen] = useState(false);
  const [modelsEditing, setModelsEditing] = useState(false);
  const [modelsTab, setModelsTab] = useState<"whisper" | "mlx-local" | "cloud">(
    "mlx-local",
  );
  const [mlxDeps, setMlxDeps] = useState<MlxDependencyStatus | null>(null);
  const [mlxAction, setMlxAction] = useState<
    "install" | "setup" | "reset" | null
  >(null);
  const [elevenlabsKeyStatus, setElevenlabsKeyStatus] = useState<
    "unknown" | "testing" | "valid" | "invalid"
  >("unknown");
  const [legacyModels, setLegacyModels] = useState<LegacyModelsInfo | null>(null);
  const [legacyAction, setLegacyAction] = useState<"cleaning" | null>(null);
  const [deleteConfirmId, setDeleteConfirmId] = useState<string | null>(null);
  const [permissionStatus, setPermissionStatus] = useState<PermissionStatus | null>(null);
  const [showOnboarding, setShowOnboarding] = useState(false);
  const [models, setModels] = useState<ModelInfo[]>([]);
  const [activeModelId, setActiveModelId] = useState<string | null>(null);
  const [modelAction, setModelAction] = useState<{
    id: string;
    type: "download" | "activate";
  } | null>(null);
  const [downloadProgress, setDownloadProgress] = useState<
    Record<string, number>
  >({});
  const isAnyDownloading = Object.keys(downloadProgress).length > 0;
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
      id: "permissions",
      label: t("pagePermissions"),
      description: t("pagePermissionsDesc"),
      icon: Shield,
    },
    {
      id: "logs",
      label: t("pageLogs"),
      description: t("pageLogsDesc"),
      icon: FileText,
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

  const refreshModels = async () => {
    try {
      const [list, active] = await Promise.all([
        invoke<ModelInfo[]>("list_models"),
        invoke<string>("get_active_model"),
      ]);
      setModels(list);
      const activeModel = list.find((m) => m.id === active);
      if (activeModel && !activeModel.downloaded) {
        const fallback = list.find((m) => m.downloaded);
        if (fallback) {
          try {
            const next = await invoke<string>("set_active_model", { modelId: fallback.id });
            setActiveModelId(next);
          } catch {
            setActiveModelId(active);
          }
        } else {
          setActiveModelId(active);
        }
      } else {
        setActiveModelId(active);
      }
    } catch (err) {
      setError(String(err));
    }
  };

  const refreshMlxDeps = async () => {
    try {
      const status = await invoke<MlxDependencyStatus>(
        "mlx_dependency_status",
      );
      setMlxDeps(status);
    } catch (err) {
      setError(String(err));
    }
  };

  useEffect(() => {
    void refreshStatus();
    void refreshLogs();
    void refreshModels();
    void refreshMlxDeps();
    void invoke<LegacyModelsInfo>("check_legacy_models").then(setLegacyModels).catch(() => {});
    void invoke<PermissionStatus>("check_all_permissions").then((status) => {
      setPermissionStatus(status);
      if (!status.inputMonitoring || status.microphone !== "granted" || !status.accessibility) {
        setShowOnboarding(true);
      }
    }).catch(() => {
      setShowOnboarding(true);
    });
    void (async () => {
      try {
        const settings = await invoke<UiSettings>("get_ui_settings");
        setUiSettings({
          ...settings,
          language: settings.language ?? detectSystemLanguage(),
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
    }, 2000);
    return () => clearInterval(timer);
  }, []);

  useEffect(() => {
    void (async () => {
      try {
        const initial = await invoke<string>("get_app_status");
        setAppStatus(initial as typeof appStatus);
      } catch (err) {
        console.warn("Failed to fetch initial app status:", err);
      }
    })();
    let unlisten: (() => void) | null = null;
    void (async () => {
      unlisten = await listen<{ status: string }>(
        "app-status-changed",
        (event) => {
          setAppStatus(event.payload.status as typeof appStatus);
        },
      );
    })();
    return () => {
      if (unlisten) {
        unlisten();
      }
    };
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
  const mlxSupported = mlxDeps?.supported ?? true;
  const mlxReady = mlxDeps?.ready ?? false;

  // Fall back to whisper tab if MLX is confirmed unsupported
  useEffect(() => {
    if (mlxDeps && !mlxDeps.supported && modelsTab === "mlx-local") {
      setModelsTab("whisper");
    }
  }, [mlxDeps, modelsTab]);
  const logsStreaming =
    activePage === "logs" || mlxAction === "install" || mlxAction === "setup";
  const whisperModels = models.filter((model) => model.engine === "whisper");
  const mlxLocalModels = models.filter(
    (model) => model.engine !== "whisper",
  );
  const cloudModels = [
    {
      id: "elevenlabs:scribe_v2",
      name: t("scribeV2"),
      size: "Cloud",
      description: t("scribeV2Desc"),
      engine: "elevenlabs",
      downloaded: true,
      downloadUrl: "",
      localPath: null,
    },
    {
      id: "elevenlabs:scribe_v2_realtime",
      name: t("scribeV2Realtime"),
      size: "Cloud",
      description: t("scribeV2RealtimeDesc"),
      engine: "elevenlabs",
      downloaded: true,
      downloadUrl: "",
      localPath: null,
    },
  ];

  useEffect(() => {
    if (activePage !== "models") {
      setModelsEditing(false);
      setDeleteConfirmId(null);
    }
  }, [activePage]);

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
          page === "permissions" ||
          page === "logs" ||
          page === "settings"
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
      unlisten = await listen<DictationStateEvent>(
        "dictation-state-changed",
        (event) => {
          setDictationState(event.payload.state);
          setDictationQueueCount(event.payload.queueLen);
        },
      );
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
      unlisten = await listen<PlaygroundTranscriptionResult>(
        "playground-transcription-result",
        (event) => {
          const { text, error: err } = event.payload;
          if (err) {
            setError(err);
          } else if (text.trim()) {
            setPlaygroundText((prev) =>
              prev ? prev + "\n" + text.trim() : text.trim(),
            );
          }
          setPlaygroundStatus("idle");
        },
      );
    })();
    return () => {
      if (unlisten) {
        unlisten();
      }
    };
  }, []);

  useEffect(() => {
    void (async () => {
      try {
        const state = await invoke<DictationStateEvent>("get_dictation_state");
        setDictationState(state.state);
        setDictationQueueCount(state.queueLen);
      } catch (err) {
        console.warn("Failed to fetch initial dictation state:", err);
      }
    })();
  }, []);

  useEffect(() => {
    let unlisten: (() => void) | null = null;
    void (async () => {
      unlisten = await listen<DownloadProgressEvent>(
        "download-progress",
        (event) => {
          const { modelId, percent, done, error } = event.payload;
          if (done) {
            setDownloadProgress((prev) => {
              const next = { ...prev };
              delete next[modelId];
              return next;
            });
            if (error) {
              setError(error);
            }
            void refreshModels();
          } else {
            setDownloadProgress((prev) => ({ ...prev, [modelId]: percent }));
          }
        },
      );
    })();
    return () => {
      if (unlisten) {
        unlisten();
      }
    };
  }, []);

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
    try {
      await invoke("open_permission_settings", { target });
    } catch (err) {
      setError(String(err));
    }
  };

  const startPlaygroundRecording = async () => {
    if (playgroundStatus === "recording") {
      return;
    }
    try {
      await invoke("start_playground_recording");
      setPlaygroundStatus("recording");
    } catch (err) {
      setError(String(err));
    }
  };

  const stopPlaygroundAndTranscribe = async () => {
    setPlaygroundStatus("transcribing");
    try {
      await invoke("stop_playground_recording");
    } catch (err) {
      setError(String(err));
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

  const handleDownloadModel = async (modelId: string) => {
    setError(null);
    const model = models.find((item) => item.id === modelId);
    if (model?.engine === "mlx" && !(mlxDeps?.ready ?? false)) {
      setError(t("runtimeRequired"));
      return;
    }
    setDownloadProgress((prev) => ({ ...prev, [modelId]: 0 }));
    try {
      await invoke("download_model", { modelId });
    } catch (err) {
      setDownloadProgress((prev) => {
        const next = { ...prev };
        delete next[modelId];
        return next;
      });
      setError(String(err));
    }
  };

  const handleInstallMlx = async () => {
    setError(null);
    setMlxAction("install");
    try {
      const status = await invoke<MlxDependencyStatus>(
        "mlx_install_dependencies",
      );
      setMlxDeps(status);
      if (status.ready) {
        setSuccessMessage(t("runtimeInstallSuccess"));
      }
    } catch (err) {
      setError(String(err));
    } finally {
      setMlxAction(null);
      void refreshMlxDeps();
    }
  };

  const handleSetupMlxModel = async (modelId: string) => {
    if (mlxDeps && !mlxDeps.supported) {
      setError(t("runtimeUnsupported"));
      return;
    }
    setError(null);
    setMlxAction("setup");
    try {
      if (!(mlxDeps?.ready ?? false)) {
        const status = await invoke<MlxDependencyStatus>("mlx_install_dependencies");
        await refreshMlxDeps();
        if (status.ready) {
          setSuccessMessage(t("runtimeInstallSuccess"));
        }
      }
      setDownloadProgress((prev) => ({ ...prev, [modelId]: 0 }));
      await invoke("download_model", { modelId });
    } catch (err) {
      setDownloadProgress((prev) => {
        const next = { ...prev };
        delete next[modelId];
        return next;
      });
      setError(String(err));
    } finally {
      setMlxAction(null);
    }
  };

  const handleResetMlxRuntime = async () => {
    setError(null);
    setMlxAction("reset");
    try {
      await invoke("mlx_reset_runtime");
      await refreshMlxDeps();
    } catch (err) {
      setError(String(err));
    } finally {
      setMlxAction(null);
    }
  };

  const handleTestElevenlabsKey = async () => {
    setElevenlabsKeyStatus("testing");
    try {
      const valid = await invoke<boolean>("test_elevenlabs_api_key", {
        apiKey: uiSettings.elevenlabsApiKey,
      });
      setElevenlabsKeyStatus(valid ? "valid" : "invalid");
      if (valid) {
        // Save settings only after successful test
        await invoke("set_ui_settings", { settings: uiSettings });
      }
    } catch {
      setElevenlabsKeyStatus("invalid");
    }
  };

  const handleActivateModel = async (modelId: string) => {
    setError(null);
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

  const handleDeleteModel = async (modelId: string) => {
    setError(null);
    setDeleteConfirmId(null);
    try {
      await invoke("delete_model", { modelId });
      await refreshModels();
    } catch (err) {
      setError(String(err));
    }
  };

  const refreshPermissions = async () => {
    try {
      const status = await invoke<PermissionStatus>("check_all_permissions");
      setPermissionStatus(status);
      return status;
    } catch {
      return null;
    }
  };

  useEffect(() => {
    if (!successMessage) return;
    const timer = setTimeout(() => setSuccessMessage(null), 5000);
    return () => clearTimeout(timer);
  }, [successMessage]);

  const allPermissionsGranted = permissionStatus
    ? permissionStatus.inputMonitoring &&
      permissionStatus.microphone === "granted" &&
      permissionStatus.accessibility
    : false;

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
                <div className={`status-pill is-${isRunning ? "ready" : "stopped"}`}>
                  <span className="status-dot" />
                  {t("gateway")}: {isRunning ? t("running") : t("stopped")}
                </div>
                <div className={`status-pill is-${dictationState === "idle" ? (mlxReady ? "ready" : "loading") : dictationState === "listening" ? "listening" : "transcribing"}`}>
                  <span className="status-dot" />
                  {dictationState === "idle" ? (mlxReady ? t("dictationIdle") : t("runtimeMissing")) : dictationState === "listening" ? t("dictationListening") : t("dictationProcessing")}
                  {dictationState !== "idle" && (
                    <button
                      className="stop-button-inline"
                      onClick={() => invoke("stop_dictation")}
                      aria-label={t("stopDictation")}
                    >
                      <Square size={8} fill="currentColor" />
                    </button>
                  )}
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
                {activePage === "models" ? (
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
            {error && (
              <div className="error-banner global-error">
                <span>{error}</span>
                <button className="error-dismiss" onClick={() => setError(null)} aria-label="Dismiss">
                  <X size={12} strokeWidth={2} />
                </button>
              </div>
            )}
            {successMessage && (
              <div className="success-banner global-error">
                <span>{successMessage}</span>
                <button className="success-dismiss" onClick={() => setSuccessMessage(null)} aria-label="Dismiss">
                  <X size={12} strokeWidth={2} />
                </button>
              </div>
            )}
            {activePage === "overview" && (
              <>
                {/* MLX Runtime */}
                <div className="card">
                  <div className="card-header">
                    <div>
                      <h2>{t("runtimeTitle")}</h2>
                      <p className="muted">{t("runtimeDesc")}</p>
                    </div>
                    <span
                      className={`runtime-status ${
                        mlxReady
                          ? "is-ready"
                          : mlxSupported
                            ? "is-missing"
                            : "is-unsupported"
                      }`}
                    >
                      {mlxReady
                        ? t("runtimeReady")
                        : mlxSupported
                          ? t("runtimeMissing")
                          : t("runtimeUnsupported")}
                    </span>
                  </div>
                  <div className="settings-group">
                    <div className="settings-row">
                      <div>
                        <div className="settings-label">
                          {t("runtimeTitle")}
                        </div>
                      </div>
                      <div className="badge-row">
                        {mlxSupported && !mlxReady && (
                          <button
                            className="button tiny primary"
                            onClick={handleInstallMlx}
                            disabled={mlxAction === "install"}
                          >
                            {mlxAction === "install"
                              ? t("runtimeInstalling")
                              : t("runtimeInstall")}
                          </button>
                        )}
                        {(mlxAction === "install" || mlxAction === "setup") && (
                          <button
                            className="button tiny ghost"
                            onClick={() => setActivePage("logs")}
                          >
                            {t("viewLogs")}
                          </button>
                        )}
                      </div>
                    </div>
                  </div>
                </div>

                {/* Model */}
                <div className="card">
                  <div className="card-header">
                    <div>
                      <h2>{t("overviewModelTitle")}</h2>
                      <p className="muted">{t("overviewModelDesc")}</p>
                    </div>
                  </div>
                  <div className="settings-group">
                    <div className="settings-row">
                      <div>
                        <div className="settings-label">
                          {t("activeModel")}
                        </div>
                        {!activeModelId && (
                          <div className="settings-hint">
                            {t("noActiveModelHint")}
                          </div>
                        )}
                      </div>
                      <div className="badge-row">
                        <span className="badge">
                          {activeModelId
                            ? (models.find((m) => m.id === activeModelId)?.name ?? activeModelId)
                            : t("noActiveModel")}
                        </span>
                        <button
                          className="button tiny"
                          onClick={() => setActivePage("models")}
                        >
                          {t("dictationShortcutChange")}
                        </button>
                      </div>
                    </div>
                  </div>
                </div>

                {/* Dictation */}
                <div className="card">
                  <div className="card-header">
                    <div>
                      <h2>{t("dictationTitle")}</h2>
                      <p className="muted">{t("dictationDesc")}</p>
                    </div>
                  </div>
                  <div className="settings-group">
                    <div className="settings-row">
                      <div>
                        <div className="settings-label">
                          {t("dictationShortcutLabel")}
                        </div>
                      </div>
                      <div className="badge-row">
                        <span className="badge">
                          {dictationCapture
                            ? t("dictationShortcutListening")
                            : formatShortcutLabel(uiSettings.dictationShortcut)}
                        </span>
                        <button
                          className="button tiny"
                          onClick={() => setDictationCapture(!dictationCapture)}
                        >
                          {dictationCapture
                            ? t("done")
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

                {/* Playground */}
                <div className="card">
                  <div className="card-header">
                    <div>
                      <h2>{t("playgroundTitle")}</h2>
                      <p className="muted">{t("playgroundDesc")}</p>
                    </div>
                    <div className="badge-row">
                      {playgroundStatus !== "idle" && (
                        <span className="badge">
                          {playgroundStatus === "recording"
                            ? t("playgroundRecording")
                            : t("playgroundTranscribing")}
                        </span>
                      )}
                    </div>
                  </div>
                  <div className="settings-group">
                    <div className="settings-row">
                      <div>
                        <div className="settings-label">
                          {t("playgroundRecord")}
                        </div>
                      </div>
                      <div className="badge-row">
                        <button
                          className="button tiny primary"
                          onClick={startPlaygroundRecording}
                          disabled={playgroundStatus !== "idle"}
                        >
                          {t("playgroundRecord")}
                        </button>
                        <button
                          className="button tiny"
                          onClick={stopPlaygroundAndTranscribe}
                          disabled={playgroundStatus !== "recording"}
                        >
                          {t("playgroundStop")}
                        </button>
                      </div>
                    </div>
                    {playgroundText && (
                      <div className="settings-row">
                        <div style={{ flex: 1, minWidth: 0 }}>
                          <div className="settings-label">
                            {t("playgroundPlaceholder")}
                          </div>
                          <div className="settings-hint">{playgroundText}</div>
                        </div>
                      </div>
                    )}
                  </div>
                </div>

                {/* Gateway (Service Control) */}
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

                {/* Endpoints */}
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
                  {mlxSupported && (
                    <button
                      className={`model-tab ${
                        modelsTab === "mlx-local" ? "is-active" : ""
                      }`}
                      onClick={() => setModelsTab("mlx-local")}
                    >
                      {t("mlxModels")}
                    </button>
                  )}
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
                      modelsTab === "cloud" ? "is-active" : ""
                    }`}
                    onClick={() => setModelsTab("cloud")}
                  >
                    {t("cloudModels")}
                  </button>
                </div>

                {modelsTab === "mlx-local" && mlxLocalModels.length > 0 && (
                  <div className="runtime-row">
                    <div>
                      <div className="runtime-title">{t("runtimeTitle")}</div>
                      <div className="muted">{t("runtimeDesc")}</div>
                    </div>
                    <div className="runtime-actions">
                      <span
                        className={`runtime-status ${
                          mlxReady
                            ? "is-ready"
                            : mlxSupported
                              ? "is-missing"
                              : "is-unsupported"
                        }`}
                      >
                        {mlxReady
                          ? t("runtimeReady")
                          : mlxSupported
                            ? t("runtimeMissing")
                            : t("runtimeUnsupported")}
                      </span>
                      {!mlxReady && mlxSupported && (
                        <button
                          className="button tiny"
                          onClick={handleInstallMlx}
                          disabled={mlxAction === "install"}
                        >
                          {mlxAction === "install"
                            ? t("runtimeInstalling")
                            : t("runtimeInstall")}
                        </button>
                      )}
                      {(mlxAction === "install" || mlxAction === "setup") && (
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
                    <div className="runtime-row" style={{ marginBottom: 12 }}>
                      <div style={{ flex: 1 }}>
                        <div className="runtime-title">{t("elevenlabsApiKey")}</div>
                        <div className="muted">{t("elevenlabsApiKeyHint")}</div>
                      </div>
                      <div style={{ display: "flex", gap: 8, alignItems: "center" }}>
                        <input
                          type="password"
                          className="input"
                          placeholder={t("elevenlabsApiKeyPlaceholder")}
                          value={uiSettings.elevenlabsApiKey}
                          onChange={(e) => {
                            const updated = { ...uiSettings, elevenlabsApiKey: e.target.value };
                            setUiSettings(updated);
                            setElevenlabsKeyStatus("unknown");
                          }}
                          style={{ width: 220 }}
                        />
                        <button
                          className="button tiny"
                          onClick={handleTestElevenlabsKey}
                          disabled={!uiSettings.elevenlabsApiKey || elevenlabsKeyStatus === "testing"}
                        >
                          {elevenlabsKeyStatus === "testing" ? t("testingApiKey") : t("testApiKey")}
                        </button>
                        <span
                          className={`runtime-status ${
                            elevenlabsKeyStatus === "valid"
                              ? "is-ready"
                              : elevenlabsKeyStatus === "invalid"
                                ? "is-missing"
                                : ""
                          }`}
                        >
                          {elevenlabsKeyStatus === "valid"
                            ? t("apiKeyValid")
                            : elevenlabsKeyStatus === "invalid"
                              ? t("apiKeyInvalid")
                              : elevenlabsKeyStatus === "testing"
                                ? t("testingApiKey")
                                : uiSettings.elevenlabsApiKey
                                  ? t("apiKeyNotVerified")
                                  : t("apiKeyNotSet")}
                        </span>
                      </div>
                    </div>
                    <div className="model-list">
                      {cloudModels.map((model) => {
                        const isActive = model.id === activeModelId;
                        const isActivating =
                          modelAction?.id === model.id && modelAction.type === "activate";
                        const hasApiKey = Boolean(uiSettings.elevenlabsApiKey);
                        return (
                          <div
                            key={model.id}
                            className={`model-row ${isActive ? "is-active" : ""}`}
                          >
                            <div className="model-info">
                              <div className="model-title">
                                <span>{model.name}</span>
                                <span className="model-size">{model.size}</span>
                              </div>
                              <div className="model-desc">{model.description}</div>
                            </div>
                            <div className="model-actions">
                              {isActive ? (
                                <span className="pill">{t("active")}</span>
                              ) : (
                                <button
                                  className="button tiny"
                                  onClick={() => handleActivateModel(model.id)}
                                  disabled={Boolean(modelAction) || !hasApiKey}
                                >
                                  {isActivating ? t("activating") : t("activate")}
                                </button>
                              )}
                            </div>
                          </div>
                        );
                      })}
                    </div>
                  </div>
                ) : (
                  <div className="model-section">
                    <div className="section-title">
                      {modelsTab === "whisper"
                        ? t("whisperModels")
                        : t("mlxModels")}
                    </div>
                    {modelsTab === "whisper" && mlxSupported && (
                      <div className="runtime-row" style={{ marginBottom: 12 }}>
                        <div className="muted">{t("mlxRecommendation")}</div>
                        <button
                          className="button tiny"
                          onClick={() => setModelsTab("mlx-local")}
                        >
                          {t("switchToMlx")}
                        </button>
                      </div>
                    )}
                    <div className="model-list">
                      {(modelsTab === "whisper" ? whisperModels : mlxLocalModels)
                        .length === 0 ? (
                        <div className="empty">{t("noModels")}</div>
                      ) : (
                        (modelsTab === "whisper" ? whisperModels : mlxLocalModels).map(
                          (model) => {
                            const isActive = model.id === activeModelId;
                            const dlPercent = downloadProgress[model.id];
                            const isDownloading = dlPercent !== undefined;
                            const isActivating =
                              modelAction?.id === model.id &&
                              modelAction.type === "activate";
                            const isMlx = model.engine === "mlx";
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
                                      ) : deleteConfirmId === model.id ? (
                                        <button
                                          className="button tiny danger"
                                          onClick={() =>
                                            handleDeleteModel(model.id)
                                          }
                                          disabled={Boolean(modelAction)}
                                        >
                                          <Trash2 size={12} strokeWidth={1.6} />
                                          {t("deleteConfirm", { name: model.name })}
                                        </button>
                                      ) : (
                                        <button
                                          className="button tiny danger"
                                          onClick={() =>
                                            setDeleteConfirmId(model.id)
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
                                        isMlx && !(mlxDeps?.ready ?? false)
                                          ? handleSetupMlxModel(model.id)
                                          : handleDownloadModel(model.id)
                                      }
                                      disabled={
                                        isAnyDownloading || mlxAction === "setup"
                                      }
                                    >
                                      {isMlx && !(mlxDeps?.ready ?? false)
                                        ? mlxAction === "setup"
                                          ? t("settingUp")
                                          : t("setup")
                                        : isDownloading
                                          ? isMlx
                                            ? t("downloading")
                                            : `${dlPercent}%`
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

            {activePage === "permissions" && (
              <>
                <div style={{ display: "flex", justifyContent: "flex-end", marginBottom: 4 }}>
                  <button
                    className="button tiny"
                    onClick={refreshPermissions}
                  >
                    {t("permissionRefreshAll")}
                  </button>
                </div>

                {/* Input Monitoring */}
                <div className="card">
                  <div className="settings-group">
                    <div className="settings-row">
                      <div>
                        <div className="settings-label">{t("inputMonitoringPermission")}</div>
                        <div className="settings-hint">{t("inputMonitoringPermissionHint")}</div>
                      </div>
                      <div className="settings-actions">
                        <span
                          className={`runtime-status ${permissionStatus?.inputMonitoring ? "is-ready" : "is-missing"}`}
                        >
                          {permissionStatus?.inputMonitoring ? t("permissionGranted") : t("permissionNotGranted")}
                        </span>
                        {!permissionStatus?.inputMonitoring && (
                          <button
                            className="button tiny primary"
                            onClick={() => invoke("open_permission_settings", { target: "input_monitoring" })}
                          >
                            {t("onboardingOpenSettings")}
                          </button>
                        )}
                        <button
                          className="button tiny"
                          onClick={refreshPermissions}
                        >
                          {t("onboardingCheckPermission")}
                        </button>
                      </div>
                    </div>
                  </div>
                </div>

                {/* Microphone */}
                <div className="card">
                  <div className="settings-group">
                    <div className="settings-row">
                      <div>
                        <div className="settings-label">{t("microphonePermission")}</div>
                        <div className="settings-hint">{t("microphonePermissionHint")}</div>
                      </div>
                      <div className="settings-actions">
                        <span
                          className={`runtime-status ${permissionStatus?.microphone === "granted" ? "is-ready" : "is-missing"}`}
                        >
                          {permissionStatus?.microphone === "granted" ? t("permissionGranted") : t("permissionNotGranted")}
                        </span>
                        {permissionStatus?.microphone !== "granted" && (
                          permissionStatus?.microphone === "denied" ? (
                            <button
                              className="button tiny primary"
                              onClick={() => handleOpenPermissionSettings("microphone")}
                            >
                              {t("openMicrophoneSettings")}
                            </button>
                          ) : (
                            <button
                              className="button tiny primary"
                              onClick={async () => {
                                await handleRequestMicrophone();
                                await refreshPermissions();
                              }}
                            >
                              {t("onboardingGrantAccess")}
                            </button>
                          )
                        )}
                        <button
                          className="button tiny"
                          onClick={refreshPermissions}
                        >
                          {t("onboardingCheckPermission")}
                        </button>
                      </div>
                    </div>
                  </div>
                </div>

                {/* Accessibility */}
                <div className="card">
                  <div className="settings-group">
                    <div className="settings-row">
                      <div>
                        <div className="settings-label">{t("accessibilityPermission")}</div>
                        <div className="settings-hint">{t("accessibilityPermissionHint")}</div>
                      </div>
                      <div className="settings-actions">
                        <span
                          className={`runtime-status ${permissionStatus?.accessibility ? "is-ready" : "is-missing"}`}
                        >
                          {permissionStatus?.accessibility ? t("permissionGranted") : t("permissionNotGranted")}
                        </span>
                        {!permissionStatus?.accessibility && (
                          <button
                            className="button tiny primary"
                            onClick={() => handleOpenPermissionSettings("accessibility")}
                          >
                            {t("onboardingOpenSettings")}
                          </button>
                        )}
                        <button
                          className="button tiny"
                          onClick={refreshPermissions}
                        >
                          {t("onboardingCheckPermission")}
                        </button>
                      </div>
                    </div>
                  </div>
                </div>
              </>
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
                        className={`status-chip ${mlxReady ? "is-online" : ""}`}
                      >
                        {mlxReady
                          ? t("runtimeReady")
                          : mlxSupported
                            ? t("runtimeMissing")
                            : t("runtimeUnsupported")}
                      </span>
                    </div>
                    <div className="settings-row">
                      <div>
                        <div className="settings-label">{t("runtimeInstall")}</div>
                        <div className="settings-hint">
                          {mlxDeps?.python ?? "python3"}
                        </div>
                      </div>
                      <button
                        className="button tiny"
                        onClick={handleInstallMlx}
                        disabled={mlxAction === "install" || !mlxSupported || mlxReady}
                      >
                        {mlxAction === "install"
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
                        onClick={handleResetMlxRuntime}
                        disabled={mlxAction === "reset" || !mlxSupported}
                      >
                        {mlxAction === "reset"
                          ? t("runtimeResetting")
                          : t("runtimeReset")}
                      </button>
                    </div>
                    {legacyModels?.found && (
                      <div className="settings-row">
                        <div>
                          <div className="settings-label">{t("legacyTitle")}</div>
                          <div className="settings-hint">
                            {t("legacyFound", {
                              size: legacyModels.sizeBytes >= 1073741824
                                ? `${(legacyModels.sizeBytes / 1073741824).toFixed(1)} GB`
                                : `${Math.round(legacyModels.sizeBytes / 1048576)} MB`,
                            })}
                          </div>
                        </div>
                        <button
                          className="button tiny"
                          onClick={async () => {
                            setLegacyAction("cleaning");
                            try {
                              await invoke("clean_legacy_models");
                              setLegacyModels({ found: false, path: legacyModels.path, sizeBytes: 0 });
                            } catch (err) {
                              setError(String(err));
                            } finally {
                              setLegacyAction(null);
                            }
                          }}
                          disabled={legacyAction === "cleaning"}
                        >
                          {legacyAction === "cleaning"
                            ? t("legacyCleaning")
                            : t("legacyClean")}
                        </button>
                      </div>
                    )}
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

      {showOnboarding && (
        <div className="onboarding-overlay">
          <div className="onboarding-dialog">
            <h2 className="onboarding-title">{t("onboardingTitle")}</h2>
            <p className="onboarding-subtitle">{t("onboardingSubtitle")}</p>

            <div className="onboarding-steps">
              {/* Step 1: Input Monitoring */}
              <div className={`onboarding-step ${permissionStatus?.inputMonitoring ? "is-granted" : ""}`}>
                <div className="onboarding-step-header">
                  <div className="onboarding-step-number">1</div>
                  <div>
                    <div className="onboarding-step-title">{t("onboardingInputMonitoring")}</div>
                    <div className="onboarding-step-desc">{t("onboardingInputMonitoringDesc")}</div>
                  </div>
                  {permissionStatus?.inputMonitoring && (
                    <span className="onboarding-check"><Check size={14} /></span>
                  )}
                </div>
                {!permissionStatus?.inputMonitoring && (
                  <div className="onboarding-step-actions">
                    <button
                      className="button tiny primary"
                      onClick={() => invoke("open_permission_settings", { target: "input_monitoring" })}
                    >
                      {t("onboardingOpenSettings")}
                    </button>
                    <button
                      className="button tiny"
                      onClick={refreshPermissions}
                    >
                      {t("onboardingCheckPermission")}
                    </button>
                  </div>
                )}
              </div>

              {/* Step 2: Microphone */}
              <div className={`onboarding-step ${permissionStatus?.microphone === "granted" ? "is-granted" : ""}`}>
                <div className="onboarding-step-header">
                  <div className="onboarding-step-number">2</div>
                  <div>
                    <div className="onboarding-step-title">{t("onboardingMicrophone")}</div>
                    <div className="onboarding-step-desc">{t("onboardingMicrophoneDesc")}</div>
                  </div>
                  {permissionStatus?.microphone === "granted" && (
                    <span className="onboarding-check"><Check size={14} /></span>
                  )}
                </div>
                {permissionStatus?.microphone !== "granted" && (
                  <div className="onboarding-step-actions">
                    {permissionStatus?.microphone === "denied" ? (
                      <button
                        className="button tiny primary"
                        onClick={() => invoke("open_permission_settings", { target: "microphone" })}
                      >
                        {t("onboardingOpenSettings")}
                      </button>
                    ) : (
                      <button
                        className="button tiny primary"
                        onClick={async () => {
                          try {
                            const stream = await navigator.mediaDevices.getUserMedia({ audio: true });
                            stream.getTracks().forEach((track) => track.stop());
                          } catch { /* user denied */ }
                          await refreshPermissions();
                        }}
                      >
                        {t("onboardingGrantAccess")}
                      </button>
                    )}
                    <button
                      className="button tiny"
                      onClick={refreshPermissions}
                    >
                      {t("onboardingCheckPermission")}
                    </button>
                  </div>
                )}
              </div>

              {/* Step 3: Accessibility */}
              <div className={`onboarding-step ${permissionStatus?.accessibility ? "is-granted" : ""}`}>
                <div className="onboarding-step-header">
                  <div className="onboarding-step-number">3</div>
                  <div>
                    <div className="onboarding-step-title">{t("onboardingAccessibility")}</div>
                    <div className="onboarding-step-desc">{t("onboardingAccessibilityDesc")}</div>
                  </div>
                  {permissionStatus?.accessibility && (
                    <span className="onboarding-check"><Check size={14} /></span>
                  )}
                </div>
                {!permissionStatus?.accessibility && (
                  <div className="onboarding-step-actions">
                    <button
                      className="button tiny primary"
                      onClick={() => invoke("open_permission_settings", { target: "accessibility" })}
                    >
                      {t("onboardingOpenSettings")}
                    </button>
                    <button
                      className="button tiny"
                      onClick={refreshPermissions}
                    >
                      {t("onboardingCheckPermission")}
                    </button>
                  </div>
                )}
              </div>
            </div>

            <div className="onboarding-footer">
              {allPermissionsGranted ? (
                <>
                  <div className="onboarding-all-done">{t("onboardingAllDone")}</div>
                  <button
                    className="button primary"
                    onClick={() => invoke("restart_app")}
                  >
                    {t("onboardingRestartApp")}
                  </button>
                </>
              ) : (
                <button
                  className="button ghost"
                  onClick={() => setShowOnboarding(false)}
                >
                  {t("onboardingSkip")}
                </button>
              )}
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

export default App;
