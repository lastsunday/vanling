use async_trait::async_trait;
use framework::error::AppError;
use service::ling::asr::{Asr, AsrStream, RecognizerResult};

pub struct AsrVoid {}

pub struct VoidStream;

impl AsrStream for VoidStream {
    fn accept_waveform(&self, _samples: &[f32]) {}

    fn decode(&self) {}

    fn is_endpoint(&self) -> bool {
        false
    }

    fn get_partial(&self) -> Option<String> {
        None
    }

    fn finish(&self) -> Option<RecognizerResult> {
        Some(RecognizerResult {
            text: "void".into(),
            prob: 1.0,
        })
    }

    fn reset(&self) {}
}

impl AsrVoid {
    pub fn new() -> Result<Self, AppError> {
        Ok(Self {})
    }
}

#[async_trait]
impl Asr for AsrVoid {
    fn create_stream(&self) -> Box<dyn AsrStream> {
        Box::new(VoidStream)
    }

    async fn transcribe(
        &self,
        _sample_rate: u32,
        _samples: &[f32],
    ) -> Result<RecognizerResult, AppError> {
        Ok(RecognizerResult {
            text: "void".into(),
            prob: 1.0,
        })
    }
}
