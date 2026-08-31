use std::time::Duration;

use tokio::sync::mpsc;

use service::frame::{Frame, FrameResult, InputMode};
use service::message::{hello::HelloMessage, tts::TtsState};
use tracing_test::traced_test;

use crate::common::tear_down;
use crate::session::helpers::{
    create_mini_session_channel, create_mini_session_with_timeout, create_no_tts_session_channel,
    create_session_channel, get_tts_audio, recv_frame, recv_llm_tts_loop, resample_opus_audio,
    send_frame,
};
use service::message::{AudioFormat, hello::AudioParam};

#[tokio::test]
#[traced_test]
async fn test_chat_flow_hello() -> anyhow::Result<()> {
    /*
     * -> = client -> server, <- = server -> client
     *
     * -> hello
     * <- hello result
     */
    let (input_tx, mut output_rx) = create_mini_session_channel().await;
    send_frame(&input_tx, Frame::Hello(HelloMessage::default()));
    let f = recv_frame(&mut output_rx, "hello result").await;
    match f {
        // TtsMute 引擎自报下行格式 (16000, 1, 60)，经模板 look up 驱动握手声明。
        FrameResult::HelloResult(HelloMessage { audio_params, .. }) => match audio_params {
            Some(AudioParam {
                format,
                sample_rate,
                channels,
                frame_duration,
            }) => {
                assert!(matches!(format, AudioFormat::Opus));
                assert_eq!(sample_rate, 16000);
                assert_eq!(channels, 1);
                assert_eq!(frame_duration, 60);
            }
            None => panic!("expected audio_params from capability look up, got None"),
        },
        other => panic!("expected HelloResult, got {other}"),
    }
    drop(input_tx);
    Ok(())
}

#[tokio::test]
#[traced_test]
async fn test_chat_flow_hello_no_tts_declares_no_audio_params() -> anyhow::Result<()> {
    /*
     * 无 TTS 模板 ⇒ capability look up 得 None ⇒ 握手 audio_params: None（不声明下行语音能力）。
     */
    let (input_tx, mut output_rx) = create_no_tts_session_channel().await;
    send_frame(&input_tx, Frame::Hello(HelloMessage::default()));
    let f = recv_frame(&mut output_rx, "hello result").await;
    match f {
        FrameResult::HelloResult(HelloMessage { audio_params, .. }) => {
            assert!(audio_params.is_none(), "no TTS template ⇒ no audio_params");
        }
        other => panic!("expected HelloResult, got {other}"),
    }
    drop(input_tx);
    Ok(())
}

#[tokio::test]
#[traced_test]
async fn test_chat_flow_listen_manual() -> anyhow::Result<()> {
    /*
     * -> = client -> server, <- = server -> client
     *
     * -> hello
     * <- hello result
     * -> listen start(barge_in=true)
     * -> voice *X
     * -> listen stop
     * <- stt result
     * <- tts result start
     * loop {
     *     <- llm result
     *     <- tts result sentence start
     *     <- audio result *X
     *     <- tts result sentence end
     * }
     * <- tts result stop
     */

    let audio = get_tts_audio("你好").await;
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
    for n in 0..audio.len() {
        send_frame(
            &input_tx,
            Frame::Voice {
                data: audio.get(n).unwrap().to_vec(),
            },
        );
    }
    send_frame(&input_tx, Frame::ListenStop);
    let f = recv_frame(&mut output_rx, "stt result").await;
    assert!(
        matches!(f, FrameResult::STTResult(..)),
        "expected STTResult, got {f}"
    );
    let f = recv_frame(&mut output_rx, "tts start").await;
    assert!(
        matches!(f, FrameResult::TTSResult(ref msg) if msg.state == Some(TtsState::Start)),
        "expected TTSResult(Start), got {f}"
    );
    recv_llm_tts_loop(&mut output_rx, "round 1").await;

    drop(input_tx);
    let _ = &state.conn.close().await?;
    tear_down(container).await;
    Ok(())
}

