mod audio;
mod dictation;
pub mod elevenlabs_realtime;
mod models;
mod recording;

use axum::{
    extract::{Multipart, State as AxumState},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
#[cfg(target_os = "macos")]
use core_foundation::runloop::{kCFRunLoopCommonModes, CFRunLoop};
#[cfg(target_os = "macos")]
use core_graphics::event::{
    CGEventFlags, CGEventTap, CGEventTapLocation, CGEventTapOptions, CGEventTapPlacement,
    CGEventType, EventField, KeyCode,
};
use flate2::read::GzDecoder;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use std::{
    io::BufRead,
    net::SocketAddr,
    path::{Path, PathBuf},
    process::Stdio,
    str::FromStr,
    sync::Arc,
    thread,
    time::{Instant, SystemTime, UNIX_EPOCH},
};
use tar::Archive;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{TrayIconBuilder, TrayIconEvent};
use tauri::{Emitter, Manager, State as TauriState};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};
use tokio::{
    io::AsyncWriteExt,
    process::Command,
    sync::{oneshot, Mutex},
    time::{sleep, Duration},
};
use whisper_rs::{
    FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters, WhisperState,
};

#[derive(Clone)]
struct AppState {
    runtime: Arc<Mutex<Option<ServerRuntime>>>,
    port: Arc<Mutex<u16>>,
    started_at: Arc<Mutex<Option<u64>>>,
    requests: Arc<Mutex<u64>>,
    logs: Arc<LogStore>,
    ui_settings: Arc<Mutex<UiSettings>>,
    settings_path: Arc<Mutex<Option<PathBuf>>>,
    models_dir: Arc<Mutex<Option<PathBuf>>>,
    config_path: Arc<Mutex<Option<PathBuf>>>,
    active_model_id: Arc<Mutex<String>>,
    cached_context: Arc<Mutex<Option<CachedWhisperContext>>>,
    mlx_sidecar: Arc<Mutex<Option<MlxSidecar>>>,
    mlx_ready: Arc<Mutex<bool>>,

    app_handle: Arc<Mutex<Option<tauri::AppHandle>>>,
    tray_snapshot: Arc<Mutex<Option<TraySnapshot>>>,
    dictation_shortcut: Arc<Mutex<Option<Shortcut>>>,
    dictation_tray_state: Arc<Mutex<DictationTrayState>>,
    dictation: Arc<dictation::DictationManager>,
    downloading: Arc<Mutex<bool>>,
    app_status: Arc<Mutex<AppStatus>>,
}

struct ServerRuntime {
    shutdown: oneshot::Sender<()>,
    handle: tauri::async_runtime::JoinHandle<()>,
}

struct LogStore {
    state: Mutex<LogState>,
    max_entries: usize,
    log_path: Mutex<Option<PathBuf>>,
}

struct MlxSidecar {
    model_id: String,
    port: u16,
    child: tokio::process::Child,
}

const TRAY_ID: &str = "openstt-tray";
const TRAY_OPEN: &str = "tray-open";
const TRAY_START: &str = "tray-start";
const TRAY_STOP: &str = "tray-stop";
const TRAY_SETTINGS: &str = "tray-settings";
const TRAY_LOGS: &str = "tray-logs";
const TRAY_QUIT: &str = "tray-quit";

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PermissionStatus {
    accessibility: bool,
    microphone: String,
    input_monitoring: bool,
}

