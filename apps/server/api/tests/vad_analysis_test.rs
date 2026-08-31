use std::path::PathBuf;

use api::component::vad::model::earshot::VadEarshot;
use api::config::vad::VadConfig;
use earshot::Detector;
use service::component::vad::Vad;
use tracing::info;
use tracing_test::traced_test;

mod common;
use common::vad::*;

struct SegmentReport {
    index: usize,
    start_ms: i32,
    duration_ms: f64,
    samples: usize,
}

#[tokio::test]
#[traced_test]
async fn test_vad_analysis() -> anyhow::Result<()> {
    let (speech, sr) = read_wav(&resource_path("speech_a.wav").to_string_lossy());
    assert_eq!(sr, SAMPLE_RATE);
    let input_dur_ms = speech.len() as f64 / SAMPLE_RATE as f64 * 1000.0;

    info!("=== VAD Analysis Report ===");
    info!("Input file: speech_a.wav");
    info!(
        "Input duration: {:.0} ms ({} samples at {} Hz)",
        input_dur_ms,
        speech.len(),
        SAMPLE_RATE
    );

    // ── Feed speech and collect segments ──
    // We use separate VAD instances per segment because VadEarshot
    // clears internally when silence exceeds min_silence_duration.
    let segments = detect_segments(&speech).await?;

    info!("");
    info!("--- Segments ---");
    info!(
        "{:<6} {:<12} {:<12} {:<10}",
        "#", "start(ms)", "duration(ms)", "samples"
    );
    for seg in &segments {
        info!(
            "{:<6} {:<12} {:<12.0} {:<10}",
            seg.index, seg.start_ms, seg.duration_ms, seg.samples
        );
    }
    info!("Detected {} speech segment(s)", segments.len());

    let total_speech_ms: f64 = segments.iter().map(|s| s.duration_ms).sum();
    let silence_ms = input_dur_ms - total_speech_ms;
    info!(
        "Speech: {:.0} ms ({:.1}%), Silence/Noise: {:.0} ms ({:.1}%)",
        total_speech_ms,
        total_speech_ms / input_dur_ms * 100.0,
        silence_ms.max(0.0),
        (silence_ms.max(0.0)) / input_dur_ms * 100.0,
    );

    // ── Export VAD-detected segment to WAV for manual listening ──
    std::fs::create_dir_all("./test_data")?;

    let segments = detect_segments(&speech).await?;
    if let Some(seg) = segments.first() {
        let start_sample = (seg.start_ms as f64 * SAMPLE_RATE as f64 / 1000.0) as usize;
        let end_sample = (start_sample + seg.samples).min(speech.len());
        let export_path = PathBuf::from("./test_data/vad_analysis_export.wav");
        wavers::write(
            &export_path,
            &speech[start_sample..end_sample],
            SAMPLE_RATE as i32,
            1,
        )?;
        info!("");
        info!("Exported first VAD-detected segment to: test_data/vad_analysis_export.wav");
        info!("Run: open test_data/vad_analysis_export.wav");
    }

    Ok(())
}

static TEST_FILES: &[&str] = &[
    "testset-audio-01",
    "testset-audio-10",
    "testset-audio-15",
    "testset-audio-17",
    "testset-audio-20",
    "testset-audio-22",
    "testset-audio-25",
];

async fn evaluate_file(name: &str, threshold: f32) -> anyhow::Result<FileAccuracy> {
    let wav_path = ten_vad_path(&format!("{name}.wav"));
    let scv_path = ten_vad_path(&format!("{name}.scv"));

    let (samples, sr) = read_wav(&wav_path.to_string_lossy());
    anyhow::ensure!(
        sr == SAMPLE_RATE,
        "{name}: expected {SAMPLE_RATE} Hz, got {sr}"
    );

    let segments = parse_scv(&scv_path.to_string_lossy());
    let ground_truth = frame_labels(samples.len(), sr, WINDOW_SIZE, &segments);

    let mut detector = Detector::default();
    let mut predictions = Vec::new();
    for chunk in samples.chunks(WINDOW_SIZE) {
        let mut frame = chunk.to_vec();
        if frame.len() < WINDOW_SIZE {
            frame.resize(WINDOW_SIZE, 0.0);
        }
        let score = detector.predict_f32(&frame);
        predictions.push(score > threshold);
    }

    let n = ground_truth.len().min(predictions.len());
    let (mut tp, mut fp, mut tn, mut fneg) = (0, 0, 0, 0);
    for i in 0..n {
        match (predictions[i], ground_truth[i]) {
            (true, true) => tp += 1,
            (true, false) => fp += 1,
            (false, false) => tn += 1,
            (false, true) => fneg += 1,
        }
    }

    Ok(FileAccuracy {
        name: name.to_string(),
        frames: n,
        correct: tp + tn,
        tp,
        fp,
        tn,
        fn_count: fneg,
    })
}

