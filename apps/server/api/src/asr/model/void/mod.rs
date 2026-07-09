use async_trait::async_trait;
use framework::error::AppError;
use service::chobits::asr::{Asr, RecognizerResult};

pub struct AsrVoid {}

impl AsrVoid {
    pub fn new() -> Result<Self, AppError> {
        Ok(Self {})
    }
}

#[async_trait]
impl Asr for AsrVoid {
    async fn transcribe(
        &mut self,
        _sample_rate: u32,
        _samples: &[f32],
    ) -> Result<RecognizerResult, AppError> {
        Ok(RecognizerResult {
            text: String::new(),
            prob: 1.0,
        })
    }
}
