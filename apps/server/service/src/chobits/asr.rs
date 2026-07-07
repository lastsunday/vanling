use async_trait::async_trait;
use framework::error::AppError;

#[async_trait]
pub trait Asr: Send + Sync {
    async fn transcribe(
        &mut self,
        sample_rate: u32,
        samples: &[f32],
    ) -> Result<RecognizerResult, AppError>;
}

#[derive(Debug, Clone)]
pub struct RecognizerResult {
    pub text: String,
    pub prob: f32,
}
