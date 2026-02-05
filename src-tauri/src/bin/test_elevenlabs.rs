use std::env;

#[tokio::main]
async fn main() {
    let api_key = env::args()
        .nth(1)
        .expect("Usage: test_elevenlabs <api_key>");

    println!("Testing ElevenLabs Realtime API...");
    println!("API Key: {}...", &api_key[..20]);

    match openstt_app_lib::elevenlabs_realtime::RealtimeSession::start(&api_key, 16000, None).await
    {
        Ok(session) => {
            println!("✓ WebSocket connected");

            // Send some silence as test audio
            let silence: Vec<i16> = vec![0; 1600];
            match session.send_audio(silence).await {
                Ok(_) => println!("✓ Audio chunk sent"),
                Err(e) => println!("✗ Failed to send audio: {}", e),
            }

            // Wait a moment for any response
            tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

            // Stop
            match session.stop().await {
                Ok(_) => println!("✓ Session stopped"),
                Err(e) => println!("✗ Failed to stop: {}", e),
            }

            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
            println!("\n✓ API connection test PASSED");
        }
        Err(e) => {
            println!("✗ Connection failed: {}", e);
        }
    }
}
