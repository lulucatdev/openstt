use crate::elevenlabs_realtime::{RealtimeSession, TypeAction};
use crate::recording::{self, RecordingSession, StreamingRecordingSession};
use crate::AppState;
use serde::Serialize;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex as StdMutex};
use tauri::Emitter;
use tokio::sync::{mpsc, Mutex};

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
    realtime_session: Arc<Mutex<Option<RealtimeSession>>>,
    realtime_active: Arc<Mutex<bool>>,
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

impl DictationManager {
    pub fn new() -> Self {
        Self {
            recording: StdMutex::new(None),
            streaming_recording: Mutex::new(None),
            realtime_session: Arc::new(Mutex::new(None)),
            realtime_active: Arc::new(Mutex::new(false)),
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

    pub async fn is_realtime_active(&self) -> bool {
        let active = *self.realtime_active.lock().await;
        eprintln!("[dictation] is_realtime_active: {}", active);
        active
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

    /// Start realtime dictation with ElevenLabs WebSocket
    pub async fn start_realtime(
        &self,
        api_key: &str,
        language: Option<String>,
        _app_handle: tauri::AppHandle,
    ) -> Result<(), String> {
        eprintln!("[dictation] start_realtime called");
        // Create channel for audio chunks
        let (chunk_tx, mut chunk_rx) = mpsc::channel::<Vec<i16>>(100);

        // Start streaming recording
        let streaming = StreamingRecordingSession::start(chunk_tx)?;
        let sample_rate = streaming.sample_rate;

        // Start ElevenLabs realtime session
        let realtime = RealtimeSession::start(api_key, sample_rate, language).await?;

        // Get transcript receiver before storing session
        let transcript_rx = realtime.clone_transcript_rx();

        *self.streaming_recording.lock().await = Some(streaming);
        *self.realtime_session.lock().await = Some(realtime);
        *self.realtime_active.lock().await = true;

        // Spawn task to forward audio chunks to ElevenLabs
        let realtime_session = self.realtime_session.clone();
        tokio::spawn(async move {
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

        // Spawn task to receive transcripts and type them
        let realtime_active = self.realtime_active.clone();
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

        // Type received transcripts
        tokio::spawn(async move {
            eprintln!("[dictation] typing task started");
            let mut chars_typed: usize = 0;

            // Commit-only mode: partials are NOT typed into the target app.
            // Only committed transcripts are pasted via the clipboard (Cmd+V).
            // This eliminates all backspace events and the cascading errors
            // they cause when macOS / the IME drops HID key events.
            let handle_action = |action: TypeAction, chars: &mut usize| match action {
                TypeAction::SetDraft(text) => {
                    eprintln!(
                        "[dictation] set_draft (skip): len={}",
                        text.chars().count()
                    );
                    // Don't type anything – just log.
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

            // Main loop: process actions while the session is active.
            loop {
                tokio::select! {
                    result = typing_rx.recv() => {
                        match result {
                            Some(action) => {
                                let action = coalesce_typing_action(action, &mut typing_rx);
                                handle_action(action, &mut chars_typed);
                            }
                            None => {
                                // Channel closed – pipeline fully shut down.
                                eprintln!("[dictation] typing channel closed");
                                break;
                            }
                        }
                    }
                    _ = tokio::time::sleep(tokio::time::Duration::from_millis(100)) => {
                        if !*realtime_active.lock().await {
                            break;
                        }
                    }
                }
            }

            // Drain phase: the WS receiver may still be flushing the last
            // partial as a CommitDraft.  Keep receiving until the channel
            // closes (all senders dropped) or a safety timeout expires.
            let deadline = tokio::time::Instant::now()
                + tokio::time::Duration::from_secs(1);
            eprintln!("[dictation] entering drain phase");
            loop {
                tokio::select! {
                    result = typing_rx.recv() => {
                        match result {
                            Some(action) => {
                                let action = coalesce_typing_action(action, &mut typing_rx);
                                handle_action(action, &mut chars_typed);
                            }
                            None => {
                                eprintln!("[dictation] drain: channel closed");
                                break;
                            }
                        }
                    }
                    _ = tokio::time::sleep_until(deadline) => {
                        eprintln!("[dictation] drain: timeout");
                        break;
                    }
                }
            }

            eprintln!("[dictation] typing task ended, chars_typed={}", chars_typed);
        });

        Ok(())
    }

    /// Stop realtime dictation
    pub async fn stop_realtime(&self) -> Result<DictationState, String> {
        eprintln!("[dictation] stop_realtime called");
        *self.realtime_active.lock().await = false;
        eprintln!("[dictation] realtime_active set to false");

        // Stop the realtime session
        if let Some(session) = self.realtime_session.lock().await.take() {
            let _ = session.stop().await;
        }

        // Stop the streaming recording
        if let Some(streaming) = self.streaming_recording.lock().await.take() {
            streaming.stop();
        }

        Ok(self.current_state())
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
