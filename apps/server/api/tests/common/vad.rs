#![allow(dead_code)]

use std::path::{Path, PathBuf};

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
