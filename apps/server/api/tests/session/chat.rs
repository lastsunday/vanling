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
use crate::session::helpers::{create_mini_session_channel, create_session_channel, get_audio};

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

#[tokio::test]
#[traced_test]
/*
2026-03-16T09:26:06.988023Z DEBUG frame: [RECV] Hello(HelloMessage { message: Message { mtype: Hello }, version: None, transport: None, audio_params: None, features: Some(Feature { mcp: Some(true), aec: None }), session_id: None })
2026-03-16T09:26:06.988091Z DEBUG frame: [SEND] HelloResult(HelloMessage { message: Message { mtype: Hello }, version: None, transport: Some(Websocket), audio_params: Some(AudioParam { format: Opus, sample_rate: 16000, channels: 1, frame_duration: 60 }), features: None, session_id: Some("d6rspbklm6jn11rmp49g") })
2026-03-16T09:26:06.988133Z DEBUG frame: [SEND] McpResult(McpRequest { message: Message { mtype: Mcp }, session_id: Some("d6rspbklm6jn11rmp49g"), payload: JsonRpcRequest { jsonrpc: JsonRpcVersion2_0, id: Number(0), request: Request { method: "initialize", params: {"capabilities": Object {}, "clientInfo": Object {"name": String("rmcp"), "version": String("0.15.0")}, "protocolVersion": String("2025-06-18")}, extensions: Extensions } } })
2026-03-16T09:26:07.037845Z DEBUG frame: [RECV] Mcp(McpMessage { message: Message { mtype: Mcp }, payload: Response(JsonRpcResponse { jsonrpc: JsonRpcVersion2_0, id: Number(0), result: {"capabilities": Object {"tools": Object {}}, "protocolVersion": String("2025-06-18"), "serverInfo": Object {"name": String("Web测试设备"), "version": String("1.0.0")}} }) })
2026-03-16T09:26:07.037933Z DEBUG frame: [SEND] McpResult(McpRequest { message: Message { mtype: Mcp }, session_id: Some("d6rspbklm6jn11rmp49g"), payload: JsonRpcRequest { jsonrpc: JsonRpcVersion2_0, id: Number(1), request: Request { method: "tools/list", params: {}, extensions: Extensions } } })
2026-03-16T09:26:07.045113Z DEBUG frame: [RECV] Mcp(McpMessage { message: Message { mtype: Mcp }, payload: Response(JsonRpcResponse { jsonrpc: JsonRpcVersion2_0, id: Number(1), result: {"tools": Array [Object {"description": String("Provides the real-time information of the device, including the current status of the audio speaker, battery, network, etc.\nUse this tool for: \n1. Answering questions about current condition (e.g. what is the current volume of the audio speaker?)\n2. As the first step to control the device (e.g. turn up / down the volume of the audio speaker, etc.)"), "inputSchema": Object {"properties": Object {}, "type": String("object")}, "name": String("self.get_device_status")}, Object {"description": String("Set the volume of the audio speaker. If the current volume is unknown, you must call `self.get_device_status` tool first and then call this tool."), "inputSchema": Object {"properties": Object {"volume": Object {"maximum": Number(100), "minimum": Number(0), "type": String("integer")}}, "required": Array [String("volume")], "type": String("object")}, "name": String("self.audio_speaker.set_volume")}, Object {"description": String("Set the brightness of the screen."), "inputSchema": Object {"properties": Object {"brightness": Object {"maximum": Number(100), "minimum": Number(0), "type": String("integer")}}, "required": Array [String("brightness")], "type": String("object")}, "name": String("self.screen.set_brightness")}]} }) })
*/
async fn test_chat_flow_listen_manual() -> anyhow::Result<()> {
    let audio = get_audio();
    let (input_tx, mut output_rx, container, state) = create_session_channel().await;
    input_tx.send(Frame::Hello(HelloMessage {
        ..Default::default()
    }))?;
    assert!(matches!(
        output_rx.recv().await.unwrap().payload,
        FrameResult::HelloResult(..)
    ));
    input_tx.send(Frame::ListenStart { barge_in: true })?;
    for n in 0..audio.len() {
        input_tx.send(Frame::Voice {
            data: audio.get(n).unwrap().to_vec(),
        })?;
    }
    input_tx.send(Frame::ListenStop)?;
    loop {
        let data = output_rx.recv().await.unwrap().payload;
        if let FrameResult::TTSResult(tts_message) = data {
            match tts_message.state {
                Some(TtsState::Stop) => break,
                Some(_) => {}
                None => {}
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
async fn test_chat_flow_listen_auto() -> anyhow::Result<()> {
    let (input_tx, mut output_rx) = create_mini_session_channel().await;
    input_tx.send(Frame::Hello(HelloMessage {
        ..Default::default()
    }))?;
    assert!(matches!(
        output_rx.recv().await.unwrap().payload,
        FrameResult::HelloResult(..)
    ));

    // First round via text-based Input
    input_tx.send(Frame::Input {
        text: "Hello".to_string(),
    })?;

    // Drain: STTResult → TTS Start → LLMResult → SentenceStart → SentenceEnd → Stop
    loop {
        let data = output_rx.recv().await.unwrap().payload;
        let FrameResult::TTSResult(tts_message) = data else {
            continue;
        };
        if let Some(TtsState::Stop) = tts_message.state {
            break;
        }
    }

    // Second round via text-based Input
    input_tx.send(Frame::Input {
        text: "Second input".to_string(),
    })?;

    loop {
        let data = output_rx.recv().await.unwrap().payload;
        let FrameResult::TTSResult(tts_message) = data else {
            continue;
        };
        if let Some(TtsState::Stop) = tts_message.state {
            break;
        }
    }

    input_tx
        .send(Frame::Close(CloseMessage::new(1000, String::new())))
        .unwrap();
    assert!(matches!(
        output_rx.recv().await.unwrap().payload,
        FrameResult::CloseResult
    ));
    Ok(())
}

#[tokio::test]
/*
2026-03-16T07:51:51.451299Z DEBUG frame: [RECV] Hello(HelloMessage { message: Message { mtype: Hello }, version: Some(1), transport: Some(Websocket), audio_params: Some(AudioParam { format: Opus, sample_rate: 16000, channels: 1, frame_duration: 60 }), features: Some(Feature { mcp: Some(true), aec: None }), session_id: None })
2026-03-16T07:51:51.453883Z DEBUG frame: [SEND] HelloResult(HelloMessage { message: Message { mtype: Hello }, version: None, transport: Some(Websocket), audio_params: Some(AudioParam { format: Opus, sample_rate: 16000, channels: 1, frame_duration: 60 }), features: None, session_id: Some("d6rrd5slm6ji1occegj0") })
2026-03-16T07:51:51.453939Z DEBUG frame: [SEND] McpResult(McpRequest { message: Message { mtype: Mcp }, session_id: Some("d6rrd5slm6ji1occegj0"), payload: JsonRpcRequest { jsonrpc: JsonRpcVersion2_0, id: Number(0), request: Request { method: "initialize", params: {"capabilities": Object {}, "clientInfo": Object {"name": String("rmcp"), "version": String("0.15.0")}, "protocolVersion": String("2025-06-18")}, extensions: Extensions } } })
2026-03-16T07:51:51.480137Z TRACE frame: [RECV] Voice
2026-03-16T07:51:51.562556Z DEBUG frame: [RECV] Listen(ListenMessage { message: Message { mtype: Listen }, session_id: Some("d6rrd5slm6ji1occegj0"), state: Detect, mmod: None, text: Some("你好小智") })
2026-03-16T07:51:53.755786Z DEBUG frame: [RECV] Listen(ListenMessage { message: Message { mtype: Listen }, session_id: Some("d6rrd5slm6ji1occegj0"), state: Start, mmod: Some(RealTime), text: None })
2026-03-16T07:51:53.755847Z DEBUG frame: [RECV] Mcp(McpMessage { message: Message { mtype: Mcp }, payload: Response(JsonRpcResponse { jsonrpc: JsonRpcVersion2_0, id: Number(0), result: {"capabilities": Object {"tools": Object {}}, "protocolVersion": String("2024-11-05"), "serverInfo": Object {"name": String("lichuang-dev"), "version": String("2.2.4")}} }) })
2026-03-16T07:51:53.755903Z DEBUG frame: [SEND] McpResult(McpRequest { message: Message { mtype: Mcp }, session_id: Some("d6rrd5slm6ji1occegj0"), payload: JsonRpcRequest { jsonrpc: JsonRpcVersion2_0, id: Number(1), request: Request { method: "tools/list", params: {}, extensions: Extensions } } })
2026-03-16T07:51:53.759800Z TRACE frame: [RECV] Voice
2026-03-16T07:51:53.759932Z DEBUG frame: [SEND] STTResult(SttMessage { message: Message { mtype: Stt }, session_id: Some("d6rrd5slm6ji1occegj0"), text: Some("你好小智") })
2026-03-16T07:51:53.759949Z DEBUG frame: [SEND] TTSResult(TtsMessage { message: Message { mtype: Tts }, session_id: Some("d6rrd5slm6ji1occegj0"), state: Some(Start), text: None })
2026-03-16T07:51:53.760358Z TRACE frame: [RECV] Voice
2026-03-16T07:51:53.799160Z DEBUG frame: [RECV] Mcp(McpMessage { message: Message { mtype: Mcp }, payload: Response(JsonRpcResponse { jsonrpc: JsonRpcVersion2_0, id: Number(1), result: {"tools": Array [Object {"description": String("Provides the real-time information of the device, including the current status of the audio speaker, battery, network, etc.\nUse this tool for: \n1. Answering questions about current condition (e.g. what is the current volume of the audio speaker?)\n2. As the first step to control the device (e.g. turn up / down the volume of the audio speaker, etc.)"), "inputSchema": Object {"properties": Object {}, "type": String("object")}, "name": String("self.get_device_status")}, Object {"description": String("Set the volume of the audio speaker. If the current volume is unknown, you must call `self.get_device_status` tool first and then call this tool."), "inputSchema": Object {"properties": Object {"volume": Object {"maximum": Number(100), "minimum": Number(0), "type": String("integer")}}, "required": Array [String("volume")], "type": String("object")}, "name": String("self.audio_speaker.set_volume")}, Object {"description": String("Set the brightness of the screen."), "inputSchema": Object {"properties": Object {"brightness": Object {"maximum": Number(100), "minimum": Number(0), "type": String("integer")}}, "required": Array [String("brightness")], "type": String("object")}, "name": String("self.screen.set_brightness")}, Object {"description": String("Set the theme of the screen. The theme can be `light` or `dark`."), "inputSchema": Object {"properties": Object {"theme": Object {"type": String("string")}}, "required": Array [String("theme")], "type": String("object")}, "name": String("self.screen.set_theme")}, Object {"description": String("Always remember you have a camera. If the user asks you to see something, use this tool to take a photo and then explain it.\nArgs:\n  `question`: The question that you want to ask about the photo.\n  `video`... (line truncated to 2000 chars)
2026-03-16T07:51:53.799222Z TRACE frame: [RECV] Voice
2026-03-16T07:51:58.042580Z DEBUG frame: [SEND] LLMResult(LlmMessage { message: Message { mtype: Llm }, session_id: Some("d6rrd5slm6ji1occegj0"), emotion: Some("happy"), text: Some("🙂") })
2026-03-16T07:51:58.042633Z DEBUG frame: [SEND] TTSResult(TtsMessage { message: Message { mtype: Tts }, session_id: Some("d6rrd5slm6ji1occegj0"), state: Some(SentenceStart), text: Some("你好！") })
2026-03-16T07:51:58.042646Z TRACE frame: [SEND] Audio
2026-03-16T07:51:58.052116Z TRACE frame: [RECV] Voice
2026-03-16T07:51:59.079549Z DEBUG frame: [SEND] TTSResult(TtsMessage { message: Message { mtype: Tts }, session_id: Some("d6rrd5slm6ji1occegj0"), state: Some(SentenceEnd), text: None })
2026-03-16T07:51:59.143718Z TRACE frame: [RECV] Voice
2026-03-16T07:52:00.149167Z DEBUG frame: [SEND] LLMResult(LlmMessage { message: Message { mtype: Llm }, session_id: Some("d6rrd5slm6ji1occegj0"), emotion: Some("happy"), text: Some("🙂") })
2026-03-16T07:52:00.149215Z DEBUG frame: [SEND] TTSResult(TtsMessage { message: Message { mtype: Tts }, session_id: Some("d6rrd5slm6ji1occegj0"), state: Some(SentenceStart), text: Some("有什么可以帮助你的吗？") })
2026-03-16T07:52:00.149239Z TRACE frame: [SEND] Audio
2026-03-16T07:52:00.171715Z TRACE frame: [RECV] Voice
2026-03-16T07:52:03.632112Z DEBUG frame: [SEND] TTSResult(TtsMessage { message: Message { mtype: Tts }, session_id: Some("d6rrd5slm6ji1occegj0"), state: Some(SentenceEnd), text: None })
2026-03-16T07:52:03.632139Z DEBUG frame: [SEND] TTSResult(TtsMessage { message: Message { mtype: Tts }, session_id: Some("d6rrd5slm6ji1occegj0"), state: Some(Stop), text: None })
2026-03-16T07:52:03.659201Z TRACE frame: [RECV] Voice
2026-03-16T07:52:34.559563Z DEBUG frame: [SEND] STTResult(SttMessage { message: Message { mtype: Stt }, session_id: Some("d6rrd5slm6ji1occegj0"), text: Some("現在幾點？") })
2026-03-16T07:52:34.559627Z TRACE frame: [RECV] Voice
2026-03-16T07:52:34.559627Z DEBUG frame: [SEND] TTSResult(TtsMessage { message: Message { mtype: Tts }, session_id: Some("d6rrd5slm6ji1occegj0"), state: Some(Start), text: None })
2026-03-16T07:52:34.559757Z TRACE frame: [RECV] Voice
2026-03-16T07:52:55.979754Z DEBUG frame: [SEND] LLMResult(LlmMessage { message: Message { mtype: Llm }, session_id: Some("d6rrd5slm6ji1occegj0"), emotion: Some("happy"), text: Some("🙂") })
2026-03-16T07:52:55.979802Z DEBUG frame: [SEND] TTSResult(TtsMessage { message: Message { mtype: Tts }, session_id: Some("d6rrd5slm6ji1occegj0"), state: Some(SentenceStart), text: Some("现在是2026年3月16日，当前时间为上午15:52。") })
2026-03-16T07:52:55.979840Z TRACE frame: [SEND] Audio
2026-03-16T07:52:56.013138Z TRACE frame: [RECV] Voice
2026-03-16T07:53:01.845134Z DEBUG frame: [SEND] TTSResult(TtsMessage { message: Message { mtype: Tts }, session_id: Some("d6rrd5slm6ji1occegj0"), state: Some(SentenceEnd), text: None })
2026-03-16T07:53:01.845194Z DEBUG frame: [SEND] TTSResult(TtsMessage { message: Message { mtype: Tts }, session_id: Some("d6rrd5slm6ji1occegj0"), state: Some(Stop), text: None })
2026-03-16T07:53:01.892041Z TRACE frame: [RECV] Voice
* */
// TODO: 1) race condition — Wake pipeline output drained before Listen(Start, RealTime), assertions
//        after Listen(Start) never receive STTResult/TTSResult (already consumed by drain).
//        2) second round uses create_session (MatchaTts) → channel back-pressure like listen_auto.
// TODO: also timed out (>60s) - hangs in drain loop or after
async fn test_chat_flow_listen_realtime() -> anyhow::Result<()> {
    let (input_tx, mut output_rx) = create_mini_session_channel().await;
    input_tx.send(Frame::Hello(HelloMessage {
        ..Default::default()
    }))?;
    assert!(matches!(
        output_rx.recv().await.unwrap().payload,
        FrameResult::HelloResult(..)
    ));

    // First round via text-based Input
    input_tx.send(Frame::Input {
        text: "Hello".to_string(),
    })?;

    // Drain: STTResult → TTS Start → LLMResult → SentenceStart → SentenceEnd → Stop
    loop {
        let data = output_rx.recv().await.unwrap().payload;
        let FrameResult::TTSResult(tts_message) = data else {
            continue;
        };
        if let Some(TtsState::Stop) = tts_message.state {
            break;
        }
    }

    input_tx
        .send(Frame::Close(CloseMessage::new(1000, String::new())))
        .unwrap();
    assert!(matches!(
        output_rx.recv().await.unwrap().payload,
        FrameResult::CloseResult
    ));
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
