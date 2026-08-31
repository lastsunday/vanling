use std::sync::Arc;
use std::sync::Mutex as StdMutex;

use rubato::audioadapter_buffers::direct::SequentialSliceOfVecs;
use rubato::{Fft, FixedSync, Indexing, Resampler};

use crate::pipeline::{EventStream, Node, NodeContext, PipelineEvent, ReleaseMode};

const MAX_OPUS_FRAME_MS: u64 = 120;
const DEFAULT_SAMPLE_RATE: u32 = 16000;
/// 统一输出采样率：下游 VAD(Earshot)/ASR(sherpa) 均硬编码假设 16k 输入，链上无其它重采样点。
const OUTPUT_SAMPLE_RATE: u32 = 16000;
/// 重采样输入缓冲目标长度（~20ms，实际以 `input_frames_next()` 为准）。
const RESAMPLER_CHUNK_SIZE: usize = 960;
const RESAMPLER_SUB_CHUNKS: usize = 1;
const RESAMPLER_CHANNELS: usize = 1;

fn clamp_sample(s: f32) -> f32 {
    s.clamp(-1.0, 1.0)
}

/// 重采样器内部状态：`resampler` 惰性构建（按配置采样率→16k），`pending` 缓冲待送入的输入样本。
#[derive(Default)]
struct ResampleState {
    resampler: Option<Fft<f32>>,
    pending: Vec<f32>,
}

impl ResampleState {
    /// 重建（采样率变化时调用）：丢弃旧重采样器与未送完的缓存。
    fn reset(&mut self) {
        self.resampler = None;
        self.pending.clear();
    }

    /// 送一段解码后 PCM，返回重采样到 16k 的输出（可能为空，因内部缓冲）。
    fn push(&mut self, source_rate: u32, samples: &[f32]) -> Vec<f32> {
        let resampler = self.resampler.get_or_insert_with(|| {
            Fft::new(
                source_rate as usize,
                OUTPUT_SAMPLE_RATE as usize,
                RESAMPLER_CHUNK_SIZE,
                RESAMPLER_SUB_CHUNKS,
                RESAMPLER_CHANNELS,
                FixedSync::Input,
            )
            .expect("failed to build resampler")
        });
        self.pending.extend_from_slice(samples);

        let mut out = Vec::new();
        let need = resampler.input_frames_next();
        while self.pending.len() >= need {
            let chunk: Vec<f32> = self.pending.drain(..need).collect();
            out.extend(resample_full(resampler, &chunk));
        }
        out
    }
}

fn resample_full(resampler: &mut Fft<f32>, chunk: &[f32]) -> Vec<f32> {
    let channels = RESAMPLER_CHANNELS;
    let input_data = vec![chunk.to_vec()];
    let input = SequentialSliceOfVecs::new(&input_data, channels, chunk.len())
        .expect("Failed to create input adapter");
    let output_frames = resampler.output_frames_next();
    let mut output_data = vec![vec![0.0f32; output_frames]; channels];
    let mut output = SequentialSliceOfVecs::new_mut(&mut output_data, channels, output_frames)
        .expect("Failed to create output adapter");
    let indexing = Indexing {
        input_offset: 0,
        output_offset: 0,
        active_channels_mask: None,
        partial_len: Some(chunk.len()),
    };
    let (_, nbr_out) = resampler
        .process_into_buffer(&input, &mut output, Some(&indexing))
        .expect("Resampling failed");
    output_data[0][..nbr_out].to_vec()
}

/// 把 Opus `AudioFrame` 解码为 PCM `PcmFrame`，统一重采样到 16k；配置态置于 `Arc` 内供闭包捕获。
pub struct OpusDecodeNode {
    decoder: Arc<StdMutex<ropus::Decoder>>,
    client_input_sample_rate: Arc<StdMutex<u32>>,
    resample: Arc<StdMutex<ResampleState>>,
}

impl OpusDecodeNode {
    pub fn new() -> Self {
        Self {
            decoder: Arc::new(StdMutex::new(
                ropus::Decoder::new(DEFAULT_SAMPLE_RATE, ropus::Channels::Mono).unwrap(),
            )),
            client_input_sample_rate: Arc::new(StdMutex::new(DEFAULT_SAMPLE_RATE)),
            resample: Arc::new(StdMutex::new(ResampleState::default())),
        }
    }

    /// 以模板的**已配置采样率**派生一个新实例（解码器/重采样器全新，采样率继承自模板）。
    fn at_sample_rate(sample_rate: u32) -> Self {
        Self {
            decoder: Arc::new(StdMutex::new(
                ropus::Decoder::new(sample_rate, ropus::Channels::Mono).unwrap(),
            )),
            client_input_sample_rate: Arc::new(StdMutex::new(sample_rate)),
            resample: Arc::new(StdMutex::new(ResampleState::default())),
        }
    }