#[tokio::test]
#[traced_test]
async fn test_chat_flow_listen_manual_deferred_until_stop() -> anyhow::Result<()> {
    /*
     * 按键录音（manual）：按住说话 + 继续按住静默 → 即使流内预解码，也不产 STT/partial；
     * 释放（listen stop）后才成文识别（“服务器不要抢答”）。
     *
     * -> hello
     * <- hello result
     * -> listen start(barge_in=true)
     * -> voice "你好" *N
     * -> voice[0;960] *8   (≈480ms 静默，远超 200ms 静音确认阈值)
     * :: 等待 600ms：静默确认 / transport-stall 均不得在按键中抢先 finish
     * -> listen stop
     * <- stt result
     * <- tts result start
     * loop { llm/tts }
     */
    let audio = get_tts_audio("你好").await;
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
    for n in 0..audio.len() {
        send_frame(
            &input_tx,
            Frame::Voice {
                data: audio.get(n).unwrap().to_vec(),
            },
        );
    }
    for _ in 0..8 {
        send_frame(
            &input_tx,
            Frame::Voice {
                data: vec![0u8; 960],
            },
        );
    }
    assert!(
        tokio::time::timeout(Duration::from_millis(600), output_rx.recv())
            .await
            .is_err(),
        "按键录音中不得实时识别产出结果（须等 ListenStop）"
    );
    send_frame(&input_tx, Frame::ListenStop);
    let f = recv_frame(&mut output_rx, "stt result").await;
    assert!(
        matches!(f, FrameResult::STTResult(..)),
        "expected STTResult, got {f}"
    );
    let f = recv_frame(&mut output_rx, "tts start").await;
    assert!(
        matches!(f, FrameResult::TTSResult(ref msg) if msg.state == Some(TtsState::Start)),
        "expected TTSResult(Start), got {f}"
    );
    recv_llm_tts_loop(&mut output_rx, "round 1").await;

    drop(input_tx);
    let _ = &state.conn.close().await?;
    tear_down(container).await;
    Ok(())
}

#[tokio::test]
#[traced_test]
async fn test_chat_flow_listen_auto() -> anyhow::Result<()> {
    /*
     * -> = client -> server, <- = server -> client
     *
     * -> hello
     * <- hello result
     * //wake word start
     * -> voice *X
     * -> input
     * -> listen start(barge_in=false)
     * <- stt result
     * <- tts result start
     * loop {
     *     <- llm result
     *     <- tts result sentence start
     *     <- audio result *X
     *     <- tts result sentence end
     * }
     * <- tts result stop
     * //wake word end
     * -> voice *X
     * <- stt result
     * <- tts result start
     * loop {
     *     <- llm result
     *     <- tts result sentence start
     *     <- audio result *X
     *     <- tts result sentence end
     * }
     * <- tts result stop
     */
    let audio = get_tts_audio("你好").await;
    let (input_tx, mut output_rx, container, state) = create_session_channel().await;
    send_frame(&input_tx, Frame::Hello(HelloMessage::default()));
    let f = recv_frame(&mut output_rx, "hello result").await;
    assert!(
        matches!(f, FrameResult::HelloResult(..)),
        "expected HelloResult, got {f}"
    );

    // Round 1: wake word — voice + input + listen start
    for n in 0..audio.len() {
        send_frame(
            &input_tx,
            Frame::Voice {
                data: audio.get(n).unwrap().to_vec(),
            },
        );
    }
    send_frame(
        &input_tx,
        Frame::Input {
            text: "Hello".to_string(),
            mode: InputMode::Wake,
        },
    );
    send_frame(
        &input_tx,
        Frame::ListenStart {
            barge_in: false,
            is_voice_break_detect: true,
        },
    );
    let f = recv_frame(&mut output_rx, "stt result").await;
    assert!(
        matches!(f, FrameResult::STTResult(..)),
        "expected STTResult, got {f}"
    );
    let f = recv_frame(&mut output_rx, "tts start").await;
    assert!(
        matches!(f, FrameResult::TTSResult(ref msg) if msg.state == Some(TtsState::Start)),
        "expected TTSResult(Start), got {f}"
    );
    recv_llm_tts_loop(&mut output_rx, "round 1").await;

    // Round 2: auto-detect voice after TTS stop, no listen stop sent by client
    for n in 0..audio.len() {
        send_frame(
            &input_tx,
            Frame::Voice {
                data: audio.get(n).unwrap().to_vec(),
            },
        );
    }
    let f = recv_frame(&mut output_rx, "stt result round 2").await;
    assert!(
        matches!(f, FrameResult::STTResult(..)),
        "expected STTResult, got {f}"
    );
    let f = recv_frame(&mut output_rx, "tts start round 2").await;
    assert!(
        matches!(f, FrameResult::TTSResult(ref msg) if msg.state == Some(TtsState::Start)),
        "expected TTSResult(Start), got {f}"
    );
    recv_llm_tts_loop(&mut output_rx, "round 2").await;

    drop(input_tx);
    let _ = &state.conn.close().await?;
    tear_down(container).await;
    Ok(())
}

