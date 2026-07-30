use async_trait::async_trait;

use crate::ling::message::hello::AudioParam;

#[derive(Debug, Clone)]
pub enum ListenInput {
    Text(String),
    Audio(Vec<u8>),
}

#[derive(Debug, Clone)]
pub struct TurnResult {
    pub text: String,
    pub prob: f32,
    pub voice_data: Vec<f32>,
}

#[derive(Debug, Clone)]
pub enum TurnOutput {
    SpeechStarted,
    PartialTranscript(String),
    TurnComplete(TurnResult),
}

#[async_trait]
pub trait Listener: Send + Sync {
    async fn accept(&mut self, input: ListenInput);
    async fn drain_outputs(&mut self) -> Vec<TurnOutput>;
    async fn flush(&mut self) -> Option<TurnResult>;
    fn has_active_speech(&self) -> bool;
    fn reconfigure(&mut self, params: &AudioParam);
    async fn reset(&mut self, silence_voice_timeout: Option<i64>);
}
