#![allow(dead_code)]

use std::path::{Path, PathBuf};

use anyhow;
use api::config::vad::VadConfig;
use service::ling::vad::Vad;

pub const SAMPLE_RATE: u32 = 16000;
pub const WINDOW_SIZE: usize = 256;

pub fn ws_root() -> &'static Path {
    static ROOT: std::sync::LazyLock<PathBuf> = std::sync::LazyLock::new(|| {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .to_path_buf()
    });
    &ROOT
}

pub fn resource_path(name: &str) -> PathBuf {
    ws_root().join("apps/server/api/resources/test").join(name)
}

pub fn read_wav(path: &str) -> (Vec<f32>, u32) {
    let result: (wavers::Samples<f32>, i32) = wavers::read(path).unwrap();
    (result.0.to_vec(), result.1 as u32)
}

pub fn silence_frame() -> Vec<f32> {
    vec![0.0; WINDOW_SIZE]
}

/// Parse TEN-vad SCV annotation file.
/// Format: "filename,start1,end1,label1,start2,end2,label2,..."
/// label: 0=non-speech, 1=speech
pub fn parse_scv(path: &str) -> Vec<(f64, f64, bool)> {
    let content = std::fs::read_to_string(path).unwrap();
    let parts: Vec<&str> = content.trim().split(',').collect();
    let mut segments = Vec::new();
    for chunk in parts[1..].chunks(3) {
        let start: f64 = chunk[0].parse().unwrap();
        let end: f64 = chunk[1].parse().unwrap();
        let is_speech = chunk[2] == "1";
        segments.push((start, end, is_speech));
    }
    segments
}

/// Convert segment-level SCV labels to frame-level bool array.
/// `hop` = samples per frame (256 for earshot).
pub fn frame_labels(
    audio_len: usize,
    sr: u32,
    hop: usize,
    segments: &[(f64, f64, bool)],
) -> Vec<bool> {
    let n_frames = audio_len.div_ceil(hop);
    let mut labels = vec![false; n_frames];
    for &(start, end, is_speech) in segments {
        if !is_speech {
            continue;
        }
        let frame_start = (start * sr as f64 / hop as f64) as usize;
        let frame_end = (end * sr as f64 / hop as f64) as usize;
        for f in frame_start..frame_end.min(n_frames) {
            labels[f] = true;
        }
    }
    labels
}

/// Path to a file in resources/test/ten_vad/
pub fn ten_vad_path(name: &str) -> PathBuf {
    resource_path("ten_vad").join(name)
}

/// Config with threshold=0.5, deactivation_threshold=0.5 for VAD tests.
pub fn vad_test_config(min_silence_duration: f32) -> VadConfig {
    VadConfig {
        threshold: Some(0.5),
        deactivation_threshold: Some(0.5),
        min_silence_duration: Some(min_silence_duration),
        ..Default::default()
    }
}

/// Load a WAV from resources/test/ and split into padded WINDOW_SIZE frames.
pub fn load_test_wav(name: &str) -> (Vec<Vec<f32>>, u32) {
    let (audio, sr) = read_wav(&resource_path(name).to_string_lossy());
    let frames = audio
        .chunks(WINDOW_SIZE)
        .map(|chunk| {
            let mut frame = chunk.to_vec();
            if frame.len() < WINDOW_SIZE {
                frame.resize(WINDOW_SIZE, 0.0);
            }
            frame
        })
        .collect();
    (frames, sr)
}

/// Feed N seconds of silence to the VAD.
pub fn feed_silence_seconds(vad: &mut impl Vad, seconds: u32) -> anyhow::Result<()> {
    let frames = (seconds as usize * SAMPLE_RATE as usize) / WINDOW_SIZE;
    for _ in 0..frames {
        vad.accept_waveform(&silence_frame())?;
    }
    Ok(())
}

/// Assert VAD never triggers speech on any of the frames.
pub fn assert_no_trigger(
    vad: &mut impl Vad,
    frames: &[Vec<f32>],
    label: &str,
) -> anyhow::Result<()> {
    for (i, frame) in frames.iter().enumerate() {
        vad.accept_waveform(frame)?;
        if vad.is_speech() {
            anyhow::bail!(
                "VAD falsely triggered on {label} at frame {i} ({}ms)",
                i * 16
            );
        }
    }
    Ok(())
}

/// Process frames until VAD triggers, then return.
pub fn process_until_trigger(
    vad: &mut impl Vad,
    frames: &[Vec<f32>],
    label: &str,
) -> anyhow::Result<()> {
    for (i, frame) in frames.iter().enumerate() {
        vad.accept_waveform(frame)?;
        if vad.is_speech() {
            tracing::info!("VAD triggered on {label} at frame {i} ({}ms)", i * 16);
            return Ok(());
        }
    }
    Ok(())
}