#[tokio::test]
#[traced_test]
async fn test_chat_flow_listen_realtime() -> anyhow::Result<()> {
    /*
     * -> = client -> server, <- = server -> client
     * 备注:
     * 1. voice 一直处于发送状态
     * 2. voice 可随时中断tts并开启新一轮
     *
     * -> hello
     * <- hello result
     * //wake word start
     * -> voice *X
     * -> input
     * -> listen start(barge_in=true)
     * <- stt result
     * <- tts result start
     * loop {
     *     <- llm result
     *     <- tts result sentence start
     *     <- audio result *X
     *     <- tts result sentence end
     * }
     * <- tts result stop
     * //wake word end
     * -> voice *X
     * <- stt result
     * <- tts result start
     * loop {
     *     <- llm result
     *     <- tts result sentence start
     *     <- audio result *X
     *     <- tts result sentence end
     * }
     * <- tts result stop
     */
    let audio = get_tts_audio("你好，小叽").await;
    let (input_tx, mut output_rx, container, state) = create_session_channel().await;
    send_frame(&input_tx, Frame::Hello(HelloMessage::default()));
    let f = recv_frame(&mut output_rx, "hello result").await;
    assert!(
        matches!(f, FrameResult::HelloResult(..)),
        "expected HelloResult, got {f}"
    );

    // Round 1: wake word — voice + input + listen start(barge_in=true)
    for n in 0..audio.len() {
        send_frame(
            &input_tx,
            Frame::Voice {
                data: audio.get(n).unwrap().to_vec(),
            },
        );
    }
    send_frame(
        &input_tx,
        Frame::Input {
            text: "你好，小叽".to_string(),
            mode: InputMode::Wake,
        },
    );
    send_frame(
        &input_tx,
        Frame::ListenStart {
            barge_in: true,
            is_voice_break_detect: true,
        },
    );
    let f = recv_frame(&mut output_rx, "stt result").await;
    assert!(
        matches!(f, FrameResult::STTResult(..)),
        "expected STTResult, got {f}"
    );
    let f = recv_frame(&mut output_rx, "tts start").await;
    assert!(
        matches!(f, FrameResult::TTSResult(ref msg) if msg.state == Some(TtsState::Start)),
        "expected TTSResult(Start), got {f}"
    );
    recv_llm_tts_loop(&mut output_rx, "round 1").await;

    // Round 2: voice with interrupt — during TTS loop, send voice to break
    for n in 0..audio.len() {
        send_frame(
            &input_tx,
            Frame::Voice {
                data: audio.get(n).unwrap().to_vec(),
            },
        );
    }
    let f = recv_frame(&mut output_rx, "stt result round 2").await;
    assert!(
        matches!(f, FrameResult::STTResult(..)),
        "expected STTResult, got {f}"
    );
    let f = recv_frame(&mut output_rx, "tts start round 2").await;
    assert!(
        matches!(f, FrameResult::TTSResult(ref msg) if msg.state == Some(TtsState::Start)),
        "expected TTSResult(Start), got {f}"
    );
    let mut interrupted = false;
    let mut has_interrupt_stt = false;
    loop {
        match recv_frame(&mut output_rx, "llm result or stop round 2").await {
            FrameResult::STTResult(..) => {
                has_interrupt_stt = true;
                break;
            }
            FrameResult::LLMResult(..) => {
                let f = recv_frame(&mut output_rx, "sentence start round 2").await;
                assert!(
                    matches!(f, FrameResult::TTSResult(ref msg) if msg.state == Some(TtsState::SentenceStart)),
                    "expected TTSResult(SentenceStart), got {f}"
                );
                if !interrupted {
                    for n in 0..audio.len() {
                        send_frame(
                            &input_tx,
                            Frame::Voice {
                                data: audio.get(n).unwrap().to_vec(),
                            },
                        );
                    }
                    interrupted = true;
                }
                loop {
                    match recv_frame(&mut output_rx, "audio or sentence end round 2").await {
                        FrameResult::AudioResult(_) => {}
                        FrameResult::TTSResult(msg) if msg.state == Some(TtsState::SentenceEnd) => {
                            break;
                        }
                        // Barge-in stops the round mid-sentence; a new round's
                        // STTResult follows the Stop frame.
                        FrameResult::TTSResult(msg) if msg.state == Some(TtsState::Stop) => {
                            break;
                        }
                        FrameResult::STTResult(..) => {
                            has_interrupt_stt = true;
                            break;
                        }
                        other => {
                            panic!("expected AudioResult/SentenceEnd/STTResult, got {other}")
                        }
                    }
                }
                if has_interrupt_stt {
                    break;
                }
            }
            FrameResult::TTSResult(msg) if msg.state == Some(TtsState::Stop) => break,
            other => panic!("expected LLMResult or TTSResult(Stop) or STTResult, got {other}"),
        }
    }

    // Interrupt round: expect new STTResult (interrupt triggered new round)
    if !has_interrupt_stt {
        let f = recv_frame(&mut output_rx, "stt result interrupt round").await;
        assert!(
            matches!(f, FrameResult::STTResult(..)),
            "expected STTResult, got {f}"
        );
    }
    let f = recv_frame(&mut output_rx, "tts start interrupt round").await;
    assert!(
        matches!(f, FrameResult::TTSResult(ref msg) if msg.state == Some(TtsState::Start)),
        "expected TTSResult(Start), got {f}"
    );
    recv_llm_tts_loop(&mut output_rx, "interrupt round").await;

    drop(input_tx);
    let _ = &state.conn.close().await?;
    tear_down(container).await;
    Ok(())
}

