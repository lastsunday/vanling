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
}

impl VadEarshot {
    pub fn new(config: &VadConfig) -> Result<Self, AppError> {
        let detector = Detector::default();
        Ok(Self {
            detector,
            is_speech: false,
            min_silence_duration: config
                .min_silence_duration
                .expect("min_silence_duration should have default"),
            current_silence_duration: 0.0,
            prediction_list: Vec::new(),
            threshold: config.threshold.expect("threshold should have default"),
        })
    }
}

impl Vad for VadEarshot {
    fn accept_waveform(&mut self, samples: &[f32]) -> Result<f32, AppError> {
        let sample_rate: i64 = 16000;
        let score = self.detector.predict_f32(samples);
        if !self.is_speech {
            if score >= self.threshold {
                self.prediction_list.push(score);
            } else {
                self.clear();
            }

            // avoid some noise trigger speech detect
            if self.prediction_list.len() >= 5 && !self.is_speech {
                self.is_speech = true;
            }
        } else if score >= self.threshold {
            self.current_silence_duration = 0.0;
        } else {
            if self.current_silence_duration > self.min_silence_duration {
                self.clear();
            }
            self.current_silence_duration += (samples.len() as f32 / sample_rate as f32) * 1000.0;
        }
        Ok(score)
    }

    fn is_speech(&mut self) -> bool {
        self.is_speech
    }

    fn clear(&mut self) {
        self.detector.reset();
        self.is_speech = false;
        self.current_silence_duration = 0.0;
        self.prediction_list.clear();
    }

    fn window_size(&self) -> usize {
        256
    }
}
