use crate::recording::{self, RecordingSession};
use crate::AppState;
use serde::Serialize;
use std::collections::VecDeque;
use std::sync::Mutex as StdMutex;
use tauri::Emitter;
use tokio::sync::Mutex;

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
    playground_recording: StdMutex<Option<RecordingSession>>,
    queue: Mutex<VecDeque<Vec<u8>>>,
    processing: Mutex<bool>,
}

impl DictationManager {
    pub fn new() -> Self {
        Self {
            recording: StdMutex::new(None),
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
        if is_recording {
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

    pub async fn process_queue(
        &self,
        app_state: &AppState,
        app_handle: &tauri::AppHandle,
    ) {
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
                            let auto_paste = app_state
                                .ui_settings
                                .lock()
                                .await
                                .dictation_auto_paste;
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
                        .push("error", format!("Dictation transcription failed: {}", err.message))
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
