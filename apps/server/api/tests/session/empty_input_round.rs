use framework::id::gen_id;
use futures::StreamExt;
use service::frame::{Frame, FrameResult, InputMode};
use service::message::{hello::HelloMessage, tts::TtsState};
use service::pipeline::{AudioSpec, EventStream, Node, NodeCapability, NodeContext, PipelineEvent};
use service::session::{SessionBuilder, SessionConfig as ServiceSessionConfig};
use service::types::EmptyKind;
use tracing_test::traced_test;

use crate::common::tear_down;
use crate::session::helpers::{
    create_session_channel, get_tts_audio, recv_frame, recv_llm_tts_loop, send_frame,
};

/// 全 mock 链节点：把 empty 轮 / 文本轮映射为下游所需的事件序列，
/// 驱动 round 观察者（`tap_rx`）与 Session 生命周期。纯确定性。
struct MockChainNode;

impl Node for MockChainNode {
    fn new_instance(&self) -> std::sync::Arc<dyn Node> {
        std::sync::Arc::new(MockChainNode)
    }

    fn stream(&self, upstream: EventStream, _ctx: &NodeContext) -> EventStream {
        Box::pin(upstream.flat_map(move |r| {
            let evs: EventStream = match r {
                Ok(event) => match event {
                    // 空输入：提示语表达
                    PipelineEvent::FinishTurn => {
                        let v: Vec<_> = vec![
                            PipelineEvent::EmptyInput,
                            PipelineEvent::TextChunk {
                                text: "请再说一遍，我没有听清。".to_string(),
                                emotion: None,
                            },
                            PipelineEvent::AudioOut {
                                audio: vec![vec![0u8; 20]],
                                is_first: true,
                                is_last: false,
                            },
                            PipelineEvent::AudioOut {
                                audio: vec![vec![0u8; 20]],
                                is_first: false,
                                is_last: true,
                            },
                        ];
                        Box::pin(futures::stream::iter(v.into_iter().map(Ok)))
                    }
                    // Voice → VAD 检出语音（置 Session speech_active，避免 ListenStop 注入与上方空输入双发）
                    PipelineEvent::AudioFrame(_) => Box::pin(futures::stream::iter(vec![Ok(
                        PipelineEvent::SpeechStarted,
                    )])),
                    // 文本轮：正常表达
                    PipelineEvent::TurnText { text, prob } => {
                        let v: Vec<_> = vec![
                            PipelineEvent::TurnComplete {
                                text: text.clone(),
                                prob,
                            },
                            PipelineEvent::TextChunk {
                                text,
                                emotion: None,
                            },
                            PipelineEvent::AudioOut {
                                audio: vec![vec![0u8; 20]],
                                is_first: true,
                                is_last: false,
                            },
                            PipelineEvent::AudioOut {
                                audio: vec![vec![0u8; 20]],
                                is_first: false,
                                is_last: true,
                            },
                        ];
                        Box::pin(futures::stream::iter(v.into_iter().map(Ok)))
                    }
                    other => Box::pin(futures::stream::iter(vec![Ok(other)])),
                },
                Err(e) => Box::pin(futures::stream::iter(vec![Err(e)])),
            };
            evs
        }))
    }
}

/// 模拟"录无人声"：VAD 不触发（不发 `SpeechStarted`）、`FinishTurn` 不产表达（ASR 无流→`Nothing`），
/// 提示语仅来自 Session 在 `ListenStop` 注入的 `EmptyInput`。
struct NoVoiceChainNode;

impl Node for NoVoiceChainNode {
    fn new_instance(&self) -> std::sync::Arc<dyn Node> {
        std::sync::Arc::new(NoVoiceChainNode)
    }

    fn stream(&self, upstream: EventStream, _ctx: &NodeContext) -> EventStream {
        Box::pin(upstream.flat_map(move |r| {
            let evs: EventStream = match r {
                Ok(event) => match event {
                    // 先原样透传 EmptyInput 让 round `(After, EmptyInput)` 触发 EmptyTurn 轮转，
                    // 再产出提示语 TTS（等价真实链中 EmptyInput 经 Opus/VAD/ASR 透传）。
                    PipelineEvent::EmptyInput => {
                        let v: Vec<_> = vec![
                            PipelineEvent::EmptyInput,
                            PipelineEvent::TextChunk {
                                text: "请再说一遍，我没有听清。".to_string(),
                                emotion: None,
                            },
                            PipelineEvent::AudioOut {
                                audio: vec![vec![0u8; 20]],
                                is_first: true,
                                is_last: false,
                            },
                            PipelineEvent::AudioOut {
                                audio: vec![vec![0u8; 20]],
                                is_first: false,
                                is_last: true,
                            },
                        ];
                        Box::pin(futures::stream::iter(v.into_iter().map(Ok)))
                    }
                    PipelineEvent::TurnText { text, prob } => {
                        let v: Vec<_> = vec![
                            PipelineEvent::TurnComplete {
                                text: text.clone(),
                                prob,
                            },
                            PipelineEvent::TextChunk {
                                text,
                                emotion: None,
                            },
                            PipelineEvent::AudioOut {
                                audio: vec![vec![0u8; 20]],
                                is_first: true,
                                is_last: false,
                            },
                            PipelineEvent::AudioOut {
                                audio: vec![vec![0u8; 20]],
                                is_first: false,
                                is_last: true,
                            },
                        ];
                        Box::pin(futures::stream::iter(v.into_iter().map(Ok)))
                    }
                    // Voice/FinishTurn：VAD 未触发，透传不产表达
                    other => Box::pin(futures::stream::iter(vec![Ok(other)])),
                },
                Err(e) => Box::pin(futures::stream::iter(vec![Err(e)])),
            };
            evs
        }))
    }
}

