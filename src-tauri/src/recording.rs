use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::sync::{Arc, Mutex as StdMutex};

pub struct RecordingSession {
    stream: cpal::Stream,
    buffer: Arc<StdMutex<Vec<f32>>>,
    sample_rate: u32,
}

// Safety: RecordingSession is always protected by a StdMutex and accessed
// from one logical owner at a time. cpal::Stream is not Send/Sync due to
// platform internals, but we never share the stream across threads without
// synchronisation.
unsafe impl Send for RecordingSession {}
unsafe impl Sync for RecordingSession {}

impl RecordingSession {
    pub fn start() -> Result<Self, String> {
        let host = cpal::default_host();
        let device = host
            .default_input_device()
            .ok_or_else(|| "No input device available".to_string())?;
        let config = device
            .default_input_config()
            .map_err(|err| format!("Failed to get input config: {err}"))?;
        let sample_rate = config.sample_rate().0;
        let channels = config.channels() as usize;
        let buffer: Arc<StdMutex<Vec<f32>>> = Arc::new(StdMutex::new(Vec::new()));
        let buf_clone = Arc::clone(&buffer);

        let err_fn = |err: cpal::StreamError| {
            eprintln!("[recording] stream error: {err}");
        };

        let stream = match config.sample_format() {
            cpal::SampleFormat::F32 => device
                .build_input_stream(
                    &config.into(),
                    move |data: &[f32], _: &cpal::InputCallbackInfo| {
                        let mut buf = buf_clone.lock().unwrap();
                        if channels == 1 {
                            buf.extend_from_slice(data);
                        } else {
                            for chunk in data.chunks(channels) {
                                let sum: f32 = chunk.iter().sum();
                                buf.push(sum / channels as f32);
                            }
                        }
                    },
                    err_fn,
                    None,
                )
                .map_err(|err| format!("Failed to build input stream: {err}"))?,
            cpal::SampleFormat::I16 => {
                let buf_clone = Arc::clone(&buffer);
                device
                    .build_input_stream(
                        &config.into(),
                        move |data: &[i16], _: &cpal::InputCallbackInfo| {
                            let mut buf = buf_clone.lock().unwrap();
                            if channels == 1 {
                                for &sample in data {
                                    buf.push(sample as f32 / i16::MAX as f32);
                                }
                            } else {
                                for chunk in data.chunks(channels) {
                                    let sum: f32 =
                                        chunk.iter().map(|&s| s as f32 / i16::MAX as f32).sum();
                                    buf.push(sum / channels as f32);
                                }
                            }
                        },
                        err_fn,
                        None,
                    )
                    .map_err(|err| format!("Failed to build input stream: {err}"))?
            }
            cpal::SampleFormat::U16 => {
                let buf_clone = Arc::clone(&buffer);
                device
                    .build_input_stream(
                        &config.into(),
                        move |data: &[u16], _: &cpal::InputCallbackInfo| {
                            let mut buf = buf_clone.lock().unwrap();
                            if channels == 1 {
                                for &sample in data {
                                    buf.push(
                                        (sample as f32 / u16::MAX as f32) * 2.0 - 1.0,
                                    );
                                }
                            } else {
                                for chunk in data.chunks(channels) {
                                    let sum: f32 = chunk
                                        .iter()
                                        .map(|&s| (s as f32 / u16::MAX as f32) * 2.0 - 1.0)
                                        .sum();
                                    buf.push(sum / channels as f32);
                                }
                            }
                        },
                        err_fn,
                        None,
                    )
                    .map_err(|err| format!("Failed to build input stream: {err}"))?
            }
            format => return Err(format!("Unsupported sample format: {format:?}")),
        };

        stream
            .play()
            .map_err(|err| format!("Failed to start recording: {err}"))?;

        Ok(Self {
            stream,
            buffer,
            sample_rate,
        })
    }

    pub fn stop(self) -> (Vec<f32>, u32) {
        drop(self.stream);
        let samples = self.buffer.lock().unwrap().clone();
        (samples, self.sample_rate)
    }
}

pub fn is_too_short(num_samples: usize, sample_rate: u32) -> bool {
    let min_samples = (sample_rate as f64 * 0.15) as usize;
    num_samples < min_samples
}

pub fn encode_wav(samples: &[f32], sample_rate: u32) -> Vec<u8> {
    let data_size = (samples.len() * 2) as u32;
    let mut buffer = Vec::with_capacity(44 + data_size as usize);

    // RIFF header
    buffer.extend_from_slice(b"RIFF");
    buffer.extend_from_slice(&(36 + data_size).to_le_bytes());
    buffer.extend_from_slice(b"WAVE");

    // fmt chunk
    buffer.extend_from_slice(b"fmt ");
    buffer.extend_from_slice(&16u32.to_le_bytes());
    buffer.extend_from_slice(&1u16.to_le_bytes()); // PCM
    buffer.extend_from_slice(&1u16.to_le_bytes()); // mono
    buffer.extend_from_slice(&sample_rate.to_le_bytes());
    buffer.extend_from_slice(&(sample_rate * 2).to_le_bytes()); // byte rate
    buffer.extend_from_slice(&2u16.to_le_bytes()); // block align
    buffer.extend_from_slice(&16u16.to_le_bytes()); // bits per sample

    // data chunk
    buffer.extend_from_slice(b"data");
    buffer.extend_from_slice(&data_size.to_le_bytes());

    for &sample in samples {
        let clamped = sample.clamp(-1.0, 1.0);
        let int_val = if clamped < 0.0 {
            (clamped * 0x8000 as f32) as i16
        } else {
            (clamped * 0x7FFF as f32) as i16
        };
        buffer.extend_from_slice(&int_val.to_le_bytes());
    }

    buffer
}
