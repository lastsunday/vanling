use std::collections::VecDeque;

use earshot::Detector;
use framework::error::AppError;
use service::ling::vad::Vad;

use crate::config::vad::VadConfig;

mod preprocessor;
use self::preprocessor::RnnoisePreprocessor;

const VAD_RING_SIZE: usize = 40;
const VAD_RING_TRIGGER_RATIO: f32 = 0.55;
const VAD_WINDOW_SIZE: usize = 256;
const FRAME_DURATION_MS: f32 = 16.0;
const INPUT_FRAME_16KHZ: usize = 160;

pub struct VadEarshot {
    detector: Detector,
    is_speech: bool,
    min_silence_duration: f32,
    current_silence_duration: f32,
    prediction_list: Vec<f32>,
    threshold: f32,
    deactivation_threshold: f32,
    min_speech_duration: f32,
    speech_duration_ms: f32,
    consecutive_above_threshold: u32,
    is_speech_ring: VecDeque<bool>,
    rms_threshold: f32,
    raw_buffer: Vec<f32>,
    denoised_buffer: VecDeque<f32>,
    last_score: f32,
    preprocessor: RnnoisePreprocessor,
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
            consecutive_above_threshold: 0,
            is_speech_ring: VecDeque::with_capacity(VAD_RING_SIZE),
            rms_threshold: 0.003,
            raw_buffer: Vec::with_capacity(INPUT_FRAME_16KHZ),
            denoised_buffer: VecDeque::with_capacity(INPUT_FRAME_16KHZ * 4),
            last_score: 0.0,
            preprocessor: RnnoisePreprocessor::new()?,
        })
    }
}

impl VadEarshot {
    fn clear_state(&mut self) {
        self.detector.reset();
        self.is_speech = false;
        self.current_silence_duration = 0.0;
        self.prediction_list.clear();
        self.speech_duration_ms = 0.0;
        self.consecutive_above_threshold = 0;
        self.is_speech_ring.clear();
        self.raw_buffer.clear();
        self.denoised_buffer.clear();
        self.last_score = 0.0;
        self.preprocessor.reset();
    }

    fn ring_speech_ratio(&self) -> f32 {
        if self.is_speech_ring.is_empty() {
            return 1.0;
        }
        let count = self.is_speech_ring.iter().filter(|&&b| b).count();
        count as f32 / self.is_speech_ring.len() as f32
    }

    fn compute_rms(samples: &[f32]) -> f32 {
        let sum_sq: f32 = samples.iter().map(|s| s * s).sum();
        (sum_sq / samples.len() as f32).sqrt()
    }
}

impl Vad for VadEarshot {
    fn accept_waveform(&mut self, samples: &[f32]) -> Result<f32, AppError> {
        self.raw_buffer.extend_from_slice(samples);

        while self.raw_buffer.len() >= INPUT_FRAME_16KHZ {
            let mut input_16k = [0.0f32; INPUT_FRAME_16KHZ];
            for (dst, src) in input_16k
                .iter_mut()
                .zip(self.raw_buffer.drain(..INPUT_FRAME_16KHZ))
            {
                *dst = src;
            }

            let mut denoised = [0.0f32; INPUT_FRAME_16KHZ];
            self.preprocessor.process(&input_16k, &mut denoised)?;

            for s in denoised.iter_mut() {
                *s = s.clamp(-1.0, 1.0);
            }
            self.denoised_buffer.extend(denoised);
        }

        while self.denoised_buffer.len() >= VAD_WINDOW_SIZE {
            self.denoised_buffer.make_contiguous();
            let (slice, _) = self.denoised_buffer.as_slices();
            let denoised_window = &slice[..VAD_WINDOW_SIZE];

            let score = self.detector.predict_f32(denoised_window);
            let rms = Self::compute_rms(denoised_window);

            let above = rms >= self.rms_threshold && score >= self.threshold;

            self.is_speech_ring.push_back(above);
            if self.is_speech_ring.len() > VAD_RING_SIZE {
                self.is_speech_ring.pop_front();
            }

            if !self.is_speech {
                if above {
                    self.consecutive_above_threshold += 1;
                    self.prediction_list.push(score);
                    self.speech_duration_ms += FRAME_DURATION_MS;
                    self.current_silence_duration = 0.0;
                } else {
                    self.consecutive_above_threshold = 0;
                    self.current_silence_duration += FRAME_DURATION_MS;
                    if self.current_silence_duration > 80.0 {
                        self.prediction_list.clear();
                        self.speech_duration_ms = 0.0;
                    }
                }

                if self.consecutive_above_threshold >= 3
                    && self.prediction_list.len() >= 5
                    && self.speech_duration_ms >= self.min_speech_duration
                    && self.ring_speech_ratio() >= VAD_RING_TRIGGER_RATIO
                {
                    self.is_speech = true;
                }
            } else {
                let active = rms >= self.rms_threshold && score >= self.deactivation_threshold;

                if active {
                    self.current_silence_duration = 0.0;
                } else {
                    if self.current_silence_duration > self.min_silence_duration {
                        self.clear_state();
                        return Ok(score);
                    }
                    self.current_silence_duration += FRAME_DURATION_MS;
                }
            }
            self.last_score = score;

            self.denoised_buffer.drain(..VAD_WINDOW_SIZE);
        }

        Ok(self.last_score)
    }

    fn is_speech(&mut self) -> bool {
        self.is_speech
    }

    fn clear(&mut self) {
        self.clear_state();
    }

    fn window_size(&self) -> usize {
        VAD_WINDOW_SIZE
    }
}