/// 复现：空输入提示轮之后，新一轮文本输入必须仍能响应。
/// 修复前：提示轮无 `TurnComplete` → Session 不轮转，`shadow_round` 变为已死轮，
/// 后续 `Frame::Input` 投喂进死链无人消费 → 无 STT → 断言超时（复现 bug）。
/// 修复后：EmptyInput → `RoundEvent::EmptyTurn` → 轮转出全新 shadow → 文本轮 STT+TTS 正常。
#[tokio::test]
#[traced_test]
async fn test_empty_input_prompt_round_then_text_input_responds() -> anyhow::Result<()> {
    let session_id = gen_id();
    let session_ctx = SessionBuilder::new()
        .with_id(session_id)
        .with_node_templates(vec![std::sync::Arc::new(MockChainNode)])
        .with_config(ServiceSessionConfig {
            close_connection_no_activity_time: Some(30000),
            silence_voice_timeout: Some(1200),
            barge_in_lockout_ms: Some(250),
        })
        .build();
    let input_tx = session_ctx.input_tx;
    let mut output_rx = session_ctx.output_rx;
    tokio::spawn(session_ctx.session.start());

    send_frame(&input_tx, Frame::Hello(HelloMessage::default()));
    let f = recv_frame(&mut output_rx, "hello result").await;
    assert!(
        matches!(f, FrameResult::HelloResult(..)),
        "expected HelloResult, got {f}"
    );

    // 空输入提示轮：listen start → voice（任意一帧）→ listen stop
    send_frame(
        &input_tx,
        Frame::ListenStart {
            barge_in: true,
            is_voice_break_detect: false,
        },
    );
    send_frame(
        &input_tx,
        Frame::Voice {
            data: vec![0u8; 20],
        },
    );
    send_frame(&input_tx, Frame::ListenStop);

    // 提示语 TTS 完整播出：Start → LLMResult(固定句) → SentenceStart → Audio → SentenceEnd → Stop
    let f = recv_frame(&mut output_rx, "empty tts start").await;
    assert!(
        matches!(f, FrameResult::TTSResult(ref m) if m.state == Some(TtsState::Start)),
        "expected TTSResult(Start), got {f}"
    );
    let sentences = recv_llm_tts_loop(&mut output_rx, "empty prompt").await;
    assert_eq!(sentences, 1, "expected one prompt sentence");

    // 关键：空输入提示轮之后，新一轮文本输入必须正常响应
    send_frame(
        &input_tx,
        Frame::Input {
            text: "你好".to_string(),
            mode: InputMode::Normal,
        },
    );
    let f = recv_frame(&mut output_rx, "stt after empty").await;
    assert!(
        matches!(f, FrameResult::STTResult(..)),
        "空输入提示轮后新一轮文本应产出 STT，got {f}"
    );
    let f = recv_frame(&mut output_rx, "tts start after empty").await;
    assert!(
        matches!(f, FrameResult::TTSResult(ref m) if m.state == Some(TtsState::Start)),
        "expected TTSResult(Start) after empty, got {f}"
    );
    let sentences = recv_llm_tts_loop(&mut output_rx, "text after empty").await;
    assert_eq!(sentences, 1, "expected one sentence after empty");

    drop(input_tx);
    Ok(())
}

/// 连续多轮空输入：每轮都必须产出提示语 TTS，且之后仍未卡死。
async fn empty_round_prompt(
    input_tx: &tokio::sync::mpsc::UnboundedSender<Frame>,
    output_rx: &mut tokio::sync::mpsc::UnboundedReceiver<service::frame::OutputMessage>,
    round: &str,
) {
    send_frame(
        input_tx,
        Frame::ListenStart {
            barge_in: true,
            is_voice_break_detect: false,
        },
    );
    send_frame(
        input_tx,
        Frame::Voice {
            data: vec![0u8; 20],
        },
    );
    send_frame(input_tx, Frame::ListenStop);
    let f = recv_frame(output_rx, &format!("{round} tts start")).await;
    assert!(
        matches!(
            f,
            FrameResult::TTSResult(ref m) if m.state == Some(TtsState::Start)
        ),
        "{round}: expected TTSResult(Start), got {f}"
    );
    let sentences = recv_llm_tts_loop(output_rx, round).await;
    assert_eq!(sentences, 1, "{round}: expected one prompt sentence");
}

