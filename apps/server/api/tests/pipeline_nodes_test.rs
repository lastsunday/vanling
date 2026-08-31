use std::pin::Pin;
use std::sync::Arc;

use async_trait::async_trait;
use framework::error::AppError;
use futures::stream::{self, Stream, StreamExt};
use service::component::asr::{Asr, AsrStream, RecognizerResult};
use service::component::tts::{Tts, TtsPacket};
use service::ling::Ling;
use service::message::{AudioFormat, hello::AudioParam};
use service::pipeline::{
    AsrNode, AudioSpec, EventStream, LingNode, Node, NodeContext, OpusDecodeNode, PipelineEvent,
    TtsNode,
};
use service::types::{EmptyKind, Input, OutputBlock};
use tokio_util::sync::CancellationToken;

fn test_error() -> AppError {
    AppError::App {
        code: 999999,
        message: "test error".into(),
        extra_message: None,
        file: None,
        line: None,
        error: None,
    }
}

/// Fake Ling that replays a fixed sequence of blocks (taken once per `ask`).
struct FakeLing {
    blocks: std::sync::Mutex<Vec<Result<OutputBlock, AppError>>>,
}

#[async_trait]
impl Ling for FakeLing {
    async fn ask(
        &self,
        _input: Input,
        _cancel: CancellationToken,
    ) -> Pin<Box<dyn Stream<Item = Result<OutputBlock, AppError>> + Send + 'static>> {
        let blocks = std::mem::take(&mut *self.blocks.lock().unwrap());
        Box::pin(stream::iter(blocks))
    }
}

/// Fake ASR whose stream `finish()` yields no result, exercising the empty-input path.
struct EmptyAsr;

struct EmptyAsrStream;

impl AsrStream for EmptyAsrStream {
    fn accept_waveform(&self, _samples: &[f32]) {}
    fn decode(&self) {}
    fn is_endpoint(&self) -> bool {
        false
    }
    fn get_partial(&self) -> Option<String> {
        None
    }
    fn finish(&self) -> Option<RecognizerResult> {
        None
    }
    fn reset(&self) {}
}

#[async_trait]
impl Asr for EmptyAsr {
    fn create_stream(&self) -> Box<dyn AsrStream> {
        Box::new(EmptyAsrStream)
    }

    async fn transcribe(
        &self,
        _sample_rate: u32,
        _samples: &[f32],
    ) -> Result<RecognizerResult, AppError> {
        Ok(RecognizerResult {
            text: String::new(),
            prob: 0.0,
        })
    }
}

/// Fake TTS that emits one packet per received text line, in order.
struct FakeTts;

#[async_trait]
impl Tts for FakeTts {
    fn audio_spec(&self) -> AudioSpec {
        AudioSpec {
            sample_rate: 16000,
            channel: 1,
            frame_duration_ms: 60,
        }
    }

    async fn stream(
        &self,
        text_stream: Pin<Box<dyn Stream<Item = String> + Send + 'static>>,
        _cancel: CancellationToken,
    ) -> Pin<Box<dyn Stream<Item = Result<TtsPacket, AppError>> + Send + 'static>> {
        let mut idx = 0usize;
        let s = text_stream.map(move |text| {
            let packet = TtsPacket {
                text: Arc::from(text.as_str()),
                audio: vec![vec![idx as u8; 8]],
                is_first: idx == 0,
                is_last: true,
            };
            idx += 1;
            Ok(packet)
        });
        Box::pin(s)
    }
}

/// Fake TTS that always fails encoding.
struct FailingTts;

#[async_trait]
impl Tts for FailingTts {
    fn audio_spec(&self) -> AudioSpec {
        AudioSpec {
            sample_rate: 16000,
            channel: 1,
            frame_duration_ms: 60,
        }
    }

    async fn stream(
        &self,
        _text_stream: Pin<Box<dyn Stream<Item = String> + Send + 'static>>,
        _cancel: CancellationToken,
    ) -> Pin<Box<dyn Stream<Item = Result<TtsPacket, AppError>> + Send + 'static>> {
        Box::pin(stream::once(async { Err(test_error()) }))
    }
}

async fn run_ling(
    node: &LingNode,
    cancel: CancellationToken,
) -> Vec<Result<PipelineEvent, AppError>> {
    let nctx = NodeContext::new(cancel);
    let upstream: EventStream = Box::pin(stream::iter(vec![Ok(PipelineEvent::TurnText {
        text: "prompt".into(),
        prob: 1.0,
    })]));
    let stream = node.stream(upstream, &nctx);
    Box::pin(stream).collect::<Vec<_>>().await
}