#[cfg(target_os = "macos")]
extern "C" {
    fn AXIsProcessTrusted() -> bool;
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct MlxDependencyStatus {
    supported: bool,
    ready: bool,
    python: Option<String>,
    venv: bool,
    mlx_audio: bool,
    message: Option<String>,
}

struct CachedWhisperContext {
    model_id: String,
    context: Arc<WhisperContext>,
    state: WhisperState,
}

struct LogState {
    next_id: u64,
    entries: Vec<LogEntry>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct LogEntry {
    id: u64,
    timestamp: u64,
    level: String,
    message: String,
}

#[derive(Clone, PartialEq)]
struct TraySnapshot {
    running: bool,
    port: u16,
    model_id: String,
    dictation_state: String,
    dictation_queue_len: u32,
    dictation_elapsed: Option<String>,
}

#[derive(Clone)]
struct DictationTrayState {
    state: String,
    queue_len: u32,
    phase_started: Option<Instant>,
}

impl Default for DictationTrayState {
    fn default() -> Self {
        Self {
            state: "idle".to_string(),
            queue_len: 0,
            phase_started: None,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
enum AppStatus {
    Stopped,
    Loading,
    Ready,
    Listening,
    Transcribing,
}

impl AppStatus {
    fn as_str(self) -> &'static str {
        match self {
            AppStatus::Stopped => "stopped",
            AppStatus::Loading => "loading",
            AppStatus::Ready => "ready",
            AppStatus::Listening => "listening",
            AppStatus::Transcribing => "transcribing",
        }
    }
}

#[derive(Serialize, Clone)]
struct AppStatusEvent {
    status: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ServerStatus {
    running: bool,
    port: u16,
    url: Option<String>,
    started_at: Option<u64>,
    requests: u64,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase", default)]
struct UiSettings {
    reduced_transparency: bool,
    language: String,
    dictation_shortcut: DictationShortcut,
    dictation_auto_paste: bool,
    elevenlabs_api_key: String,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase", default)]
struct DictationShortcut {
    key: String,
    modifiers: Vec<String>,
}

impl Default for DictationShortcut {
    fn default() -> Self {
        Self {
            key: "AltLeft".to_string(),
            modifiers: Vec::new(),
        }
    }
}

impl Default for UiSettings {
    fn default() -> Self {
        Self {
            reduced_transparency: false,
            language: "en".to_string(),
            dictation_shortcut: DictationShortcut::default(),
            dictation_auto_paste: true,
            elevenlabs_api_key: String::new(),
        }
    }
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase", default)]
struct AppConfig {
    active_model_id: String,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            active_model_id: default_model_id(),
        }
    }
}

#[derive(Serialize)]
struct HealthResponse {
    status: &'static str,
}

#[derive(Serialize)]
struct TranscriptionResponse {
    text: String,
}

#[derive(Deserialize)]
struct MlxTranscriptionResponse {
    text: String,
}

pub(crate) struct TranscribeError {
    pub(crate) message: String,
}

impl TranscribeError {
    fn bad_request(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    fn internal(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TranscribeAudioRequest {
    bytes: Vec<u8>,
    file_name: Option<String>,
    model_id: Option<String>,
    language: Option<String>,
}

impl LogStore {
    fn new(max_entries: usize) -> Self {
        Self {
            state: Mutex::new(LogState {
                next_id: 1,
                entries: Vec::new(),
            }),
            max_entries,
            log_path: Mutex::new(None),
        }
    }

    async fn push(&self, level: &str, message: impl Into<String>) {
        let timestamp = now_millis();
        let message = message.into();
        let mut guard = self.state.lock().await;
        let entry = LogEntry {
            id: guard.next_id,
            timestamp,
            level: level.to_string(),
            message: message.clone(),
        };
        guard.next_id += 1;
        guard.entries.insert(0, entry);
        if guard.entries.len() > self.max_entries {
            guard.entries.truncate(self.max_entries);
        }
        drop(guard);

        let log_path = self.log_path.lock().await.clone();
        if let Some(path) = log_path {
            let record = serde_json::json!({
                "timestamp": timestamp,
                "level": level,
                "message": message,
            });
            if let Ok(line) = serde_json::to_string(&record) {
                if let Ok(mut file) = tokio::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&path)
                    .await
                {
                    let _ = file.write_all(line.as_bytes()).await;
                    let _ = file.write_all(b"\n").await;
                }
            }
        }
    }

    async fn list(&self) -> Vec<LogEntry> {
        let guard = self.state.lock().await;
        guard.entries.clone()
    }

    async fn clear(&self) {
        let mut guard = self.state.lock().await;
        guard.entries.clear();
        guard.next_id = 1;
        drop(guard);

        let log_path = self.log_path.lock().await.clone();
        if let Some(path) = log_path {
            let _ = tokio::fs::remove_file(path).await;
        }
    }

    fn set_log_path(&self, path: PathBuf) {
        let mut guard = self.log_path.blocking_lock();
        *guard = Some(path);
    }

    fn load_from_file(&self, path: &Path) {
        let file = match std::fs::File::open(path) {
            Ok(file) => file,
            Err(_) => return,
        };
        let reader = std::io::BufReader::new(file);
        let mut entries = std::collections::VecDeque::new();
        let mut next_id = 1u64;

        for line in reader.lines().flatten() {
            let record = serde_json::from_str::<serde_json::Value>(&line).ok();
            let (timestamp, level, message) = match record {
                Some(value) => (
                    value.get("timestamp").and_then(|v| v.as_u64()).unwrap_or(0),
                    value
                        .get("level")
                        .and_then(|v| v.as_str())
                        .unwrap_or("info")
                        .to_string(),
                    value
                        .get("message")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                ),
                None => continue,
            };
            let entry = LogEntry {
                id: next_id,
                timestamp,
                level,
                message,
            };
            next_id += 1;
            entries.push_back(entry);
            if entries.len() > self.max_entries {
                entries.pop_front();
            }
        }

        let mut guard = self.state.blocking_lock();
        guard.entries = entries.into_iter().rev().collect();
        guard.next_id = next_id;
    }
}

impl AppState {
    fn new() -> Self {
        Self {
            runtime: Arc::new(Mutex::new(None)),
            port: Arc::new(Mutex::new(8787)),
            started_at: Arc::new(Mutex::new(None)),
            requests: Arc::new(Mutex::new(0)),
            logs: Arc::new(LogStore::new(1000)),
            ui_settings: Arc::new(Mutex::new(UiSettings::default())),
            settings_path: Arc::new(Mutex::new(None)),
            models_dir: Arc::new(Mutex::new(None)),
            config_path: Arc::new(Mutex::new(None)),
            active_model_id: Arc::new(Mutex::new(default_model_id())),
            cached_context: Arc::new(Mutex::new(None)),
            mlx_sidecar: Arc::new(Mutex::new(None)),
            mlx_ready: Arc::new(Mutex::new(false)),

            app_handle: Arc::new(Mutex::new(None)),
            tray_snapshot: Arc::new(Mutex::new(None)),
            dictation_shortcut: Arc::new(Mutex::new(None)),
            dictation_tray_state: Arc::new(Mutex::new(DictationTrayState::default())),
            dictation: Arc::new(dictation::DictationManager::new()),
            downloading: Arc::new(Mutex::new(false)),
            app_status: Arc::new(Mutex::new(AppStatus::Stopped)),
        }
    }
}

fn default_model_id() -> String {
    std::env::var("OPENSTT_DEFAULT_MODEL").unwrap_or_else(|_| "base".to_string())
}

fn normalize_model_id(model_id: &str) -> String {
    model_id.to_string()
}

fn is_modifier_code(code: Code) -> bool {
    matches!(
        code,
        Code::AltLeft
            | Code::AltRight
            | Code::ShiftLeft
            | Code::ShiftRight
            | Code::ControlLeft
            | Code::ControlRight
            | Code::MetaLeft
            | Code::MetaRight
    )
}

fn parse_dictation_shortcut(shortcut: &DictationShortcut) -> Result<Shortcut, String> {
    let key_value = if shortcut.key.trim().is_empty() {
        "AltLeft"
    } else {
        shortcut.key.trim()
    };
    let key =
        Code::from_str(key_value).map_err(|_| format!("Unknown shortcut key: {key_value}"))?;
    let mut mods = Modifiers::empty();
    for modifier in &shortcut.modifiers {
        match modifier.trim().to_lowercase().as_str() {
            "alt" | "option" => mods |= Modifiers::ALT,
            "shift" => mods |= Modifiers::SHIFT,
            "control" | "ctrl" => mods |= Modifiers::CONTROL,
            "meta" | "command" | "cmd" | "super" => mods |= Modifiers::SUPER,
            _ => {}
        }
    }
    let mods = if mods.is_empty() || is_modifier_code(key) {
        None
    } else {
        Some(mods)
    };
    Ok(Shortcut::new(mods, key))
}

fn is_modifier_only_shortcut(shortcut: &DictationShortcut) -> bool {
    let key_value = if shortcut.key.trim().is_empty() {
        "AltLeft"
    } else {
        shortcut.key.trim()
    };
    let key = Code::from_str(key_value);
    shortcut.modifiers.is_empty() && key.map(is_modifier_code).unwrap_or(false)
}

#[cfg(target_os = "macos")]
fn modifier_keycode_and_flag(shortcut: &DictationShortcut) -> Option<(u16, CGEventFlags)> {
    if !shortcut.modifiers.is_empty() {
        return None;
    }
    let key_value = if shortcut.key.trim().is_empty() {
        "AltLeft"
    } else {
        shortcut.key.trim()
    };
    match key_value {
        "AltLeft" => Some((KeyCode::OPTION, CGEventFlags::CGEventFlagAlternate)),
        "AltRight" => Some((KeyCode::RIGHT_OPTION, CGEventFlags::CGEventFlagAlternate)),
        "ShiftLeft" => Some((KeyCode::SHIFT, CGEventFlags::CGEventFlagShift)),
        "ShiftRight" => Some((KeyCode::RIGHT_SHIFT, CGEventFlags::CGEventFlagShift)),
        "ControlLeft" => Some((KeyCode::CONTROL, CGEventFlags::CGEventFlagControl)),
        "ControlRight" => Some((KeyCode::RIGHT_CONTROL, CGEventFlags::CGEventFlagControl)),
        "MetaLeft" => Some((KeyCode::COMMAND, CGEventFlags::CGEventFlagCommand)),
        "MetaRight" => Some((KeyCode::RIGHT_COMMAND, CGEventFlags::CGEventFlagCommand)),
        _ => None,
    }
}

#[cfg(target_os = "macos")]
fn start_modifier_event_tap(app: tauri::AppHandle, state: AppState) {
    thread::spawn(move || {
        let logs = state.logs.clone();
        let tap = CGEventTap::new(
            CGEventTapLocation::HID,
            CGEventTapPlacement::HeadInsertEventTap,
            CGEventTapOptions::Default,
            vec![CGEventType::FlagsChanged],
            move |_proxy, event_type, event| {
                if !matches!(event_type, CGEventType::FlagsChanged) {
                    return Some(event.clone());
                }
                let shortcut = state.ui_settings.blocking_lock().dictation_shortcut.clone();
                let Some((target_key, target_flag)) = modifier_keycode_and_flag(&shortcut) else {
                    return Some(event.clone());
                };
                let keycode =
                    event.get_integer_value_field(EventField::KEYBOARD_EVENT_KEYCODE) as u16;
                if keycode != target_key {
                    return Some(event.clone());
                }
                let flags = event.get_flags();
                let is_down = flags.contains(target_flag);
                let app_state = state.clone();
                let app_handle = app.clone();
                if is_down {
                    tauri::async_runtime::spawn(async move {
                        if let Err(err) = start_dictation_inner(&app_state, &app_handle).await {
                            app_state
                                .logs
                                .push("error", format!("Dictation start failed: {err}"))
                                .await;
                            return;
                        }
                        app_state.dictation.emit_state(&app_handle).await;
                        let mut tray = app_state.dictation_tray_state.lock().await;
                        tray.state = "listening".to_string();
                        tray.queue_len = app_state.dictation.queue_len().await;
                        tray.phase_started = Some(Instant::now());
                        drop(tray);
                        refresh_tray(&app_state).await;
                        recompute_and_emit_app_status(&app_state).await;
                    });
                } else {
                    eprintln!("[lib] modifier event tap: key released");
                    tauri::async_runtime::spawn(async move {
                        if let Err(err) = stop_dictation_inner(&app_state, &app_handle).await {
                            app_state
                                .logs
                                .push("error", format!("Dictation stop failed: {err}"))
                                .await;
                        }
                    });
                }
                Some(event.clone())
            },
        );

        let tap = match tap {
            Ok(tap) => tap,
            Err(_) => {
                tauri::async_runtime::spawn(async move {
                    logs.push(
                        "error",
                        "Failed to start modifier key listener. Enable Input Monitoring for OpenSTT in System Settings.".to_string(),
                    )
                    .await;
                });
                return;
            }
        };

        if let Ok(source) = tap.mach_port.create_runloop_source(0) {
            let run_loop = CFRunLoop::get_current();
            unsafe {
                run_loop.add_source(&source, kCFRunLoopCommonModes);
            }
            tap.enable();
            CFRunLoop::run_current();
        } else {
            tauri::async_runtime::spawn(async move {
                logs.push("error", "Failed to attach modifier key listener.")
                    .await;
            });
        }
    });
}

fn show_main_window<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.set_focus();
    }
}

fn open_page<R: tauri::Runtime>(app: &tauri::AppHandle<R>, page: &str) {
    show_main_window(app);
    let _ = app.emit("open-page", page);
}

fn format_dictation_state(state: &str, elapsed: &Option<String>) -> String {
    let label = match state {
        "listening" => "Listening",
        "processing" => "Processing",
        _ => return "Idle".to_string(),
    };
    match elapsed {
        Some(e) => format!("{label} {e}"),
        None => label.to_string(),
    }
}

fn tray_tooltip(snapshot: &TraySnapshot) -> String {
    let status = if snapshot.running {
        "Running"
    } else {
        "Stopped"
    };
    format!("OpenSTT - {status} - {}", snapshot.model_id)
}

fn build_tray_menu<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    snapshot: &TraySnapshot,
) -> Result<Menu<R>, tauri::Error> {
    let open_item = MenuItem::with_id(app, TRAY_OPEN, "Open OpenSTT", true, None::<String>)?;
    let start_item = MenuItem::with_id(
        app,
        TRAY_START,
        "Start Gateway",
        !snapshot.running,
        None::<String>,
    )?;
    let stop_item = MenuItem::with_id(
        app,
        TRAY_STOP,
        "Stop Gateway",
        snapshot.running,
        None::<String>,
    )?;
    let settings_item =
        MenuItem::with_id(app, TRAY_SETTINGS, "Open Settings", true, None::<String>)?;
    let logs_item = MenuItem::with_id(app, TRAY_LOGS, "Open Logs", true, None::<String>)?;
    let model_item = MenuItem::with_id(
        app,
        "tray-model",
        format!("Model: {}", snapshot.model_id),
        false,
        None::<String>,
    )?;
    let status_item = MenuItem::with_id(
        app,
        "tray-status",
        if snapshot.running {
            "Gateway: Running"
        } else {
            "Gateway: Stopped"
        },
        false,
        None::<String>,
    )?;
    let port_item = MenuItem::with_id(
        app,
        "tray-port",
        format!("Port: {}", snapshot.port),
        false,
        None::<String>,
    )?;
    let dictation_item = MenuItem::with_id(
        app,
        "tray-dictation",
        format!(
            "Dictation: {}",
            format_dictation_state(&snapshot.dictation_state, &snapshot.dictation_elapsed)
        ),
        false,
        None::<String>,
    )?;
    let dictation_queue_item = MenuItem::with_id(
        app,
        "tray-dictation-queue",
        format!("Dictation queue: {}", snapshot.dictation_queue_len),
        false,
        None::<String>,
    )?;
    let separator = PredefinedMenuItem::separator(app)?;
    let quit_item = MenuItem::with_id(app, TRAY_QUIT, "Quit OpenSTT", true, None::<String>)?;

    let menu = Menu::new(app)?;
    menu.append(&open_item)?;
    menu.append(&settings_item)?;
    menu.append(&logs_item)?;
    menu.append(&model_item)?;
    menu.append(&status_item)?;
    menu.append(&port_item)?;
    menu.append(&dictation_item)?;
    menu.append(&dictation_queue_item)?;
    menu.append(&separator)?;
    menu.append(&start_item)?;
    menu.append(&stop_item)?;
    menu.append(&separator)?;
    menu.append(&quit_item)?;
    Ok(menu)
}

async fn tray_snapshot(state: &AppState) -> TraySnapshot {
    let running = state.runtime.lock().await.is_some();
    let port = *state.port.lock().await;
    let model_id = state.active_model_id.lock().await.clone();
    let dictation = state.dictation_tray_state.lock().await.clone();
    let dictation_elapsed = dictation
        .phase_started
        .map(|started| format!("{:.1}s", started.elapsed().as_secs_f64()));
    TraySnapshot {
        running,
        port,
        model_id,
        dictation_state: dictation.state,
        dictation_queue_len: dictation.queue_len,
        dictation_elapsed,
    }
}

async fn refresh_tray(state: &AppState) {
    let app_handle = state.app_handle.lock().await.clone();
    let Some(app_handle) = app_handle else {
        return;
    };
    let snapshot = tray_snapshot(state).await;
    {
        let mut last_snapshot = state.tray_snapshot.lock().await;
        if last_snapshot.as_ref() == Some(&snapshot) {
            return;
        }
        *last_snapshot = Some(snapshot.clone());
    }
    let menu = match build_tray_menu(&app_handle, &snapshot) {
        Ok(menu) => menu,
        Err(_) => return,
    };
    if let Some(tray) = app_handle.tray_by_id(TRAY_ID) {
        let _ = tray.set_menu(Some(menu));
        let _ = tray.set_tooltip(Some(tray_tooltip(&snapshot)));
        let title = match snapshot.dictation_state.as_str() {
            "listening" | "processing" => {
                format_dictation_state(&snapshot.dictation_state, &snapshot.dictation_elapsed)
            }
            _ => String::new(),
        };
        let _ = tray.set_title(Some(&title));
    }
}

async fn register_dictation_shortcut(
    app: &tauri::AppHandle,
    state: &AppState,
    shortcut: &DictationShortcut,
) -> Result<(), String> {
    let mut current = state.dictation_shortcut.lock().await;
    if is_modifier_only_shortcut(shortcut) {
        if let Some(existing) = current.take() {
            let _ = app.global_shortcut().unregister(existing);
        }
        return Ok(());
    }
    let parsed = parse_dictation_shortcut(shortcut)?;
    if current.as_ref().map(|item| item.id()) == Some(parsed.id()) {
        return Ok(());
    }
    if let Some(existing) = current.take() {
        let _ = app.global_shortcut().unregister(existing);
    }
    app.global_shortcut()
        .register(parsed)
        .map_err(|err| format!("Failed to register shortcut: {err}"))?;
    *current = Some(parsed);
    Ok(())
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

async fn emit_app_status(state: &AppState, status: AppStatus) {
    *state.app_status.lock().await = status;
    if let Some(handle) = state.app_handle.lock().await.clone() {
        let _ = handle.emit(
            "app-status-changed",
            AppStatusEvent {
                status: status.as_str().to_string(),
            },
        );
    }
}

async fn recompute_and_emit_app_status(state: &AppState) {
    let dictation_state = state.dictation.current_state();
    let status = match dictation_state {
        dictation::DictationState::Listening => AppStatus::Listening,
        dictation::DictationState::Processing => AppStatus::Transcribing,
        dictation::DictationState::Idle => {
            let mlx_ok = *state.mlx_ready.lock().await;
            if state.runtime.lock().await.is_some() && mlx_ok {
                AppStatus::Ready
            } else if state.runtime.lock().await.is_some() {
                AppStatus::Loading
            } else {
                AppStatus::Stopped
            }
        }
    };
    emit_app_status(state, status).await;
}

fn is_realtime_model(model_id: &str) -> bool {
    model_id == "elevenlabs:scribe_v2_realtime"
}

async fn start_dictation_inner(
    state: &AppState,
    app_handle: &tauri::AppHandle,
) -> Result<(), String> {
    let model_id = state.active_model_id.lock().await.clone();
    eprintln!("[lib] start_dictation_inner, model_id: {}", model_id);

    if is_realtime_model(&model_id) {
        eprintln!("[lib] is realtime model, starting realtime");
        let settings = state.ui_settings.lock().await;
        let api_key = settings.elevenlabs_api_key.clone();
        let language = settings.language.clone();
        drop(settings);

        if api_key.is_empty() {
            return Err("ElevenLabs API key not configured".to_string());
        }

        state
            .dictation
            .start_realtime(&api_key, Some(language), app_handle.clone())
            .await
    } else {
        eprintln!("[lib] not realtime model, starting normal recording");
        state.dictation.start_recording()
    }
}

async fn stop_dictation_inner(
    state: &AppState,
    app_handle: &tauri::AppHandle,
) -> Result<(), String> {
    eprintln!("[lib] stop_dictation_inner called");
    let is_realtime = state.dictation.is_realtime_active().await;
    eprintln!("[lib] is_realtime_active returned: {}", is_realtime);
    if is_realtime {
        eprintln!("[lib] calling stop_realtime");
        state.dictation.stop_realtime().await?;
        eprintln!("[lib] stop_realtime completed");
        // Update UI state
        state.dictation.emit_state(app_handle).await;
        {
            let mut tray = state.dictation_tray_state.lock().await;
            tray.state = "idle".to_string();
            tray.phase_started = None;
            tray.queue_len = 0;
        }
        refresh_tray(state).await;
        recompute_and_emit_app_status(state).await;
        Ok(())
    } else {
        eprintln!("[lib] not realtime, calling stop_recording");
        state.dictation.stop_recording().await?;
        // Process queue for non-realtime mode
        state.dictation.emit_state(app_handle).await;
        let has_queue = state.dictation.queue_len().await > 0;
        {
            let mut tray = state.dictation_tray_state.lock().await;
            if has_queue {
                tray.state = "processing".to_string();
                tray.phase_started = Some(Instant::now());
            } else {
                tray.state = "idle".to_string();
                tray.phase_started = None;
            }
            tray.queue_len = state.dictation.queue_len().await;
        }
        refresh_tray(state).await;
        recompute_and_emit_app_status(state).await;
        state.dictation.process_queue(state, app_handle).await;
        {
            let mut tray = state.dictation_tray_state.lock().await;
            tray.state = "idle".to_string();
            tray.phase_started = None;
            tray.queue_len = 0;
        }
        refresh_tray(state).await;
        recompute_and_emit_app_status(state).await;
        Ok(())
    }
}

fn openstt_dir() -> PathBuf {
    let home = std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::current_dir().unwrap_or_else(|_| ".".into()));
    home.join(".openstt")
}

fn settings_path() -> PathBuf {
    openstt_dir().join("settings.json")
}

fn config_path() -> PathBuf {
    openstt_dir().join("state.json")
}

fn models_dir() -> PathBuf {
    openstt_dir().join("models")
}

fn logs_path() -> PathBuf {
    openstt_dir().join("logs").join("openstt.log")
}

fn mlx_cache_dir() -> PathBuf {
    models_dir().join("mlx").join("cache")
}

fn mlx_sidecar_script() -> PathBuf {
    if let Ok(path) = std::env::var("OPENSTT_MLX_SIDECAR") {
        return PathBuf::from(path);
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("sidecar")
        .join("mlx_stt.py")
}

const STANDALONE_PYTHON_URL: &str =
    "https://github.com/astral-sh/python-build-standalone/releases/download/\
     20260127/cpython-3.12.12+20260127-aarch64-apple-darwin-install_only.tar.gz";

fn standalone_python_dir() -> PathBuf {
    openstt_dir().join("python")
}

fn standalone_python_path() -> PathBuf {
    standalone_python_dir().join("bin").join("python3")
}

fn python_command() -> String {
    if let Ok(path) = std::env::var("OPENSTT_PYTHON") {
        return path;
    }
    let venv_python = venv_python_path();
    if venv_python.exists() {
        return venv_python.to_string_lossy().to_string();
    }
    let standalone = standalone_python_path();
    if standalone.exists() {
        return standalone.to_string_lossy().to_string();
    }
    "python3".to_string()
}

fn venv_dir() -> PathBuf {
    openstt_dir().join("venv")
}

fn venv_python_path() -> PathBuf {
    venv_dir().join("bin").join("python3")
}

fn mlx_supported() -> bool {
    matches!(std::env::consts::ARCH, "aarch64" | "arm64") && std::env::consts::OS == "macos"
}

async fn run_python_check(python: &str, args: &[&str]) -> bool {
    Command::new(python)
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await
        .map(|status| status.success())
        .unwrap_or(false)
}

async fn mlx_dependency_status_inner() -> MlxDependencyStatus {
    if !mlx_supported() {
        return MlxDependencyStatus {
            supported: false,
            ready: false,
            python: None,
            venv: venv_python_path().exists(),
            mlx_audio: false,
            message: Some("MLX runtime requires Apple Silicon".to_string()),
        };
    }

    let python = python_command();
    let python_ok = run_python_check(&python, &["-c", "import sys"]).await;
    let mlx_audio = if python_ok {
        run_python_check(&python, &["-c", "import mlx_audio"]).await
    } else {
        false
    };

    MlxDependencyStatus {
        supported: true,
        ready: python_ok && mlx_audio,
        python: python_ok.then(|| python),
        venv: venv_python_path().exists(),
        mlx_audio,
        message: if python_ok {
            None
        } else {
            Some("Python not available".to_string())
        },
    }
}

#[tauri::command]
async fn mlx_dependency_status(
    state: TauriState<'_, AppState>,
) -> Result<MlxDependencyStatus, String> {
    let status = mlx_dependency_status_inner().await;
    *state.mlx_ready.lock().await = status.ready;
    recompute_and_emit_app_status(&state).await;
    Ok(status)
}

async fn download_standalone_python(logs: &LogStore) -> Result<(), String> {
    let python_dir = standalone_python_dir();
    // Clean up partial install if binary is missing
    if python_dir.exists() && !standalone_python_path().exists() {
        tokio::fs::remove_dir_all(&python_dir)
            .await
            .map_err(|e| format!("Failed to remove partial python dir: {e}"))?;
    }

    logs.push("info", "Downloading standalone Python…").await;

    let response = reqwest::get(STANDALONE_PYTHON_URL)
        .await
        .map_err(|e| format!("Failed to download Python: {e}"))?;

    if !response.status().is_success() {
        return Err(format!(
            "Failed to download Python: HTTP {}",
            response.status()
        ));
    }

    let total_size = response.content_length();
    let mut stream = response.bytes_stream();
    let mut data: Vec<u8> = Vec::new();
    let mut last_log = Instant::now();

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("Download error: {e}"))?;
        data.extend_from_slice(&chunk);
        if last_log.elapsed() >= std::time::Duration::from_secs(2) {
            last_log = Instant::now();
            if let Some(total) = total_size {
                let pct = (data.len() as f64 / total as f64 * 100.0) as u32;
                logs.push("info", format!("Downloading Python: {pct}%"))
                    .await;
            } else {
                logs.push(
                    "info",
                    format!("Downloading Python: {} MB", data.len() / (1024 * 1024)),
                )
                .await;
            }
        }
    }

    logs.push("info", "Extracting Python…").await;

    let target_dir = openstt_dir();
    tokio::task::spawn_blocking(move || {
        let decoder = GzDecoder::new(&data[..]);
        let mut archive = Archive::new(decoder);
        archive
            .unpack(&target_dir)
            .map_err(|e| format!("Failed to extract Python: {e}"))
    })
    .await
    .map_err(|e| format!("Extract task failed: {e}"))??;

    if !standalone_python_path().exists() {
        return Err("Python extraction succeeded but binary not found".to_string());
    }

    logs.push("info", "Standalone Python installed").await;
    Ok(())
}

async fn ensure_python(state: &AppState) -> Result<String, String> {
    // 1. OPENSTT_PYTHON env override
    if let Ok(path) = std::env::var("OPENSTT_PYTHON") {
        if Path::new(&path).exists() {
            return Ok(path);
        }
    }

    // 2. Already-downloaded standalone python
    let standalone = standalone_python_path();
    if standalone.exists() {
        return Ok(standalone.to_string_lossy().to_string());
    }

    // 3. System python3
    let system_ok = Command::new("python3")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await
        .map(|s| s.success())
        .unwrap_or(false);
    if system_ok {
        return Ok("python3".to_string());
    }

    // 4. Download standalone python
    download_standalone_python(&state.logs).await?;
    Ok(standalone_python_path().to_string_lossy().to_string())
}

#[tauri::command]
async fn mlx_install_dependencies(
    state: TauriState<'_, AppState>,
) -> Result<MlxDependencyStatus, String> {
    if !mlx_supported() {
        return Err("MLX runtime requires Apple Silicon".to_string());
    }
    let base_python = ensure_python(&state).await?;
    let venv = venv_dir();
    if !venv.exists() {
        let status = Command::new(&base_python)
            .arg("-m")
            .arg("venv")
            .arg(&venv)
            .status()
            .await
            .map_err(|err| format!("Failed to create venv: {err}"))?;
        if !status.success() {
            return Err(
                "Failed to create Python venv. Make sure python3 has the venv module installed."
                    .to_string(),
            );
        }
    }

    let venv_python = venv_python_path();
    let status = Command::new(&venv_python)
        .arg("-m")
        .arg("pip")
        .arg("install")
        .arg("-U")
        .arg("pip")
        .env("PIP_DISABLE_PIP_VERSION_CHECK", "1")
        .status()
        .await
        .map_err(|err| format!("Failed to update pip: {err}"))?;
    if !status.success() {
        return Err("Failed to update pip".to_string());
    }

    state
        .logs
        .push("info", "Installing MLX runtime (mlx-audio)")
        .await;

    let status = Command::new(&venv_python)
        .arg("-m")
        .arg("pip")
        .arg("install")
        .arg("-U")
        .arg("mlx-audio")
        .env("PIP_DISABLE_PIP_VERSION_CHECK", "1")
        .status()
        .await
        .map_err(|err| format!("Failed to install mlx-audio: {err}"))?;
    if !status.success() {
        return Err("Failed to install mlx-audio".to_string());
    }

    let status = mlx_dependency_status_inner().await;
    *state.mlx_ready.lock().await = status.ready;
    recompute_and_emit_app_status(&state).await;
    Ok(status)
}

#[tauri::command]
async fn mlx_reset_runtime(state: TauriState<'_, AppState>) -> Result<(), String> {
    if let Some(mut sidecar) = state.mlx_sidecar.lock().await.take() {
        let _ = sidecar.child.kill().await;
    }
    let venv = venv_dir();
    if venv.exists() {
        tokio::fs::remove_dir_all(&venv)
            .await
            .map_err(|err| format!("Failed to remove venv: {err}"))?;
    }
    state.logs.push("info", "MLX runtime reset").await;
    *state.mlx_ready.lock().await = false;
    recompute_and_emit_app_status(&state).await;
    Ok(())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct LegacyModelsInfo {
    found: bool,
    path: String,
    size_bytes: u64,
}

#[tauri::command]
async fn check_legacy_models() -> Result<LegacyModelsInfo, String> {
    let legacy_dir = models_dir().join("glm");
    if !legacy_dir.exists() {
        return Ok(LegacyModelsInfo {
            found: false,
            path: legacy_dir.to_string_lossy().to_string(),
            size_bytes: 0,
        });
    }
    let mut total: u64 = 0;
    let mut stack = vec![legacy_dir.clone()];
    while let Some(dir) = stack.pop() {
        let mut entries = tokio::fs::read_dir(&dir)
            .await
            .map_err(|err| format!("Failed to read directory: {err}"))?;
        while let Ok(Some(entry)) = entries.next_entry().await {
            let meta = entry
                .metadata()
                .await
                .map_err(|err| format!("Failed to read metadata: {err}"))?;
            if meta.is_dir() {
                stack.push(entry.path());
            } else {
                total += meta.len();
            }
        }
    }
    Ok(LegacyModelsInfo {
        found: true,
        path: legacy_dir.to_string_lossy().to_string(),
        size_bytes: total,
    })
}

#[tauri::command]
async fn clean_legacy_models(state: TauriState<'_, AppState>) -> Result<(), String> {
    let legacy_dir = models_dir().join("glm");
    if legacy_dir.exists() {
        tokio::fs::remove_dir_all(&legacy_dir)
            .await
            .map_err(|err| format!("Failed to remove legacy models: {err}"))?;
    }
    state.logs.push("info", "Legacy models cleaned").await;
    Ok(())
}

fn load_ui_settings(path: &Path) -> UiSettings {
    if let Ok(contents) = std::fs::read_to_string(path) {
        if let Ok(settings) = serde_json::from_str::<UiSettings>(&contents) {
            return settings;
        }
    }
    UiSettings::default()
}

fn save_ui_settings(path: &Path, settings: &UiSettings) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|err| format!("Failed to create settings directory: {err}"))?;
    }
    let payload = serde_json::to_string_pretty(settings)
        .map_err(|err| format!("Failed to serialize settings: {err}"))?;
    std::fs::write(path, payload).map_err(|err| format!("Failed to save settings: {err}"))
}

fn load_app_config(path: &Path) -> AppConfig {
    if let Ok(contents) = std::fs::read_to_string(path) {
        if let Ok(config) = serde_json::from_str::<AppConfig>(&contents) {
            return config;
        }
    }
    AppConfig::default()
}

fn save_app_config(path: &Path, config: &AppConfig) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|err| format!("Failed to create config directory: {err}"))?;
    }
    let payload = serde_json::to_string_pretty(config)
        .map_err(|err| format!("Failed to serialize config: {err}"))?;
    std::fs::write(path, payload).map_err(|err| format!("Failed to save config: {err}"))
}