#[tokio::test]
#[traced_test]
async fn test_empty_input_consecutive_rounds_prompt_each_time() -> anyhow::Result<()> {
    let session_id = gen_id();
    let session_ctx = SessionBuilder::new()
        .with_id(session_id)
        .with_node_templates(vec![std::sync::Arc::new(MockChainNode)])
        .with_config(ServiceSessionConfig {
            close_connection_no_activity_time: Some(30000),
            silence_voice_timeout: Some(1200),
            barge_in_lockout_ms: Some(250),
        })
        .build();
    let input_tx = session_ctx.input_tx;
    let mut output_rx = session_ctx.output_rx;
    tokio::spawn(session_ctx.session.start());

    send_frame(&input_tx, Frame::Hello(HelloMessage::default()));
    let f = recv_frame(&mut output_rx, "hello result").await;
    assert!(
        matches!(f, FrameResult::HelloResult(..)),
        "expected HelloResult, got {f}"
    );

    for i in 1..=3 {
        empty_round_prompt(&input_tx, &mut output_rx, &format!("empty round {i}")).await;
    }

    send_frame(
        &input_tx,
        Frame::Input {
            text: "你好".to_string(),
            mode: InputMode::Normal,
        },
    );
    let f = recv_frame(&mut output_rx, "stt after consecutive empty").await;
    assert!(
        matches!(f, FrameResult::STTResult(..)),
        "连续空输入后文本应产出 STT，got {f}"
    );
    let f = recv_frame(&mut output_rx, "tts start after consecutive empty").await;
    assert!(
        matches!(f, FrameResult::TTSResult(ref m) if m.state == Some(TtsState::Start)),
        "expected TTSResult(Start), got {f}"
    );
    let sentences = recv_llm_tts_loop(&mut output_rx, "text after consecutive empty").await;
    assert_eq!(
        sentences, 1,
        "expected one sentence after consecutive empty"
    );

    drop(input_tx);
    Ok(())
}

#[tokio::test]
#[traced_test]
async fn test_empty_input_voice_during_prompt_barge_in_responds() -> anyhow::Result<()> {
    let session_id = gen_id();
    let session_ctx = SessionBuilder::new()
        .with_id(session_id)
        .with_node_templates(vec![std::sync::Arc::new(MockChainNode)])
        .with_config(ServiceSessionConfig {
            close_connection_no_activity_time: Some(30000),
            silence_voice_timeout: Some(1200),
            barge_in_lockout_ms: Some(250),
        })
        .build();
    let input_tx = session_ctx.input_tx;
    let mut output_rx = session_ctx.output_rx;
    tokio::spawn(session_ctx.session.start());

    send_frame(&input_tx, Frame::Hello(HelloMessage::default()));
    let f = recv_frame(&mut output_rx, "hello result").await;
    assert!(
        matches!(f, FrameResult::HelloResult(..)),
        "expected HelloResult, got {f}"
    );

    // 空输入提示轮：只等提示语开始播放（SentenceStart），不断言 Stop，模拟"播放中"。
    send_frame(
        &input_tx,
        Frame::ListenStart {
            barge_in: true,
            is_voice_break_detect: false,
        },
    );
    send_frame(
        &input_tx,
        Frame::Voice {
            data: vec![0u8; 20],
        },
    );
    send_frame(&input_tx, Frame::ListenStop);
    let f = recv_frame(&mut output_rx, "empty tts start").await;
    assert!(
        matches!(f, FrameResult::TTSResult(ref m) if m.state == Some(TtsState::Start)),
        "expected TTSResult(Start), got {f}"
    );
    match recv_frame(&mut output_rx, "empty llm or sentence start").await {
        FrameResult::LLMResult(..) => {
            let f = recv_frame(&mut output_rx, "empty sentence start").await;
            assert!(
                matches!(f, FrameResult::TTSResult(ref m) if m.state == Some(TtsState::SentenceStart)),
                "expected TTSResult(SentenceStart), got {f}"
            );
        }
        other => panic!("expected LLMResult, got {other}"),
    }

    // 提示语播放中：新 ListenStart 触发 barge-in 打断运行轮，再喂新一轮语音。
    send_frame(
        &input_tx,
        Frame::ListenStart {
            barge_in: true,
            is_voice_break_detect: false,
        },
    );
    send_frame(
        &input_tx,
        Frame::Voice {
            data: vec![0u8; 20],
        },
    );
    send_frame(&input_tx, Frame::ListenStop);

    // 排空被 barge-in 中断轮的残留，直到出现新一轮 STT 或提示语（即会话仍响应，未"无反应"）。
    loop {
        let f = recv_frame(&mut output_rx, "post-barge-in drain to stt").await;
        if matches!(f, FrameResult::STTResult(..)) {
            let f = recv_frame(&mut output_rx, "post-barge-in tts start").await;
            assert!(
                matches!(f, FrameResult::TTSResult(ref m) if m.state == Some(TtsState::Start)),
                "expected TTSResult(Start) post-barge-in, got {f}"
            );
            recv_llm_tts_loop(&mut output_rx, "post-barge-in round").await;
            break;
        }
        if matches!(f, FrameResult::TTSResult(ref m) if m.state == Some(TtsState::Start)) {
            let sentences = recv_llm_tts_loop(&mut output_rx, "post-barge-in prompt").await;
            assert_eq!(sentences, 1, "expected one prompt sentence post-barge-in");
            break;
        }
    }

    drop(input_tx);
    Ok(())
}