#[tokio::test]
async fn ling_emits_sentence_and_text_blocks_in_order() {
    let fake = Arc::new(FakeLing {
        blocks: std::sync::Mutex::new(vec![
            Ok(OutputBlock::Sentence(service::types::Sentence {
                text: "hello".into(),
                emotion: Some("happy".into()),
            })),
            Ok(OutputBlock::Text("world".into())),
        ]),
    });
    let node = LingNode::new(fake);
    let out = run_ling(&node, CancellationToken::new()).await;

    let events: Vec<&PipelineEvent> = out.iter().filter_map(|r| r.as_ref().ok()).collect();
    assert_eq!(events.len(), 2);
    match events[0] {
        PipelineEvent::TextChunk { text, emotion } => {
            assert_eq!(text, "hello");
            assert_eq!(emotion.as_deref(), Some("happy"));
        }
        other => panic!("expected TextChunk, got {other:?}"),
    }
    match events[1] {
        PipelineEvent::TextChunk { text, emotion } => {
            assert_eq!(text, "world");
            assert_eq!(emotion, &None);
        }
        other => panic!("expected TextChunk, got {other:?}"),
    }
}

#[tokio::test]
async fn ling_reports_llm_error_via_result_stream() {
    let fake = Arc::new(FakeLing {
        blocks: std::sync::Mutex::new(vec![
            Ok(OutputBlock::Text("hello".into())),
            Err(test_error()),
        ]),
    });
    let node = LingNode::new(fake);
    let out = run_ling(&node, CancellationToken::new()).await;

    // First valid block still streams, then the Err is surfaced in-band.
    assert!(matches!(out[0], Ok(PipelineEvent::TextChunk { .. })));
    assert!(out[1].is_err());
}

/// `FakeLing` streams move `self.blocks` into the returned stream; created with `Arc`.
#[tokio::test]
async fn ling_stops_when_cancelled() {
    let fake = Arc::new(FakeLing {
        blocks: std::sync::Mutex::new(vec![Ok(OutputBlock::Text("hello".into()))]),
    });
    let node = LingNode::new(fake);
    let cancel = CancellationToken::new();
    // Cancel before the round starts; the node checks the token before every item
    // and stops without emitting the pending block.
    cancel.cancel();

    let item = tokio::time::timeout(std::time::Duration::from_secs(1), run_ling(&node, cancel))
        .await
        .expect("a cancelled round should close the ling stream promptly");
    assert!(
        item.is_empty(),
        "expected no event after cancel, got {item:?}"
    );
}

#[tokio::test]
async fn ling_handles_prompt_by_asking_llm() {
    let fake = Arc::new(FakeLing {
        blocks: std::sync::Mutex::new(vec![Ok(OutputBlock::Text("请再说一遍".into()))]),
    });
    let node = LingNode::new(fake);
    let cancel = CancellationToken::new();
    let nctx = NodeContext::new(cancel);
    let upstream: EventStream = Box::pin(stream::iter(vec![Ok(PipelineEvent::Prompt {
        kind: EmptyKind::Manual,
        count: 1,
    })]));
    let stream = node.stream(upstream, &nctx);
    let out: Vec<Result<PipelineEvent, AppError>> = Box::pin(stream).collect().await;

    assert!(matches!(
        out.as_slice(),
        [Ok(PipelineEvent::TextChunk { text, .. })] if text == "请再说一遍"
    ));
}

#[tokio::test]
async fn ling_passes_empty_input_through() {
    let fake = Arc::new(FakeLing {
        blocks: std::sync::Mutex::new(vec![Ok(OutputBlock::Text("should not emit".into()))]),
    });
    let node = LingNode::new(fake);
    let cancel = CancellationToken::new();
    let nctx = NodeContext::new(cancel);
    // 原始 EmptyInput 仅透传、不驱动生成（生成由中枢注 Prompt 显式驱动）。
    let upstream: EventStream = Box::pin(stream::iter(vec![Ok(PipelineEvent::EmptyInput)]));
    let stream = node.stream(upstream, &nctx);
    let out: Vec<Result<PipelineEvent, AppError>> = Box::pin(stream).collect().await;

    assert!(
        out.iter()
            .any(|e| matches!(e, Ok(PipelineEvent::EmptyInput))),
        "EmptyInput 应原样透传, got {out:?}"
    );
    assert!(
        !out.iter()
            .any(|e| matches!(e, Ok(PipelineEvent::TextChunk { .. }))),
        "EmptyInput 不应驱动 LLM 生成, got {out:?}"
    );
}

