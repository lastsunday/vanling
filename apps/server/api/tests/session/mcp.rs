use rmcp::model::{
    CallToolResult, Content, Icon, Implementation, InitializeResult, ListToolsResult,
    ProtocolVersion, RawTextContent, ServerCapabilities,
};
use service::chobits::message::{
    audio::AudioMessage,
    hello::{Feature, HelloMessage},
    mcp::McpMessage,
    tts::{TtsMessage, TtsState},
};
use service::ws::frame::{Frame, FrameResult};
use std::sync::atomic::{AtomicI64, Ordering};
use tracing::debug;
use tracing_test::traced_test;

use crate::common::tear_down;
use crate::session::helpers::{create_session_channel, to_json_rpc_response};

#[tokio::test]
/// 1. [Device -> Server] hello request
/// 2. [Server -> Device] hello response
/// 3.1.1. [Server -> Device] mcp initialize request
/// 3.1.2. [Device -> Server] mcp initialize response
/// 3.1.3. [Server -> Device] mcp tools list request
/// 3.1.4. [Device -> Server] mcp tools list response
/// 3.2.1. [Device -> Server] voice request
/// 3.2.2. [Device -> Server] detect wake request
/// 4.1.1. [Device -> Server] listen start reqeust
/// 4.1.2. [Device -> Server] voice request (loop forever)
/// 4.2.0.1. [Server] vad
/// 4.2.0.2. [Server] asr
/// 4.2.0.3. [Server] llm (user input replace by wake word)
/// 4.2.1. [Server -> Device] llm text response (for detect wake word)
/// 4.2.2. [Server -> Device] tts response (for detect wake wake word)
/// 5.1.0.1. [Server] vad
/// 5.1.0.2. [Server] asr
/// 5.1.0.3. [Server] llm
/// 5.1.0.4. [Server -> Device] mcp call tool(for device call)
/// 5.1.0.5. [Device -> Server] mcp call response
/// 5.1.0.6. [Server] llm
/// 5.1.1. [Server -> Device] llm text response
/// 5.1.2. [Server -> Device] tts response
async fn test_mcp_flow_server_client() -> anyhow::Result<()> {
    let _ = tracing::subscriber::set_global_default(
        tracing_subscriber::fmt::Subscriber::builder()
            .compact()
            .with_max_level(tracing::Level::TRACE)
            .finish(),
    );
    let device_mcp_tools_list_response: &'static str = r#"
[
  {
    "name": "self.get_device_status",
    "description": "Provides the real-time information of the device, including the current status of the audio speaker, screen, battery, network, etc.\nUse this tool for: \n1. Answering questions about current condition (e.g. what is the current volume of the audio speaker?)\n2. As the first step to control the device (e.g. turn up / down the volume of the audio speaker, etc.)",
    "inputSchema": {
      "properties": {},
      "type": "object"
    }
  },
  {
    "name": "self.audio_speaker.set_volume",
    "description": "Set the volume of the audio speaker. If the current volume is unknown, you must call `self.get_device_status` tool first and then call this tool.",
    "inputSchema": {
      "properties": {
        "volume": {
          "maximum": 100,
          "minimum": 0,
          "type": "integer"
        }
      },
      "required": ["volume"],
      "type": "object"
    }
  },
  {
    "name": "self.screen.set_brightness",
    "description": "Set the brightness of the screen.",
    "inputSchema": {
      "properties": {
        "brightness": {
          "maximum": 100,
          "minimum": 0,
          "type": "integer"
        }
      },
      "required": ["brightness"],
      "type": "object"
    }
  },
  {
    "name": "self.screen.set_theme",
    "description": "Set the theme of the screen. The theme can be `light` or `dark`.",
    "inputSchema": {
      "properties": {
        "theme": {
          "type": "string"
        }
      },
      "required": ["theme"],
      "type": "object"
    }
  },
  {
    "name": "self.camera.take_photo",
    "description": "Take a photo and explain it. Use this tool after the user asks you to see something.\nArgs:\n  `question`: The question that you want to ask about the photo.\nReturn:\n  A JSON object that provides the photo information.",
    "inputSchema": {
      "properties": {
        "question": {
          "type": "string"
        }
      },
      "required": ["question"],
      "type": "object"
    }
  }
]"#;

    let request_id = AtomicI64::new(0);
    let (input_tx, mut output_rx, container, state) = create_session_channel().await;
    input_tx.send(Frame::Hello(HelloMessage {
        features: Some(Feature {
            mcp: Some(true),
            aec: None,
        }),
        ..Default::default()
    }))?;
    assert!(matches!(
        output_rx.recv().await.unwrap().payload,
        FrameResult::HelloResult(..)
    ));
    let frame_result = output_rx.recv().await.unwrap().payload;
    assert!(matches!(frame_result, FrameResult::McpResult(..)));
    if let FrameResult::McpResult(request) = frame_result {
        assert_eq!(request.payload.request.method, "initialize");
    }

    input_tx.send(Frame::Mcp(McpMessage::new(to_json_rpc_response(
        request_id.fetch_add(1, Ordering::Relaxed),
        InitializeResult {
            protocol_version: ProtocolVersion::default(),
            capabilities: ServerCapabilities::default(),
            server_info: Implementation {
                name: "icon-server".to_string(),
                title: None,
                version: "2.0.0".to_string(),
                icons: Some(vec![Icon {
                    src: "https://example.com/server.png".to_string(),
                    mime_type: Some("image/png".to_string()),
                    sizes: None,
                }]),
                website_url: Some("https://docs.example.com".to_string()),
                description: None,
            },
            instructions: None,
        },
    ))))?;

    let frame_result = output_rx.recv().await.unwrap().payload;
    debug!("{:?}", &frame_result);
    if let FrameResult::McpResult(request) = frame_result {
        assert_eq!(request.payload.request.method, "tools/list");
    }

    input_tx.send(Frame::Mcp(McpMessage::new(to_json_rpc_response(
        request_id.fetch_add(1, Ordering::Relaxed),
        ListToolsResult {
            next_cursor: None,
            tools: serde_json::from_str(device_mcp_tools_list_response).unwrap(),
            meta: None,
        },
    ))))?;

    input_tx.send(Frame::Input {
        text: "现在几点".to_string(),
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
    assert!(matches!(
        output_rx.recv().await.unwrap().payload,
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
    drop(input_tx);
    let _ = &state.conn.close().await?;
    tear_down(container).await;
    Ok(())
}

// TODO: blocked — requires a tool-calling LLM (Qwen3). Echo LLM never generates
// tool calls, so the server never issues McpResult::CallTool (step 5.1.0.4).
#[ignore]
#[tokio::test]
#[traced_test]
async fn test_mcp_flow_device_client() -> anyhow::Result<()> {
    let device_mcp_tools_list_response: &'static str = r#"
[
  {
    "name": "self.get_device_status",
    "description": "Provides the real-time information of the device, including the current status of the audio speaker, screen, battery, network, etc.\nUse this tool for: \n1. Answering questions about current condition (e.g. what is the current volume of the audio speaker?)\n2. As the first step to control the device (e.g. turn up / down the volume of the audio speaker, etc.)",
    "inputSchema": {
      "properties": {},
      "type": "object"
    }
  },
  {
    "name": "self.audio_speaker.set_volume",
    "description": "Set the volume of the audio speaker. If the current volume is unknown, you must call `self.get_device_status` tool first and then call this tool.",
    "inputSchema": {
      "properties": {
        "volume": {
          "maximum": 100,
          "minimum": 0,
          "type": "integer"
        }
      },
      "required": ["volume"],
      "type": "object"
    }
  },
  {
    "name": "self.screen.set_brightness",
    "description": "Set the brightness of the screen.",
    "inputSchema": {
      "properties": {
        "brightness": {
          "maximum": 100,
          "minimum": 0,
          "type": "integer"
        }
      },
      "required": ["brightness"],
      "type": "object"
    }
  },
  {
    "name": "self.screen.set_theme",
    "description": "Set the theme of the screen. The theme can be `light` or `dark`.",
    "inputSchema": {
      "properties": {
        "theme": {
          "type": "string"
        }
      },
      "required": ["theme"],
      "type": "object"
    }
  },
  {
    "name": "self.camera.take_photo",
    "description": "Take a photo and explain it. Use this tool after the user asks you to see something.\nArgs:\n  `question`: The question that you want to ask about the photo.\nReturn:\n  A JSON object that provides the photo information.",
    "inputSchema": {
      "properties": {
        "question": {
          "type": "string"
        }
      },
      "required": ["question"],
      "type": "object"
    }
  }
]"#;

    let request_id = AtomicI64::new(0);
    let (input_tx, mut output_rx, container, state) = create_session_channel().await;
    input_tx.send(Frame::Hello(HelloMessage {
        features: Some(Feature {
            mcp: Some(true),
            aec: None,
        }),
        ..Default::default()
    }))?;
    assert!(matches!(
        output_rx.recv().await.unwrap().payload,
        FrameResult::HelloResult(..)
    ));
    let frame_result = output_rx.recv().await.unwrap().payload;
    assert!(matches!(frame_result, FrameResult::McpResult(..)));
    if let FrameResult::McpResult(request) = frame_result {
        assert_eq!(request.payload.request.method, "initialize");
    }

    input_tx.send(Frame::Mcp(McpMessage::new(to_json_rpc_response(
        request_id.fetch_add(1, Ordering::Relaxed),
        InitializeResult {
            protocol_version: ProtocolVersion::default(),
            capabilities: ServerCapabilities::default(),
            server_info: Implementation {
                name: "icon-server".to_string(),
                title: None,
                version: "2.0.0".to_string(),
                icons: Some(vec![Icon {
                    src: "https://example.com/server.png".to_string(),
                    mime_type: Some("image/png".to_string()),
                    sizes: None,
                }]),
                website_url: Some("https://docs.example.com".to_string()),
                description: None,
            },
            instructions: None,
        },
    ))))?;

    let frame_result = output_rx.recv().await.unwrap().payload;
    debug!("{:?}", &frame_result);
    if let FrameResult::McpResult(request) = frame_result {
        assert_eq!(request.payload.request.method, "tools/list");
    }

    input_tx.send(Frame::Mcp(McpMessage::new(to_json_rpc_response(
        request_id.fetch_add(1, Ordering::Relaxed),
        ListToolsResult {
            next_cursor: None,
            tools: serde_json::from_str(device_mcp_tools_list_response).unwrap(),
            meta: None,
        },
    ))))?;

    input_tx.send(Frame::Input {
        text: "get device status".to_string(),
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
    assert!(matches!(frame_result, FrameResult::McpResult(..)));

    let mcp_tool_call_response = r#"
    {
    "audio_speaker": {
        "volume": 50,
        "muted": false
    },
    "screen": {
        "brightness": 80,
        "theme": "light"
    },
    "battery": {
        "level": 85,
        "charging": false
    },
    "network": {
        "connected": true,
        "type": "wifi"
    }
    }"#;
    input_tx.send(Frame::Mcp(McpMessage::new(to_json_rpc_response(
        request_id.fetch_add(1, Ordering::Relaxed),
        CallToolResult {
            content: vec![Content {
                raw: rmcp::model::RawContent::Text(RawTextContent {
                    text: mcp_tool_call_response.to_string(),
                    meta: None,
                }),
                annotations: None,
            }],
            structured_content: None,
            is_error: None,
            meta: None,
        },
    ))))?;

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
    assert!(matches!(
        output_rx.recv().await.unwrap().payload,
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
    drop(input_tx);
    let _ = &state.conn.close().await?;
    tear_down(container).await;
    Ok(())
}