fn print_summary(title: &str, all: &[FileAccuracy]) {
    let total_frames: usize = all.iter().map(|a| a.frames).sum();
    let total_tp: usize = all.iter().map(|a| a.tp).sum();
    let total_fp: usize = all.iter().map(|a| a.fp).sum();
    let total_fn: usize = all.iter().map(|a| a.fn_count).sum();
    let total_correct: usize = all.iter().map(|a| a.correct).sum();

    let w_precision = if total_tp + total_fp > 0 {
        total_tp as f64 / (total_tp + total_fp) as f64
    } else {
        0.0
    };
    let w_recall = if total_tp + total_fn > 0 {
        total_tp as f64 / (total_tp + total_fn) as f64
    } else {
        0.0
    };
    let w_f1 = if w_precision + w_recall > 0.0 {
        2.0 * w_precision * w_recall / (w_precision + w_recall)
    } else {
        0.0
    };
    let accuracy = total_correct as f64 / total_frames as f64;
    let total_tn = total_frames - total_tp - total_fp - total_fn;

    info!("");
    info!("=== {title} ===");
    info!(
        "{:<8} {:<8} {:<8} {:<8} {:<8} {:<8} {:<12} {:<12} {:<12}",
        "frames", "correct", "TP", "FP", "TN", "FN", "Precision", "Recall", "F1"
    );
    info!(
        "{total_frames:<8} {total_correct:<8} {total_tp:<8} {total_fp:<8} {total_tn:<8} {total_fn:<8} {w_precision:<12.4} {w_recall:<12.4} {w_f1:<12.4}"
    );
    info!("Overall accuracy: {:.4}", accuracy);
    info!("Weighted F1: {:.4}", w_f1);
}

#[tokio::test]
#[traced_test]
async fn test_vad_accuracy() -> anyhow::Result<()> {
    let mut all_accuracy: Vec<FileAccuracy> = Vec::new();

    for name in TEST_FILES {
        let fa = evaluate_file(name, 0.5).await?;

        let precision = if fa.tp + fa.fp > 0 {
            fa.tp as f64 / (fa.tp + fa.fp) as f64
        } else {
            0.0
        };
        let recall = if fa.tp + fa.fn_count > 0 {
            fa.tp as f64 / (fa.tp + fa.fn_count) as f64
        } else {
            0.0
        };
        let f1 = if precision + recall > 0.0 {
            2.0 * precision * recall / (precision + recall)
        } else {
            0.0
        };

        info!("");
        info!("=== {} ===", fa.name);
        info!(
            "{:<8} {:<8} {:<8} {:<8} {:<8} {:<8} {:<12} {:<12} {:<12}",
            "frames", "correct", "TP", "FP", "TN", "FN", "Precision", "Recall", "F1"
        );
        info!(
            "{:<8} {:<8} {:<8} {:<8} {:<8} {:<8} {:<12.4} {:<12.4} {:<12.4}",
            fa.frames, fa.correct, fa.tp, fa.fp, fa.tn, fa.fn_count, precision, recall, f1
        );

        all_accuracy.push(fa);
    }

    print_summary("Summary (threshold=0.5)", &all_accuracy);

    let w_f1 = {
        let total_tp: usize = all_accuracy.iter().map(|a| a.tp).sum();
        let total_fp: usize = all_accuracy.iter().map(|a| a.fp).sum();
        let total_fn: usize = all_accuracy.iter().map(|a| a.fn_count).sum();
        let w_precision = if total_tp + total_fp > 0 {
            total_tp as f64 / (total_tp + total_fp) as f64
        } else {
            0.0
        };
        let w_recall = if total_tp + total_fn > 0 {
            total_tp as f64 / (total_tp + total_fn) as f64
        } else {
            0.0
        };
        if w_precision + w_recall > 0.0 {
            2.0 * w_precision * w_recall / (w_precision + w_recall)
        } else {
            0.0
        }
    };

    anyhow::ensure!(w_f1 > 0.5, "VAD accuracy F1={w_f1:.4} below threshold 0.5");

    Ok(())
}

