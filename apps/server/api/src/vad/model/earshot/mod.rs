use earshot::Detector;
use framework::error::AppError;
use service::chobits::vad::Vad;

use crate::config::vad::VadConfig;

pub struct VadEarshot {
    detector: Detector,
    is_speech: bool,
    /// unit ms
    min_silence_duration: f32,
    /// unit ms
    current_silence_duration: f32,
    prediction_list: Vec<f32>,
    threshold: f32,
    /// Deactivation threshold for hysteresis. Must be < threshold.
    /// Speech stops when score drops below this value during active speech.
    deactivation_threshold: f32,
    /// Minimum continuous speech duration (ms) before triggering is_speech.
    min_speech_duration: f32,
    /// Accumulated speech duration (ms) in current pre-speech window.
    speech_duration_ms: f32,
}

impl VadEarshot {
    pub fn new(config: &VadConfig) -> Result<Self, AppError> {
        let detector = Detector::default();
        let threshold = config.threshold.expect("threshold should have default");
        let deactivation_threshold = config
            .deactivation_threshold
            .unwrap_or(threshold)
            .min(threshold);
        Ok(Self {
            detector,
            is_speech: false,
            min_silence_duration: config
                .min_silence_duration
                .expect("min_silence_duration should have default"),
            current_silence_duration: 0.0,
            prediction_list: Vec::new(),
            threshold,
            deactivation_threshold,
            min_speech_duration: config.min_speech_duration.unwrap_or(300.0),
            speech_duration_ms: 0.0,
        })
    }
}

impl VadEarshot {
    fn clear_state(&mut self) {
        self.is_speech = false;
        self.current_silence_duration = 0.0;
        self.prediction_list.clear();
        self.speech_duration_ms = 0.0;
    }
}

impl Vad for VadEarshot {
    fn accept_waveform(&mut self, samples: &[f32]) -> Result<f32, AppError> {
        let sample_rate: i64 = 16000;
        let score = self.detector.predict_f32(samples);
        if !self.is_speech {
            if score >= self.threshold {
                self.prediction_list.push(score);
                self.speech_duration_ms += (samples.len() as f32 / sample_rate as f32) * 1000.0;
            } else {
                self.prediction_list.clear();
                self.speech_duration_ms = 0.0;
            }

            if self.prediction_list.len() >= 10
                && self.speech_duration_ms >= self.min_speech_duration
            {
                self.is_speech = true;
            }
        } else if score >= self.deactivation_threshold {
            self.current_silence_duration = 0.0;
        } else {
            if self.current_silence_duration > self.min_silence_duration {
                self.clear_state();
            }
            self.current_silence_duration += (samples.len() as f32 / sample_rate as f32) * 1000.0;
        }
        Ok(score)
    }

    fn is_speech(&mut self) -> bool {
        self.is_speech
    }

    fn clear(&mut self) {
        self.clear_state();
    }

    fn window_size(&self) -> usize {
        256
    }
}
