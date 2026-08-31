use std::sync::{Arc, Mutex};

use crate::component::asr::{Asr, AsrStream, RecognizerResult};
use crate::pipeline::nodes::{PREFIX_SAMPLES_MAX, push_capped};
use crate::pipeline::{EventStream, Node, NodeContext, PipelineEvent, ReleaseMode};

/// 静音确认阈值（样本折算 ms）：VAD 报非语音后触发 ASR finish。
const SILENCE_CONFIRM_MS: u64 = 200;

/// ASR 节点：消费 `PcmFrame` / `SpeechStarted`，产出 `PartialTranscript` / `TurnText`。
/// 内部管理 ASR 流生命周期（创建 / 喂入 / finish），状态置于 `Arc` 内供流变换闭包捕获。
pub struct AsrNode {
    asr: Arc<dyn Asr>,
    inner: Arc<Mutex<AsrInner>>,
}

struct AsrInner {
    stream: Option<Box<dyn AsrStream>>,
    prefix_buffer: Vec<f32>,
    /// 流式实时识别（auto/realtime）。`false`=按键录音：
    /// 仍逐帧喂流预解码（暖管），但抑制 partial / 静音确认，`FinishTurn` 时才成文。
    streaming: bool,
    speech_active: bool,
    spoken: bool,
    silence_samples: u64,
    sample_rate: u32,
    last_partial: Option<String>,
}

impl AsrNode {
    pub fn new(asr: Arc<dyn Asr>) -> Self {
        Self {
            asr,
            inner: Arc::new(Mutex::new(AsrInner {
                stream: None,
                prefix_buffer: Vec::with_capacity(PREFIX_SAMPLES_MAX),
                streaming: true,
                speech_active: false,
                spoken: false,
                silence_samples: 0,
                sample_rate: 16000,
                last_partial: None,
            })),
        }
    }
}

fn transform_event(
    asr: &Arc<dyn Asr>,
    inner: &Arc<Mutex<AsrInner>>,
    session_id: &str,
    event: PipelineEvent,
) -> EventStream {
    let mut outputs: Vec<PipelineEvent> = Vec::new();
    match event {
        PipelineEvent::PcmFrame(samples, sample_rate) => {
            let streaming = inner.lock().unwrap().streaming;
            {
                let mut inner = inner.lock().unwrap();
                inner.sample_rate = sample_rate;
                inner.silence_samples += samples.len() as u64;
                push_capped(&mut inner.prefix_buffer, &samples, PREFIX_SAMPLES_MAX);
                if let Some(s) = inner.stream.as_mut() {
                    s.accept_waveform(&samples);
                    s.decode();
                    if s.is_endpoint() {
                        tracing::debug!(
                            component = "ASR",
                            event = "endpoint_detected",
                            session_id = %session_id,
                            "asr endpoint detected"
                        );
                    }
                }
            }
            if !streaming {
                // 按键录音：预解码暖管，但抑制 partial / 静音确认，结果仅在 FinishTurn 成文。
                return Box::pin(futures::stream::iter(
                    outputs.into_iter().map(Ok::<_, framework::error::AppError>),
                ));
            }
            // 读取 partial（不持流借用）
            let partial = {
                let inner = inner.lock().unwrap();
                inner.stream.as_ref().and_then(|s| s.get_partial())
            };
            if let Some(partial) = partial {
                let mut inner = inner.lock().unwrap();
                if inner.last_partial.as_deref() != Some(&partial) {
                    inner.last_partial = Some(partial.clone());
                    outputs.push(PipelineEvent::PartialTranscript(partial));
                }
            } else {
                let mut inner = inner.lock().unwrap();
                inner.last_partial = None;
            }

            // 静音确认：已说话 + 当前非语音 + 静音样本 ≥ 确认阈值 → finish 本轮
            let confirm = {
                let i = inner.lock().unwrap();
                let threshold = SILENCE_CONFIRM_MS * i.sample_rate as u64 / 1000;
                i.spoken && !i.speech_active && i.silence_samples >= threshold
            };
            if confirm {
                let outcome = finish_stream(session_id, inner);
                outcome.push_into(&mut outputs);
            }
        }
        PipelineEvent::SpeechStarted => {
            let mut inner = inner.lock().unwrap();
            inner.spoken = true;
            if inner.stream.is_none() {
                let new_stream = asr.create_stream();
                new_stream.accept_waveform(&inner.prefix_buffer);
                inner.prefix_buffer.clear();
                inner.speech_active = true;
                inner.spoken = true;
                inner.silence_samples = 0;
                inner.stream = Some(new_stream);
            }
        }
        PipelineEvent::SpeechEnded => {
            inner.lock().unwrap().speech_active = false;
        }
        PipelineEvent::ListenMode { streaming } => {
            inner.lock().unwrap().streaming = streaming;
        }
        PipelineEvent::FinishTurn => {
            let outcome = finish_stream(session_id, inner);
            outcome.push_into(&mut outputs);
        }
        other => {
            return Box::pin(futures::stream::iter([Ok(other)]));
        }
    }
    Box::pin(futures::stream::iter(
        outputs.into_iter().map(Ok::<_, framework::error::AppError>),
    ))
}

