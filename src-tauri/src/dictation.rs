use crate::elevenlabs_realtime::{RealtimeSession as ElevenLabsSession, TypeAction as ElevenLabsTypeAction};
use crate::soniox_realtime::{RealtimeSession as SonioxSession, TypeAction as SonioxTypeAction};
use crate::recording::{self, RecordingSession, StreamingRecordingSession};
use crate::AppState;
use cpal::traits::{DeviceTrait, HostTrait};
use serde::Serialize;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex as StdMutex};
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::Emitter;
use tokio::sync::{mpsc, Mutex};

// Unified TypeAction enum for internal use
pub enum TypeAction {
    SetDraft(String),
    CommitDraft(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SonioxWarmPolicy {
    Off,
    WindowMinutes(u32),
    Forever,
}

// Unified RealtimeSession wrapper to handle both providers
enum RealtimeSessionWrapper {
    ElevenLabs(ElevenLabsSession),
    Soniox(SonioxSession),
}

impl RealtimeSessionWrapper {
    async fn send_audio(&self, samples: Vec<i16>) -> Result<(), String> {
        match self {
            RealtimeSessionWrapper::ElevenLabs(s) => s.send_audio(samples).await,
            RealtimeSessionWrapper::Soniox(s) => s.send_audio(samples).await,
        }
    }

    async fn stop(&self) -> Result<(), String> {
        match self {
            RealtimeSessionWrapper::ElevenLabs(s) => s.stop().await,
            RealtimeSessionWrapper::Soniox(s) => s.stop().await,
        }
    }

    fn clone_transcript_rx(&self) -> Arc<Mutex<mpsc::Receiver<TypeAction>>> {
        match self {
            RealtimeSessionWrapper::ElevenLabs(s) => {
                let rx = s.clone_transcript_rx();
                // Convert receiver to our unified TypeAction
                Arc::new(Mutex::new(async_bridge_elevenlabs(rx)))
            }
            RealtimeSessionWrapper::Soniox(s) => {
                let rx = s.clone_transcript_rx();
                Arc::new(Mutex::new(async_bridge_soniox(rx)))
            }
        }
    }
}

fn async_bridge_elevenlabs(rx: Arc<Mutex<mpsc::Receiver<ElevenLabsTypeAction>>>) -> mpsc::Receiver<TypeAction> {
    let (tx, new_rx) = mpsc::channel(100);
    tokio::spawn(async move {
        while let Some(action) = rx.lock().await.recv().await {
            let unified = match action {
                ElevenLabsTypeAction::SetDraft(text) => TypeAction::SetDraft(text),
                ElevenLabsTypeAction::CommitDraft(text) => TypeAction::CommitDraft(text),
            };
            if tx.send(unified).await.is_err() {
                break;
            }
        }
    });
    new_rx
}

fn async_bridge_soniox(rx: Arc<Mutex<mpsc::Receiver<SonioxTypeAction>>>) -> mpsc::Receiver<TypeAction> {
    let (tx, new_rx) = mpsc::channel(100);
    tokio::spawn(async move {
        while let Some(action) = rx.lock().await.recv().await {
            let unified = match action {
                SonioxTypeAction::SetDraft(text) => TypeAction::SetDraft(text),
                SonioxTypeAction::CommitDraft(text) => TypeAction::CommitDraft(text),
            };
            if tx.send(unified).await.is_err() {
                break;
            }
        }
    });
    new_rx
}

#[derive(Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DictationState {
    Idle,
    Listening,
    Processing,
}

impl DictationState {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Listening => "listening",
            Self::Processing => "processing",
        }
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DictationStateEvent {
    pub state: String,
    pub queue_len: u32,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaygroundTranscriptionResult {
    pub text: String,
    pub error: Option<String>,
}

pub struct DictationManager {
    recording: StdMutex<Option<RecordingSession>>,
    streaming_recording: Mutex<Option<StreamingRecordingSession>>,
    realtime_session: Arc<Mutex<Option<RealtimeSessionWrapper>>>,
    realtime_active: Arc<Mutex<bool>>,
    realtime_pipeline_running: Arc<Mutex<bool>>,
    audio_forward_handle: Mutex<Option<tokio::task::JoinHandle<()>>>,
    soniox_warm_until_ms: Arc<Mutex<Option<u64>>>,
    soniox_warm_forever: Arc<Mutex<bool>>,
    soniox_warm_close_handle: Mutex<Option<tokio::task::JoinHandle<()>>>,
    playground_recording: StdMutex<Option<RecordingSession>>,
    queue: Mutex<VecDeque<Vec<u8>>>,
    processing: Mutex<bool>,
}

fn coalesce_typing_action(initial: TypeAction, rx: &mut mpsc::Receiver<TypeAction>) -> TypeAction {
    match initial {
        TypeAction::SetDraft(mut latest) => {
            while let Ok(next) = rx.try_recv() {
                match next {
                    TypeAction::SetDraft(text) => {
                        latest = text;
                    }
                    TypeAction::CommitDraft(text) => {
                        return TypeAction::CommitDraft(text);
                    }
                }
            }
            TypeAction::SetDraft(latest)
        }
        other => other,
    }
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn default_input_sample_rate() -> Result<u32, String> {
    let host = cpal::default_host();
    let device = host
        .default_input_device()
        .ok_or_else(|| "No input device available".to_string())?;
    let config = device
        .default_input_config()
        .map_err(|err| format!("Failed to get input config: {err}"))?;
    Ok(config.sample_rate().0)
}

impl DictationManager {
    pub fn new() -> Self {
        Self {
            recording: StdMutex::new(None),
            streaming_recording: Mutex::new(None),
            realtime_session: Arc::new(Mutex::new(None)),
            realtime_active: Arc::new(Mutex::new(false)),
            realtime_pipeline_running: Arc::new(Mutex::new(false)),
            audio_forward_handle: Mutex::new(None),
            soniox_warm_until_ms: Arc::new(Mutex::new(None)),
            soniox_warm_forever: Arc::new(Mutex::new(false)),
            soniox_warm_close_handle: Mutex::new(None),
            playground_recording: StdMutex::new(None),
            queue: Mutex::new(VecDeque::new()),
            processing: Mutex::new(false),
        }
    }

    /// Derive state from actual conditions rather than storing it.
    /// Recording takes priority: if mic is active the user sees "listening"
    /// even while the queue is draining in the background.
    pub fn current_state(&self) -> DictationState {
        let is_recording = self.recording.lock().unwrap().is_some();
        let is_realtime = self.realtime_active.try_lock().map(|g| *g).unwrap_or(false);
        if is_recording || is_realtime {
            return DictationState::Listening;
        }
        let is_processing = self.processing.try_lock().map(|g| *g).unwrap_or(false);
        if is_processing {
            return DictationState::Processing;
        }
        DictationState::Idle
    }

    pub async fn queue_len(&self) -> u32 {
        self.queue.lock().await.len() as u32
    }

    pub fn start_recording(&self) -> Result<(), String> {
        let session = RecordingSession::start()?;
        let mut rec = self.recording.lock().unwrap();
        *rec = Some(session);
        Ok(())
    }

    pub async fn stop_recording(&self) -> Result<DictationState, String> {
        let session = {
            let mut rec = self.recording.lock().unwrap();
            rec.take()
        };

        let Some(session) = session else {
            return Ok(self.current_state());
        };

        let (samples, sample_rate) = session.stop();
        if recording::is_too_short(samples.len(), sample_rate) {
            return Ok(self.current_state());
        }

        let wav = recording::encode_wav(&samples, sample_rate);
        self.queue.lock().await.push_back(wav);

        Ok(self.current_state())
    }

    /// Start realtime dictation with cloud provider WebSocket
    pub async fn start_realtime(
        &self,
        provider: &str,
        api_key: &str,
        language: Option<String>,
        _app_handle: tauri::AppHandle,
    ) -> Result<(), String> {
        eprintln!("[dictation] start_realtime called, provider: {}", provider);

        // Stop any previous forwarder (should already be finished)
        if let Some(handle) = self.audio_forward_handle.lock().await.take() {
            handle.abort();
        }

        // Cancel any pending warm-close timer when dictation starts.
        if let Some(handle) = self.soniox_warm_close_handle.lock().await.take() {
            handle.abort();
        }
        *self.soniox_warm_until_ms.lock().await = None;
        *self.soniox_warm_forever.lock().await = false;

        // Create channel for audio chunks
        let (chunk_tx, mut chunk_rx) = mpsc::channel::<Vec<i16>>(100);

        // Start streaming recording immediately (buffer chunks while connecting)
        let streaming = StreamingRecordingSession::start(chunk_tx)?;
        let sample_rate = streaming.sample_rate;

        // If the provider changed, stop the previous session.
        let mut stop_previous: Option<RealtimeSessionWrapper> = None;
        {
            let mut guard = self.realtime_session.lock().await;
            let matches_provider = match (&*guard, provider) {
                (Some(RealtimeSessionWrapper::Soniox(session)), "soniox") => {
                    session.is_alive() && session.sample_rate() == sample_rate
                }
                (Some(RealtimeSessionWrapper::ElevenLabs(_)), "elevenlabs") => true,
                (None, _) => true,
                _ => false,
            };
            if !matches_provider {
                stop_previous = guard.take();
            }
        }
        if let Some(old) = stop_previous {
            let _ = old.stop().await;
            *self.realtime_pipeline_running.lock().await = false;
        }

        // Ensure realtime session exists (reuse if possible)
        let mut created_new_session = false;
        {
            let mut guard = self.realtime_session.lock().await;
            if guard.is_none() {
                let realtime: RealtimeSessionWrapper = match provider {
                    "soniox" => {
                        let session = SonioxSession::start(api_key, sample_rate, language.clone()).await?;
                        RealtimeSessionWrapper::Soniox(session)
                    }
                    _ => {
                        // Default to ElevenLabs
                        let session = ElevenLabsSession::start(api_key, sample_rate, language.clone()).await?;
                        RealtimeSessionWrapper::ElevenLabs(session)
                    }
                };
                *guard = Some(realtime);
                created_new_session = true;
            }
        }

        // Store streaming recording and mark active
        *self.streaming_recording.lock().await = Some(streaming);
        *self.realtime_active.lock().await = true;

        // Spawn task to forward audio chunks to the active realtime session
        let realtime_session = self.realtime_session.clone();
        let forward_handle = tokio::spawn(async move {
            while let Some(samples) = chunk_rx.recv().await {
                let session = realtime_session.lock().await;
                if let Some(ref s) = *session {
                    if let Err(e) = s.send_audio(samples).await {
                        eprintln!("[dictation] send_audio error: {}", e);
                    }
                } else {
                    eprintln!("[dictation] session gone, stopping audio forward");
                    break;
                }
            }
            eprintln!("[dictation] audio forward task ended");
        });
        *self.audio_forward_handle.lock().await = Some(forward_handle);

        // Start transcript->typing pipeline once per realtime session
        let mut pipeline_running = self.realtime_pipeline_running.lock().await;
        if !*pipeline_running || created_new_session {
            *pipeline_running = true;
            drop(pipeline_running);

            let transcript_rx = {
                let guard = self.realtime_session.lock().await;
                guard
                    .as_ref()
                    .ok_or_else(|| "Realtime session missing".to_string())?
                    .clone_transcript_rx()
            };

            let pipeline_flag = self.realtime_pipeline_running.clone();
            let (typing_tx, mut typing_rx) = mpsc::channel::<TypeAction>(100);

            // Forward transcripts to typing channel
            tokio::spawn(async move {
                eprintln!("[dictation] transcript receiver task started");
                while let Some(action) = transcript_rx.lock().await.recv().await {
                    if typing_tx.send(action).await.is_err() {
                        break;
                    }
                }
                eprintln!("[dictation] transcript receiver task ended");
            });

            // Type received transcripts (commit-only)
            tokio::spawn(async move {
                eprintln!("[dictation] typing task started");
                let mut chars_typed: usize = 0;

                let handle_action = |action: TypeAction, chars: &mut usize| match action {
                    TypeAction::SetDraft(text) => {
                        eprintln!(
                            "[dictation] set_draft (skip): len={}",
                            text.chars().count()
                        );
                    }
                    TypeAction::CommitDraft(text) => {
                        let len = text.chars().count();
                        eprintln!("[dictation] commit_draft: len={}", len);
                        if !text.is_empty() {
                            if type_via_clipboard(&text).is_ok() {
                                *chars += len;
                            }
                        }
                    }
                };

                while let Some(action) = typing_rx.recv().await {
                    let action = coalesce_typing_action(action, &mut typing_rx);
                    handle_action(action, &mut chars_typed);
                }

                eprintln!("[dictation] typing task ended, chars_typed={}", chars_typed);
                *pipeline_flag.lock().await = false;
            });
        }

        Ok(())
    }

    /// Stop realtime dictation.
    pub async fn stop_realtime(&self, soniox_warm: SonioxWarmPolicy) -> Result<DictationState, String> {
        eprintln!("[dictation] stop_realtime called");
        *self.realtime_active.lock().await = false;
        eprintln!("[dictation] realtime_active set to false");

        // Stop the streaming recording
        if let Some(streaming) = self.streaming_recording.lock().await.take() {
            streaming.stop();
        }

        // Wait briefly for the audio forwarder to drain buffered mic chunks.
        if let Some(handle) = self.audio_forward_handle.lock().await.take() {
            let _ = tokio::time::timeout(tokio::time::Duration::from_millis(800), handle).await;
        }

        // Stop or finalize the realtime session
        let mut guard = self.realtime_session.lock().await;
        match guard.as_ref() {
            Some(RealtimeSessionWrapper::Soniox(session)) => {
                match soniox_warm {
                    SonioxWarmPolicy::Off => {
                        if let Some(session) = guard.take() {
                            let _ = session.stop().await;
                            *self.realtime_pipeline_running.lock().await = false;
                        }
                        if let Some(handle) = self.soniox_warm_close_handle.lock().await.take() {
                            handle.abort();
                        }
                        *self.soniox_warm_until_ms.lock().await = None;
                        *self.soniox_warm_forever.lock().await = false;
                    }
                    SonioxWarmPolicy::Forever => {
                        let _ = session.finalize().await;
                        if let Some(handle) = self.soniox_warm_close_handle.lock().await.take() {
                            handle.abort();
                        }
                        *self.soniox_warm_until_ms.lock().await = None;
                        *self.soniox_warm_forever.lock().await = true;
                    }
                    SonioxWarmPolicy::WindowMinutes(warm_minutes) => {
                        if warm_minutes == 0 {
                            // Treat 0 as Off to avoid ambiguous configuration.
                            if let Some(session) = guard.take() {
                                let _ = session.stop().await;
                                *self.realtime_pipeline_running.lock().await = false;
                            }
                            if let Some(handle) = self.soniox_warm_close_handle.lock().await.take() {
                                handle.abort();
                            }
                            *self.soniox_warm_until_ms.lock().await = None;
                            *self.soniox_warm_forever.lock().await = false;
                            return Ok(self.current_state());
                        }

                        let _ = session.finalize().await;
                        *self.soniox_warm_forever.lock().await = false;

                        // Schedule close after the warm window.
                        let until_ms = now_millis() + (warm_minutes as u64) * 60_000;
                        *self.soniox_warm_until_ms.lock().await = Some(until_ms);

                        if let Some(handle) = self.soniox_warm_close_handle.lock().await.take() {
                            handle.abort();
                        }

                        let warm_until = self.soniox_warm_until_ms.clone();
                        let realtime_session = self.realtime_session.clone();
                        let realtime_active = self.realtime_active.clone();
                        let pipeline_running = self.realtime_pipeline_running.clone();
                        let warm_forever_flag = self.soniox_warm_forever.clone();

                        let handle = tokio::spawn(async move {
                            tokio::time::sleep(tokio::time::Duration::from_secs(
                                (warm_minutes as u64) * 60,
                            ))
                            .await;

                            // If user started dictation again, don't close.
                            if *realtime_active.lock().await {
                                return;
                            }

                            // If warm window was updated, don't close.
                            if *warm_until.lock().await != Some(until_ms) {
                                return;
                            }

                            // If switched to forever mode, don't close.
                            if *warm_forever_flag.lock().await {
                                return;
                            }

                            let session = realtime_session.lock().await.take();
                            if let Some(session) = session {
                                let _ = session.stop().await;
                            }
                            *warm_until.lock().await = None;
                            *pipeline_running.lock().await = false;
                            eprintln!("[dictation] soniox warm window expired, session closed");
                        });

                        *self.soniox_warm_close_handle.lock().await = Some(handle);
                    }
                }
            }
            Some(RealtimeSessionWrapper::ElevenLabs(_)) => {
                if let Some(session) = guard.take() {
                    let _ = session.stop().await;
                    *self.realtime_pipeline_running.lock().await = false;
                }
            }
            None => {}
        }

        Ok(self.current_state())
    }

    pub async fn ensure_soniox_warm_connection(
        &self,
        api_key: &str,
        language: Option<String>,
        policy: SonioxWarmPolicy,
    ) -> Result<(), String> {
        if api_key.is_empty() {
            return Err("Soniox API key not configured".to_string());
        }

        // Never interfere with an active dictation.
        if *self.realtime_active.lock().await {
            return Ok(());
        }

        // Cancel any pending close timer.
        if let Some(handle) = self.soniox_warm_close_handle.lock().await.take() {
            handle.abort();
        }

        // Reuse an existing live Soniox session if possible.
        {
            let guard = self.realtime_session.lock().await;
            if let Some(RealtimeSessionWrapper::Soniox(session)) = guard.as_ref() {
                if session.is_alive() {
                    match policy {
                        SonioxWarmPolicy::Forever => {
                            *self.soniox_warm_forever.lock().await = true;
                            *self.soniox_warm_until_ms.lock().await = None;
                        }
                        SonioxWarmPolicy::WindowMinutes(minutes) => {
                            if minutes > 0 {
                                *self.soniox_warm_forever.lock().await = false;
                                *self.soniox_warm_until_ms.lock().await =
                                    Some(now_millis() + (minutes as u64) * 60_000);
                            }
                        }
                        SonioxWarmPolicy::Off => {}
                    }
                    return Ok(());
                }
            }
        }

        // Stop any previous session (provider might be different).
        let previous = {
            let mut guard = self.realtime_session.lock().await;
            guard.take()
        };
        if let Some(session) = previous {
            let _ = session.stop().await;
        }
        *self.realtime_pipeline_running.lock().await = false;

        // Establish a new Soniox WebSocket session.
        let sample_rate = default_input_sample_rate()?;
        let session = SonioxSession::start(api_key, sample_rate, language).await?;
        {
            let mut guard = self.realtime_session.lock().await;
            *guard = Some(RealtimeSessionWrapper::Soniox(session));
        }

        // Apply warm policy metadata for status display.
        match policy {
            SonioxWarmPolicy::Forever => {
                *self.soniox_warm_forever.lock().await = true;
                *self.soniox_warm_until_ms.lock().await = None;
            }
            SonioxWarmPolicy::WindowMinutes(minutes) => {
                if minutes > 0 {
                    *self.soniox_warm_forever.lock().await = false;
                    *self.soniox_warm_until_ms.lock().await =
                        Some(now_millis() + (minutes as u64) * 60_000);
                }
            }
            SonioxWarmPolicy::Off => {
                *self.soniox_warm_forever.lock().await = false;
                *self.soniox_warm_until_ms.lock().await = None;
            }
        }

        Ok(())
    }

    pub async fn close_realtime_session(&self) {
        // Stop any active mic stream.
        if let Some(streaming) = self.streaming_recording.lock().await.take() {
            streaming.stop();
        }
        if let Some(handle) = self.audio_forward_handle.lock().await.take() {
            handle.abort();
        }
        if let Some(handle) = self.soniox_warm_close_handle.lock().await.take() {
            handle.abort();
        }
        *self.soniox_warm_until_ms.lock().await = None;
        *self.soniox_warm_forever.lock().await = false;
        if let Some(session) = self.realtime_session.lock().await.take() {
            let _ = session.stop().await;
        }
        *self.realtime_active.lock().await = false;
        *self.realtime_pipeline_running.lock().await = false;
    }

    pub async fn soniox_realtime_connected(&self) -> bool {
        let guard = self.realtime_session.lock().await;
        match guard.as_ref() {
            Some(RealtimeSessionWrapper::Soniox(session)) => session.is_alive(),
            _ => false,
        }
    }

    pub async fn soniox_warm_until_ms(&self) -> Option<u64> {
        *self.soniox_warm_until_ms.lock().await
    }

    pub async fn soniox_warm_forever(&self) -> bool {
        *self.soniox_warm_forever.lock().await
    }

    pub async fn process_queue(&self, app_state: &AppState, app_handle: &tauri::AppHandle) {
        {
            let mut processing = self.processing.lock().await;
            if *processing {
                return;
            }
            *processing = true;
        }
        self.emit_state(app_handle).await;

        loop {
            let wav = self.queue.lock().await.pop_front();
            let Some(wav) = wav else {
                break;
            };

            self.emit_state(app_handle).await;

            let result = crate::transcribe_bytes(
                app_state,
                None,
                Some("dictation.wav".to_string()),
                wav,
                None,
                None,
                None,
                None,
            )
            .await;

            match result {
                Ok(text) => {
                    let trimmed = text.trim().to_string();
                    if !trimmed.is_empty() {
                        if let Err(err) = write_clipboard(&trimmed) {
                            app_state
                                .logs
                                .push("error", format!("Clipboard write failed: {err}"))
                                .await;
                        } else {
                            let auto_paste =
                                app_state.ui_settings.lock().await.dictation_auto_paste;
                            if auto_paste {
                                tokio::time::sleep(tokio::time::Duration::from_millis(80)).await;
                                let _ = crate::paste_clipboard_inner().await;
                            }
                        }
                    }
                }
                Err(err) => {
                    app_state
                        .logs
                        .push(
                            "error",
                            format!("Dictation transcription failed: {}", err.message),
                        )
                        .await;
                }
            }
        }

        {
            let mut processing = self.processing.lock().await;
            *processing = false;
        }
        self.emit_state(app_handle).await;
    }

    pub fn start_playground(&self) -> Result<(), String> {
        let session = RecordingSession::start()?;
        let mut rec = self.playground_recording.lock().unwrap();
        *rec = Some(session);
        Ok(())
    }

    pub async fn stop_playground_and_transcribe(
        &self,
        app_state: &AppState,
    ) -> PlaygroundTranscriptionResult {
        let session = {
            let mut rec = self.playground_recording.lock().unwrap();
            rec.take()
        };

        let Some(session) = session else {
            return PlaygroundTranscriptionResult {
                text: String::new(),
                error: Some("No playground recording in progress".to_string()),
            };
        };

        let (samples, sample_rate) = session.stop();
        if recording::is_too_short(samples.len(), sample_rate) {
            return PlaygroundTranscriptionResult {
                text: String::new(),
                error: None,
            };
        }

        let wav = recording::encode_wav(&samples, sample_rate);
        let result = crate::transcribe_bytes(
            app_state,
            None,
            Some("playground.wav".to_string()),
            wav,
            None,
            None,
            None,
            None,
        )
        .await;

        match result {
            Ok(text) => PlaygroundTranscriptionResult {
                text: text.trim().to_string(),
                error: None,
            },
            Err(err) => PlaygroundTranscriptionResult {
                text: String::new(),
                error: Some(err.message),
            },
        }
    }

    pub async fn emit_state(&self, app_handle: &tauri::AppHandle) {
        let state = self.current_state();
        let queue_len = self.queue_len().await;
        let _ = app_handle.emit(
            "dictation-state-changed",
            DictationStateEvent {
                state: state.as_str().to_string(),
                queue_len,
            },
        );
    }
}

fn write_clipboard(text: &str) -> Result<(), String> {
    let mut clipboard =
        arboard::Clipboard::new().map_err(|err| format!("Failed to open clipboard: {err}"))?;
    clipboard
        .set_text(text)
        .map_err(|err| format!("Failed to write clipboard: {err}"))
}

#[cfg(target_os = "macos")]
#[allow(dead_code)]
fn type_backspace(count: usize) -> Result<(), String> {
    use core_graphics::event::{CGEvent, CGEventTapLocation};
    use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};

    const BACKSPACE_KEYCODE: u16 = 51;
    const KEY_EVENT_DELAY_MS: u64 = 10;
    let source = CGEventSource::new(CGEventSourceStateID::HIDSystemState)
        .map_err(|_| "Failed to create event source".to_string())?;

    for _ in 0..count {
        let down = CGEvent::new_keyboard_event(source.clone(), BACKSPACE_KEYCODE, true)
            .map_err(|_| "Failed to create backspace down event".to_string())?;
        down.post(CGEventTapLocation::HID);

        let up = CGEvent::new_keyboard_event(source.clone(), BACKSPACE_KEYCODE, false)
            .map_err(|_| "Failed to create backspace up event".to_string())?;
        up.post(CGEventTapLocation::HID);

        std::thread::sleep(std::time::Duration::from_millis(KEY_EVENT_DELAY_MS));
    }

    Ok(())
}

#[cfg(not(target_os = "macos"))]
#[allow(dead_code)]
fn type_backspace(_count: usize) -> Result<(), String> {
    Err("Not supported on this platform".to_string())
}

#[cfg(target_os = "macos")]
#[allow(dead_code)]
fn type_text(text: &str) -> Result<(), String> {
    use core_graphics::event::{CGEvent, CGEventTapLocation};
    use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};

    if text.is_empty() {
        return Ok(());
    }

    const KEY_EVENT_DELAY_MS: u64 = 5;
    let source = CGEventSource::new(CGEventSourceStateID::HIDSystemState)
        .map_err(|_| "Failed to create event source".to_string())?;

    let down = CGEvent::new_keyboard_event(source.clone(), 0, true)
        .map_err(|_| "Failed to create key down event".to_string())?;
    down.set_string(text);
    down.post(CGEventTapLocation::HID);

    let up = CGEvent::new_keyboard_event(source, 0, false)
        .map_err(|_| "Failed to create key up event".to_string())?;
    up.post(CGEventTapLocation::HID);

    std::thread::sleep(std::time::Duration::from_millis(KEY_EVENT_DELAY_MS));

    Ok(())
}

#[cfg(not(target_os = "macos"))]
#[allow(dead_code)]
fn type_text(_text: &str) -> Result<(), String> {
    Err("Real-time typing not supported on this platform".to_string())
}

/// Insert text via the system clipboard (Cmd+V).  More reliable than
/// `set_string` CGEvents when a CJK input method is active, because
/// Cmd+V bypasses IME composition entirely.
#[cfg(target_os = "macos")]
fn type_via_clipboard(text: &str) -> Result<(), String> {
    use core_graphics::event::{CGEvent, CGEventFlags, CGEventTapLocation};
    use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};