#[tokio::test]
#[traced_test]
async fn test_threshold_sweep() -> anyhow::Result<()> {
    info!("=== VAD Threshold Sweep ===");
    info!(
        "{:<10} {:<10} {:<10} {:<10} {:<10} {:<12} {:<12} {:<12} {:<10}",
        "threshold", "frames", "TP", "FP", "FN", "Precision", "Recall", "F1", "Accuracy"
    );

    let mut best = (0.0f32, 0.0f64);

    let mut t = 0.30;
    while t <= 0.80 {
        let mut all = Vec::new();
        for name in TEST_FILES {
            all.push(evaluate_file(name, t).await?);
        }

        let total_frames: usize = all.iter().map(|a| a.frames).sum();
        let total_tp: usize = all.iter().map(|a| a.tp).sum();
        let total_fp: usize = all.iter().map(|a| a.fp).sum();
        let total_fn: usize = all.iter().map(|a| a.fn_count).sum();
        let total_correct: usize = all.iter().map(|a| a.correct).sum();

        let w_precision = if total_tp + total_fp > 0 {
            total_tp as f64 / (total_tp + total_fp) as f64
        } else {
            0.0
        };
        let w_recall = if total_tp + total_fn > 0 {
            total_tp as f64 / (total_tp + total_fn) as f64
        } else {
            0.0
        };
        let w_f1 = if w_precision + w_recall > 0.0 {
            2.0 * w_precision * w_recall / (w_precision + w_recall)
        } else {
            0.0
        };
        let accuracy = total_correct as f64 / total_frames as f64;

        info!(
            "{t:<10.2} {total_frames:<10} {total_tp:<10} {total_fp:<10} {total_fn:<10} {w_precision:<12.4} {w_recall:<12.4} {w_f1:<12.4} {accuracy:<10.4}"
        );

        if w_f1 > best.1 {
            best = (t, w_f1);
        }

        t = (t * 100.0 + 5.0).round() / 100.0;
    }

    info!("");
    info!("Best threshold: {:.2} (Weighted F1: {:.4})", best.0, best.1);

    Ok(())
}

struct FileAccuracy {
    name: String,
    frames: usize,
    correct: usize,
    tp: usize,
    fp: usize,
    tn: usize,
    fn_count: usize,
}

/// Feed audio and collect speech segments using polling of is_speech().
async fn detect_segments(audio: &[f32]) -> anyhow::Result<Vec<SegmentReport>> {
    let config = VadConfig::default();
    let mut vad = VadEarshot::new(&config)?;
    let mut segments: Vec<SegmentReport> = Vec::new();
    let mut was_speech = false;
    let frame_count = audio.len().div_ceil(WINDOW_SIZE);
    let frame_dur_ms = WINDOW_SIZE as f64 / SAMPLE_RATE as f64 * 1000.0;

    for i in 0..frame_count {
        let start = i * WINDOW_SIZE;
        let end = (start + WINDOW_SIZE).min(audio.len());
        let chunk_len = end - start;
        let mut frame = audio[start..end].to_vec();
        frame.resize(WINDOW_SIZE, 0.0);
        vad.accept_waveform(&frame)?;

        let now_speech = vad.is_speech();
        if now_speech && !was_speech {
            let start_ms = (i as f64 * frame_dur_ms) as i32;
            segments.push(SegmentReport {
                index: segments.len() + 1,
                start_ms,
                duration_ms: 0.0,
                samples: 0,
            });
        }

        if now_speech && let Some(last) = segments.last_mut() {
            last.duration_ms += frame_dur_ms;
            last.samples += chunk_len;
        }

        was_speech = now_speech;
    }

    Ok(segments)
}
