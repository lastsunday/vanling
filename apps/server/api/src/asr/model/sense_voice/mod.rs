use async_trait::async_trait;
use framework::error::AppError;
use service::chobits::asr::{Asr, AsrStream, RecognizerResult};
use sherpa_onnx::{OfflineRecognizer, OfflineRecognizerConfig, OfflineSenseVoiceModelConfig};
use std::sync::{Arc, Mutex};

use crate::common::ModelError;

pub struct AsrSenseVoice {
    recognizer: Arc<OfflineRecognizer>,
}

pub struct SenseVoiceStream {
    recognizer: Arc<OfflineRecognizer>,
    samples: Mutex<Vec<f32>>,
    sample_rate: i32,
}

impl AsrSenseVoice {
    pub fn new(path: &str) -> Result<Self, ModelError> {
        let model_path = auto_discover_onnx(path, "model")
            .ok_or_else(|| ModelError::ModelFileNotFound(format!("model.int8.onnx in {path}")))?;
        let tokens_path = format!("{path}tokens.txt");
        if !std::path::Path::new(&tokens_path).exists() {
            return Err(ModelError::ModelFileNotFound(format!(
                "tokens.txt in {path}"
            )));
        }

        let mut config = OfflineRecognizerConfig::default();
        config.model_config.sense_voice = OfflineSenseVoiceModelConfig {
            model: Some(model_path),
            language: Some("auto".into()),
            use_itn: true,
        };
        config.model_config.tokens = Some(tokens_path);
        config.model_config.num_threads = 2;
        config.model_config.model_type = Some("sense_voice".into());

        let recognizer = OfflineRecognizer::create(&config)
            .ok_or_else(|| ModelError::Asr("failed to create SenseVoice recognizer".into()))?;
        Ok(Self {
            recognizer: Arc::new(recognizer),
        })
    }
}

impl AsrStream for SenseVoiceStream {
    fn accept_waveform(&self, samples: &[f32]) {
        self.samples.lock().unwrap().extend_from_slice(samples);
    }

    fn decode(&self) {}

    fn is_endpoint(&self) -> bool {
        false
    }

    fn get_partial(&self) -> Option<String> {
        None
    }

    fn finish(&self) -> Option<RecognizerResult> {
        let samples = self.samples.lock().unwrap().clone();
        if samples.is_empty() {
            return None;
        }
        let stream = self.recognizer.create_stream();
        stream.accept_waveform(self.sample_rate, &samples);
        self.recognizer.decode(&stream);
        stream.get_result().map(|r| RecognizerResult {
            text: r.text,
            prob: 1.0,
        })
    }

    fn reset(&self) {
        self.samples.lock().unwrap().clear();
    }
}

#[async_trait]
impl Asr for AsrSenseVoice {
    fn create_stream(&self) -> Box<dyn AsrStream> {
        Box::new(SenseVoiceStream {
            recognizer: self.recognizer.clone(),
            samples: Mutex::new(Vec::new()),
            sample_rate: 16000,
        })
    }

    async fn transcribe(
        &self,
        sample_rate: u32,
        samples: &[f32],
    ) -> Result<RecognizerResult, AppError> {
        let stream = self.recognizer.create_stream();
        stream.accept_waveform(sample_rate as i32, samples);
        self.recognizer.decode(&stream);
        stream
            .get_result()
            .ok_or_else(|| ModelError::Asr("SenseVoice returned no result".into()))
            .map_err(AppError::from)
            .map(|r| RecognizerResult {
                text: r.text,
                prob: 1.0,
            })
    }
}

fn auto_discover_onnx(dir: &str, prefix: &str) -> Option<String> {
    let p = std::path::Path::new(dir);
    std::fs::read_dir(p).ok().and_then(|mut entries| {
        entries.find_map(|entry| {
            entry.ok().and_then(|e| {
                let path = e.path();
                if path.extension().is_some_and(|ext| ext == "onnx")
                    && path
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .is_some_and(|stem| stem.contains(prefix))
                {
                    path.to_str().map(|s| s.to_string())
                } else {
                    None
                }
            })
        })
    })
}