async fn prepare_mlx_model(
    state: &AppState,
    models_root: &Path,
    entry: models::CatalogEntry,
) -> Result<(), String> {
    let deps = mlx_dependency_status_inner().await;
    if !deps.ready {
        return Err(deps
            .message
            .unwrap_or_else(|| "MLX runtime not installed".to_string()));
    }
    let marker = models::model_path(models_root, entry.id)
        .ok_or_else(|| format!("Unknown model: {}", entry.id))?;
    if marker.exists() {
        return Ok(());
    }
    if let Some(parent) = marker.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|err| format!("Failed to create model directory: {err}"))?;
    }
    let script = mlx_sidecar_script();
    if !script.exists() {
        return Err("MLX sidecar script not found".to_string());
    }

    state
        .logs
        .push("info", format!("Preparing model {}...", entry.id))
        .await;

    emit_download_progress_async(state, entry.id, 0, false, None).await;

    let output = Command::new(python_command())
        .arg(script)
        .arg("--model")
        .arg(entry.download_url)
        .arg("--preload")
        .env("HF_HOME", mlx_cache_dir())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await
        .map_err(|err| format!("Failed to run MLX preload: {err}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let msg = format!("MLX preload failed: {stderr}");
        emit_download_progress_async(state, entry.id, 0, true, Some(msg.clone())).await;
        return Err(msg);
    }

    tokio::fs::write(&marker, b"ready")
        .await
        .map_err(|err| format!("Failed to write model marker: {err}"))?;

    state
        .logs
        .push("info", format!("Model {} ready", entry.id))
        .await;
    emit_download_progress_async(state, entry.id, 100, true, None).await;
    Ok(())
}

