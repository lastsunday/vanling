use async_trait::async_trait;
use framework::error::AppError;

use crate::chobits::message::hello::AudioParam;

#[derive(Debug, Clone)]
pub enum ListenInput {
    Text(String),
    Audio(Vec<u8>),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ListenState {
    Idle,
    Listening { is_speech: bool },
    End,
}

#[derive(Debug, Clone)]
pub enum ListenResult {
    Text(String),
    Audio { text: String, prob: f32 },
}

#[async_trait]
pub trait Listener: Send + Sync {
    async fn accept(&mut self, input: ListenInput);
    fn set_state(&mut self, state: ListenState);
    fn get_state(&self) -> ListenState;
    async fn reset(&mut self, silence_voice_timeout: Option<i64>);
    fn reconfigure(&mut self, params: &AudioParam);
    async fn take_voice(&mut self) -> Vec<f32> {
        Vec::new()
    }
    async fn take_result(&mut self) -> (Vec<f32>, Result<ListenResult, AppError>);
    async fn get_raw_pcm(&mut self) -> Vec<f32> {
        Vec::new()
    }
    fn poll_timeout(&mut self) -> Option<()>;
}
