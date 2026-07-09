use api::config::{TtsModel, audio::AudioConfig};
use tracing::info;
use tracing_test::traced_test;

mod common;
use common::tts::*;

/// Scan length_scale values to find optimal timing (duration closest to standard).
async fn run_length_scale_scan(
    dir: &str,
    audio_cfg: &AudioConfig,
    wav_prefix: &str,
    ls_values: &[f32],
) -> anyhow::Result<()> {
    let path = ws_root().join(dir).to_string_lossy().into_owned();
    let mut rows: Vec<(f32, f64, f64, f64)> = Vec::new();

    for &ls in ls_values {
        let wav = format!("./test_data/{wav_prefix}_ls{ls}.wav");
        let opts = serde_json::json!({
            "num_threads": 2,
            "noise_scale": 0.667,
            "length_scale": ls,
            "speed": 1.0,
            "debug": false,
        });
        run_tts_test(
            &api::config::tts::TtsConfig {
                model: Some(TtsModel::MatchaTts),
                path: Some(path.clone()),
                options: Some(opts),
                ..Default::default()
            },
            audio_cfg,
            &wav,
        )
        .await?;

        let (samples, sr): (wavers::Samples<f32>, i32) = wavers::read(&wav)?;
        let diag = analyze_audio(
            &samples,
            sr as u32,
            std::time::Duration::ZERO,
            *TEST_TTS_TEXT_WEIGHT / 12.0,
        );
        info!("ls={ls}: {}", diag);
        let dev_pct = diag.std_diff_secs / diag.std_duration_secs * 100.0;
        rows.push((ls, diag.duration_secs, diag.std_diff_secs.abs(), dev_pct));
    }

    rows.sort_by(|a, b| a.2.partial_cmp(&b.2).unwrap());
    info!("=== length_scale scan summary (sorted by abs deviation) ===");
    info!(
        "{:<8} {:<10} {:<10} {:<10}",
        "ls", "duration", "dev_abs", "dev_pct"
    );
    for &(ls, dur, dev, dev_pct) in &rows {
        info!("{ls:<8.3} {dur:<6.2}s   {dev:<6.2}s   {dev_pct:<6.1}%");
    }
    if let Some(best) = rows.first() {
        info!(
            "Best length_scale: {:.3} (dev={:.2}s, {:.1}%)",
            best.0, best.2, best.3
        );
    }
    Ok(())
}

#[tokio::test]
#[traced_test]
async fn test_tts_matcha_zh_en_scan_ls() -> anyhow::Result<()> {
    run_length_scale_scan(
        "data/tts/model/matcha/matcha-icefall-zh-en/",
        &test_audio_config(),
        "matcha_zh_en",
        &[1.2, 1.3, 1.4, 1.5],
    )
    .await
}