async fn mlx_health(port: u16) -> bool {
    let url = format!("http://127.0.0.1:{port}/health");
    let client = reqwest::Client::builder()
        .timeout(Duration::from_millis(500))
        .build();
    if let Ok(client) = client {
        return client
            .get(url)
            .send()
            .await
            .map(|r| r.status().is_success())
            .unwrap_or(false);
    }
    false
}

async fn ensure_mlx_sidecar(state: &AppState, model_id: &str) -> Result<u16, String> {
    let existing = {
        let guard = state.mlx_sidecar.lock().await;
        guard
            .as_ref()
            .map(|sidecar| (sidecar.model_id.clone(), sidecar.port))
    };

    if let Some((existing_model, existing_port)) = existing {
        if existing_model == model_id && mlx_health(existing_port).await {
            return Ok(existing_port);
        }
    }

    if let Some(mut old) = state.mlx_sidecar.lock().await.take() {
        let _ = old.child.kill().await;
    }

    let port = std::env::var("OPENSTT_MLX_PORT")
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(8791);
    let script = mlx_sidecar_script();
    if !script.exists() {
        return Err("MLX sidecar script not found".to_string());
    }

    let child = Command::new(python_command())
        .arg(script)
        .arg("--model")
        .arg(model_id)
        .arg("--port")
        .arg(port.to_string())
        .env("HF_HOME", mlx_cache_dir())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|err| format!("Failed to start MLX sidecar: {err}"))?;

    let mut attempts = 0;
    while attempts < 20 {
        if mlx_health(port).await {
            let mut guard = state.mlx_sidecar.lock().await;
            *guard = Some(MlxSidecar {
                model_id: model_id.to_string(),
                port,
                child,
            });
            return Ok(port);
        }
        attempts += 1;
        sleep(Duration::from_millis(250)).await;
    }

    let stderr = child
        .wait_with_output()
        .await
        .map(|output| String::from_utf8_lossy(&output.stderr).to_string())
        .unwrap_or_else(|_| "".to_string());
    Err(format!("MLX sidecar failed to start. {stderr}"))
}

async fn mlx_transcribe(
    state: &AppState,
    model_id: &str,
    audio_path: &Path,
) -> Result<String, String> {
    let port = ensure_mlx_sidecar(state, model_id).await?;
    let url = format!("http://127.0.0.1:{port}/transcribe");
    let payload = serde_json::json!({
        "audio_path": audio_path.to_string_lossy().to_string()
    });
    let response = reqwest::Client::new()
        .post(url)
        .json(&payload)
        .send()
        .await
        .map_err(|err| format!("MLX sidecar request failed: {err}"))?;
    if !response.status().is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err(format!("MLX sidecar error: {body}"));
    }
    let payload = response
        .json::<MlxTranscriptionResponse>()
        .await
        .map_err(|err| format!("MLX response parse failed: {err}"))?;
    Ok(payload.text)
}

#[derive(Deserialize)]
struct ElevenLabsResponse {
    text: String,
}

async fn elevenlabs_transcribe(
    api_key: &str,
    audio_bytes: &[u8],
    elevenlabs_model: &str,
    language: Option<&str>,
) -> Result<String, TranscribeError> {
    let client = reqwest::Client::new();

    let mut form = reqwest::multipart::Form::new()
        .part(
            "file",
            reqwest::multipart::Part::bytes(audio_bytes.to_vec())
                .file_name("audio.wav")
                .mime_str("audio/wav")
                .unwrap(),
        )
        .text("model_id", elevenlabs_model.to_string());

    if let Some(lang) = language {
        form = form.text("language_code", lang.to_string());
    }

    let response = client
        .post("https://api.elevenlabs.io/v1/speech-to-text")
        .header("xi-api-key", api_key)
        .multipart(form)
        .send()
        .await
        .map_err(|e| TranscribeError::internal(e.to_string()))?;

    if !response.status().is_success() {
        let error = response.text().await.unwrap_or_default();
        return Err(TranscribeError::internal(format!(
            "ElevenLabs API error: {}",
            error
        )));
    }

    let result: ElevenLabsResponse = response
        .json()
        .await
        .map_err(|e| TranscribeError::internal(e.to_string()))?;

    Ok(result.text)
}

