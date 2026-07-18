use service::chobits::frame::{Frame, FrameResult};
use service::chobits::message::{
    audio::AudioMessage,
    close::CloseMessage,
    hello::HelloMessage,
    tts::{TtsMessage, TtsState},
};
use tracing::debug;
use tracing_test::traced_test;

use crate::common::tear_down;
use crate::session::helpers::{
    create_mini_session_channel, create_session_channel, get_audio, recv_frame,
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
    input_tx.send(Frame::Hello(HelloMessage {
        ..Default::default()
    }))?;
    assert!(matches!(
        output_rx.recv().await.unwrap().payload,
        FrameResult::HelloResult(..)
    ));
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

    let audio = get_audio();
    let (input_tx, mut output_rx, container, state) = create_session_channel().await;
    // -> hello
    input_tx.send(Frame::Hello(HelloMessage {
        ..Default::default()
    }))?;
    // <- hello result
    assert!(matches!(
        recv_frame(&mut output_rx, "hello result").await,
        FrameResult::HelloResult(..)
    ));
    // -> listen start
    input_tx.send(Frame::ListenStart { barge_in: true })?;
    // -> voice *X
    for n in 0..audio.len() {
        input_tx.send(Frame::Voice {
            data: audio.get(n).unwrap().to_vec(),
        })?;
    }
    // -> listen stop
    input_tx.send(Frame::ListenStop)?;
    // <- stt result
    assert!(matches!(
        recv_frame(&mut output_rx, "stt result").await,
        FrameResult::STTResult(..)
    ));
    // <- tts result start
    assert!(matches!(
        recv_frame(&mut output_rx, "tts start").await,
        FrameResult::TTSResult(TtsMessage {
            state: Some(TtsState::Start),
            ..
        })
    ));
    // <- tts result stop
    // loop {
    //     <- llm result
    //     <- tts result sentence start
    //     <- audio result *X
    //     <- tts result sentence end
    // }
    loop {
        match recv_frame(&mut output_rx, "llm result or stop").await {
            FrameResult::LLMResult(..) => {
                // <- tts result sentence start
                assert!(matches!(
                    recv_frame(&mut output_rx, "sentence start").await,
                    FrameResult::TTSResult(TtsMessage {
                        state: Some(TtsState::SentenceStart),
                        ..
                    })
                ));
                // <- audio result *X + <- tts result sentence end
                loop {
                    match recv_frame(&mut output_rx, "audio or sentence end").await {
                        FrameResult::AudioResult(_) => {}
                        FrameResult::TTSResult(TtsMessage {
                            state: Some(TtsState::SentenceEnd),
                            ..
                        }) => break,
                        other => panic!("expected AudioResult or SentenceEnd, got {:?}", other),
                    }
                }
            }
            FrameResult::TTSResult(TtsMessage {
                state: Some(TtsState::Stop),
                ..
            }) => break,
            other => panic!("expected LLMResult or TTSResult(Stop), got {:?}", other),
        }
    }
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
    let audio = get_audio();
    let (input_tx, mut output_rx, container, state) = create_session_channel().await;
    // -> hello
    input_tx.send(Frame::Hello(HelloMessage {
        ..Default::default()
    }))?;
    // <- hello result
    assert!(matches!(
        recv_frame(&mut output_rx, "hello result").await,
        FrameResult::HelloResult(..)
    ));

    // Round 1: wake word — voice + input + listen start
    // -> voice *X
    for n in 0..audio.len() {
        input_tx.send(Frame::Voice {
            data: audio.get(n).unwrap().to_vec(),
        })?;
    }
    // -> input
    input_tx.send(Frame::Input {
        text: "Hello".to_string(),
    })?;
    // -> listen start(barge_in=false)
    input_tx.send(Frame::ListenStart { barge_in: false })?;
    // <- stt result
    assert!(matches!(
        recv_frame(&mut output_rx, "stt result").await,
        FrameResult::STTResult(..)
    ));
    // <- tts result start
    assert!(matches!(
        recv_frame(&mut output_rx, "tts start").await,
        FrameResult::TTSResult(TtsMessage {
            state: Some(TtsState::Start),
            ..
        })
    ));
    // <- llm + tts sentence loop until stop
    loop {
        match recv_frame(&mut output_rx, "llm result or stop").await {
            FrameResult::LLMResult(..) => {
                // <- tts result sentence start
                assert!(matches!(
                    recv_frame(&mut output_rx, "sentence start").await,
                    FrameResult::TTSResult(TtsMessage {
                        state: Some(TtsState::SentenceStart),
                        ..
                    })
                ));
                // <- audio result *X + <- tts result sentence end
                loop {
                    match recv_frame(&mut output_rx, "audio or sentence end").await {
                        FrameResult::AudioResult(_) => {}
                        FrameResult::TTSResult(TtsMessage {
                            state: Some(TtsState::SentenceEnd),
                            ..
                        }) => break,
                        other => panic!("expected AudioResult or SentenceEnd, got {:?}", other),
                    }
                }
            }
            FrameResult::TTSResult(TtsMessage {
                state: Some(TtsState::Stop),
                ..
            }) => break,
            other => panic!("expected LLMResult or TTSResult(Stop), got {:?}", other),
        }
    }

    // Round 2: auto-detect voice after TTS stop, no listen stop sent by client
    // -> voice *X
    for n in 0..audio.len() {
        input_tx.send(Frame::Voice {
            data: audio.get(n).unwrap().to_vec(),
        })?;
    }
    // <- stt result
    assert!(matches!(
        recv_frame(&mut output_rx, "stt result round 2").await,
        FrameResult::STTResult(..)
    ));
    // <- tts result start
    assert!(matches!(
        recv_frame(&mut output_rx, "tts start round 2").await,
        FrameResult::TTSResult(TtsMessage {
            state: Some(TtsState::Start),
            ..
        })
    ));
    loop {
        match recv_frame(&mut output_rx, "llm result or stop round 2").await {
            FrameResult::LLMResult(..) => {
                assert!(matches!(
                    recv_frame(&mut output_rx, "sentence start round 2").await,
                    FrameResult::TTSResult(TtsMessage {
                        state: Some(TtsState::SentenceStart),
                        ..
                    })
                ));
                loop {
                    match recv_frame(&mut output_rx, "audio or sentence end round 2").await {
                        FrameResult::AudioResult(_) => {}
                        FrameResult::TTSResult(TtsMessage {
                            state: Some(TtsState::SentenceEnd),
                            ..
                        }) => break,
                        other => panic!("expected AudioResult or SentenceEnd, got {:?}", other),
                    }
                }
            }
            FrameResult::TTSResult(TtsMessage {
                state: Some(TtsState::Stop),
                ..
            }) => break,
            other => panic!("expected LLMResult or TTSResult(Stop), got {:?}", other),
        }
    }

    drop(input_tx);
    let _ = &state.conn.close().await?;
    tear_down(container).await;
    Ok(())
}