#[tokio::test]
#[traced_test]
/// 真实节点链路（Opus→Earshot VAD→XAsr→Turn→Ling(Echo)→Matcha TTS）。
/// 覆盖真实「连续多轮 + TTS 播放中输入(barge-in) + 文本输入」全链，验证会话每一轮都响应、
/// 不发生"无反应"(死 round)。
///
/// 注意：真实 XAsr 对合成"噪声/无内容"返回空不可靠（Earshot = rnnoise 去噪 + 语音模型，
/// 会滤除白噪/纯音，见 earshot/preprocessor.rs）；"空输入触提示语"已在上面两个全 mock 测试
/// 确定性覆盖，故本真实测试以真实语音驱动，验证真实 VAD/TTS 时序下的轮转与 barge-in。
async fn test_real_nodes_consecutive_barge_in_and_text_responds() -> anyhow::Result<()> {
    let (input_tx, mut output_rx, container, state) = create_session_channel().await;
    send_frame(&input_tx, Frame::Hello(HelloMessage::default()));
    let f = recv_frame(&mut output_rx, "hello result").await;
    assert!(
        matches!(f, FrameResult::HelloResult(..)),
        "expected HelloResult, got {f}"
    );

    // 第一轮：喂真实 TTS 语音（确定性触发 VAD + ASR → 正常文本轮）。真实 XAsr 对合成
    // "噪声/无内容"返回空不可靠（Earshot=rnnoise+语音模型会滤除白噪/纯音），故此处以真实语音
    // 驱动真实链路，验证会话在真实 Opus/VAD/XAsr/Matcha 下每一轮都正常响应、不"无反应"。
    let speech1 = get_tts_audio("你好").await;
    send_frame(
        &input_tx,
        Frame::ListenStart {
            barge_in: true,
            is_voice_break_detect: false,
        },
    );
    for n in 0..speech1.len() {
        send_frame(
            &input_tx,
            Frame::Voice {
                data: speech1.get(n).unwrap().to_vec(),
            },
        );
    }
    send_frame(&input_tx, Frame::ListenStop);
    let f = recv_frame(&mut output_rx, "real round1 stt").await;
    assert!(
        matches!(f, FrameResult::STTResult(..)),
        "real round1 expected STTResult, got {f}"
    );
    let f = recv_frame(&mut output_rx, "real round1 tts start").await;
    assert!(
        matches!(f, FrameResult::TTSResult(ref m) if m.state == Some(TtsState::Start)),
        "real round1 expected TTSResult(Start), got {f}"
    );
    let sentences = recv_llm_tts_loop(&mut output_rx, "real round1").await;
    assert!(sentences >= 1, "real round1 expected >= 1 sentence");

    // 表达中（真实 Matcha TTS 有播放时长）：新 ListenStart 触发 barge-in，再喂真实语音。
    let real_audio = get_tts_audio("你好，小叽").await;
    send_frame(
        &input_tx,
        Frame::ListenStart {
            barge_in: true,
            is_voice_break_detect: false,
        },
    );
    for n in 0..real_audio.len() {
        send_frame(
            &input_tx,
            Frame::Voice {
                data: real_audio.get(n).unwrap().to_vec(),
            },
        );
    }
    send_frame(&input_tx, Frame::ListenStop);

    // 排空残留直到出现新一轮 STT（会话未"无反应"）。
    let f = loop {
        let f = recv_frame(&mut output_rx, "real post-barge-in drain to stt").await;
        if matches!(f, FrameResult::STTResult(..)) {
            break f;
        }
    };
    assert!(
        matches!(f, FrameResult::STTResult(..)),
        "真实环境中 TTS 播放中输入后应产出 STT（未无反应），got {f}"
    );
    let f = recv_frame(&mut output_rx, "real post-barge-in tts start").await;
    assert!(
        matches!(f, FrameResult::TTSResult(ref m) if m.state == Some(TtsState::Start)),
        "expected TTSResult(Start), got {f}"
    );
    recv_llm_tts_loop(&mut output_rx, "real post-barge-in round").await;

    // 决定性兜底：文本输入必须产出 STT（证明会话未卡死在死 round）。
    send_frame(
        &input_tx,
        Frame::Input {
            text: "你好".to_string(),
            mode: InputMode::Normal,
        },
    );
    let f = recv_frame(&mut output_rx, "real stt final text").await;
    assert!(
        matches!(f, FrameResult::STTResult(..)),
        "真实环境文本输入应产出 STT，got {f}"
    );
    let f = recv_frame(&mut output_rx, "real tts start final text").await;
    assert!(
        matches!(f, FrameResult::TTSResult(ref m) if m.state == Some(TtsState::Start)),
        "expected TTSResult(Start) final, got {f}"
    );
    recv_llm_tts_loop(&mut output_rx, "real final text round").await;

    drop(input_tx);
    let _ = &state.conn.close().await?;
    tear_down(container).await;
    Ok(())
}

