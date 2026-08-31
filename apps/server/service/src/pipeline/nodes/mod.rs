pub mod asr_node;
pub mod ling_node;
pub mod opus_node;
pub mod tts_node;
pub mod turn_node;
pub mod vad_node;

pub use asr_node::AsrNode;
pub use ling_node::LingNode;
pub use opus_node::OpusDecodeNode;
pub use tts_node::TtsNode;
pub use turn_node::TurnNode;
pub use vad_node::VadNode;

/// 共享常量：VAD 语音前的音频前缀缓冲上限（样本数，~16k×600ms）。
pub(crate) const PREFIX_SAMPLES_MAX: usize = 9600;

/// 环形追加：超过 `cap` 时丢弃最旧的样本（保留说话的 prefix）。
pub(crate) fn push_capped(buf: &mut Vec<f32>, samples: &[f32], cap: usize) {
    buf.extend_from_slice(samples);
    if buf.len() > cap {
        let excess = buf.len() - cap;
        buf.drain(..excess);
    }
}