pub(crate) async fn transcribe_bytes(
    state: &AppState,
    model: Option<String>,
    file_name: Option<String>,
    file_bytes: Vec<u8>,
    language: Option<String>,
    task: Option<String>,
    temperature: Option<f32>,
    prompt: Option<String>,
) -> Result<String, TranscribeError> {
    if file_bytes.is_empty() {
        return Err(TranscribeError::bad_request("Empty audio payload"));
    }

    {
        let mut count = state.requests.lock().await;
        *count += 1;
    }

    let selected_model = if let Some(model_id) = model {
        model_id
    } else {
        state.active_model_id.lock().await.clone()
    };
    let model_id = normalize_model_id(&selected_model);

    // Handle ElevenLabs cloud models
    if model_id.starts_with("elevenlabs:") {
        let file_label = file_name.clone().unwrap_or_else(|| "unknown".to_string());
        let size_label = file_bytes.len().to_string();
        state
            .logs
            .push(
                "info",
                format!(
                    "Transcription request model={model_id} file={file_label} bytes={size_label}"
                ),
            )
            .await;

        let api_key = state.ui_settings.lock().await.elevenlabs_api_key.clone();
        if api_key.is_empty() {
            let message = "ElevenLabs API key not configured".to_string();
            state.logs.push("error", message.clone()).await;
            return Err(TranscribeError::bad_request(message));
        }

        // Extract ElevenLabs model ID (e.g., "elevenlabs:scribe_v2" -> "scribe_v2")
        let elevenlabs_model = model_id.strip_prefix("elevenlabs:").unwrap_or("scribe_v2");
        let text =
            elevenlabs_transcribe(&api_key, &file_bytes, elevenlabs_model, language.as_deref())
                .await?;
        state
            .logs
            .push(
                "info",
                format!("Transcription complete: {} chars", text.len()),
            )
            .await;
        return Ok(text);
    }

    let entry = match models::model_entry(&model_id) {
        Some(entry) => entry,
        None => {
            let message = format!("Unknown model: {model_id}");
            state.logs.push("error", message.clone()).await;
            return Err(TranscribeError::bad_request(message));
        }
    };
    let file_label = file_name.clone().unwrap_or_else(|| "unknown".to_string());
    let size_label = file_bytes.len().to_string();
    state
        .logs
        .push(
            "info",
            format!("Transcription request model={model_id} file={file_label} bytes={size_label}"),
        )
        .await;

    let extension = file_name
        .as_ref()
        .and_then(|name| Path::new(name).extension())
        .and_then(|value| value.to_str())
        .unwrap_or("bin");
    let temp_path =
        std::env::temp_dir().join(format!("openstt-upload-{}.{}", now_millis(), extension));
    if let Err(err) = tokio::fs::write(&temp_path, &file_bytes).await {
        let message = format!("Failed to write temp file: {err}");
        state.logs.push("error", message.clone()).await;
        return Err(TranscribeError::internal(message));
    }

    if entry.engine == models::ModelEngine::Mlx {
        let dir = match resolve_models_dir(state).await {
            Ok(dir) => dir,
            Err(err) => {
                state.logs.push("error", err.clone()).await;
                return Err(TranscribeError::internal(err));
            }
        };
        let marker =
            models::model_path(&dir, &model_id).ok_or_else(|| format!("Unknown model: {model_id}"));
        let marker = match marker {
            Ok(path) => path,
            Err(err) => {
                state.logs.push("error", err.clone()).await;
                return Err(TranscribeError::bad_request(err));
            }
        };
        if !marker.exists() {
            if std::env::var("OPENSTT_AUTO_DOWNLOAD").ok().as_deref() == Some("1") {
                if let Err(err) = download_model_inner(state, &model_id).await {
                    state.logs.push("error", err.clone()).await;
                    return Err(TranscribeError::bad_request(err));
                }
            } else {
                let err = format!("Model {model_id} not prepared");
                state.logs.push("error", err.clone()).await;
                return Err(TranscribeError::bad_request(err));
            }
        }

        let text = match mlx_transcribe(state, entry.download_url, &temp_path).await {
            Ok(text) => text,
            Err(err) => {
                state.logs.push("error", err.clone()).await;
                return Err(TranscribeError::internal(err));
            }
        };

        let _ = std::fs::remove_file(&temp_path);
        return Ok(text);
    }

    let model_path = match ensure_whisper_model_path(state, &model_id).await {
        Ok(path) => path,
        Err(err) => {
            state.logs.push("error", err.clone()).await;
            return Err(TranscribeError::bad_request(err));
        }
    };

    // Take the cached context+state out of the mutex (we'll put it back after)
    let cached = {
        let mut guard = state.cached_context.lock().await;
        guard.take()
    };
    let (cached_ctx, cached_state) = match cached {
        Some(c) if c.model_id == model_id => (Some(c.context), Some(c.state)),
        other => {
            // Put back if model didn't match (different model cached)
            let mut guard = state.cached_context.lock().await;
            *guard = other;
            (None, None)
        }
    };

    let language_value = language.clone();
    let prompt_value = prompt.clone();
    let translate = task.as_deref() == Some("translate");
    let temperature_value = temperature.unwrap_or(0.0);
    let model_path_value = model_path.clone();
    let temp_path_value = temp_path.clone();

    let result = tokio::task::spawn_blocking(move || {
        let audio = audio::load_and_resample(&temp_path_value)?;
        let context = if let Some(context) = cached_ctx {
            context
        } else {
            let mut params = WhisperContextParameters::default();
            params.use_gpu(true);
            params.flash_attn(true);
            let context = WhisperContext::new_with_params(
                model_path_value
                    .to_str()
                    .ok_or_else(|| "Invalid model path".to_string())?,
                params,
            )
            .map_err(|err| format!("Failed to load model: {err:?}"))?;
            Arc::new(context)
        };

        let mut wstate = if let Some(wstate) = cached_state {
            wstate
        } else {
            context
                .create_state()
                .map_err(|err| format!("Failed to create whisper state: {err:?}"))?
        };

        let params = build_whisper_params(
            language_value.as_deref(),
            prompt_value.as_deref(),
            translate,
            temperature_value,
        );
        wstate
            .full(params, &audio)
            .map_err(|err| format!("Transcription failed: {err:?}"))?;
        let segments = wstate
            .full_n_segments()
            .map_err(|err| format!("Failed to read segments: {err:?}"))?;
        let mut text = String::new();
        for index in 0..segments {
            let segment = wstate
                .full_get_segment_text(index)
                .map_err(|err| format!("Failed to read segment text: {err:?}"))?;
            text.push_str(&segment);
        }
        Ok::<(String, Arc<WhisperContext>, WhisperState), String>((
            text.trim().to_string(),
            context,
            wstate,
        ))
    })
    .await;

    let _ = std::fs::remove_file(&temp_path);

    let (text, context, wstate) = match result {
        Ok(Ok(output)) => output,
        Ok(Err(err)) => {
            state.logs.push("error", err.clone()).await;
            return Err(TranscribeError::internal(err));
        }
        Err(err) => {
            let message = format!("Transcription task failed: {err}");
            state.logs.push("error", message.clone()).await;
            return Err(TranscribeError::internal(message));
        }
    };

    {
        let mut cached = state.cached_context.lock().await;
        *cached = Some(CachedWhisperContext {
            model_id: model_id.clone(),
            context,
            state: wstate,
        });
    }

    Ok(text)
}

async fn resolve_models_dir(state: &AppState) -> Result<PathBuf, String> {
    state
        .models_dir
        .lock()
        .await
        .clone()
        .ok_or_else(|| "Models directory not initialized".to_string())
}

fn hf_cache_dir_for(model_id: &str) -> PathBuf {
    let folder = model_id.replace('/', "--");
    mlx_cache_dir()
        .join("hub")
        .join(format!("models--{folder}"))
}

async fn build_status(state: &AppState) -> ServerStatus {
    let running = state.runtime.lock().await.is_some();
    let port = *state.port.lock().await;
    let started_at = *state.started_at.lock().await;
    let requests = *state.requests.lock().await;
    let url = if running {
        Some(format!("http://127.0.0.1:{port}"))
    } else {
        None
    };
    ServerStatus {
        running,
        port,
        url,
        started_at,
        requests,
    }
}

async fn start_server_inner(app_state: AppState, port: u16) -> Result<ServerStatus, String> {
    if port == 0 {
        return Err("Port must be between 1 and 65535".to_string());
    }

    if app_state.runtime.lock().await.is_some() {
        return Ok(build_status(&app_state).await);
    }

    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .map_err(|err| format!("Failed to bind {addr}: {err}"))?;

    *app_state.port.lock().await = port;
    *app_state.started_at.lock().await = Some(now_millis());
    *app_state.requests.lock().await = 0;
    app_state
        .logs
        .push(
            "info",
            format!("Gateway starting on http://127.0.0.1:{port}"),
        )
        .await;

    let router = Router::new()
        .route("/health", get(health))
        .route("/v1/audio/transcriptions", post(transcribe))
        .with_state(app_state.clone());

    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
    let server_state = app_state.clone();
    let handle = tauri::async_runtime::spawn(async move {
        let result = axum::serve(listener, router)
            .with_graceful_shutdown(async move {
                let _ = shutdown_rx.await;
            })
            .await;
        if let Err(err) = result {
            server_state
                .logs
                .push("error", format!("Gateway error: {err}"))
                .await;
        }
    });

    let runtime = ServerRuntime {
        shutdown: shutdown_tx,
        handle,
    };
    *app_state.runtime.lock().await = Some(runtime);

    refresh_tray(&app_state).await;
    recompute_and_emit_app_status(&app_state).await;

    Ok(build_status(&app_state).await)
}

async fn stop_server_inner(app_state: AppState) -> Result<ServerStatus, String> {
    let runtime = { app_state.runtime.lock().await.take() };

    if let Some(runtime) = runtime {
        let _ = runtime.shutdown.send(());
        let _ = runtime.handle.await;
        app_state.logs.push("info", "Gateway stopped").await;
    }

    *app_state.started_at.lock().await = None;
    refresh_tray(&app_state).await;
    emit_app_status(&app_state, AppStatus::Stopped).await;
    Ok(build_status(&app_state).await)
}

#[cfg(target_os = "macos")]
fn check_accessibility() -> bool {
    unsafe { AXIsProcessTrusted() }
}

#[cfg(not(target_os = "macos"))]
fn check_accessibility() -> bool {
    true
}

#[cfg(target_os = "macos")]
fn check_microphone() -> String {
    use std::ffi::c_void;

    extern "C" {
        static AVMediaTypeAudio: *const c_void;
        fn objc_getClass(name: *const u8) -> *const c_void;
        fn sel_registerName(name: *const u8) -> *const c_void;
    }
    type MsgSendFn = unsafe extern "C" fn(*const c_void, *const c_void, *const c_void) -> isize;
    extern "C" {
        fn objc_msgSend();
    }

    unsafe {
        let class = objc_getClass(b"AVCaptureDevice\0".as_ptr());
        let sel = sel_registerName(b"authorizationStatusForMediaType:\0".as_ptr());
        let send: MsgSendFn = std::mem::transmute(objc_msgSend as *const c_void);
        let status = send(class, sel, AVMediaTypeAudio);
        // AVAuthorizationStatus: 0=notDetermined, 1=restricted, 2=denied, 3=authorized
        match status {
            3 => "granted",
            1 | 2 => "denied",
            _ => "not_determined",
        }
        .to_string()
    }
}

#[cfg(not(target_os = "macos"))]
fn check_microphone() -> String {
    "granted".to_string()
}

#[cfg(target_os = "macos")]
fn check_input_monitoring() -> bool {
    let tap = CGEventTap::new(
        CGEventTapLocation::HID,
        CGEventTapPlacement::HeadInsertEventTap,
        CGEventTapOptions::ListenOnly,
        vec![CGEventType::KeyDown],
        |_proxy, _event_type, event| Some(event.clone()),
    );
    tap.is_ok()
}

#[cfg(not(target_os = "macos"))]
fn check_input_monitoring() -> bool {
    true
}

#[tauri::command]
fn check_all_permissions() -> Result<PermissionStatus, String> {
    Ok(PermissionStatus {
        accessibility: check_accessibility(),
        microphone: check_microphone(),
        input_monitoring: check_input_monitoring(),
    })
}

#[tauri::command]
async fn open_permission_settings(target: String) -> Result<(), String> {
    let url = match target.as_str() {
        "input_monitoring" => {
            "x-apple.systempreferences:com.apple.preference.security?Privacy_ListenEvent"
        }
        "microphone" => {
            "x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone"
        }
        "accessibility" => {
            "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility"
        }
        _ => return Err(format!("Unknown permission target: {target}")),
    };
    Command::new("open")
        .arg(url)
        .output()
        .await
        .map_err(|err| format!("Failed to open settings: {err}"))?;
    Ok(())
}

#[tauri::command]
async fn test_elevenlabs_api_key(api_key: String) -> Result<bool, String> {
    // Create a minimal valid WAV file (44 bytes header + 2 bytes of silence)
    let wav_header: [u8; 46] = [
        0x52, 0x49, 0x46, 0x46, // "RIFF"
        0x26, 0x00, 0x00, 0x00, // File size - 8 (38 bytes)
        0x57, 0x41, 0x56, 0x45, // "WAVE"
        0x66, 0x6D, 0x74, 0x20, // "fmt "
        0x10, 0x00, 0x00, 0x00, // Subchunk1Size (16 for PCM)
        0x01, 0x00, // AudioFormat (1 = PCM)
        0x01, 0x00, // NumChannels (1 = mono)
        0x80, 0x3E, 0x00, 0x00, // SampleRate (16000)
        0x00, 0x7D, 0x00, 0x00, // ByteRate (32000)
        0x02, 0x00, // BlockAlign (2)
        0x10, 0x00, // BitsPerSample (16)
        0x64, 0x61, 0x74, 0x61, // "data"
        0x02, 0x00, 0x00, 0x00, // Subchunk2Size (2 bytes)
        0x00, 0x00, // 1 sample of silence
    ];

    let client = reqwest::Client::new();
    let form = reqwest::multipart::Form::new()
        .part(
            "file",
            reqwest::multipart::Part::bytes(wav_header.to_vec())
                .file_name("test.wav")
                .mime_str("audio/wav")
                .unwrap(),
        )
        .text("model_id", "scribe_v2");

    let response = client
        .post("https://api.elevenlabs.io/v1/speech-to-text")
        .header("xi-api-key", &api_key)
        .multipart(form)
        .send()
        .await
        .map_err(|e| e.to_string())?;

    // 200 = success, 422 = audio too short (but key is valid)
    // 401 = invalid key or missing permissions
    if response.status().is_success() || response.status().as_u16() == 422 {
        return Ok(true);
    }

    let body = response.text().await.unwrap_or_default();
    // If we get "invalid_api_key" or "missing_permissions", key is not valid for STT
    Ok(!body.contains("invalid_api_key") && !body.contains("missing_permissions"))
}