#[tokio::test]
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
    let audio = get_audio();
    let (input_tx, mut output_rx, container, state) = create_session_channel().await;
    // -> hello
    input_tx.send(Frame::Hello(HelloMessage {
        ..Default::default()
    }))?;
    // <- hello result
    assert!(matches!(
        recv_frame(&mut output_rx, "hello result").await,
        FrameResult::HelloResult(..)
    ));

    // Round 1: wake word — voice + input + listen start(barge_in=true)
    // -> voice *X
    for n in 0..audio.len() {
        input_tx.send(Frame::Voice {
            data: audio.get(n).unwrap().to_vec(),
        })?;
    }
    // -> input
    input_tx.send(Frame::Input {
        text: "Hello".to_string(),
    })?;
    // -> listen start(barge_in=true)
    input_tx.send(Frame::ListenStart { barge_in: true })?;
    // <- stt result
    assert!(matches!(
        recv_frame(&mut output_rx, "stt result").await,
        FrameResult::STTResult(..)
    ));
    // <- tts result start
    assert!(matches!(
        recv_frame(&mut output_rx, "tts start").await,
        FrameResult::TTSResult(TtsMessage {
            state: Some(TtsState::Start),
            ..
        })
    ));
    // <- llm + tts sentence loop until stop
    loop {
        match recv_frame(&mut output_rx, "llm result or stop").await {
            FrameResult::LLMResult(..) => {
                // <- tts result sentence start
                assert!(matches!(
                    recv_frame(&mut output_rx, "sentence start").await,
                    FrameResult::TTSResult(TtsMessage {
                        state: Some(TtsState::SentenceStart),
                        ..
                    })
                ));
                // <- audio result *X + <- tts result sentence end
                loop {
                    match recv_frame(&mut output_rx, "audio or sentence end").await {
                        FrameResult::AudioResult(_) => {}
                        FrameResult::TTSResult(TtsMessage {
                            state: Some(TtsState::SentenceEnd),
                            ..
                        }) => break,
                        other => panic!("expected AudioResult or SentenceEnd, got {:?}", other),
                    }
                }
            }
            FrameResult::TTSResult(TtsMessage {
                state: Some(TtsState::Stop),
                ..
            }) => break,
            other => panic!("expected LLMResult or TTSResult(Stop), got {:?}", other),
        }
    }

    // Round 2: voice with interrupt — during TTS loop, send voice to break
    // -> voice *X
    for n in 0..audio.len() {
        input_tx.send(Frame::Voice {
            data: audio.get(n).unwrap().to_vec(),
        })?;
    }
    // <- stt result
    assert!(matches!(
        recv_frame(&mut output_rx, "stt result round 2").await,
        FrameResult::STTResult(..)
    ));
    // <- tts result start
    assert!(matches!(
        recv_frame(&mut output_rx, "tts start round 2").await,
        FrameResult::TTSResult(TtsMessage {
            state: Some(TtsState::Start),
            ..
        })
    ));
    // <- llm + tts sentence loop, send interrupt voice after first sentence start
    let mut interrupted = false;
    loop {
        match recv_frame(&mut output_rx, "llm result or stop round 2").await {
            FrameResult::LLMResult(..) => {
                assert!(matches!(
                    recv_frame(&mut output_rx, "sentence start round 2").await,
                    FrameResult::TTSResult(TtsMessage {
                        state: Some(TtsState::SentenceStart),
                        ..
                    })
                ));
                // After first sentence start, send interrupt voice
                if !interrupted {
                    for n in 0..audio.len() {
                        input_tx.send(Frame::Voice {
                            data: audio.get(n).unwrap().to_vec(),
                        })?;
                    }
                    interrupted = true;
                }
                // <- audio result *X + <- tts result sentence end
                loop {
                    match recv_frame(&mut output_rx, "audio or sentence end round 2").await {
                        FrameResult::AudioResult(_) => {}
                        FrameResult::TTSResult(TtsMessage {
                            state: Some(TtsState::SentenceEnd),
                            ..
                        }) => break,
                        other => panic!("expected AudioResult or SentenceEnd, got {:?}", other),
                    }
                }
                // After interrupt sent, break outer loop to assert new round
                if interrupted {
                    break;
                }
            }
            FrameResult::TTSResult(TtsMessage {
                state: Some(TtsState::Stop),
                ..
            }) => break,
            other => panic!("expected LLMResult or TTSResult(Stop), got {:?}", other),
        }
    }

    // Interrupt round: expect new STTResult (interrupt triggered new round)
    // <- stt result
    assert!(matches!(
        recv_frame(&mut output_rx, "stt result interrupt round").await,
        FrameResult::STTResult(..)
    ));
    // <- tts result start
    assert!(matches!(
        recv_frame(&mut output_rx, "tts start interrupt round").await,
        FrameResult::TTSResult(TtsMessage {
            state: Some(TtsState::Start),
            ..
        })
    ));
    // <- llm + tts sentence loop until stop
    loop {
        match recv_frame(&mut output_rx, "llm result or stop interrupt round").await {
            FrameResult::LLMResult(..) => {
                assert!(matches!(
                    recv_frame(&mut output_rx, "sentence start interrupt round").await,
                    FrameResult::TTSResult(TtsMessage {
                        state: Some(TtsState::SentenceStart),
                        ..
                    })
                ));
                loop {
                    match recv_frame(&mut output_rx, "audio or sentence end interrupt round").await
                    {
                        FrameResult::AudioResult(_) => {}
                        FrameResult::TTSResult(TtsMessage {
                            state: Some(TtsState::SentenceEnd),
                            ..
                        }) => break,
                        other => {
                            panic!("expected AudioResult or SentenceEnd, got {:?}", other)
                        }
                    }
                }
            }
            FrameResult::TTSResult(TtsMessage {
                state: Some(TtsState::Stop),
                ..
            }) => break,
            other => panic!("expected LLMResult or TTSResult(Stop), got {:?}", other),
        }
    }

    drop(input_tx);
    let _ = &state.conn.close().await?;
    tear_down(container).await;
    Ok(())
}