    if text.is_empty() {
        return Ok(());
    }

    let mut clipboard =
        arboard::Clipboard::new().map_err(|e| format!("Failed to open clipboard: {e}"))?;
    let saved = clipboard.get_text().ok();

    clipboard
        .set_text(text)
        .map_err(|e| format!("Failed to set clipboard: {e}"))?;

    // Brief pause to ensure the pasteboard is ready
    std::thread::sleep(std::time::Duration::from_millis(3));

    // Cmd+V
    const V_KEYCODE: u16 = 9;
    let source = CGEventSource::new(CGEventSourceStateID::HIDSystemState)
        .map_err(|_| "Failed to create event source".to_string())?;

    let down = CGEvent::new_keyboard_event(source.clone(), V_KEYCODE, true)
        .map_err(|_| "Failed to create key event".to_string())?;
    down.set_flags(CGEventFlags::CGEventFlagCommand);
    down.post(CGEventTapLocation::HID);

    let up = CGEvent::new_keyboard_event(source, V_KEYCODE, false)
        .map_err(|_| "Failed to create key event".to_string())?;
    up.set_flags(CGEventFlags::CGEventFlagCommand);
    up.post(CGEventTapLocation::HID);

    // Wait for the target app to process the paste before restoring
    std::thread::sleep(std::time::Duration::from_millis(20));

    // Restore the original clipboard content
    match saved {
        Some(original) => {
            let _ = clipboard.set_text(original);
        }
        None => {
            let _ = clipboard.set_text("");
        }
    }

    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn type_via_clipboard(_text: &str) -> Result<(), String> {
    Err("Not supported on this platform".to_string())
}