#[tokio::test]
async fn asr_nodes_emits_empty_input_on_empty_result_after_silence() {
    let node = AsrNode::new(Arc::new(EmptyAsr));
    let cancel = CancellationToken::new();
    let nctx = NodeContext::new(cancel);
    // rate * 200ms / 1000 = 3200 samples silence threshold.
    let silence = vec![0.0f32; 3200];
    let events: Vec<Result<PipelineEvent, AppError>> = vec![
        Ok(PipelineEvent::SpeechStarted),
        Ok(PipelineEvent::PcmFrame(vec![0.0f32; 200], 16000)),
        Ok(PipelineEvent::SpeechEnded),
        Ok(PipelineEvent::PcmFrame(silence, 16000)),
    ];
    let upstream: EventStream = Box::pin(stream::iter(events));
    let out: Vec<Result<PipelineEvent, AppError>> =
        Box::pin(node.stream(upstream, &nctx)).collect().await;

    assert!(
        out.iter()
            .any(|e| matches!(e, Ok(PipelineEvent::EmptyInput))),
        "expected EmptyInput after empty ASR result, got {out:?}"
    );
}

fn feed_text_chunks(
    events: Vec<PipelineEvent>,
) -> Pin<Box<dyn Stream<Item = Result<PipelineEvent, AppError>> + Send + 'static>> {
    Box::pin(stream::iter(events.into_iter().map(Ok)))
}