#[tokio::test]
#[traced_test]
/// mock 复现真实"录无人声（静音）"：VAD 从不触发（`NoVoiceChainNode`），
/// `ListenStop` 时 Session 注入 `EmptyInput` → 提示语。修复前：无任何事件 → 超时（"无反应"）。
/// 修复后：提示语 TTS 播出一次，且之后文本输入仍正常响应（未卡死）。
async fn test_manual_no_voice_input_prompts_and_stays_responsive() -> anyhow::Result<()> {
    let session_id = gen_id();
    let session_ctx = SessionBuilder::new()
        .with_id(session_id)
        .with_node_templates(vec![std::sync::Arc::new(NoVoiceChainNode)])
        .with_config(ServiceSessionConfig {
            close_connection_no_activity_time: Some(30000),
            silence_voice_timeout: Some(1200),
            barge_in_lockout_ms: Some(250),
        })
        .build();
    let input_tx = session_ctx.input_tx;
    let mut output_rx = session_ctx.output_rx;
    tokio::spawn(session_ctx.session.start());

    send_frame(&input_tx, Frame::Hello(HelloMessage::default()));
    let f = recv_frame(&mut output_rx, "hello result").await;
    assert!(
        matches!(f, FrameResult::HelloResult(..)),
        "expected HelloResult, got {f}"
    );

    send_frame(
        &input_tx,
        Frame::ListenStart {
            barge_in: true,
            is_voice_break_detect: false,
        },
    );
    send_frame(
        &input_tx,
        Frame::Voice {
            data: vec![0u8; 20],
        },
    );
    send_frame(&input_tx, Frame::ListenStop);

    let f = recv_frame(&mut output_rx, "no-voice tts start").await;
    assert!(
        matches!(f, FrameResult::TTSResult(ref m) if m.state == Some(TtsState::Start)),
        "manual 无人声应播报提示语（修复后），got {f}"
    );
    let sentences = recv_llm_tts_loop(&mut output_rx, "no-voice prompt").await;
    assert_eq!(sentences, 1, "expected one prompt sentence");

    send_frame(
        &input_tx,
        Frame::Input {
            text: "你好".to_string(),
            mode: InputMode::Normal,
        },
    );
    let f = recv_frame(&mut output_rx, "stt after no-voice").await;
    assert!(
        matches!(f, FrameResult::STTResult(..)),
        "无人声提示后新一轮文本应产出 STT，got {f}"
    );
    let f = recv_frame(&mut output_rx, "tts start after no-voice").await;
    assert!(
        matches!(f, FrameResult::TTSResult(ref m) if m.state == Some(TtsState::Start)),
        "expected TTSResult(Start) after no-voice, got {f}"
    );
    let sentences = recv_llm_tts_loop(&mut output_rx, "text after no-voice").await;
    assert_eq!(sentences, 1, "expected one sentence after no-voice");

    drop(input_tx);
    Ok(())
}

#[tokio::test]
#[traced_test]
/// 真实节点链路下的"manual 录无人声"修复验证。
/// 完全静音（零 PCM）经 Opus 解零 → 真实 Earshot VAD（rnnoise+语音模型）必不触发
/// `SpeechStarted` → XAsr 无流 → 返回 `Nothing`（确定性）。修复前：无事件→超时（"无反应"）；
/// 修复后：`ListenStop` 注入 `EmptyInput` → Echo 提示语 TTS 播出一次，之后文本输入仍响应。
async fn test_real_manual_no_voice_input_prompts_and_stays_responsive() -> anyhow::Result<()> {
    let (input_tx, mut output_rx, container, state) = create_session_channel().await;
    send_frame(&input_tx, Frame::Hello(HelloMessage::default()));
    let f = recv_frame(&mut output_rx, "hello result").await;
    assert!(
        matches!(f, FrameResult::HelloResult(..)),
        "expected HelloResult, got {f}"
    );

    send_frame(
        &input_tx,
        Frame::ListenStart {
            barge_in: true,
            is_voice_break_detect: false,
        },
    );
    for _ in 0..5 {
        send_frame(
            &input_tx,
            Frame::Voice {
                data: vec![0u8; 960],
            },
        );
    }
    send_frame(&input_tx, Frame::ListenStop);

    let f = recv_frame(&mut output_rx, "real no-voice tts start").await;
    assert!(
        matches!(f, FrameResult::TTSResult(ref m) if m.state == Some(TtsState::Start)),
        "真实 manual 无人声应播报提示语（修复后），got {f}"
    );
    let sentences = recv_llm_tts_loop(&mut output_rx, "real no-voice prompt").await;
    assert_eq!(sentences, 1, "expected one prompt sentence");

    send_frame(
        &input_tx,
        Frame::Input {
            text: "你好".to_string(),
            mode: InputMode::Normal,
        },
    );
    let f = recv_frame(&mut output_rx, "real stt after no-voice").await;
    assert!(
        matches!(f, FrameResult::STTResult(..)),
        "真实无人声提示后新一轮文本应产出 STT，got {f}"
    );
    let f = recv_frame(&mut output_rx, "real tts start after no-voice").await;
    assert!(
        matches!(f, FrameResult::TTSResult(ref m) if m.state == Some(TtsState::Start)),
        "expected TTSResult(Start) after no-voice, got {f}"
    );
    let sentences = recv_llm_tts_loop(&mut output_rx, "real text after no-voice").await;
    assert_eq!(sentences, 1, "expected one sentence after no-voice");

    drop(input_tx);
    let _ = &state.conn.close().await?;
    tear_down(container).await;
    Ok(())
}