#[tokio::test]
#[traced_test]
async fn test_chat_flow_text_input() -> anyhow::Result<()> {
    /*
     * -> = client -> server, <- = server -> client
     * 备注:
     * 1. 可随时input开启新一轮
     *
     * -> hello
     * <- hello result
     * -> input
     * <- stt result
     * <- tts result start
     * loop {
     *     <- llm result
     *     <- tts result sentence start
     *     <- audio result *X
     *     <- tts result sentence end
     * }
     * <- tts result stop
     */
    let (input_tx, mut output_rx, container, state) = create_session_channel().await;
    send_frame(&input_tx, Frame::Hello(HelloMessage::default()));
    let f = recv_frame(&mut output_rx, "hello result").await;
    assert!(
        matches!(f, FrameResult::HelloResult(..)),
        "expected HelloResult, got {f}"
    );

    // Round 1: normal completion
    send_frame(
        &input_tx,
        Frame::Input {
            text: "Hello".to_string(),
            mode: InputMode::Normal,
        },
    );
    let f = recv_frame(&mut output_rx, "stt result round 1").await;
    assert!(
        matches!(f, FrameResult::STTResult(..)),
        "expected STTResult, got {f}"
    );
    let f = recv_frame(&mut output_rx, "tts start round 1").await;
    assert!(
        matches!(f, FrameResult::TTSResult(ref msg) if msg.state == Some(TtsState::Start)),
        "expected TTSResult(Start), got {f}"
    );
    recv_llm_tts_loop(&mut output_rx, "round 1").await;

    // Round 2: interrupted by a new input during TTS loop
    send_frame(
        &input_tx,
        Frame::Input {
            text: "Second".to_string(),
            mode: InputMode::Normal,
        },
    );
    let f = recv_frame(&mut output_rx, "stt result round 2").await;
    assert!(
        matches!(f, FrameResult::STTResult(..)),
        "expected STTResult, got {f}"
    );
    let f = recv_frame(&mut output_rx, "tts start round 2").await;
    assert!(
        matches!(f, FrameResult::TTSResult(ref msg) if msg.state == Some(TtsState::Start)),
        "expected TTSResult(Start), got {f}"
    );
    match recv_frame(&mut output_rx, "llm result or stop round 2").await {
        FrameResult::LLMResult(..) => {
            let f = recv_frame(&mut output_rx, "sentence start round 2").await;
            assert!(
                matches!(f, FrameResult::TTSResult(ref msg) if msg.state == Some(TtsState::SentenceStart)),
                "expected TTSResult(SentenceStart), got {f}"
            );
            // Send interrupt input to trigger new round.
            // Epoch bump discards old round output; next frame is Round 3 STTResult.
            send_frame(
                &input_tx,
                Frame::Input {
                    text: "Third".to_string(),
                    mode: InputMode::Normal,
                },
            );
        }
        FrameResult::TTSResult(msg) if msg.state == Some(TtsState::Stop) => {}
        other => panic!("expected LLMResult or TTSResult(Stop), got {other}"),
    }

    // Round 3: interrupt round — drain old round residues, expect STTResult from interrupt input
    loop {
        let f = recv_frame(&mut output_rx, "stt result round 3").await;
        if matches!(f, FrameResult::STTResult(..)) {
            break;
        }
    }
    let f = recv_frame(&mut output_rx, "tts start round 3").await;
    assert!(
        matches!(f, FrameResult::TTSResult(ref msg) if msg.state == Some(TtsState::Start)),
        "expected TTSResult(Start), got {f}"
    );
    recv_llm_tts_loop(&mut output_rx, "round 3").await;

    drop(input_tx);
    let _ = &state.conn.close().await?;
    tear_down(container).await;
    Ok(())
}