#[tokio::test]
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
    // -> hello
    input_tx.send(Frame::Hello(HelloMessage {
        ..Default::default()
    }))?;
    // <- hello result
    assert!(matches!(
        recv_frame(&mut output_rx, "hello result").await,
        FrameResult::HelloResult(..)
    ));

    // Round 1: normal completion
    // -> input
    input_tx.send(Frame::Input {
        text: "Hello".to_string(),
    })?;
    // <- stt result
    assert!(matches!(
        recv_frame(&mut output_rx, "stt result round 1").await,
        FrameResult::STTResult(..)
    ));
    // <- tts result start
    assert!(matches!(
        recv_frame(&mut output_rx, "tts start round 1").await,
        FrameResult::TTSResult(TtsMessage {
            state: Some(TtsState::Start),
            ..
        })
    ));
    // <- llm + tts sentence loop until stop
    loop {
        match recv_frame(&mut output_rx, "llm result or stop round 1").await {
            FrameResult::LLMResult(..) => {
                assert!(matches!(
                    recv_frame(&mut output_rx, "sentence start round 1").await,
                    FrameResult::TTSResult(TtsMessage {
                        state: Some(TtsState::SentenceStart),
                        ..
                    })
                ));
                loop {
                    match recv_frame(&mut output_rx, "audio or sentence end round 1").await {
                        FrameResult::AudioResult(_) => {}
                        FrameResult::TTSResult(TtsMessage {
                            state: Some(TtsState::SentenceEnd),
                            ..
                        }) => break,
                        other => panic!("expected AudioResult or SentenceEnd, got {:?}", other),
                    }
                }
            }
            FrameResult::TTSResult(TtsMessage {
                state: Some(TtsState::Stop),
                ..
            }) => break,
            other => panic!("expected LLMResult or TTSResult(Stop), got {:?}", other),
        }
    }

    // Round 2: interrupted by a new input during TTS loop
    // -> input
    input_tx.send(Frame::Input {
        text: "Second".to_string(),
    })?;
    // <- stt result
    assert!(matches!(
        recv_frame(&mut output_rx, "stt result round 2").await,
        FrameResult::STTResult(..)
    ));
    // <- tts result start
    assert!(matches!(
        recv_frame(&mut output_rx, "tts start round 2").await,
        FrameResult::TTSResult(TtsMessage {
            state: Some(TtsState::Start),
            ..
        })
    ));
    // <- llm + tts sentence loop, send interrupt input after first sentence start
    loop {
        match recv_frame(&mut output_rx, "llm result or stop round 2").await {
            FrameResult::LLMResult(..) => {
                assert!(matches!(
                    recv_frame(&mut output_rx, "sentence start round 2").await,
                    FrameResult::TTSResult(TtsMessage {
                        state: Some(TtsState::SentenceStart),
                        ..
                    })
                ));
                // Send interrupt input to trigger new round.
                // Epoch bump discards old round output; next frame is Round 3 STTResult.
                input_tx.send(Frame::Input {
                    text: "Third".to_string(),
                })?;
                break;
            }
            FrameResult::TTSResult(TtsMessage {
                state: Some(TtsState::Stop),
                ..
            }) => break,
            other => panic!("expected LLMResult or TTSResult(Stop), got {:?}", other),
        }
    }

    // Round 3: interrupt round — drain old round residues, expect STTResult from interrupt input
    // <- stt result
    loop {
        if matches!(
            recv_frame(&mut output_rx, "stt result round 3").await,
            FrameResult::STTResult(..)
        ) {
            break;
        }
    }
    // <- tts result start
    assert!(matches!(
        recv_frame(&mut output_rx, "tts start round 3").await,
        FrameResult::TTSResult(TtsMessage {
            state: Some(TtsState::Start),
            ..
        })
    ));
    // <- llm + tts sentence loop until stop
    loop {
        match recv_frame(&mut output_rx, "llm result or stop round 3").await {
            FrameResult::LLMResult(..) => {
                assert!(matches!(
                    recv_frame(&mut output_rx, "sentence start round 3").await,
                    FrameResult::TTSResult(TtsMessage {
                        state: Some(TtsState::SentenceStart),
                        ..
                    })
                ));
                loop {
                    match recv_frame(&mut output_rx, "audio or sentence end round 3").await {
                        FrameResult::AudioResult(_) => {}
                        FrameResult::TTSResult(TtsMessage {
                            state: Some(TtsState::SentenceEnd),
                            ..
                        }) => break,
                        other => panic!("expected AudioResult or SentenceEnd, got {:?}", other),
                    }
                }
            }
            FrameResult::TTSResult(TtsMessage {
                state: Some(TtsState::Stop),
                ..
            }) => break,
            other => panic!("expected LLMResult or TTSResult(Stop), got {:?}", other),
        }
    }

    drop(input_tx);
    let _ = &state.conn.close().await?;
    tear_down(container).await;
    Ok(())
}
