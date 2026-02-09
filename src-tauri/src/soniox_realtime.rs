use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use tokio_tungstenite::tungstenite::Message;

const SONIOX_WEBSOCKET_URL: &str = "wss://stt-rt.soniox.com/transcribe-websocket";
const SONIOX_REALTIME_MODEL: &str = "stt-rt-v4";

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
struct StartConfig {
    api_key: String,
    model: String,
    audio_format: String,
    sample_rate: u32,
    num_channels: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    language_hints: Option<Vec<String>>,
    enable_endpoint_detection: bool,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "snake_case")]
struct WsToken {
    #[serde(default)]
    text: String,
    #[serde(default)]
    is_final: bool,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "snake_case")]
struct WsResponse {
    #[serde(default)]
    tokens: Vec<WsToken>,
    #[serde(default)]
    finished: bool,
    #[serde(default)]
    error_code: Option<u16>,
    #[serde(default)]
    error_message: Option<String>,
}

pub enum TypeAction {
    /// Replace the current draft utterance with this full partial text.
    SetDraft(String),
    /// Finalize current utterance with this committed text.
    CommitDraft(String),
}

enum ControlMessage {
    Finalize,
    Close,
}

pub struct RealtimeSession {
    audio_tx: mpsc::Sender<Vec<i16>>,
    control_tx: mpsc::Sender<ControlMessage>,
    transcript_rx: Arc<Mutex<mpsc::Receiver<TypeAction>>>,
    alive: Arc<AtomicBool>,
    sample_rate: u32,
}