#[tokio::test]
#[traced_test]
async fn test_no_activity_timeout() -> anyhow::Result<()> {
    let audio = get_tts_audio("你好").await;
    let (input_tx, mut output_rx) = create_mini_session_with_timeout(3000).await;
    send_frame(&input_tx, Frame::Hello(HelloMessage::default()));
    let f = recv_frame(&mut output_rx, "hello result").await;
    assert!(
        matches!(f, FrameResult::HelloResult(..)),
        "expected HelloResult, got {f}"
    );

    // Round: realtime wake word trigger
    for n in 0..audio.len() {
        send_frame(
            &input_tx,
            Frame::Voice {
                data: audio.get(n).unwrap().to_vec(),
            },
        );
    }
    send_frame(
        &input_tx,
        Frame::Input {
            text: "你好".to_string(),
            mode: InputMode::Wake,
        },
    );
    send_frame(
        &input_tx,
        Frame::ListenStart {
            barge_in: true,
            is_voice_break_detect: true,
        },
    );
    let f = recv_frame(&mut output_rx, "stt result").await;
    assert!(
        matches!(f, FrameResult::STTResult(..)),
        "expected STTResult, got {f}"
    );
    let f = recv_frame(&mut output_rx, "tts start").await;
    assert!(
        matches!(f, FrameResult::TTSResult(ref msg) if msg.state == Some(TtsState::Start)),
        "expected TTSResult(Start), got {f}"
    );
    recv_llm_tts_loop(&mut output_rx, "round 1").await;

    // Simulate real device: keep sending silence voice frames while waiting for CloseResult.
    // Listener stays in Listening { is_speech: false } the entire time,
    // exercising the new idle path (not End/Idle).
    let silence = vec![0u8; 960];
    let close = tokio::select! {
        _ = async {
            loop {
                if input_tx.send(Frame::Voice { data: silence.clone() }).is_err() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
        } => {
            recv_frame(&mut output_rx, "close after send fail").await
        }
        f = recv_frame(&mut output_rx, "close") => f,
    };
    assert!(
        matches!(close, FrameResult::CloseResult),
        "expected CloseResult from no-activity timeout, got {close}"
    );
    Ok(())
}

#[tokio::test]
#[traced_test]
async fn test_chat_flow_voice_text_voice_round_trips() -> anyhow::Result<()> {
    /*
     * 回归：真机时序"语音① → 文本 → 语音②"（同一会话内交替）。
     * 曾观测：语音①正常、文本正常，但随后第二次语音的 STT 未到达前端。
     * 期望每一段输入都能回到一轮完整的 STT → LLM → TTS → Stop。
     *
     * -> hello
     * <- hello result
     * // 语音①
     * -> listen start(barge_in=true)
     * -> voice *X
     * -> listen stop
     * <- stt result
     * <- tts start
     * loop { <- llm result, <- sentence start, <- audio, <- sentence end }
     * <- tts stop
     * // 文本
     * -> input
     * <- stt result
     * <- tts start
     * loop { <- llm result, <- sentence start, <- audio, <- sentence end }
     * <- tts stop
     * // 语音②
     * -> listen start(barge_in=true)
     * -> voice *X
     * -> listen stop
     * <- stt result   <-- 断言此处
     * <- tts start
     * loop { <- llm result, <- sentence start, <- audio, <- sentence end }
     * <- tts stop
     */
    let audio = get_tts_audio("你好").await;
    let (input_tx, mut output_rx, container, state) = create_session_channel().await;
    send_frame(&input_tx, Frame::Hello(HelloMessage::default()));
    let f = recv_frame(&mut output_rx, "hello result").await;
    assert!(
        matches!(f, FrameResult::HelloResult(..)),
        "expected HelloResult, got {f}"
    );

    // 语音①
    send_frame(
        &input_tx,
        Frame::ListenStart {
            barge_in: true,
            is_voice_break_detect: false,
        },
    );
    for n in 0..audio.len() {
        send_frame(
            &input_tx,
            Frame::Voice {
                data: audio.get(n).unwrap().to_vec(),
            },
        );
    }
    send_frame(&input_tx, Frame::ListenStop);
    let f = recv_frame(&mut output_rx, "stt voice 1").await;
    assert!(
        matches!(f, FrameResult::STTResult(..)),
        "expected STTResult (voice 1), got {f}"
    );
    let f = recv_frame(&mut output_rx, "tts start voice 1").await;
    assert!(
        matches!(f, FrameResult::TTSResult(ref msg) if msg.state == Some(TtsState::Start)),
        "expected TTSResult(Start) (voice 1), got {f}"
    );
    recv_llm_tts_loop(&mut output_rx, "voice 1").await;

    // 文本
    send_frame(
        &input_tx,
        Frame::Input {
            text: "你好".to_string(),
            mode: InputMode::Normal,
        },
    );
    let f = recv_frame(&mut output_rx, "stt text").await;
    assert!(
        matches!(f, FrameResult::STTResult(..)),
        "expected STTResult (text), got {f}"
    );
    let f = recv_frame(&mut output_rx, "tts start text").await;
    assert!(
        matches!(f, FrameResult::TTSResult(ref msg) if msg.state == Some(TtsState::Start)),
        "expected TTSResult(Start) (text), got {f}"
    );
    recv_llm_tts_loop(&mut output_rx, "text").await;

    // 语音②
    send_frame(
        &input_tx,
        Frame::ListenStart {
            barge_in: true,
            is_voice_break_detect: false,
        },
    );
    for n in 0..audio.len() {
        send_frame(
            &input_tx,
            Frame::Voice {
                data: audio.get(n).unwrap().to_vec(),
            },
        );
    }
    send_frame(&input_tx, Frame::ListenStop);
    let f = recv_frame(&mut output_rx, "stt voice 2").await;
    assert!(
        matches!(f, FrameResult::STTResult(..)),
        "expected STTResult (voice 2), got {f}"
    );
    let f = recv_frame(&mut output_rx, "tts start voice 2").await;
    assert!(
        matches!(f, FrameResult::TTSResult(ref msg) if msg.state == Some(TtsState::Start)),
        "expected TTSResult(Start) (voice 2), got {f}"
    );
    recv_llm_tts_loop(&mut output_rx, "voice 2").await;

    drop(input_tx);
    let _ = &state.conn.close().await?;
    tear_down(container).await;
    Ok(())
}

/// 单轮 manual 语音（device 帧序列：listen start(manual) → voice → listen stop）。
async fn voice_round(input_tx: &mpsc::UnboundedSender<Frame>, audio: &[Vec<u8>]) {
    send_frame(
        input_tx,
        Frame::ListenStart {
            barge_in: true,
            is_voice_break_detect: false,
        },
    );
    for n in 0..audio.len() {
        send_frame(
            input_tx,
            Frame::Voice {
                data: audio.get(n).unwrap().to_vec(),
            },
        );
    }
    send_frame(input_tx, Frame::ListenStop);
}

#[tokio::test]
#[traced_test]
async fn test_chat_flow_voice_voice_manual() -> anyhow::Result<()> {
    /*
     * 覆盖：连续两轮 manual 语音（无文本插入）。
     * -> hello -> hello result
     * -> listen start(manual) -> voice -> listen stop -> stt -> tts -> stop   // 语音①
     * -> listen start(manual) -> voice -> listen stop -> stt -> tts -> stop   // 语音②
     */
    let audio = get_tts_audio("你好").await;
    let (input_tx, mut output_rx, container, state) = create_session_channel().await;
    send_frame(&input_tx, Frame::Hello(HelloMessage::default()));
    let f = recv_frame(&mut output_rx, "hello result").await;
    assert!(
        matches!(f, FrameResult::HelloResult(..)),
        "expected HelloResult, got {f}"
    );

    voice_round(&input_tx, &audio).await;
    let f = recv_frame(&mut output_rx, "stt voice 1").await;
    assert!(
        matches!(f, FrameResult::STTResult(..)),
        "expected STTResult (voice 1), got {f}"
    );
    let f = recv_frame(&mut output_rx, "tts start voice 1").await;
    assert!(
        matches!(f, FrameResult::TTSResult(ref msg) if msg.state == Some(TtsState::Start)),
        "expected TTSResult(Start) (voice 1), got {f}"
    );
    recv_llm_tts_loop(&mut output_rx, "voice 1").await;

    voice_round(&input_tx, &audio).await;
    let f = recv_frame(&mut output_rx, "stt voice 2").await;
    assert!(
        matches!(f, FrameResult::STTResult(..)),
        "expected STTResult (voice 2), got {f}"
    );
    let f = recv_frame(&mut output_rx, "tts start voice 2").await;
    assert!(
        matches!(f, FrameResult::TTSResult(ref msg) if msg.state == Some(TtsState::Start)),
        "expected TTSResult(Start) (voice 2), got {f}"
    );
    recv_llm_tts_loop(&mut output_rx, "voice 2").await;

    drop(input_tx);
    let _ = &state.conn.close().await?;
    tear_down(container).await;
    Ok(())
}

#[tokio::test]
#[traced_test]
async fn test_chat_flow_text_voice() -> anyhow::Result<()> {
    /*
     * 覆盖：文本在前、语音在后。
     * -> hello -> hello result
     * -> input -> stt -> tts -> stop                                        // 文本
     * -> listen start(manual) -> voice -> listen stop -> stt -> tts -> stop // 语音②
     */
    let audio = get_tts_audio("你好").await;
    let (input_tx, mut output_rx, container, state) = create_session_channel().await;
    send_frame(&input_tx, Frame::Hello(HelloMessage::default()));
    let f = recv_frame(&mut output_rx, "hello result").await;
    assert!(
        matches!(f, FrameResult::HelloResult(..)),
        "expected HelloResult, got {f}"
    );

    send_frame(
        &input_tx,
        Frame::Input {
            text: "你好".to_string(),
            mode: InputMode::Normal,
        },
    );
    let f = recv_frame(&mut output_rx, "stt text").await;
    assert!(
        matches!(f, FrameResult::STTResult(..)),
        "expected STTResult (text), got {f}"
    );
    let f = recv_frame(&mut output_rx, "tts start text").await;
    assert!(
        matches!(f, FrameResult::TTSResult(ref msg) if msg.state == Some(TtsState::Start)),
        "expected TTSResult(Start) (text), got {f}"
    );
    recv_llm_tts_loop(&mut output_rx, "text").await;

    voice_round(&input_tx, &audio).await;
    let f = recv_frame(&mut output_rx, "stt voice 2").await;
    assert!(
        matches!(f, FrameResult::STTResult(..)),
        "expected STTResult (voice 2), got {f}"
    );
    let f = recv_frame(&mut output_rx, "tts start voice 2").await;
    assert!(
        matches!(f, FrameResult::TTSResult(ref msg) if msg.state == Some(TtsState::Start)),
        "expected TTSResult(Start) (voice 2), got {f}"
    );
    recv_llm_tts_loop(&mut output_rx, "voice 2").await;

    drop(input_tx);
    let _ = &state.conn.close().await?;
    tear_down(container).await;
    Ok(())
}

#[tokio::test]
#[traced_test]
async fn test_chat_flow_voice_text_voice_barge_in() -> anyhow::Result<()> {
    /*
     * 覆盖：语音①→文本→语音②，其中语音②在文本 TTS 未结束（SentenceStart 后）时打断。
     * 期望 barge-in 开启新一轮，仍能收到语音②的 STT。
     * 对应真机最常见的"上一段还没播完就再说话"。
     */
    let audio = get_tts_audio("你好").await;
    let (input_tx, mut output_rx, container, state) = create_session_channel().await;
    send_frame(&input_tx, Frame::Hello(HelloMessage::default()));
    let f = recv_frame(&mut output_rx, "hello result").await;
    assert!(
        matches!(f, FrameResult::HelloResult(..)),
        "expected HelloResult, got {f}"
    );

    // 语音①
    voice_round(&input_tx, &audio).await;
    let f = recv_frame(&mut output_rx, "stt voice 1").await;
    assert!(
        matches!(f, FrameResult::STTResult(..)),
        "expected STTResult (voice 1), got {f}"
    );
    let f = recv_frame(&mut output_rx, "tts start voice 1").await;
    assert!(
        matches!(f, FrameResult::TTSResult(ref msg) if msg.state == Some(TtsState::Start)),
        "expected TTSResult(Start) (voice 1), got {f}"
    );
    recv_llm_tts_loop(&mut output_rx, "voice 1").await;

    // 文本（视作一段表达）；在其 TTS 播放中，直接开启第二段语音（barge-in）
    send_frame(
        &input_tx,
        Frame::Input {
            text: "你好".to_string(),
            mode: InputMode::Normal,
        },
    );
    let f = recv_frame(&mut output_rx, "stt text").await;
    assert!(
        matches!(f, FrameResult::STTResult(..)),
        "expected STTResult (text), got {f}"
    );
    let f = recv_frame(&mut output_rx, "tts start text").await;
    assert!(
        matches!(f, FrameResult::TTSResult(ref msg) if msg.state == Some(TtsState::Start)),
        "expected TTSResult(Start) (text), got {f}"
    );
    // 等文本第一句开始，表示 TTS 仍在播放
    match recv_frame(&mut output_rx, "text llm or sentence start").await {
        FrameResult::LLMResult(..) => {
            let f = recv_frame(&mut output_rx, "text sentence start").await;
            assert!(
                matches!(f, FrameResult::TTSResult(ref msg) if msg.state == Some(TtsState::SentenceStart)),
                "expected TTSResult(SentenceStart), got {f}"
            );
        }
        other => panic!("expected LLMResult, got {other}"),
    }

    // 在文本 TTS 播放中开启语音② → barge-in 新一轮
    // barge-in 会中断文本轮，输出队列里可能有残留(被中断轮末尾的 Audio/Stop)，
    // 这里跳过残留直到出现新一轮 STTResult（与 listen_realtime 打断模式一致）。
    voice_round(&input_tx, &audio).await;
    let f = loop {
        let f = recv_frame(&mut output_rx, "voice 2 drain to stt").await;
        if matches!(f, FrameResult::STTResult(..)) {
            break f;
        }
    };
    assert!(
        matches!(f, FrameResult::STTResult(..)),
        "expected STTResult (voice 2), got {f}"
    );
    let f = recv_frame(&mut output_rx, "tts start voice 2").await;
    assert!(
        matches!(f, FrameResult::TTSResult(ref msg) if msg.state == Some(TtsState::Start)),
        "expected TTSResult(Start) (voice 2), got {f}"
    );
    recv_llm_tts_loop(&mut output_rx, "voice 2").await;

    drop(input_tx);
    let _ = &state.conn.close().await?;
    tear_down(container).await;
    Ok(())
}

#[tokio::test]
#[traced_test]
async fn test_chat_flow_resample_48k_multi_round() -> anyhow::Result<()> {
    /*
     * 覆盖：客户端通过 hello 声明非默认上行采样率(48k)，opus 节点重采样到 16k 后，
     * 多轮（语音①→文本→语音②）都能正确出 STT。真机浏览器 AudioContext 常为 48k。
     * -> hello(audio_params 48k) -> hello result
     * -> listen start(manual) -> voice(48k opus) -> listen stop -> stt -> tts -> stop  // 语音①
     * -> input -> stt -> tts -> stop                                                  // 文本
     * -> listen start(manual) -> voice(48k opus) -> listen stop -> stt -> tts -> stop // 语音②
     */
    let audio = get_tts_audio("你好").await;
    let audio48 = resample_opus_audio(&audio, 48000);
    let (input_tx, mut output_rx, container, state) = create_session_channel().await;
    send_frame(
        &input_tx,
        Frame::Hello(HelloMessage {
            audio_params: Some(AudioParam {
                format: AudioFormat::Opus,
                sample_rate: 48000,
                channels: 1,
                frame_duration: 20,
            }),
            ..Default::default()
        }),
    );
    let f = recv_frame(&mut output_rx, "hello result").await;
    assert!(
        matches!(f, FrameResult::HelloResult(..)),
        "expected HelloResult, got {f}"
    );

    // 语音①（48k 上行）
    voice_round(&input_tx, &audio48).await;
    let f = recv_frame(&mut output_rx, "stt voice 1").await;
    assert!(
        matches!(f, FrameResult::STTResult(..)),
        "expected STTResult (voice 1), got {f}"
    );
    let f = recv_frame(&mut output_rx, "tts start voice 1").await;
    assert!(
        matches!(f, FrameResult::TTSResult(ref msg) if msg.state == Some(TtsState::Start)),
        "expected TTSResult(Start) (voice 1), got {f}"
    );
    recv_llm_tts_loop(&mut output_rx, "voice 1").await;

    // 文本
    send_frame(
        &input_tx,
        Frame::Input {
            text: "你好".to_string(),
            mode: InputMode::Normal,
        },
    );
    let f = recv_frame(&mut output_rx, "stt text").await;
    assert!(
        matches!(f, FrameResult::STTResult(..)),
        "expected STTResult (text), got {f}"
    );
    let f = recv_frame(&mut output_rx, "tts start text").await;
    assert!(
        matches!(f, FrameResult::TTSResult(ref msg) if msg.state == Some(TtsState::Start)),
        "expected TTSResult(Start) (text), got {f}"
    );
    recv_llm_tts_loop(&mut output_rx, "text").await;

    // 语音②（48k 上行）
    voice_round(&input_tx, &audio48).await;
    let f = recv_frame(&mut output_rx, "stt voice 2").await;
    assert!(
        matches!(f, FrameResult::STTResult(..)),
        "expected STTResult (voice 2), got {f}"
    );
    let f = recv_frame(&mut output_rx, "tts start voice 2").await;
    assert!(
        matches!(f, FrameResult::TTSResult(ref msg) if msg.state == Some(TtsState::Start)),
        "expected TTSResult(Start) (voice 2), got {f}"
    );
    recv_llm_tts_loop(&mut output_rx, "voice 2").await;

    drop(input_tx);
    let _ = &state.conn.close().await?;
    tear_down(container).await;
    Ok(())
}