#[tauri::command]
async fn restart_app(app: tauri::AppHandle) -> Result<(), String> {
    app.restart();
}

#[tauri::command]
async fn start_server(state: TauriState<'_, AppState>, port: u16) -> Result<ServerStatus, String> {
    start_server_inner((*state).clone(), port).await
}

#[tauri::command]
async fn stop_server(state: TauriState<'_, AppState>) -> Result<ServerStatus, String> {
    stop_server_inner((*state).clone()).await
}

#[tauri::command]
async fn get_server_status(state: TauriState<'_, AppState>) -> Result<ServerStatus, String> {
    Ok(build_status(&(*state).clone()).await)
}

#[tauri::command]
async fn get_app_status(state: TauriState<'_, AppState>) -> Result<String, String> {
    Ok(state.app_status.lock().await.as_str().to_string())
}

#[tauri::command]
async fn get_logs(state: TauriState<'_, AppState>) -> Result<Vec<LogEntry>, String> {
    Ok(state.logs.list().await)
}

#[tauri::command]
async fn clear_logs(state: TauriState<'_, AppState>) -> Result<(), String> {
    state.logs.clear().await;
    Ok(())
}

#[tauri::command]
async fn get_ui_settings(state: TauriState<'_, AppState>) -> Result<UiSettings, String> {
    Ok(state.ui_settings.lock().await.clone())
}

#[tauri::command]
async fn set_ui_settings(
    state: TauriState<'_, AppState>,
    settings: UiSettings,
) -> Result<UiSettings, String> {
    *state.ui_settings.lock().await = settings.clone();
    if let Some(path) = state.settings_path.lock().await.clone() {
        save_ui_settings(&path, &settings)?;
    }
    if let Some(app_handle) = state.app_handle.lock().await.clone() {
        if let Err(err) =
            register_dictation_shortcut(&app_handle, &state, &settings.dictation_shortcut).await
        {
            state
                .logs
                .push(
                    "error",
                    format!("Failed to update dictation shortcut: {err}"),
                )
                .await;
        }
    }
    Ok(settings)
}

#[tauri::command]
async fn transcribe_audio(
    state: TauriState<'_, AppState>,
    request: TranscribeAudioRequest,
) -> Result<String, String> {
    let result = transcribe_bytes(
        &state,
        request.model_id,
        request.file_name,
        request.bytes,
        request.language,
        None,
        None,
        None,
    )
    .await;
    match result {
        Ok(text) => Ok(text),
        Err(err) => Err(err.message),
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DictationStateResponse {
    state: String,
    queue_len: u32,
}

#[tauri::command]
async fn start_dictation(state: TauriState<'_, AppState>) -> Result<(), String> {
    let app_handle = state
        .app_handle
        .lock()
        .await
        .clone()
        .ok_or_else(|| "App handle not available".to_string())?;
    start_dictation_inner(&state, &app_handle).await?;
    state.dictation.emit_state(&app_handle).await;
    let mut tray_state = state.dictation_tray_state.lock().await;
    tray_state.state = "listening".to_string();
    tray_state.queue_len = state.dictation.queue_len().await;
    tray_state.phase_started = Some(Instant::now());
    drop(tray_state);
    refresh_tray(&state).await;
    recompute_and_emit_app_status(&state).await;
    Ok(())
}

#[tauri::command]
async fn stop_dictation(state: TauriState<'_, AppState>) -> Result<(), String> {
    let app_handle = state
        .app_handle
        .lock()
        .await
        .clone()
        .ok_or_else(|| "App handle not available".to_string())?;
    stop_dictation_inner(&state, &app_handle).await?;
    Ok(())
}

#[tauri::command]
async fn start_playground_recording(state: TauriState<'_, AppState>) -> Result<(), String> {
    state.dictation.start_playground()
}

#[tauri::command]
async fn stop_playground_recording(state: TauriState<'_, AppState>) -> Result<(), String> {
    let app_state = (*state).clone();
    let app_handle = state.app_handle.lock().await.clone();
    tauri::async_runtime::spawn(async move {
        let result = app_state
            .dictation
            .stop_playground_and_transcribe(&app_state)
            .await;
        if let Some(app_handle) = app_handle {
            let _ = app_handle.emit("playground-transcription-result", result);
        }
    });
    Ok(())
}

#[tauri::command]
async fn get_dictation_state(
    state: TauriState<'_, AppState>,
) -> Result<DictationStateResponse, String> {
    Ok(DictationStateResponse {
        state: state.dictation.current_state().as_str().to_string(),
        queue_len: state.dictation.queue_len().await,
    })
}

pub(crate) async fn paste_clipboard_inner() -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let status = Command::new("osascript")
            .arg("-e")
            .arg("tell application \"System Events\" to keystroke \"v\" using command down")
            .status()
            .await
            .map_err(|err| format!("Failed to run paste helper: {err}"))?;
        if !status.success() {
            return Err("Paste command failed".to_string());
        }
        return Ok(());
    }
    #[cfg(not(target_os = "macos"))]
    {
        Err("Auto paste is only supported on macOS".to_string())
    }
}

#[tauri::command]
async fn paste_clipboard() -> Result<(), String> {
    paste_clipboard_inner().await
}

#[tauri::command]
async fn list_models(state: TauriState<'_, AppState>) -> Result<Vec<models::ModelInfo>, String> {
    let dir = resolve_models_dir(&*state).await?;
    Ok(models::list_models(&dir))
}

#[tauri::command]
async fn download_model(state: TauriState<'_, AppState>, model_id: String) -> Result<(), String> {
    {
        let mut downloading = state.downloading.lock().await;
        if *downloading {
            return Err("Another download is already in progress".to_string());
        }
        *downloading = true;
    }
    let app_state = (*state).clone();
    let model_id_clone = model_id.clone();
    tauri::async_runtime::spawn(async move {
        match download_model_inner(&app_state, &model_id_clone).await {
            Ok(_) => {}
            Err(err) => {
                app_state.logs.push("error", err.clone()).await;
                emit_download_progress_async(&app_state, &model_id_clone, 0, true, Some(err)).await;
            }
        }
        *app_state.downloading.lock().await = false;
    });
    Ok(())
}

#[tauri::command]
async fn delete_model(state: TauriState<'_, AppState>, model_id: String) -> Result<(), String> {
    let active_model = state.active_model_id.lock().await.clone();
    if active_model == model_id {
        return Err("Cannot delete the active model".to_string());
    }
    let dir = resolve_models_dir(&*state).await?;
    let entry =
        models::model_entry(&model_id).ok_or_else(|| format!("Unknown model: {model_id}"))?;
    let path =
        models::model_path(&dir, &model_id).ok_or_else(|| format!("Unknown model: {model_id}"))?;
    if entry.engine == models::ModelEngine::Mlx {
        if path.exists() {
            tokio::fs::remove_file(&path)
                .await
                .map_err(|err| format!("Failed to delete model marker: {err}"))?;
        }
        let cache_dir = hf_cache_dir_for(entry.download_url);
        if cache_dir.exists() {
            tokio::fs::remove_dir_all(&cache_dir)
                .await
                .map_err(|err| format!("Failed to delete model cache: {err}"))?;
        }
    } else {
        if !path.exists() {
            return Err("Model file not found".to_string());
        }
        tokio::fs::remove_file(&path)
            .await
            .map_err(|err| format!("Failed to delete model: {err}"))?;
    }
    state
        .logs
        .push("info", format!("Model {model_id} deleted"))
        .await;
    Ok(())
}

#[tauri::command]
async fn get_active_model(state: TauriState<'_, AppState>) -> Result<String, String> {
    Ok(state.active_model_id.lock().await.clone())
}

#[tauri::command]
async fn set_active_model(
    state: TauriState<'_, AppState>,
    model_id: String,
) -> Result<String, String> {
    let model_id = normalize_model_id(&model_id);
    // Allow ElevenLabs cloud models or catalog models
    let is_elevenlabs = model_id.starts_with("elevenlabs:");
    if !is_elevenlabs && models::model_entry(&model_id).is_none() {
        return Err(format!("Unknown model: {model_id}"));
    }
    *state.active_model_id.lock().await = model_id.clone();
    *state.cached_context.lock().await = None;
    if let Some(path) = state.config_path.lock().await.clone() {
        let config = AppConfig {
            active_model_id: model_id.clone(),
        };
        save_app_config(&path, &config)?;
    }
    state
        .logs
        .push("info", format!("Active model set to {model_id}"))
        .await;
    refresh_tray(&state).await;
    let preload_state = (*state).clone();
    tauri::async_runtime::spawn(async move {
        preload_active_model(&preload_state).await;
    });
    Ok(model_id)
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct DownloadProgressEvent {
    model_id: String,
    percent: u32,
    done: bool,
    error: Option<String>,
}

async fn emit_download_progress_async(
    state: &AppState,
    model_id: &str,
    percent: u32,
    done: bool,
    error: Option<String>,
) {
    if let Some(app_handle) = state.app_handle.lock().await.clone() {
        let _ = app_handle.emit(
            "download-progress",
            DownloadProgressEvent {
                model_id: model_id.to_string(),
                percent,
                done,
                error,
            },
        );
    }
}

async fn download_model_inner(
    state: &AppState,
    model_id: &str,
) -> Result<models::ModelInfo, String> {
    let dir = resolve_models_dir(state).await?;
    let entry =
        models::model_entry(model_id).ok_or_else(|| format!("Unknown model: {model_id}"))?;
    if entry.engine == models::ModelEngine::Mlx {
        prepare_mlx_model(state, &dir, entry).await?;
        let models = models::list_models(&dir);
        return models
            .into_iter()
            .find(|info| info.id == model_id)
            .ok_or_else(|| "Model info unavailable".to_string());
    }

    let path =
        models::model_path(&dir, model_id).ok_or_else(|| format!("Unknown model: {model_id}"))?;

    if path.exists() {
        let models = models::list_models(&dir);
        return models
            .into_iter()
            .find(|info| info.id == model_id)
            .ok_or_else(|| "Model info unavailable".to_string());
    }

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|err| format!("Failed to create model directory: {err}"))?;
    }
    state
        .logs
        .push("info", format!("Downloading model {model_id}..."))
        .await;

    let response = reqwest::get(entry.download_url)
        .await
        .map_err(|err| format!("Failed to start download: {err}"))?;
    if !response.status().is_success() {
        return Err(format!("Download failed with status {}", response.status()));
    }

    let total = response.content_length().unwrap_or(0);
    let mut file = tokio::fs::File::create(&path)
        .await
        .map_err(|err| format!("Failed to create model file: {err}"))?;
    let mut stream = response.bytes_stream();
    let mut downloaded: u64 = 0;
    let mut next_log_at = 50 * 1024 * 1024;
    let mut last_emit = Instant::now();

    emit_download_progress_async(state, model_id, 0, false, None).await;

    while let Some(chunk) = stream.next().await {
        let chunk = match chunk {
            Ok(chunk) => chunk,
            Err(err) => {
                let msg = format!("Download error: {err}");
                emit_download_progress_async(state, model_id, 0, true, Some(msg.clone())).await;
                return Err(msg);
            }
        };
        file.write_all(&chunk)
            .await
            .map_err(|err| format!("Failed to write model file: {err}"))?;
        downloaded += chunk.len() as u64;
        if total > 0 {
            let percent = (downloaded as f64 / total as f64 * 100.0).round() as u32;
            if last_emit.elapsed() >= Duration::from_millis(500) || percent >= 100 {
                emit_download_progress_async(state, model_id, percent, false, None).await;
                last_emit = Instant::now();
            }
            if downloaded >= next_log_at {
                state
                    .logs
                    .push("info", format!("Downloading {model_id}: {percent}%"))
                    .await;
                next_log_at += 50 * 1024 * 1024;
            }
        }
    }

    file.flush()
        .await
        .map_err(|err| format!("Failed to finalize model file: {err}"))?;
    state
        .logs
        .push("info", format!("Model {model_id} downloaded"))
        .await;
    emit_download_progress_async(state, model_id, 100, true, None).await;

    let models = models::list_models(&dir);
    models
        .into_iter()
        .find(|info| info.id == model_id)
        .ok_or_else(|| "Model info unavailable".to_string())
}