/// 中枢 gatekeeper 全 mock 链节点：响应 `Prompt{kind,count}`（等价真实 LingNode 把
/// `Prompt` → `Input::Empty{kind,count}` → Ling 渲染分级提示语），`AudioFrame` 不触发
/// `SpeechStarted`（模拟无人声 → ListenStop 由中枢补注入 `EmptyInput`），`EmptyInput` 原样透传
/// 以驱动 round `(After, EmptyInput)` → `EmptyTurn` 轮转。
struct GatekeeperChainNode;

impl GatekeeperChainNode {
    fn prompt_text(kind: EmptyKind, count: u32) -> String {
        match (kind, count) {
            (EmptyKind::Manual, _) => "请再说一遍，我没有听清。".to_string(),
            (EmptyKind::Wake, 1) => "想让我帮你做什么呢？".to_string(),
            (EmptyKind::Wake, _) => "你可以告诉我你的需求，比如播放音乐或设置提醒。".to_string(),
            (EmptyKind::AutoSpoke, 1) => "抱歉，我没听清，可以再说一次吗？".to_string(),
            (EmptyKind::AutoSpoke, _) => "没能听清，请换个说法或说得慢一些。".to_string(),
            (EmptyKind::Silence, 1) => "我一直在听，你可以尽管说。".to_string(),
            (EmptyKind::Silence, _) => "请开口告诉我你想做什么。".to_string(),
            // 连续监听：静默等待，不产出提示。
            (EmptyKind::Continuing, _) => String::new(),
        }
    }
}

impl Node for GatekeeperChainNode {
    fn new_instance(&self) -> std::sync::Arc<dyn Node> {
        std::sync::Arc::new(GatekeeperChainNode)
    }

    fn stream(&self, upstream: EventStream, _ctx: &NodeContext) -> EventStream {
        Box::pin(upstream.flat_map(move |r| {
            let evs: EventStream = match r {
                Ok(event) => match event {
                    PipelineEvent::Prompt { kind, count } => {
                        let text = GatekeeperChainNode::prompt_text(kind, count);
                        if text.is_empty() {
                            Box::pin(futures::stream::iter(vec![]))
                        } else {
                            Box::pin(futures::stream::iter(vec![
                                Ok(PipelineEvent::TextChunk {
                                    text,
                                    emotion: None,
                                }),
                                Ok(PipelineEvent::AudioOut {
                                    audio: vec![vec![0u8; 20]],
                                    is_first: true,
                                    is_last: false,
                                }),
                                Ok(PipelineEvent::AudioOut {
                                    audio: vec![vec![0u8; 20]],
                                    is_first: false,
                                    is_last: true,
                                }),
                            ]))
                        }
                    }
                    // 文本轮：正常表达（产出 TurnComplete → STT + TTS）。
                    PipelineEvent::TurnText { text, prob } => {
                        Box::pin(futures::stream::iter(vec![
                            Ok(PipelineEvent::TurnComplete {
                                text: text.clone(),
                                prob,
                            }),
                            Ok(PipelineEvent::TextChunk {
                                text,
                                emotion: None,
                            }),
                            Ok(PipelineEvent::AudioOut {
                                audio: vec![vec![0u8; 20]],
                                is_first: true,
                                is_last: false,
                            }),
                            Ok(PipelineEvent::AudioOut {
                                audio: vec![vec![0u8; 20]],
                                is_first: false,
                                is_last: true,
                            }),
                        ]))
                    }
                    other => Box::pin(futures::stream::iter(vec![Ok(other)])),
                },
                Err(e) => Box::pin(futures::stream::iter(vec![Err(e)])),
            };
            evs
        }))
    }
}

fn gatekeeper_session() -> (
    tokio::sync::mpsc::UnboundedSender<Frame>,
    tokio::sync::mpsc::UnboundedReceiver<service::frame::OutputMessage>,
) {
    let session_ctx = SessionBuilder::new()
        .with_id(gen_id())
        .with_node_templates(vec![std::sync::Arc::new(GatekeeperChainNode)])
        .with_config(ServiceSessionConfig {
            close_connection_no_activity_time: Some(30000),
            silence_voice_timeout: Some(1200),
            barge_in_lockout_ms: Some(250),
        })
        .build();
    let input_tx = session_ctx.input_tx;
    let output_rx = session_ctx.output_rx;
    tokio::spawn(session_ctx.session.start());
    (input_tx, output_rx)
}

async fn recv_prompt_sentence(
    output_rx: &mut tokio::sync::mpsc::UnboundedReceiver<service::frame::OutputMessage>,
    round: &str,
) -> String {
    let f = recv_frame(output_rx, &format!("{round} tts start")).await;
    assert!(
        matches!(f, FrameResult::TTSResult(ref m) if m.state == Some(TtsState::Start)),
        "gatekeeper {round}: expected TTSResult(Start), got {f}"
    );
    let mut text = String::new();
    loop {
        match recv_frame(output_rx, &format!("{round}/sentence text")).await {
            FrameResult::TTSResult(ref m) if m.state == Some(TtsState::SentenceStart) => {
                if let Some(t) = &m.text {
                    text.push_str(t);
                }
                loop {
                    match recv_frame(output_rx, &format!("{round}/audio or end")).await {
                        FrameResult::AudioResult(_) => {}
                        FrameResult::TTSResult(ref m) if m.state == Some(TtsState::SentenceEnd) => {
                            break;
                        }
                        other => {
                            panic!("{round}: expected AudioResult or SentenceEnd, got {other}")
                        }
                    }
                }
            }
            // 跳过 LLM 表情占位帧，直到句子开始。
            FrameResult::LLMResult(..) => {}
            FrameResult::TTSResult(ref m) if m.state == Some(TtsState::Stop) => break,
            other => {
                panic!("{round}: expected LLMResult or TTSResult(Stop), got {other}")
            }
        }
    }
    text
}

