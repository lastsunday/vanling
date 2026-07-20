use std::time::Duration;

use service::chobits::frame::{Frame, FrameResult, InputMode};
use service::chobits::message::{hello::HelloMessage, tts::TtsState};
use tracing_test::traced_test;

use crate::common::tear_down;
use crate::session::helpers::{
    create_mini_session_channel, create_mini_session_with_timeout, create_session_channel,
    get_tts_audio, recv_frame, recv_llm_tts_loop, send_frame,
};

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
    assert!(
        matches!(f, FrameResult::HelloResult(..)),
        "expected HelloResult, got {f}"
    );
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