async fn ensure_whisper_model_path(state: &AppState, model_id: &str) -> Result<PathBuf, String> {
    let entry =
        models::model_entry(model_id).ok_or_else(|| format!("Unknown model: {model_id}"))?;
    if entry.engine != models::ModelEngine::Whisper {
        return Err(format!("Model {model_id} is not a Whisper model"));
    }
    let dir = resolve_models_dir(state).await?;
    let path =
        models::model_path(&dir, model_id).ok_or_else(|| format!("Unknown model: {model_id}"))?;
    if path.exists() {
        return Ok(path);
    }
    if std::env::var("OPENSTT_AUTO_DOWNLOAD").ok().as_deref() == Some("1") {
        let _ = download_model_inner(state, model_id).await?;
        if path.exists() {
            return Ok(path);
        }
    }
    Err(format!("Model {model_id} not downloaded"))
}

async fn preload_whisper_model(state: &AppState) {
    let model_id = state.active_model_id.lock().await.clone();
    let model_path = match ensure_whisper_model_path(state, &model_id).await {
        Ok(p) => p,
        Err(_) => return, // model not downloaded yet, nothing to preload
    };

    // Already cached?
    {
        let guard = state.cached_context.lock().await;
        if guard.as_ref().map(|c| c.model_id.as_str()) == Some(model_id.as_str()) {
            return;
        }
    }

    state
        .logs
        .push("info", format!("Pre-loading model {model_id}…"))
        .await;

    let model_path_value = model_path.clone();
    let loaded = tokio::task::spawn_blocking(move || {
        let mut params = WhisperContextParameters::default();
        params.use_gpu(true);
        params.flash_attn(true);
        let context = WhisperContext::new_with_params(
            model_path_value
                .to_str()
                .ok_or_else(|| "Invalid model path".to_string())?,
            params,
        )
        .map_err(|err| format!("Failed to load model: {err:?}"))?;
        let context = Arc::new(context);
        let wstate = context
            .create_state()
            .map_err(|err| format!("Failed to create whisper state: {err:?}"))?;
        Ok::<(Arc<WhisperContext>, WhisperState), String>((context, wstate))
    })
    .await;

    match loaded {
        Ok(Ok((context, wstate))) => {
            let mut guard = state.cached_context.lock().await;
            *guard = Some(CachedWhisperContext {
                model_id,
                context,
                state: wstate,
            });
            state
                .logs
                .push("info", "Model pre-loaded and ready".to_string())
                .await;
        }
        Ok(Err(err)) => {
            state
                .logs
                .push("error", format!("Model pre-load failed: {err}"))
                .await;
        }
        Err(err) => {
            state
                .logs
                .push("error", format!("Model pre-load task panicked: {err}"))
                .await;
        }
    }
}

async fn preload_mlx_model(state: &AppState) {
    let model_id = state.active_model_id.lock().await.clone();
    let entry = match models::model_entry(&model_id) {
        Some(e) if e.engine == models::ModelEngine::Mlx => e,
        _ => return,
    };
    // Check if sidecar is already running for this model
    {
        let guard = state.mlx_sidecar.lock().await;
        if let Some(sidecar) = guard.as_ref() {
            if sidecar.model_id == model_id && mlx_health(sidecar.port).await {
                return;
            }
        }
    }
    emit_app_status(state, AppStatus::Loading).await;
    state
        .logs
        .push("info", format!("Pre-loading MLX model {model_id}…"))
        .await;
    match ensure_mlx_sidecar(state, entry.download_url).await {
        Ok(port) => {
            state
                .logs
                .push("info", format!("MLX sidecar ready on port {port}"))
                .await
        }
        Err(err) => {
            state
                .logs
                .push("error", format!("MLX pre-load failed: {err}"))
                .await
        }
    }
}

async fn preload_active_model(state: &AppState) {
    emit_app_status(state, AppStatus::Loading).await;
    let model_id = state.active_model_id.lock().await.clone();
    match models::model_entry(&model_id).map(|e| e.engine) {
        Some(models::ModelEngine::Whisper) => preload_whisper_model(state).await,
        Some(models::ModelEngine::Mlx) => preload_mlx_model(state).await,
        _ => {}
    }
    recompute_and_emit_app_status(state).await;
}

fn build_whisper_params<'a>(
    language: Option<&'a str>,
    prompt: Option<&'a str>,
    translate: bool,
    temperature: f32,
) -> FullParams<'a, 'a> {
    let strategy = if temperature == 0.0 {
        SamplingStrategy::Greedy { best_of: 1 }
    } else {
        SamplingStrategy::BeamSearch {
            beam_size: 5,
            patience: 1.0,
        }
    };
    let mut params = FullParams::new(strategy);
    params.set_language(language);
    params.set_translate(translate);
    params.set_print_special(false);
    params.set_print_progress(false);
    params.set_print_realtime(false);
    params.set_print_timestamps(false);
    params.set_temperature(temperature);
    if let Some(prompt) = prompt {
        params.set_initial_prompt(prompt);
    }
    params
}

async fn health() -> Json<HealthResponse> {
    Json(HealthResponse { status: "ok" })
}

