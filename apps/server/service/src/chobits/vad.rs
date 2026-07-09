use framework::error::AppError;

pub trait Vad: Send + Sync {
    fn accept_waveform(&mut self, samples: &[f32]) -> Result<f32, AppError>;

    fn is_speech(&mut self) -> bool;

    fn clear(&mut self);

    fn window_size(&self) -> usize;
}