impl RealtimeSession {
    pub async fn start(
        api_key: &str,
        sample_rate: u32,
        language: Option<String>,
    ) -> Result<Self, String> {
        eprintln!(
            "[soniox] start, key_len: {}, sample_rate: {}",
            api_key.len(),
            sample_rate
        );

        eprintln!("[soniox] connecting to {}", SONIOX_WEBSOCKET_URL);
        let (ws_stream, response) = match tokio::time::timeout(
            tokio::time::Duration::from_secs(5),
            tokio_tungstenite::connect_async(SONIOX_WEBSOCKET_URL),
        )
        .await
        {
            Ok(Ok((stream, resp))) => (stream, resp),
            Ok(Err(e)) => return Err(format!("WebSocket connection failed: {e}")),
            Err(_) => return Err("WebSocket connection timed out (5s)".to_string()),
        };

        eprintln!(
            "[soniox] WebSocket connected, response status: {:?}",
            response.status()
        );

        let (mut ws_sink, mut ws_stream_rx) = ws_stream.split();

        // Channels
        let (audio_tx, mut audio_rx) = mpsc::channel::<Vec<i16>>(100);
        let (control_tx, mut control_rx) = mpsc::channel::<ControlMessage>(16);
        let (transcript_tx, transcript_rx) = mpsc::channel::<TypeAction>(100);
        let alive = Arc::new(AtomicBool::new(true));

        // Send config as the first text message.
        let config = StartConfig {
            api_key: api_key.to_string(),
            model: SONIOX_REALTIME_MODEL.to_string(),
            audio_format: "pcm_s16le".to_string(),
            sample_rate,
            num_channels: 1,
            language_hints: language.map(|lang| vec![lang]),
            // Push-to-talk: we finalize manually on key release.
            enable_endpoint_detection: false,
        };
        eprintln!(
            "[soniox] sending config: model={}, audio_format={}, sample_rate={}, channels={}, language_hints={:?}, endpointing={}",
            config.model,
            config.audio_format,
            config.sample_rate,
            config.num_channels,
            config.language_hints,
            config.enable_endpoint_detection
        );

        let config_json = serde_json::to_string(&config)
            .map_err(|e| format!("Failed to serialize start config: {e}"))?;
        ws_sink
            .send(Message::Text(config_json))
            .await
            .map_err(|e| format!("Failed to send start config: {e}"))?;

        // Task to send audio chunks.
        // Soniox expects binary WebSocket frames containing raw audio bytes.
        let alive_for_sender = Arc::clone(&alive);
        tokio::spawn(async move {
            let keepalive_every = tokio::time::Duration::from_secs(10);
            let mut keepalive = tokio::time::interval(keepalive_every);
            keepalive.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            let mut last_audio_at = tokio::time::Instant::now();

            loop {
                tokio::select! {
                    Some(samples) = audio_rx.recv() => {
                        let bytes: Vec<u8> = samples
                            .iter()
                            .flat_map(|&s| s.to_le_bytes())
                            .collect();
                        if ws_sink.send(Message::Binary(bytes)).await.is_err() {
                            break;
                        }
                        last_audio_at = tokio::time::Instant::now();
                    }
                    Some(cmd) = control_rx.recv() => {
                        // Drain any queued audio before control actions.
                        while let Ok(samples) = audio_rx.try_recv() {
                            let bytes: Vec<u8> = samples
                                .iter()
                                .flat_map(|&s| s.to_le_bytes())
                                .collect();
                            if ws_sink.send(Message::Binary(bytes)).await.is_err() {
                                break;
                            }
                            last_audio_at = tokio::time::Instant::now();
                        }

                        // Add ~200ms of silence before finalizing.
                        // See: https://soniox.com/docs/stt/rt/manual-finalization
                        let silence_len = (sample_rate as usize) / 5;
                        if silence_len > 0 {
                            let silence = vec![0i16; silence_len];
                            let silence_bytes: Vec<u8> = silence
                                .iter()
                                .flat_map(|&s| s.to_le_bytes())
                                .collect();
                            let _ = ws_sink.send(Message::Binary(silence_bytes)).await;
                            last_audio_at = tokio::time::Instant::now();
                        }

                        match cmd {
                            ControlMessage::Finalize => {
                                let _ = ws_sink
                                    .send(Message::Text("{\"type\":\"finalize\"}".to_string()))
                                    .await;
                            }
                            ControlMessage::Close => {
                                let _ = ws_sink
                                    .send(Message::Text("{\"type\":\"finalize\"}".to_string()))
                                    .await;
                                // Send empty frame to end the stream.
                                // See: https://soniox.com/docs/stt/api-reference/websocket-api#ending-the-stream
                                let _ = ws_sink.send(Message::Binary(Vec::new())).await;
                                break;
                            }
                        }
                    }
                    _ = keepalive.tick() => {
                        // Keep the session alive when not sending audio.
                        // See: https://soniox.com/docs/stt/rt/connection-keepalive
                        if tokio::time::Instant::now().duration_since(last_audio_at)
                            >= keepalive_every
                        {
                            let _ = ws_sink
                                .send(Message::Text("{\"type\":\"keepalive\"}".to_string()))
                                .await;
                        }
                    }
                }
            }
            eprintln!("[soniox] audio sender task ended");
            alive_for_sender.store(false, Ordering::Relaxed);
        });

        // Task to receive transcripts.
        let alive_for_receiver = Arc::clone(&alive);
        tokio::spawn(async move {
            eprintln!("[soniox] receiver task started");
            let mut final_text = String::new();
            let mut last_draft = String::new();

            while let Some(msg) = ws_stream_rx.next().await {
                match msg {
                    Ok(Message::Text(text)) => {
                        eprintln!(
                            "[soniox] ws msg: {}",
                            text.chars().take(120).collect::<String>()
                        );

                        let res = match serde_json::from_str::<WsResponse>(&text) {
                            Ok(value) => value,
                            Err(err) => {
                                eprintln!("[soniox] failed to parse ws json: {err}");
                                continue;
                            }
                        };

                        if let Some(code) = res.error_code {
                            let message = res.error_message.unwrap_or_else(|| "Unknown error".to_string());
                            eprintln!("[soniox] error {}: {}", code, message);
                            break;
                        }

                        let mut non_final = String::new();
                        let mut saw_fin = false;

                        for token in res.tokens {
                            if token.text.is_empty() {
                                continue;
                            }

                            if token.is_final {
                                match token.text.as_str() {
                                    "<end>" => {
                                        // Endpoint detection is disabled for push-to-talk.
                                        // Ignore this token if it ever appears.
                                    }
                                    "<fin>" => {
                                        saw_fin = true;
                                    }
                                    _ => {
                                        final_text.push_str(&token.text);
                                    }
                                }
                            } else {
                                non_final.push_str(&token.text);
                            }
                        }

                        let draft = format!("{}{}", final_text, non_final);
                        if draft != last_draft {
                            let _ = transcript_tx.send(TypeAction::SetDraft(draft.clone())).await;
                            last_draft = draft;
                        }

                        if saw_fin {
                            let committed = last_draft.trim().to_string();
                            if !committed.is_empty() {
                                let _ = transcript_tx.send(TypeAction::CommitDraft(committed)).await;
                            }
                            final_text.clear();
                            last_draft.clear();
                        }

                        if res.finished {
                            let committed = last_draft.trim().to_string();
                            if !committed.is_empty() {
                                let _ = transcript_tx.send(TypeAction::CommitDraft(committed)).await;
                            }
                            break;
                        }
                    }
                    Ok(Message::Close(_)) => {
                        eprintln!("[soniox] ws closed");
                        break;
                    }
                    Err(e) => {
                        eprintln!("[soniox] ws error: {}", e);
                        break;
                    }
                    _ => {}
                }
            }

            // Flush the last draft as a final commit so text isn't lost.
            let committed = last_draft.trim().to_string();
            if !committed.is_empty() {
                eprintln!(
                    "[soniox] flushing last draft as commit: {:?} (len={})",
                    committed,
                    committed.chars().count()
                );
                let _ = transcript_tx.send(TypeAction::CommitDraft(committed)).await;
            }

            eprintln!("[soniox] receiver task ended");
            alive_for_receiver.store(false, Ordering::Relaxed);
        });

        Ok(Self {
            audio_tx,
            control_tx,
            transcript_rx: Arc::new(Mutex::new(transcript_rx)),
            alive,
            sample_rate,
        })
    }

    pub async fn send_audio(&self, samples: Vec<i16>) -> Result<(), String> {
        self.audio_tx
            .send(samples)
            .await
            .map_err(|e| format!("Failed to send audio: {e}"))
    }

    pub async fn finalize(&self) -> Result<(), String> {
        self.control_tx
            .send(ControlMessage::Finalize)
            .await
            .map_err(|e| format!("Failed to send finalize: {e}"))
    }

    pub async fn close(&self) -> Result<(), String> {
        self.control_tx
            .send(ControlMessage::Close)
            .await
            .map_err(|e| format!("Failed to send close: {e}"))
    }

    pub async fn stop(&self) -> Result<(), String> {
        self.close().await
    }

    pub fn is_alive(&self) -> bool {
        self.alive.load(Ordering::Relaxed)
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    pub async fn recv_transcript(&self) -> Option<TypeAction> {
        self.transcript_rx.lock().await.recv().await
    }

    pub fn clone_transcript_rx(&self) -> Arc<Mutex<mpsc::Receiver<TypeAction>>> {
        Arc::clone(&self.transcript_rx)
    }
}