/// 取出流并 finish。三态：有效输入 → `HadInput`；有流但空文本 → `EmptyInput`（触发提示语）；无流 → `Nothing`。
fn finish_stream(session_id: &str, inner: &Arc<Mutex<AsrInner>>) -> FinishOutcome {
    let mut guard = inner.lock().unwrap();
    let Some(stream) = guard.stream.take() else {
        guard.speech_active = false;
        guard.spoken = false;
        guard.silence_samples = 0;
        guard.last_partial = None;
        return FinishOutcome::Nothing;
    };
    let result = stream.finish();
    guard.stream = None;
    guard.speech_active = false;
    guard.spoken = false;
    guard.silence_samples = 0;
    guard.last_partial = None;
    let Some(result) = result else {
        return FinishOutcome::EmptyInput;
    };
    tracing::info!(
        component = "ASR",
        event = "asr_stream_finish",
        session_id = %session_id,
        text_len = result.text.len(),
        "asr: stream finish"
    );
    if result.text.trim().is_empty() {
        tracing::debug!(
            component = "ASR",
            event = "asr_empty",
            session_id = %session_id,
            "asr: empty text"
        );
        return FinishOutcome::EmptyInput;
    }
    FinishOutcome::HadInput(result)
}

#[derive(Debug)]
enum FinishOutcome {
    HadInput(RecognizerResult),
    EmptyInput,
    Nothing,
}

impl FinishOutcome {
    fn push_into(self, outputs: &mut Vec<PipelineEvent>) {
        match self {
            FinishOutcome::HadInput(result) => outputs.push(PipelineEvent::TurnText {
                text: result.text,
                prob: result.prob,
            }),
            FinishOutcome::EmptyInput => outputs.push(PipelineEvent::EmptyInput),
            FinishOutcome::Nothing => {}
        }
    }
}

impl Node for AsrNode {
    fn new_instance(&self) -> Arc<dyn Node> {
        Arc::new(AsrNode::new(self.asr.clone()))
    }

    /// `Deferred`：ASR 流（feed-frames → finish）跨越整轮识别，由 `NodeChain::finish` 统一收尾。
    fn release_mode(&self) -> ReleaseMode {
        ReleaseMode::Deferred
    }

    /// `NodeChain::finish` 触发：若仍残留活跃流则 finish 收尾，避免泄漏。
    fn on_release(&self) {
        finish_stream("", &self.inner);
    }

    fn stream(&self, upstream: EventStream, ctx: &NodeContext) -> EventStream {
        use futures::stream::StreamExt as _;
        let session_id = ctx.session_id.clone();
        let asr = self.asr.clone();
        let inner = self.inner.clone();
        Box::pin(upstream.flat_map(move |r| match r {
            Ok(event) => transform_event(&asr, &inner, &session_id, event),
            Err(e) => Box::pin(futures::stream::iter([Err(e)])),
        }))
    }
}