    fn set_sample_rate(&self, sample_rate: u32) {
        reconfigure(
            &self.decoder,
            &self.client_input_sample_rate,
            &self.resample,
            sample_rate,
        );
    }
}

impl Default for OpusDecodeNode {
    fn default() -> Self {
        Self::new()
    }
}

fn decode_opus(
    decoder: &StdMutex<ropus::Decoder>,
    client_input_sample_rate: &StdMutex<u32>,
    session_id: &str,
    data: &[u8],
) -> Option<(Vec<f32>, usize)> {
    let sample_rate = *client_input_sample_rate.lock().unwrap();
    let frame_size = ((sample_rate as u64 * MAX_OPUS_FRAME_MS) / 1000) as usize;
    let mut samples = vec![0f32; frame_size];
    let len =
        match decoder
            .lock()
            .unwrap()
            .decode_float(data, &mut samples, ropus::DecodeMode::Normal)
        {
            Ok(len) => len,
            Err(e) => {
                tracing::warn!(
                    component = "VAD",
                    event = "opus_decode_error",
                    session_id = %session_id,
                    data_len = data.len(),
                    error = %e,
                    "opus decode error"
                );
                return None;
            }
        };
    for s in samples[..len].iter_mut() {
        *s = clamp_sample(*s);
    }
    Some((samples, len))
}

impl Node for OpusDecodeNode {
    fn new_instance(&self) -> Arc<dyn Node> {
        let rate = *self.client_input_sample_rate.lock().unwrap();
        Arc::new(OpusDecodeNode::at_sample_rate(rate))
    }

    /// `Immediate`：进程结束时重置解码/重采样状态。
    fn release_mode(&self) -> ReleaseMode {
        ReleaseMode::Immediate
    }

    fn on_release(&self) {
        self.resample.lock().unwrap().reset();
    }

    /// 模板级下行配置：更新模板自身采样率，`new_instance` 时继承。
    fn on_configure(&self, event: &PipelineEvent) {
        if let PipelineEvent::Configure(params) = event {
            self.set_sample_rate(params.sample_rate);
        }
    }

    fn stream(&self, upstream: EventStream, ctx: &NodeContext) -> EventStream {
        use futures::stream::StreamExt as _;
        let session_id = ctx.session_id.clone();
        let decoder = self.decoder.clone();
        let rate = self.client_input_sample_rate.clone();
        let resample = self.resample.clone();
        Box::pin(upstream.flat_map(move |r| -> EventStream {
            match r {
                Ok(event) => match event {
                    PipelineEvent::AudioFrame(data) => {
                        if let Some((samples, len)) =
                            decode_opus(&decoder, &rate, &session_id, &data)
                        {
                            let pcm: Vec<f32> = samples.into_iter().take(len).collect();
                            let source_rate = *rate.lock().unwrap();
                            let out = resample.lock().unwrap().push(source_rate, &pcm);
                            if out.is_empty() {
                                Box::pin(futures::stream::empty())
                            } else {
                                Box::pin(futures::stream::iter([Ok(PipelineEvent::PcmFrame(
                                    out,
                                    OUTPUT_SAMPLE_RATE,
                                ))]))
                            }
                        } else {
                            Box::pin(futures::stream::empty())
                        }
                    }
                    PipelineEvent::Configure(params) => {
                        reconfigure(&decoder, &rate, &resample, params.sample_rate);
                        Box::pin(futures::stream::empty())
                    }
                    other => Box::pin(futures::stream::iter([Ok(other)])),
                },
                Err(e) => Box::pin(futures::stream::iter([Err(e)])),
            }
        }))
    }
}

fn reconfigure(
    decoder: &StdMutex<ropus::Decoder>,
    client_input_sample_rate: &StdMutex<u32>,
    resample: &StdMutex<ResampleState>,
    sample_rate: u32,
) {
    *decoder.lock().unwrap() = ropus::Decoder::new(sample_rate, ropus::Channels::Mono).unwrap();
    *client_input_sample_rate.lock().unwrap() = sample_rate;
    resample.lock().unwrap().reset();
}

/// 供测试在实例化后再配置采样率。
#[allow(dead_code)]
pub fn with_sample_rate(sample_rate: u32) -> OpusDecodeNode {
    let node = OpusDecodeNode::new();
    node.set_sample_rate(sample_rate);
    node
}
