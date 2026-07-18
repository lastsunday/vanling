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

/*
* -> = client -> server, <- = server -> client
*
* -> hello
* <- hello result
* */
#[tokio::test]
#[traced_test]
async fn test_chat_flow_hello() -> anyhow::Result<()> {
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
* */
#[tokio::test]
#[traced_test]
async fn test_chat_flow_listen_manual() -> anyhow::Result<()> {
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
* */
#[tokio::test]
#[traced_test]
async fn test_chat_flow_listen_auto() -> anyhow::Result<()> {
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
* */
#[tokio::test]
async fn test_chat_flow_listen_realtime() -> anyhow::Result<()> {
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
async fn test_chat_flow_listen_realtime_silent_voice_connection_timeout() -> anyhow::Result<()> {
    let (input_tx, mut output_rx) = create_mini_session_channel().await;
    input_tx.send(Frame::Hello(HelloMessage {
        ..Default::default()
    }))?;
    assert!(matches!(
        output_rx.recv().await.unwrap().payload,
        FrameResult::HelloResult(..)
    ));
    input_tx.send(Frame::Input {
        text: "Hello".to_string(),
    })?;
    // drain Wake pipeline before Listen(Start, RealTime) to avoid epoch bump
    // discarding the Wake pipeline's STTResult
    while let Some(data) = output_rx.recv().await {
        let data = data.payload;
        match data {
            FrameResult::TTSResult(tts_message) => {
                if let Some(TtsState::Stop) = tts_message.state {
                    break;
                }
            }
            _ => continue,
        }
    }
    input_tx.send(Frame::ListenStart { barge_in: true })?;

    // Send Close to trigger session shutdown
    input_tx
        .send(Frame::Close(CloseMessage::new(1000, String::new())))
        .unwrap();
    loop {
        let data = output_rx.recv().await.unwrap().payload;
        if let FrameResult::CloseResult = data {
            break;
        }
    }

    Ok(())
}

#[tokio::test]
#[traced_test]
async fn test_chat_flow_handle_text_message_multiple_time() -> anyhow::Result<()> {
    let (input_tx, mut output_rx, container, state) = create_session_channel().await;
    input_tx.send(Frame::Hello(HelloMessage {
        ..Default::default()
    }))?;
    assert!(matches!(
        output_rx.recv().await.unwrap().payload,
        FrameResult::HelloResult(..)
    ));
    // let mut user_answer = vec![String::from("世界上第高的山是什么，只回答结果不用详细介绍")];
    let mut user_answer = vec![String::from("世界上第高的山是什么")];
    for index in 2..20 {
        let text = format!("第{}高的呢?", index).to_owned();
        user_answer.push(text);
    }
    for index in 0..user_answer.len() {
        input_tx.send(Frame::Input {
            text: user_answer.get(index).unwrap().to_string(),
        })?;
        let frame_result = output_rx.recv().await.unwrap().payload;
        debug!("{:?}", &frame_result);
        assert!(matches!(frame_result, FrameResult::STTResult(..)));

        assert!(matches!(
            output_rx.recv().await.unwrap().payload,
            FrameResult::TTSResult(TtsMessage {
                state: Some(TtsState::Start),
                ..
            })
        ));

        let frame_result = output_rx.recv().await.unwrap().payload;
        debug!("{:?}", &frame_result);
        assert!(matches!(frame_result, FrameResult::LLMResult(..)));

        let frame_result = output_rx.recv().await.unwrap().payload;
        debug!("{:?}", frame_result);
        assert!(matches!(
            frame_result,
            FrameResult::TTSResult(TtsMessage {
                state: Some(TtsState::SentenceStart),
                ..
            })
        ));
        // has some audio result,detect first one
        let frame_result = output_rx.recv().await.unwrap().payload;
        debug!("{:?}", frame_result);
        assert!(matches!(
            frame_result,
            FrameResult::AudioResult(AudioMessage { .. })
        ));

        while let Some(data) = output_rx.recv().await {
            if let FrameResult::TTSResult(tts_message) = data.payload {
                let state = tts_message.state;
                if let Some(state) = state
                    && TtsState::Stop == state
                {
                    break;
                }
            }
        }
    }
    drop(input_tx);
    let _ = &state.conn.close().await?;
    tear_down(container).await;
    Ok(())
}

#[tokio::test]
#[traced_test]
async fn test_chat_flow_handle_text_message() -> anyhow::Result<()> {
    let (input_tx, mut output_rx, container, state) = create_session_channel().await;
    // TODO: need refactor,remove tokio::spawn
    let join_handle = tokio::spawn(async move {
        while let Some(data) = output_rx.recv().await {
            debug!(
                "session id = {}, data = {:?}",
                data.session_id, data.payload
            );
            match data.payload {
                FrameResult::HelloResult(_hello_message) => {}
                FrameResult::STTResult(_stt_message) => {}
                FrameResult::LLMResult(_llm_message) => {}
                FrameResult::TTSResult(tts_message) => {
                    let state = tts_message.state;
                    if let Some(state) = state
                        && TtsState::Stop == state
                    {
                        return;
                    }
                }
                FrameResult::AudioResult(_audio_message) => {}
                FrameResult::Error(_) => {
                    break;
                }
                _ => {
                    panic!("unexpected frame result");
                }
            }
        }
        panic!("receive hello message error");
    });
    input_tx.send(Frame::Hello(HelloMessage {
        ..Default::default()
    }))?;
    input_tx.send(Frame::Input {
        text: "Hello".to_string(),
    })?;
    join_handle.await?;
    drop(input_tx);
    let _ = &state.conn.close().await?;
    tear_down(container).await;
    Ok(())
}

#[tokio::test]
#[traced_test]
async fn test_chat_flow_break() -> anyhow::Result<()> {
    let (input_tx, mut output_rx) = create_mini_session_channel().await;
    let mut count = 0;
    // Expect 1 TTS Stop (the second/interrupting round completes;
    // the first round's output is filtered by epoch bump from interrupt_output)
    let join_handle = tokio::spawn(async move {
        while let Some(data) = output_rx.recv().await {
            debug!(
                "session id = {}, data = {:?}",
                data.session_id, data.payload
            );
            match data.payload {
                FrameResult::HelloResult(_hello_message) => {}
                FrameResult::STTResult(_stt_message) => {}
                FrameResult::LLMResult(_llm_message) => {}
                FrameResult::TTSResult(tts_message) => {
                    let state = tts_message.state;
                    if let Some(state) = state
                        && TtsState::Stop == state
                    {
                        count += 1;
                        if count >= 1 {
                            return;
                        }
                    }
                }
                FrameResult::AudioResult(_audio_message) => {}
                FrameResult::Error(_) => {
                    break;
                }
                _ => {
                    panic!("unexpected frame result");
                }
            }
        }
        panic!("receive hello message error");
    });
    input_tx.send(Frame::Hello(HelloMessage {
        ..Default::default()
    }))?;
    input_tx.send(Frame::Input {
        text: "Hello".to_string(),
    })?;
    input_tx.send(Frame::Input {
        text: "Hello".to_string(),
    })?;
    join_handle.await?;
    drop(input_tx);
    Ok(())
}