/// 契约：manual（push-to-talk）无人声，每次按键都给一次提示语（`Prompt{Manual,1}` →
/// 渲染 Manual 固定句），提示后回到监听等待；连续多次按键每次都提示，不受 Rule of three 限次。
#[tokio::test]
#[traced_test]
async fn test_gatekeeper_manual_prompts_each_keypress() -> anyhow::Result<()> {
    let (input_tx, mut output_rx) = gatekeeper_session();
    send_frame(&input_tx, Frame::Hello(HelloMessage::default()));
    let f = recv_frame(&mut output_rx, "hello result").await;
    assert!(matches!(f, FrameResult::HelloResult(..)), "got {f}");

    // 连续三次 Manual 无人声按键，每次都须产 Manual 提示语（事件驱动，不因 Rule of three 收敛）。
    for i in 1..=3 {
        send_frame(
            &input_tx,
            Frame::ListenStart {
                barge_in: true,
                is_voice_break_detect: false,
            },
        );
        send_frame(
            &input_tx,
            Frame::Voice {
                data: vec![0u8; 20],
            },
        );
        send_frame(&input_tx, Frame::ListenStop);
        let text = recv_prompt_sentence(&mut output_rx, &format!("manual keypress {i}")).await;
        assert_eq!(
            text, "请再说一遍，我没有听清。",
            "Manual 第 {i} 次按键应提示"
        );
    }

    // 提示后会话仍响应：文本输入正常 STT/TTS。
    send_frame(
        &input_tx,
        Frame::Input {
            text: "你好".to_string(),
            mode: InputMode::Normal,
        },
    );
    let f = recv_frame(&mut output_rx, "stt after manual").await;
    assert!(
        matches!(f, FrameResult::STTResult(..)),
        "Manual 提示后文本应产出 STT，got {f}"
    );

    drop(input_tx);
    Ok(())
}

/// 契约：auto（静默）无人声 → Silence 分级引导，最多 3 次（Rule of three），之后不再提示。
#[tokio::test]
#[traced_test]
async fn test_gatekeeper_silence_grades_to_three_then_stops() -> anyhow::Result<()> {
    let (input_tx, mut output_rx) = gatekeeper_session();
    send_frame(&input_tx, Frame::Hello(HelloMessage::default()));
    let f = recv_frame(&mut output_rx, "hello result").await;
    assert!(matches!(f, FrameResult::HelloResult(..)), "got {f}");

    let expected = [
        "我一直在听，你可以尽管说。",
        "请开口告诉我你想做什么。",
        "请开口告诉我你想做什么。",
    ];

    for (i, exp) in expected.iter().enumerate() {
        send_frame(
            &input_tx,
            Frame::ListenStart {
                barge_in: false,
                is_voice_break_detect: true,
            },
        );
        send_frame(
            &input_tx,
            Frame::Voice {
                data: vec![0u8; 20],
            },
        );
        send_frame(&input_tx, Frame::ListenStop);
        let text = recv_prompt_sentence(&mut output_rx, &format!("silence round {}", i + 1)).await;
        assert_eq!(text, *exp, "Silence round {} 文案", i + 1);
    }

    // 第 4 次无人声：超过 Rule of three，不再提示。
    send_frame(
        &input_tx,
        Frame::ListenStart {
            barge_in: false,
            is_voice_break_detect: true,
        },
    );
    send_frame(
        &input_tx,
        Frame::Voice {
            data: vec![0u8; 20],
        },
    );
    send_frame(&input_tx, Frame::ListenStop);

    drop(input_tx);
    Ok(())
}

/// 文本轮一次性产出多个音频包（模拟"长 TTS 已缓冲"），用于验证 barge-in 打断时
/// session 输出过滤会丢弃该 running round 的残留音频帧。
const BARGE_BATCH_PACKETS: usize = 10;

struct MultiAudioBargeNode;

impl Node for MultiAudioBargeNode {
    fn new_instance(&self) -> std::sync::Arc<dyn Node> {
        std::sync::Arc::new(MultiAudioBargeNode)
    }

    /// 声明下行音频能力（pacer 节奏），使 TTS 分帧播、给 barge-in 预留插入点。
    fn capabilities(&self) -> Vec<Box<NodeCapability>> {
        vec![Box::new(AudioSpec {
            sample_rate: 24000,
            channel: 1,
            frame_duration_ms: 20,
        })]
    }