async fn transcribe(AxumState(state): AxumState<AppState>, mut multipart: Multipart) -> Response {
    let mut file_name: Option<String> = None;
    let mut file_bytes: Option<Vec<u8>> = None;
    let mut model: Option<String> = None;
    let mut response_format: Option<String> = None;
    let mut language: Option<String> = None;
    let mut task: Option<String> = None;
    let mut temperature: Option<f32> = None;
    let mut prompt: Option<String> = None;
    let mut size_bytes: Option<usize> = None;

    loop {
        let next = match multipart.next_field().await {
            Ok(Some(field)) => field,
            Ok(None) => break,
            Err(err) => {
                state
                    .logs
                    .push("error", format!("Multipart error: {err}"))
                    .await;
                return (StatusCode::BAD_REQUEST, "Invalid multipart payload").into_response();
            }
        };

        match next.name().unwrap_or("") {
            "file" => {
                file_name = next.file_name().map(|value| value.to_string());
                match next.bytes().await {
                    Ok(bytes) => {
                        size_bytes = Some(bytes.len());
                        file_bytes = Some(bytes.to_vec());
                    }
                    Err(err) => {
                        state
                            .logs
                            .push("error", format!("Failed reading file: {err}"))
                            .await;
                        return (StatusCode::BAD_REQUEST, "Invalid file payload").into_response();
                    }
                }
            }
            "model" => {
                if let Ok(text) = next.text().await {
                    model = Some(text.trim().to_string());
                }
            }
            "response_format" => {
                if let Ok(text) = next.text().await {
                    response_format = Some(text.trim().to_string());
                }
            }
            "language" => {
                if let Ok(text) = next.text().await {
                    language = Some(text.trim().to_string());
                }
            }
            "task" => {
                if let Ok(text) = next.text().await {
                    task = Some(text.trim().to_string());
                }
            }
            "temperature" => {
                if let Ok(text) = next.text().await {
                    temperature = text.trim().parse::<f32>().ok();
                }
            }
            "prompt" => {
                if let Ok(text) = next.text().await {
                    prompt = Some(text);
                }
            }
            _ => {
                let _ = next.bytes().await;
            }
        }
    }

    let file_bytes = match file_bytes {
        Some(bytes) => bytes,
        None => {
            state.logs.push("error", "Missing file field").await;
            return (StatusCode::BAD_REQUEST, "Missing file field").into_response();
        }
    };

    {
        let mut count = state.requests.lock().await;
        *count += 1;
    }

    let selected_model = if let Some(model_id) = model {
        model_id
    } else {
        state.active_model_id.lock().await.clone()
    };
    let model_id = normalize_model_id(&selected_model);
    let entry = match models::model_entry(&model_id) {
        Some(entry) => entry,
        None => {
            let message = format!("Unknown model: {model_id}");
            state.logs.push("error", message.clone()).await;
            return (StatusCode::BAD_REQUEST, message).into_response();
        }
    };
    let file_label = file_name.clone().unwrap_or_else(|| "unknown".to_string());
    let size_label = size_bytes
        .map(|bytes| bytes.to_string())
        .unwrap_or_else(|| "unknown".to_string());
    state
        .logs
        .push(
            "info",
            format!("Transcription request model={model_id} file={file_label} bytes={size_label}"),
        )
        .await;

    let extension = file_name
        .as_ref()
        .and_then(|name| Path::new(name).extension())
        .and_then(|value| value.to_str())
        .unwrap_or("bin");
    let temp_path =
        std::env::temp_dir().join(format!("openstt-upload-{}.{}", now_millis(), extension));
    if let Err(err) = tokio::fs::write(&temp_path, &file_bytes).await {
        let message = format!("Failed to write temp file: {err}");
        state.logs.push("error", message.clone()).await;
        return (StatusCode::INTERNAL_SERVER_ERROR, message).into_response();
    }

    if entry.engine == models::ModelEngine::Mlx {
        let dir = match resolve_models_dir(&state).await {
            Ok(dir) => dir,
            Err(err) => {
                state.logs.push("error", err.clone()).await;
                return (StatusCode::INTERNAL_SERVER_ERROR, err).into_response();
            }
        };
        let marker =
            models::model_path(&dir, &model_id).ok_or_else(|| format!("Unknown model: {model_id}"));
        let marker = match marker {
            Ok(path) => path,
            Err(err) => {
                state.logs.push("error", err.clone()).await;
                return (StatusCode::BAD_REQUEST, err).into_response();
            }
        };
        if !marker.exists() {
            if std::env::var("OPENSTT_AUTO_DOWNLOAD").ok().as_deref() == Some("1") {
                if let Err(err) = download_model_inner(&state, &model_id).await {
                    state.logs.push("error", err.clone()).await;
                    return (StatusCode::BAD_REQUEST, err).into_response();
                }
            } else {
                let err = format!("Model {model_id} not prepared");
                state.logs.push("error", err.clone()).await;
                return (StatusCode::BAD_REQUEST, err).into_response();
            }
        }

        let text = match mlx_transcribe(&state, entry.download_url, &temp_path).await {
            Ok(text) => text,
            Err(err) => {
                state.logs.push("error", err.clone()).await;
                return (StatusCode::INTERNAL_SERVER_ERROR, err).into_response();
            }
        };

        let _ = std::fs::remove_file(&temp_path);
        if response_format.as_deref() == Some("text") {
            return text.into_response();
        }
        return Json(TranscriptionResponse { text }).into_response();
    }

    let model_path = match ensure_whisper_model_path(&state, &model_id).await {
        Ok(path) => path,
        Err(err) => {
            state.logs.push("error", err.clone()).await;
            return (StatusCode::BAD_REQUEST, err).into_response();
        }
    };

    let cached = {
        let mut guard = state.cached_context.lock().await;
        guard.take()
    };
    let (cached_ctx, cached_state) = match cached {
        Some(c) if c.model_id == model_id => (Some(c.context), Some(c.state)),
        other => {
            let mut guard = state.cached_context.lock().await;
            *guard = other;
            (None, None)
        }
    };

    let language_value = language.clone();
    let prompt_value = prompt.clone();
    let translate = task.as_deref() == Some("translate");
    let temperature_value = temperature.unwrap_or(0.0);
    let model_path_value = model_path.clone();
    let temp_path_value = temp_path.clone();

    let result = tokio::task::spawn_blocking(move || {
        let audio = audio::load_and_resample(&temp_path_value)?;
        let context = if let Some(context) = cached_ctx {
            context
        } else {
            let mut params = WhisperContextParameters::default();
            params.use_gpu(true);
            params.flash_attn(true);
            let context = WhisperContext::new_with_params(
                model_path_value
                    .to_str()
                    .ok_or_else(|| "Invalid model path".to_string())?,
                params,
            )
            .map_err(|err| format!("Failed to load model: {err:?}"))?;
            Arc::new(context)
        };

        let mut wstate = if let Some(wstate) = cached_state {
            wstate
        } else {
            context
                .create_state()
                .map_err(|err| format!("Failed to create whisper state: {err:?}"))?
        };

        let params = build_whisper_params(
            language_value.as_deref(),
            prompt_value.as_deref(),
            translate,
            temperature_value,
        );
        wstate
            .full(params, &audio)
            .map_err(|err| format!("Transcription failed: {err:?}"))?;
        let segments = wstate
            .full_n_segments()
            .map_err(|err| format!("Failed to read segments: {err:?}"))?;
        let mut text = String::new();
        for index in 0..segments {
            let segment = wstate
                .full_get_segment_text(index)
                .map_err(|err| format!("Failed to read segment text: {err:?}"))?;
            text.push_str(&segment);
        }
        Ok::<(String, Arc<WhisperContext>, WhisperState), String>((
            text.trim().to_string(),
            context,
            wstate,
        ))
    })
    .await;

    let _ = std::fs::remove_file(&temp_path);

    let (text, context, wstate) = match result {
        Ok(Ok(output)) => output,
        Ok(Err(err)) => {
            state.logs.push("error", err.clone()).await;
            return (StatusCode::INTERNAL_SERVER_ERROR, err).into_response();
        }
        Err(err) => {
            let message = format!("Transcription task failed: {err}");
            state.logs.push("error", message.clone()).await;
            return (StatusCode::INTERNAL_SERVER_ERROR, message).into_response();
        }
    };

    {
        let mut cached = state.cached_context.lock().await;
        *cached = Some(CachedWhisperContext {
            model_id: model_id.clone(),
            context,
            state: wstate,
        });
    }

    if response_format.as_deref() == Some("text") {
        return text.into_response();
    }

    Json(TranscriptionResponse { text }).into_response()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(AppState::new())
        .setup(|app| {
            #[cfg(target_os = "macos")]
            {
                app.set_activation_policy(tauri::ActivationPolicy::Accessory);
            }
            #[cfg(desktop)]
            {
                app.handle().plugin(
                    tauri_plugin_global_shortcut::Builder::new()
                        .with_handler(|app, shortcut, event| {
                            let state = app.state::<AppState>();
                            let current = state.dictation_shortcut.blocking_lock().clone();
                            if current.as_ref().map(|item| item.id()) != Some(shortcut.id()) {
                                return;
                            }
                            let app_state = (*state).clone();
                            let app_handle = app.clone();
                            match event.state() {
                                ShortcutState::Pressed => {
                                    tauri::async_runtime::spawn(async move {
                                        if let Err(err) =
                                            start_dictation_inner(&app_state, &app_handle).await
                                        {
                                            app_state
                                                .logs
                                                .push(
                                                    "error",
                                                    format!("Dictation start failed: {err}"),
                                                )
                                                .await;
                                            return;
                                        }
                                        app_state.dictation.emit_state(&app_handle).await;
                                        let mut tray = app_state.dictation_tray_state.lock().await;
                                        tray.state = "listening".to_string();
                                        tray.queue_len = app_state.dictation.queue_len().await;
                                        tray.phase_started = Some(Instant::now());
                                        drop(tray);
                                        refresh_tray(&app_state).await;
                                        recompute_and_emit_app_status(&app_state).await;
                                    });
                                }
                                ShortcutState::Released => {
                                    eprintln!("[lib] global shortcut: key released");
                                    tauri::async_runtime::spawn(async move {
                                        if let Err(err) =
                                            stop_dictation_inner(&app_state, &app_handle).await
                                        {
                                            app_state
                                                .logs
                                                .push(
                                                    "error",
                                                    format!("Dictation stop failed: {err}"),
                                                )
                                                .await;
                                        }
                                    });
                                }
                            }
                        })
                        .build(),
                )?;
            }
            let state = app.state::<AppState>();
            {
                let mut handle_guard = state.app_handle.blocking_lock();
                *handle_guard = Some(app.handle().clone());
            }
            #[cfg(target_os = "macos")]
            {
                start_modifier_event_tap(app.handle().clone(), (*state).clone());
            }
            let base_dir = openstt_dir();
            let _ = std::fs::create_dir_all(&base_dir);

            let settings_path = settings_path();
            {
                let mut path_guard = state.settings_path.blocking_lock();
                *path_guard = Some(settings_path.clone());
            }
            let settings = load_ui_settings(&settings_path);
            {
                let mut settings_guard = state.ui_settings.blocking_lock();
                *settings_guard = settings;
            }
            {
                let app_handle = app.handle().clone();
                let state_clone = (*state).clone();
                let dictation_shortcut = state_clone
                    .ui_settings
                    .blocking_lock()
                    .dictation_shortcut
                    .clone();
                tauri::async_runtime::spawn(async move {
                    if let Err(err) =
                        register_dictation_shortcut(&app_handle, &state_clone, &dictation_shortcut)
                            .await
                    {
                        state_clone
                            .logs
                            .push(
                                "error",
                                format!("Failed to register dictation shortcut: {err}"),
                            )
                            .await;
                    }
                });
            }

            let config_path = config_path();
            {
                let mut config_guard = state.config_path.blocking_lock();
                *config_guard = Some(config_path.clone());
            }
            let config = load_app_config(&config_path);
            let normalized_model = normalize_model_id(&config.active_model_id);
            let is_elevenlabs = normalized_model.starts_with("elevenlabs:");
            let config_model = if is_elevenlabs || models::model_entry(&normalized_model).is_some()
            {
                normalized_model
            } else {
                default_model_id()
            };
            {
                let mut model_guard = state.active_model_id.blocking_lock();
                *model_guard = config_model;
            }

            let model_dir = models_dir();
            let _ = std::fs::create_dir_all(&model_dir);
            let _ = std::fs::create_dir_all(model_dir.join("whisper"));
            let _ = std::fs::create_dir_all(model_dir.join("mlx"));
            let _ = std::fs::create_dir_all(mlx_cache_dir());
            {
                let mut model_guard = state.models_dir.blocking_lock();
                *model_guard = Some(model_dir);
            }

            let log_path = logs_path();
            if let Some(parent) = log_path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            state.logs.set_log_path(log_path.clone());
            state.logs.load_from_file(&log_path);

            let snapshot = {
                let running = state.runtime.blocking_lock().is_some();
                let port = *state.port.blocking_lock();
                let model_id = state.active_model_id.blocking_lock().clone();
                let dictation = state.dictation_tray_state.blocking_lock().clone();
                let dictation_elapsed = dictation
                    .phase_started
                    .map(|started| format!("{:.1}s", started.elapsed().as_secs_f64()));
                TraySnapshot {
                    running,
                    port,
                    model_id,
                    dictation_state: dictation.state,
                    dictation_queue_len: dictation.queue_len,
                    dictation_elapsed,
                }
            };
            let tray_menu = build_tray_menu(app.handle(), &snapshot)?;
            let mut tray_builder = TrayIconBuilder::with_id(TRAY_ID)
                .menu(&tray_menu)
                .on_menu_event(
                    |app, event: tauri::menu::MenuEvent| match event.id().as_ref() {
                        TRAY_OPEN => show_main_window(app),
                        TRAY_START => {
                            let app_handle = app.clone();
                            tauri::async_runtime::spawn(async move {
                                let state = app_handle.state::<AppState>();
                                let port = *state.port.lock().await;
                                let _ = start_server_inner((*state).clone(), port).await;
                            });
                        }
                        TRAY_STOP => {
                            let app_handle = app.clone();
                            tauri::async_runtime::spawn(async move {
                                let state = app_handle.state::<AppState>();
                                let _ = stop_server_inner((*state).clone()).await;
                            });
                        }
                        TRAY_SETTINGS => open_page(app, "settings"),
                        TRAY_LOGS => open_page(app, "logs"),
                        TRAY_QUIT => app.exit(0),
                        _ => {}
                    },
                )
                .on_tray_icon_event(
                    |tray: &tauri::tray::TrayIcon<tauri::Wry>, event: TrayIconEvent| {
                        if let TrayIconEvent::DoubleClick { .. } = event {
                            show_main_window(tray.app_handle());
                        }
                    },
                );
            if let Some(icon) = app.default_window_icon().cloned() {
                tray_builder = tray_builder.icon(icon);
            }
            #[cfg(target_os = "macos")]
            {
                tray_builder = tray_builder.icon_as_template(true);
            }
            tray_builder.build(app)?;
            tauri::async_runtime::spawn({
                let state = (*state).clone();
                async move {
                    refresh_tray(&state).await;
                }
            });

            // Spawn 100ms ticker to update tray title during active dictation phases
            tauri::async_runtime::spawn({
                let state_for_ticker = (*state).clone();
                async move {
                    loop {
                        tokio::time::sleep(Duration::from_millis(100)).await;
                        let dictation = state_for_ticker.dictation_tray_state.lock().await.clone();
                        if dictation.state == "idle" {
                            continue;
                        }
                        let elapsed = dictation
                            .phase_started
                            .map(|started| format!("{:.1}s", started.elapsed().as_secs_f64()));
                        let title = format_dictation_state(&dictation.state, &elapsed);
                        let app_handle = state_for_ticker.app_handle.lock().await.clone();
                        if let Some(app_handle) = app_handle {
                            if let Some(tray) = app_handle.tray_by_id(TRAY_ID) {
                                let _ = tray.set_title(Some(&title));
                            }
                        }
                    }
                }
            });

            // Initial MLX runtime status check
            tauri::async_runtime::spawn({
                let state_clone = (*state).clone();
                async move {
                    let status = mlx_dependency_status_inner().await;
                    *state_clone.mlx_ready.lock().await = status.ready;
                    recompute_and_emit_app_status(&state_clone).await;
                }
            });

            let autostart = std::env::var("OPENSTT_AUTOSTART")
                .ok()
                .map(|value| value != "0")
                .unwrap_or(true);
            if autostart {
                let state_clone = (*state).clone();
                let port = *state.port.blocking_lock();
                tauri::async_runtime::spawn(async move {
                    let _ = start_server_inner(state_clone, port).await;
                });
            }

            // Pre-load the active model so the first transcription is fast
            {
                let state_clone = (*state).clone();
                tauri::async_runtime::spawn(async move {
                    preload_active_model(&state_clone).await;
                });
            }

            Ok(())
        })
        .on_window_event(|window, event| {
            if window.label() != "main" {
                return;
            }
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            start_server,
            stop_server,
            get_server_status,
            get_app_status,
            get_logs,
            clear_logs,
            get_ui_settings,
            set_ui_settings,
            transcribe_audio,
            paste_clipboard,
            start_dictation,
            stop_dictation,
            start_playground_recording,
            stop_playground_recording,
            get_dictation_state,
            list_models,
            download_model,
            delete_model,
            mlx_dependency_status,
            mlx_install_dependencies,
            mlx_reset_runtime,
            get_active_model,
            set_active_model,
            check_legacy_models,
            clean_legacy_models,
            check_all_permissions,
            open_permission_settings,
            restart_app,
            test_elevenlabs_api_key
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
