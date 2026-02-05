use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use tokio_tungstenite::tungstenite::{self, Message};

const ELEVENLABS_REALTIME_URL: &str = "wss://api.elevenlabs.io/v1/speech-to-text/realtime";

#[derive(Serialize)]
struct AudioChunkMessage {
    message_type: String,
    audio_base_64: String,
}

#[derive(Deserialize, Debug)]
struct WsMessage {
    message_type: String,
    #[serde(default)]
    text: String,
    #[serde(default)]
    error: String,
}

pub enum TypeAction {
    /// Replace the current draft utterance with this full partial text.
    SetDraft(String),
    /// Finalize current utterance with this committed text.
    CommitDraft(String),
}

pub struct RealtimeSession {
    audio_tx: mpsc::Sender<Vec<i16>>,
    stop_tx: mpsc::Sender<()>,
    transcript_rx: Arc<Mutex<mpsc::Receiver<TypeAction>>>,
}

impl RealtimeSession {
    pub async fn start(
        api_key: &str,
        sample_rate: u32,
        language: Option<String>,
    ) -> Result<Self, String> {
        eprintln!(
            "[elevenlabs] start, key_len: {}, sample_rate: {}",
            api_key.len(),
            sample_rate
        );

        // Build URL with query parameters
        let audio_format = format!("pcm_{}", sample_rate);
        let mut url = format!(
            "{}?audio_format={}&commit_strategy=vad",
            ELEVENLABS_REALTIME_URL, audio_format
        );
        if let Some(ref lang) = language {
            url.push_str(&format!("&language_code={}", lang));
        }

        // Build request with xi-api-key header
        let request = tungstenite::http::Request::builder()
            .uri(&url)
            .header("xi-api-key", api_key)
            .header("Host", "api.elevenlabs.io")
            .header("Connection", "Upgrade")
            .header("Upgrade", "websocket")
            .header("Sec-WebSocket-Version", "13")
            .header(
                "Sec-WebSocket-Key",
                tungstenite::handshake::client::generate_key(),
            )
            .body(())
            .map_err(|e| format!("Failed to build request: {e}"))?;

        let (ws_stream, _) = tokio_tungstenite::connect_async(request)
            .await
            .map_err(|e| format!("WebSocket connection failed: {e}"))?;

        eprintln!("[elevenlabs] WebSocket connected");

        let (mut ws_sink, mut ws_stream_rx) = ws_stream.split();

        // Channels
        let (audio_tx, mut audio_rx) = mpsc::channel::<Vec<i16>>(100);
        let (stop_tx, mut stop_rx) = mpsc::channel::<()>(1);
        let (transcript_tx, transcript_rx) = mpsc::channel::<TypeAction>(100);

        // Task to send audio chunks
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    Some(samples) = audio_rx.recv() => {
                        let bytes: Vec<u8> = samples
                            .iter()
                            .flat_map(|&s| s.to_le_bytes())
                            .collect();
                        let audio_base_64 = BASE64.encode(&bytes);

                        let chunk_msg = AudioChunkMessage {
                            message_type: "input_audio_chunk".to_string(),
                            audio_base_64,
                        };

                        if let Ok(json) = serde_json::to_string(&chunk_msg) {
                            let _ = ws_sink.send(Message::Text(json)).await;
                        }
                    }
                    _ = stop_rx.recv() => {
                        // Close websocket
                        let _ = ws_sink.close().await;
                        break;
                    }
                }
            }
            eprintln!("[elevenlabs] audio sender task ended");
        });

        // Task to receive transcripts
        tokio::spawn(async move {
            eprintln!("[elevenlabs] receiver task started");
            let mut last_draft = String::new();

            while let Some(msg) = ws_stream_rx.next().await {
                match msg {
                    Ok(Message::Text(text)) => {
                        eprintln!(
                            "[elevenlabs] ws msg: {}",
                            text.chars().take(100).collect::<String>()
                        );
                        if let Ok(ws_msg) = serde_json::from_str::<WsMessage>(&text) {
                            match ws_msg.message_type.as_str() {
                                "partial_transcript" => {
                                    let next_draft = ws_msg.text;
                                    eprintln!(
                                        "[elevenlabs] partial: {:?} (draft_len={})",
                                        next_draft,
                                        last_draft.chars().count()
                                    );
                                    if next_draft != last_draft {
                                        let _ = transcript_tx
                                            .send(TypeAction::SetDraft(next_draft.clone()))
                                            .await;
                                        last_draft = next_draft;
                                    }
                                }
                                "committed_transcript" => {
                                    let committed = ws_msg.text;
                                    eprintln!(
                                        "[elevenlabs] committed: {:?} (draft_len={})",
                                        committed,
                                        last_draft.chars().count()
                                    );
                                    let _ = transcript_tx
                                        .send(TypeAction::CommitDraft(committed))
                                        .await;
                                    last_draft.clear();
                                }
                                "session_started" => {
                                    eprintln!("[elevenlabs] session started");
                                }
                                "auth_error" | "error" => {
                                    eprintln!("[elevenlabs] error: {}", ws_msg.error);
                                    break;
                                }
                                other => {
                                    eprintln!("[elevenlabs] unhandled: {}", other);
                                }
                            }
                        }
                    }
                    Ok(Message::Close(_)) => {
                        eprintln!("[elevenlabs] ws closed");
                        break;
                    }
                    Err(e) => {
                        eprintln!("[elevenlabs] ws error: {}", e);
                        break;
                    }
                    _ => {}
                }
            }
            // Flush the last uncommitted partial as a final commit so that
            // text is not lost when the user releases the shortcut before VAD
            // triggers a committed_transcript.
            if !last_draft.is_empty() {
                eprintln!(
                    "[elevenlabs] flushing last draft as commit: {:?} (len={})",
                    last_draft,
                    last_draft.chars().count()
                );
                let _ = transcript_tx
                    .send(TypeAction::CommitDraft(last_draft))
                    .await;
            }

            eprintln!("[elevenlabs] receiver task ended");
        });

        Ok(Self {
            audio_tx,
            stop_tx,
            transcript_rx: Arc::new(Mutex::new(transcript_rx)),
        })
    }

    pub async fn send_audio(&self, samples: Vec<i16>) -> Result<(), String> {
        self.audio_tx
            .send(samples)
            .await
            .map_err(|e| format!("Failed to send audio: {e}"))
    }

    pub async fn stop(&self) -> Result<(), String> {
        self.stop_tx
            .send(())
            .await
            .map_err(|e| format!("Failed to send stop signal: {e}"))
    }

    pub async fn recv_transcript(&self) -> Option<TypeAction> {
        self.transcript_rx.lock().await.recv().await
    }

    pub fn clone_transcript_rx(&self) -> Arc<Mutex<mpsc::Receiver<TypeAction>>> {
        Arc::clone(&self.transcript_rx)
    }
}
