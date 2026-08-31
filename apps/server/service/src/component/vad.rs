use framework::error::AppError;

pub trait Vad: Send + Sync {
    fn accept_waveform(&mut self, samples: &[f32]) -> Result<f32, AppError>;

    fn is_speech(&mut self) -> bool;

    fn clear(&mut self);

    fn window_size(&self) -> usize;
}

/// VAD 对象池：串行状态机对象按需取用 / 归还，供 `VadNode` 以 RAII 持有。
/// 具体池实现（如激活对象复用）由引擎侧提供，保证同一时刻仅被一个 Round 持有。
pub trait VadPool: Send + Sync {
    fn acquire(&self) -> Box<dyn Vad>;

    fn release(&self, vad: Box<dyn Vad>);
}
