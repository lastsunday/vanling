use async_trait::async_trait;
use framework::error::AppError;

pub trait AsrStream: Send + Sync {
    fn accept_waveform(&self, samples: &[f32]);
    fn decode(&self);
    fn is_endpoint(&self) -> bool;
    fn get_partial(&self) -> Option<String>;
    fn finish(&self) -> Option<RecognizerResult>;
    fn reset(&self);
}

#[async_trait]
pub trait Asr: Send + Sync {
    fn create_stream(&self) -> Box<dyn AsrStream>;

    async fn transcribe(
        &self,
        sample_rate: u32,
        samples: &[f32],
    ) -> Result<RecognizerResult, AppError>;
}

#[derive(Debug, Clone)]
pub struct RecognizerResult {
    pub text: String,
    pub prob: f32,
}
