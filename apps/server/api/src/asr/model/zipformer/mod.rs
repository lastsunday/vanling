use async_trait::async_trait;
use framework::error::AppError;
use service::chobits::asr::{Asr, AsrStream, RecognizerResult};
use sherpa_onnx::{
    OnlineRecognizer, OnlineRecognizerConfig, OnlineStream, OnlineTransducerModelConfig,
};
use std::sync::Arc;

use crate::common::ModelError;

pub struct AsrZipformer {
    recognizer: Arc<OnlineRecognizer>,
}

pub struct ZipformerStream {
    recognizer: Arc<OnlineRecognizer>,
    stream: OnlineStream,
    sample_rate: i32,
}

impl AsrZipformer {
    pub fn new(path: &str) -> Result<Self, ModelError> {
        let encoder = auto_discover_onnx(path, "encoder")
            .ok_or_else(|| ModelError::ModelFileNotFound(format!("encoder.onnx in {path}")))?;
        let decoder = auto_discover_onnx(path, "decoder")
            .ok_or_else(|| ModelError::ModelFileNotFound(format!("decoder.onnx in {path}")))?;
        let joiner = auto_discover_onnx(path, "joiner")
            .ok_or_else(|| ModelError::ModelFileNotFound(format!("joiner.onnx in {path}")))?;
        let tokens_path = format!("{path}tokens.txt");
        if !std::path::Path::new(&tokens_path).exists() {
            return Err(ModelError::ModelFileNotFound(format!(
                "tokens.txt in {path}"
            )));
        }

        let mut config = OnlineRecognizerConfig::default();
        config.model_config.transducer = OnlineTransducerModelConfig {
            encoder: Some(encoder),
            decoder: Some(decoder),
            joiner: Some(joiner),
        };
        config.model_config.tokens = Some(tokens_path);
        config.model_config.num_threads = 2;
        config.model_config.provider = Some("cpu".into());
        config.model_config.debug = false;
        config.enable_endpoint = true;
        config.decoding_method = Some("greedy_search".into());

        let recognizer = OnlineRecognizer::create(&config)
            .ok_or_else(|| ModelError::Asr("failed to create Zipformer OnlineRecognizer".into()))?;
        Ok(Self {
            recognizer: Arc::new(recognizer),
        })
    }
}

impl AsrStream for ZipformerStream {
    fn accept_waveform(&self, samples: &[f32]) {
        self.stream.accept_waveform(self.sample_rate, samples);
    }

    fn decode(&self) {
        while self.recognizer.is_ready(&self.stream) {
            self.recognizer.decode(&self.stream);
        }
    }

    fn is_endpoint(&self) -> bool {
        self.recognizer.is_endpoint(&self.stream)
    }

    fn get_partial(&self) -> Option<String> {
        self.recognizer
            .get_result(&self.stream)
            .map(|r| r.text)
            .filter(|t| !t.is_empty())
    }

    fn finish(&self) -> Option<RecognizerResult> {
        if !self.recognizer.is_endpoint(&self.stream) {
            self.stream.input_finished();
            while self.recognizer.is_ready(&self.stream) {
                self.recognizer.decode(&self.stream);
            }
        }
        self.recognizer
            .get_result(&self.stream)
            .map(|r| RecognizerResult {
                text: r.text,
                prob: 1.0,
            })
    }

    fn reset(&self) {
        self.recognizer.reset(&self.stream);
    }
}

#[async_trait]
impl Asr for AsrZipformer {
    fn create_stream(&self) -> Box<dyn AsrStream> {
        let stream = self.recognizer.create_stream();
        Box::new(ZipformerStream {
            recognizer: self.recognizer.clone(),
            stream,
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
        stream.input_finished();
        while self.recognizer.is_ready(&stream) {
            self.recognizer.decode(&stream);
        }
        self.recognizer
            .get_result(&stream)
            .ok_or_else(|| ModelError::Asr("Zipformer returned no result".into()).into())
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