    fn stream(&self, upstream: EventStream, _ctx: &NodeContext) -> EventStream {
        Box::pin(upstream.flat_map(move |r| {
            let evs: EventStream = match r {
                Ok(event) => match event {
                    PipelineEvent::TurnText { text, prob } => {
                        let mut v = vec![
                            Ok(PipelineEvent::TurnComplete {
                                text: text.clone(),
                                prob,
                            }),
                            Ok(PipelineEvent::TextChunk {
                                text,
                                emotion: None,
                            }),
                        ];
                        v.push(Ok(PipelineEvent::AudioOut {
                            audio: vec![vec![0u8; 20]],
                            is_first: true,
                            is_last: false,
                        }));
                        for _ in 0..BARGE_BATCH_PACKETS {
                            v.push(Ok(PipelineEvent::AudioOut {
                                audio: vec![vec![0u8; 20]],
                                is_first: false,
                                is_last: false,
                            }));
                        }
                        v.push(Ok(PipelineEvent::AudioOut {
                            audio: vec![vec![0u8; 20]],
                            is_first: false,
                            is_last: true,
                        }));
                        Box::pin(futures::stream::iter(v))
                    }
                    other => Box::pin(futures::stream::iter(vec![Ok(other)])),
                },
                Err(e) => Box::pin(futures::stream::iter(vec![Err(e)])),
            };
            evs
        }))
    }
}

fn barge_multi_session() -> (
    tokio::sync::mpsc::UnboundedSender<Frame>,
    tokio::sync::mpsc::UnboundedReceiver<service::frame::OutputMessage>,
) {
    let session_ctx = SessionBuilder::new()
        .with_id(gen_id())
        .with_node_templates(vec![std::sync::Arc::new(MultiAudioBargeNode)])
        .with_config(ServiceSessionConfig {
            close_connection_no_activity_time: Some(30000),
            silence_voice_timeout: Some(1200),
            barge_in_lockout_ms: Some(250),
        })
        .build();
    let input_tx = session_ctx.input_tx;
    let output_rx = session_ctx.output_rx;
    tokio::spawn(session_ctx.session.start());
    (input_tx, output_rx)
}

/// 契约：TTS 播放中 barge-in（=ListenStart），session 应丢弃被中断 round 已排队的
/// 残留音频帧（epoch 过滤），让 TTS 立即停，且会话对后续输入仍保持响应。
#[tokio::test]
#[traced_test]
async fn test_barge_in_drops_leftover_audio_immediately() -> anyhow::Result<()> {
    let (input_tx, mut output_rx) = barge_multi_session();
    send_frame(&input_tx, Frame::Hello(HelloMessage::default()));
    let f = recv_frame(&mut output_rx, "hello result").await;
    assert!(matches!(f, FrameResult::HelloResult(..)), "got {f}");

    // 正常文本轮：启动长 TTS并读走两包，模拟"已播放片段"。
    send_frame(
        &input_tx,
        Frame::Input {
            text: "你好".to_string(),
            mode: InputMode::Normal,
        },
    );
    let f = recv_frame(&mut output_rx, "stt").await;
    assert!(matches!(f, FrameResult::STTResult(..)), "got {f}");
    let f = recv_frame(&mut output_rx, "tts start").await;
    assert!(
        matches!(f, FrameResult::TTSResult(ref m) if m.state == Some(TtsState::Start)),
        "expected TTSResult(Start), got {f}"
    );
    let mut played = 0usize;
    while played < 2 {
        match recv_frame(&mut output_rx, "audio during playback").await {
            FrameResult::AudioResult(_) => played += 1,
            FrameResult::LLMResult(..) => {}
            FrameResult::TTSResult(ref m) if m.state == Some(TtsState::SentenceStart) => {}
            other => panic!("playback: expected AudioResult, got {other}"),
        }
    }

    // 播放中 barge-in（手动模式），随后残留音频应被丢弃；SentenceEnd/Stop 为收尾帧，跳过至 Stop。
    send_frame(
        &input_tx,
        Frame::ListenStart {
            barge_in: true,
            is_voice_break_detect: false,
        },
    );

    let mut leftover_audio = 0usize;
    loop {
        match recv_frame(&mut output_rx, "post-barge-in drain").await {
            FrameResult::AudioResult(_) => {
                leftover_audio += 1;
                assert!(
                    leftover_audio < BARGE_BATCH_PACKETS,
                    "barge-in 后残留音频未被丢弃，已收到 {leftover_audio}（batch={BARGE_BATCH_PACKETS}）"
                );
            }
            FrameResult::TTSResult(ref m) if m.state == Some(TtsState::Stop) => break,
            FrameResult::TTSResult(ref m) if m.state == Some(TtsState::SentenceEnd) => {}
            FrameResult::LLMResult(..) => {}
            other => panic!("post-barge-in: expected AudioResult/TTS Stop, got {other}"),
        }
    }

    // 新一轮文本仍应产出 STT（会话保持响应）。
    send_frame(
        &input_tx,
        Frame::Input {
            text: "再会".to_string(),
            mode: InputMode::Normal,
        },
    );
    let f = recv_frame(&mut output_rx, "stt after barge-in").await;
    assert!(
        matches!(f, FrameResult::STTResult(..)),
        "barge-in 后文本应正常 STT，got {f}"
    );

    drop(input_tx);
    Ok(())
}
