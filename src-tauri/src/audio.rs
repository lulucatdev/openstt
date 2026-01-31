use std::path::Path;

use rubato::{FftFixedIn, Resampler};
use symphonia::core::audio::{AudioBufferRef, Signal};
use symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_NULL};
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

pub const TARGET_SAMPLE_RATE: u32 = 16_000;

pub fn load_and_resample(path: &Path) -> Result<Vec<f32>, String> {
    let (samples, sample_rate) = load_audio(path)?;
    if sample_rate == TARGET_SAMPLE_RATE {
        return Ok(samples);
    }
    resample(&samples, sample_rate, TARGET_SAMPLE_RATE)
}

fn load_audio(path: &Path) -> Result<(Vec<f32>, u32), String> {
    let file =
        std::fs::File::open(path).map_err(|err| format!("Failed to open audio file: {err}"))?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());

    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|value| value.to_str()) {
        hint.with_extension(ext);
    }

    let format_opts = FormatOptions::default();
    let metadata_opts = MetadataOptions::default();
    let probed = symphonia::default::get_probe()
        .format(&hint, mss, &format_opts, &metadata_opts)
        .map_err(|err| format!("Probe failed: {err:?}"))?;

    let mut format = probed.format;
    let track = format
        .tracks()
        .iter()
        .find(|track| track.codec_params.codec != CODEC_TYPE_NULL)
        .ok_or_else(|| "No audio track found".to_string())?;

    let track_id = track.id;
    let sample_rate = track
        .codec_params
        .sample_rate
        .ok_or_else(|| "Unknown sample rate".to_string())?;
    let channels = track.codec_params.channels.map(|c| c.count()).unwrap_or(1);

    let decoder_opts = DecoderOptions::default();
    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &decoder_opts)
        .map_err(|err| format!("Failed to create decoder: {err:?}"))?;

    let mut all_samples = Vec::new();
    loop {
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            Err(SymphoniaError::IoError(err))
                if err.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                break;
            }
            Err(err) => return Err(format!("Read error: {err:?}")),
        };

        if packet.track_id() != track_id {
            continue;
        }

        let decoded = match decoder.decode(&packet) {
            Ok(decoded) => decoded,
            Err(SymphoniaError::DecodeError(_)) => continue,
            Err(err) => return Err(format!("Decode error: {err:?}")),
        };

        let samples = buffer_to_mono_f32(&decoded, channels);
        all_samples.extend(samples);
    }

    Ok((all_samples, sample_rate))
}

fn buffer_to_mono_f32(buffer: &AudioBufferRef, channels: usize) -> Vec<f32> {
    match buffer {
        AudioBufferRef::F32(buf) => mix_to_mono_f32(buf.planes().planes(), buf.frames(), channels),
        AudioBufferRef::S16(buf) => {
            let planes: Vec<Vec<f32>> = buf
                .planes()
                .planes()
                .iter()
                .map(|plane| plane.iter().map(|&s| s as f32 / 32768.0).collect())
                .collect();
            let plane_refs: Vec<&[f32]> = planes.iter().map(|p| p.as_slice()).collect();
            mix_to_mono_f32(&plane_refs, buf.frames(), channels)
        }
        AudioBufferRef::S32(buf) => {
            let planes: Vec<Vec<f32>> = buf
                .planes()
                .planes()
                .iter()
                .map(|plane| plane.iter().map(|&s| s as f32 / 2147483648.0).collect())
                .collect();
            let plane_refs: Vec<&[f32]> = planes.iter().map(|p| p.as_slice()).collect();
            mix_to_mono_f32(&plane_refs, buf.frames(), channels)
        }
        _ => Vec::new(),
    }
}

fn mix_to_mono_f32(planes: &[&[f32]], frames: usize, channels: usize) -> Vec<f32> {
    if channels == 1 || planes.len() == 1 {
        return planes
            .first()
            .map(|plane| plane[..frames].to_vec())
            .unwrap_or_default();
    }

    let mut mono = vec![0.0f32; frames];
    let scale = 1.0 / channels as f32;
    for ch in 0..channels.min(planes.len()) {
        for (idx, sample) in planes[ch][..frames].iter().enumerate() {
            mono[idx] += sample * scale;
        }
    }
    mono
}

fn resample(samples: &[f32], from_rate: u32, to_rate: u32) -> Result<Vec<f32>, String> {
    if from_rate == to_rate {
        return Ok(samples.to_vec());
    }

    let mut resampler = FftFixedIn::<f32>::new(from_rate as usize, to_rate as usize, 1024, 2, 1)
        .map_err(|err| format!("Failed to create resampler: {err:?}"))?;
    let chunk_size = resampler.input_frames_max();
    let mut output =
        Vec::with_capacity((samples.len() as f64 * to_rate as f64 / from_rate as f64) as usize);

    let mut pos = 0;
    while pos < samples.len() {
        let end = (pos + chunk_size).min(samples.len());
        let chunk = &samples[pos..end];
        let input = if chunk.len() < chunk_size {
            let mut padded = chunk.to_vec();
            padded.resize(chunk_size, 0.0);
            vec![padded]
        } else {
            vec![chunk.to_vec()]
        };

        let resampled = resampler
            .process(&input, None)
            .map_err(|err| format!("Resample failed: {err:?}"))?;
        if let Some(channel) = resampled.get(0) {
            output.extend(channel);
        }
        pos += chunk_size;
    }

    Ok(output)
}
