use std::sync::{Arc, Mutex};

use crate::component::vad::{Vad, VadPool};
use crate::pipeline::nodes::{PREFIX_SAMPLES_MAX, push_capped};
use crate::pipeline::{EventStream, Node, NodeContext, PipelineEvent, ReleaseMode};

pub struct VadNode {
    pool: Arc<dyn VadPool>,
}

impl VadNode {
    pub fn new(pool: Arc<dyn VadPool>) -> Self {
        Self { pool }
    }
}

impl Node for VadNode {
    fn new_instance(&self) -> Arc<dyn Node> {
        Arc::new(VadInstance::new(self.pool.clone()))
    }

    fn stream(&self, _upstream: EventStream, _ctx: &NodeContext) -> EventStream {
        unreachable!("prototype must be cloned before entering a chain")
    }
}

struct VadInstance {
    vad: Arc<Mutex<Option<Box<dyn Vad>>>>,
    pool: Arc<dyn VadPool>,
    inner: Arc<Mutex<VadInner>>,
}

struct VadInner {
    prefix_buffer: Vec<f32>,
    client_sample_rate: u32,
}

impl VadInstance {
    fn new(pool: Arc<dyn VadPool>) -> Self {
        Self {
            // `None` 直到 `NodeChain::begin` 触发 `on_acquire` 才从池取用。
            vad: Arc::new(Mutex::new(None)),
            pool,
            inner: Arc::new(Mutex::new(VadInner {
                prefix_buffer: Vec::with_capacity(PREFIX_SAMPLES_MAX),
                client_sample_rate: 16000,
            })),
        }
    }
}

fn transform_event(
    vad: &Mutex<Option<Box<dyn Vad>>>,
    inner: &Mutex<VadInner>,
    session_id: &str,
    event: PipelineEvent,
) -> EventStream {
    let PipelineEvent::PcmFrame(samples, sample_rate) = event else {
        return Box::pin(futures::stream::iter([Ok(event)]));
    };
    {
        let mut inner = inner.lock().unwrap();
        inner.client_sample_rate = sample_rate;
    }

    let window_size = vad
        .lock()
        .unwrap()
        .as_ref()
        .expect("vad active")
        .window_size();
    let mut outputs: Vec<PipelineEvent> = Vec::new();
    let mut pos = 0;
    let len = samples.len();
    while pos < len {
        let chunk = if len - pos < window_size {
            samples[pos..len].to_vec()
        } else {
            samples[pos..pos + window_size].to_vec()
        };
        pos += chunk.len();

        {
            let mut inner = inner.lock().unwrap();
            push_capped(&mut inner.prefix_buffer, &chunk, PREFIX_SAMPLES_MAX);
        }

        let (was_speech, is_speech) = {
            let mut vad = vad.lock().unwrap();
            let vad = vad.as_mut().expect("vad active");
            let was_speech = vad.is_speech();
            if vad.accept_waveform(&chunk).is_err() {
                break;
            }
            let is_speech = vad.is_speech();
            (was_speech, is_speech)
        };

        // 透传每个窗口的 PCM 供下游 ASR 消费
        outputs.push(PipelineEvent::PcmFrame(chunk, sample_rate));

        // 语音起始转换时产出 SpeechStarted（chunk 之后，保证下游 prefix 含该 chunk）
        if !was_speech && is_speech {
            tracing::debug!(
                component = "VAD",
                event = "speech_started",
                session_id = %session_id,
                "pipeline: speech started"
            );
            outputs.push(PipelineEvent::SpeechStarted);
        }
        // 语音结束转换时产出 SpeechEnded（供编排层切换"已说话+静音"计时）
        if was_speech && !is_speech {
            tracing::debug!(
                component = "VAD",
                event = "speech_ended",
                session_id = %session_id,
                "pipeline: speech ended"
            );
            outputs.push(PipelineEvent::SpeechEnded);
        }
    }
    Box::pin(futures::stream::iter(
        outputs.into_iter().map(Ok::<_, framework::error::AppError>),
    ))
}

impl Node for VadInstance {
    fn new_instance(&self) -> Arc<dyn Node> {
        unreachable!("instance must not be cloned again")
    }

    /// `Deferred`：VAD 状态机跨越整轮识别，由 `NodeChain::finish` 统一归还。
    fn release_mode(&self) -> ReleaseMode {
        ReleaseMode::Deferred
    }

    /// `NodeChain::begin` 触发：从池取用 VAD 实例（进行流变换前就绪）。
    fn on_acquire(&self) {
        let mut guard = self.vad.lock().unwrap();
        if guard.is_none() {
            *guard = Some(self.pool.acquire());
        }
    }

    /// `NodeChain::finish` 触发：归还 VAD 实例到池（主释放路径；`Drop` 兜底防泄漏）。
    fn on_release(&self) {
        let mut guard = self.vad.lock().unwrap();
        if let Some(vad) = guard.take() {
            self.pool.release(vad);
        }
    }

    fn stream(&self, upstream: EventStream, ctx: &NodeContext) -> EventStream {
        use futures::stream::StreamExt as _;
        let session_id = ctx.session_id.clone();
        let vad = self.vad.clone();
        let inner = self.inner.clone();
        Box::pin(upstream.flat_map(move |r| match r {
            Ok(event) => transform_event(&vad, &inner, &session_id, event),
            Err(e) => Box::pin(futures::stream::iter([Err(e)])),
        }))
    }
}

impl Drop for VadInstance {
    fn drop(&mut self) {
        if let Some(vad_arc) = Arc::get_mut(&mut self.vad)
            && let Ok(guard) = vad_arc.get_mut()
        {
            let owned = guard.take();
            if let Some(vad) = owned {
                self.pool.release(vad);
            }
        }
    }
}