#[tokio::test]
async fn tts_emits_textchunk_and_audio_preserving_emotion_fifo() {
    let node = TtsNode::new(Arc::new(FakeTts));
    let cancel = CancellationToken::new();
    let upstream = feed_text_chunks(vec![
        PipelineEvent::TextChunk {
            text: "hello".into(),
            emotion: Some("happy".into()),
        },
        PipelineEvent::TextChunk {
            text: "world".into(),
            emotion: Some("sad".into()),
        },
    ]);
    let mut out = node.stream(upstream, &NodeContext::new(cancel));

    let mut chunks: Vec<PipelineEvent> = vec![];
    while let Some(item) = out.next().await {
        match item {
            Ok(ev) => chunks.push(ev),
            Err(e) => panic!("unexpected tts error: {e:?}"),
        }
    }

    let texts: Vec<&str> = chunks
        .iter()
        .filter_map(|ev| match ev {
            PipelineEvent::TextChunk { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(texts, vec!["hello", "world"]);

    let emotions: Vec<Option<&str>> = chunks
        .iter()
        .filter_map(|ev| match ev {
            PipelineEvent::TextChunk { emotion, .. } => Some(emotion.as_deref()),
            _ => None,
        })
        .collect();
    assert_eq!(emotions, vec![Some("happy"), Some("sad")]);

    // A second event per text carries the audio.
    let audio_count = chunks
        .iter()
        .filter(|ev| matches!(ev, PipelineEvent::AudioOut { .. }))
        .count();
    assert_eq!(audio_count, 2);
}

#[tokio::test]
async fn tts_audio_is_a_complete_available_to_round() {
    let node = TtsNode::new(Arc::new(FakeTts));
    let cancel = CancellationToken::new();
    let upstream = feed_text_chunks(vec![PipelineEvent::TextChunk {
        text: "hi".into(),
        emotion: None,
    }]);
    let mut out = node.stream(upstream, &NodeContext::new(cancel));

    let mut got: Vec<PipelineEvent> = vec![];
    while let Some(item) = out.next().await {
        match item {
            Ok(ev) => got.push(ev),
            Err(e) => panic!("unexpected tts error: {e:?}"),
        }
    }

    assert_eq!(got.len(), 2);
    assert!(matches!(
        &got[0],
        PipelineEvent::TextChunk { text, .. } if text == "hi"
    ));
    match &got[1] {
        PipelineEvent::AudioOut {
            audio,
            is_first,
            is_last,
        } => {
            assert!(!audio.is_empty());
            assert!(*is_first);
            assert!(*is_last);
        }
        other => panic!("expected AudioOut, got {other:?}"),
    }
}

#[tokio::test]
async fn tts_surfaces_encode_error() {
    let node = TtsNode::new(Arc::new(FailingTts));
    let cancel = CancellationToken::new();
    let upstream = feed_text_chunks(vec![PipelineEvent::TextChunk {
        text: "hi".into(),
        emotion: None,
    }]);
    let mut out = node.stream(upstream, &NodeContext::new(cancel));

    let item = out.next().await;
    match item {
        Some(Err(_e)) => {}
        other => panic!("expected Err, got {other:?}"),
    }
}

#[tokio::test]
async fn tts_passes_through_upstream_err() {
    let node = TtsNode::new(Arc::new(FakeTts));
    let cancel = CancellationToken::new();
    // Upstream (e.g. LLM) error should pass through TTS untouched.
    let upstream: Pin<Box<dyn Stream<Item = Result<PipelineEvent, AppError>> + Send + 'static>> =
        Box::pin(stream::iter(vec![
            Ok(PipelineEvent::TextChunk {
                text: "hi".into(),
                emotion: None,
            }),
            Err(test_error()),
        ]));
    let mut out = node.stream(upstream, &NodeContext::new(cancel));

    let mut saw_err = false;
    while let Some(item) = out.next().await {
        if item.is_err() {
            saw_err = true;
        }
    }
    assert!(saw_err, "expected upstream error to pass through");
}

/// 把一段给定采样率的 Opus 帧经 `OpusDecodeNode` 解码，收集产出的 `PcmFrame`。
/// 返回每个 `PcmFrame` 的 `(样本, 采样率)`。
async fn collect_pcm_frames(node: &OpusDecodeNode, frames: Vec<Vec<u8>>) -> Vec<(Vec<f32>, u32)> {
    let cancel = CancellationToken::new();
    let events = frames
        .into_iter()
        .map(|data| Ok(PipelineEvent::AudioFrame(data)))
        .collect::<Vec<_>>();
    let upstream: EventStream = Box::pin(stream::iter(events));
    let out = node.stream(upstream, &NodeContext::new(cancel));
    Box::pin(out)
        .filter_map(|item| async move {
            match item {
                Ok(PipelineEvent::PcmFrame(samples, rate)) => Some((samples, rate)),
                _ => None,
            }
        })
        .collect()
        .await
}

/// 生成 `source_rate` 的多帧 Opus 音频（每帧 20ms）。
fn encode_opus_frames(source_rate: u32, frame_count: usize) -> Vec<Vec<u8>> {
    let frame_samples = source_rate as usize * 20 / 1000; // 20ms
    let mut encoder = ropus::Encoder::builder(
        source_rate,
        ropus::Channels::Mono,
        ropus::Application::Audio,
    )
    .build()
    .expect("build opus encoder");
    let mut pcm = vec![0i16; frame_samples];
    for (i, s) in pcm.iter_mut().enumerate() {
        let t = i as f32 / source_rate as f32;
        *s = (12000.0 * (2.0 * std::f32::consts::PI * 440.0 * t).sin()) as i16;
    }
    let mut out = [0u8; 4000];
    (0..frame_count)
        .map(|_| {
            let n = encoder.encode(&pcm, &mut out).expect("encode opus frame");
            out[..n].to_vec()
        })
        .collect()
}

fn audio_param(sample_rate: u32) -> AudioParam {
    AudioParam {
        format: AudioFormat::Opus,
        sample_rate,
        channels: 1,
        frame_duration: 20,
    }
}

/// 客户端声明非 16k 上行（如 24k 浏览器）→ on_configure 更新模板 → 解码后重采样到 16k。
#[tokio::test]
async fn opus_node_resamples_configured_24k_to_16k() {
    let node = OpusDecodeNode::new();
    node.on_configure(&PipelineEvent::Configure(audio_param(24000)));

    let frames = encode_opus_frames(24000, 8);
    let pcm = collect_pcm_frames(&node, frames).await;

    assert!(!pcm.is_empty(), "expected at least one PcmFrame");
    for (_samples, rate) in &pcm {
        assert_eq!(*rate, 16000, "downstream must receive 16k PCM, got {rate}");
    }
    let total: usize = pcm.iter().map(|(s, _)| s.len()).sum();
    assert!(total > 0, "expected non-empty resampled PCM");
}

/// 48k（多数浏览器原生 AudioContext 采样率）同样归一化到 16k。
#[tokio::test]
async fn opus_node_resamples_configured_48k_to_16k() {
    let node = OpusDecodeNode::new();
    node.on_configure(&PipelineEvent::Configure(audio_param(48000)));

    let frames = encode_opus_frames(48000, 8);
    let pcm = collect_pcm_frames(&node, frames).await;

    assert!(!pcm.is_empty(), "expected at least one PcmFrame");
    for (_samples, rate) in &pcm {
        assert_eq!(*rate, 16000, "downstream must receive 16k PCM, got {rate}");
    }
}

/// 未配置（默认 16k）时按原样透传 16k，不引入重采样。
#[tokio::test]
async fn opus_node_passthrough_16k_without_configure() {
    let node = OpusDecodeNode::new();

    let frames = encode_opus_frames(16000, 6);
    let pcm = collect_pcm_frames(&node, frames).await;

    assert!(!pcm.is_empty(), "expected at least one PcmFrame");
    for (_samples, rate) in &pcm {
        assert_eq!(*rate, 16000, "default path must stay 16k, got {rate}");
    }
}

/// 能力 look up：`TtsNode` 通过 `capabilities()` 上报 `AudioSpec`，可被 Session `downcast_ref` 解析。
#[test]
fn tts_node_reports_audio_spec_capability() {
    let node = TtsNode::new(Arc::new(FakeTts));

    let spec = node
        .capabilities()
        .into_iter()
        .find_map(|c| c.downcast_ref::<AudioSpec>().cloned());

    let spec = spec.expect("TtsNode must declare an AudioSpec capability");
    assert_eq!(spec.sample_rate, 16000);
    assert_eq!(spec.channel, 1);
    assert_eq!(spec.frame_duration_ms, 60);
}
