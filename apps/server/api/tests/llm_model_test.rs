use std::pin::Pin;
use std::sync::Arc;

use api::{
    common::ModelError,
    component::ling::Splitter,
    component::llm::LlmManager,
    config::{LlmProvider, llm::LlmConfig},
};
use framework::{
    auth::{Jwt, Principal},
    config::auth::AuthConfig,
    error::AppError,
};
use futures::{Stream, StreamExt};
use service::component::llm::{
    CompletionEvent, CompletionRequest, ContentPart, InputState, Message, Role, ToolDef,
};
use service::types::EmptyKind;
use tokio::sync::mpsc::Sender;
use tokio_util::sync::CancellationToken;
use tracing::info;
use tracing_test::traced_test;

use api::setup_mcp;
use rmcp::{
    ServiceExt as _rmcp_ServiceExt,
    model::{
        CallToolRequestParams, ClientCapabilities, ContentBlock, Implementation,
        InitializeRequestParams, PaginatedRequestParams, Tool,
    },
    transport::{
        StreamableHttpClientTransport, streamable_http_client::StreamableHttpClientTransportConfig,
    },
};
use utoipa_axum::router::OpenApiRouter;

mod common;
use common::{setup_database, tear_down};

use crate::common::router_client::RouterClient;

fn create_llm_config() -> LlmConfig {
    let model_path = crate::common::tts::ws_root()
        .join("data/llm/model/qwen3/0.6b/")
        .to_string_lossy()
        .into_owned();
    LlmConfig {
        provider: Some(LlmProvider::LocalQwen3),
        path: Some(model_path),
        ..Default::default()
    }
}

#[tokio::test]
#[traced_test]
async fn test_llm_model_echo() -> anyhow::Result<()> {
    let model = LlmManager::create_model(&LlmConfig {
        provider: Some(LlmProvider::LocalEcho),
        ..Default::default()
    });
    let request = CompletionRequest {
        preamble: None,
        messages: vec![Message {
            role: Role::User,
            parts: vec![ContentPart::Text("Hello".to_string())],
        }],
        tools: vec![],
        temperature: None,
        max_tokens: None,
    };
    let cancel = CancellationToken::new();
    let mut stream = model.stream(request, InputState::Normal, cancel).await;
    let event = stream.next().await;
    let event = event.expect("has value")?;
    let msg = match event {
        CompletionEvent::Text(text) => text,
        _ => panic!("expected Text event, got {:?}", event),
    };
    assert_eq!("Hello", msg);
    Ok(())
}

#[tokio::test]
#[traced_test]
async fn test_llm_model_echo_empty_input_returns_prompt() -> anyhow::Result<()> {
    let model = LlmManager::create_model(&LlmConfig {
        provider: Some(LlmProvider::LocalEcho),
        ..Default::default()
    });
    let request = CompletionRequest {
        preamble: None,
        messages: vec![Message {
            role: Role::User,
            parts: vec![ContentPart::Text("should not be echoed".to_string())],
        }],
        tools: vec![],
        temperature: None,
        max_tokens: None,
    };
    let cancel = CancellationToken::new();
    let mut stream = model
        .stream(
            request.clone(),
            InputState::Empty {
                kind: EmptyKind::Manual,
                count: 1,
            },
            cancel,
        )
        .await;
    let mut texts = String::new();
    while let Some(event) = stream.next().await {
        if let CompletionEvent::Text(text) = event? {
            texts.push_str(&text);
        }
    }
    assert_eq!("请再说一遍，我没有听清。", texts);
    Ok(())
}

/// 契约：Echo 按语境 + 次数返回分级固定句（Rule of three）。
#[tokio::test]
#[traced_test]
async fn test_llm_model_echo_empty_input_grades_by_kind_and_count() -> anyhow::Result<()> {
    let model = LlmManager::create_model(&LlmConfig {
        provider: Some(LlmProvider::LocalEcho),
        ..Default::default()
    });
    let request = CompletionRequest {
        preamble: None,
        messages: vec![Message {
            role: Role::User,
            parts: vec![ContentPart::Text("x".to_string())],
        }],
        tools: vec![],
        temperature: None,
        max_tokens: None,
    };

    let cases: Vec<(EmptyKind, u32, Option<&str>)> = vec![
        (EmptyKind::Wake, 1, Some("想让我帮你做什么呢？")),
        (
            EmptyKind::Wake,
            2,
            Some("你可以告诉我你的需求，比如播放音乐或设置提醒。"),
        ),
        (
            EmptyKind::AutoSpoke,
            1,
            Some("抱歉，我没听清，可以再说一次吗？"),
        ),
        (
            EmptyKind::AutoSpoke,
            2,
            Some("没能听清，请换个说法或说得慢一些。"),
        ),
        (EmptyKind::Silence, 1, Some("我一直在听，你可以尽管说。")),
        (EmptyKind::Silence, 2, Some("请开口告诉我你想做什么。")),
        (EmptyKind::Manual, 1, Some("请再说一遍，我没有听清。")),
        // 连续监听：静默等待，不产出提示文字。
        (EmptyKind::Continuing, 1, None),
    ];

    for (kind, count, expected) in cases {
        let mut stream = model
            .stream(
                request.clone(),
                InputState::Empty { kind, count },
                CancellationToken::new(),
            )
            .await;
        let mut texts = String::new();
        while let Some(event) = stream.next().await {
            if let CompletionEvent::Text(text) = event? {
                texts.push_str(&text);
            }
        }
        if let Some(exp) = expected {
            assert_eq!(texts, exp, "kind={kind:?} count={count} 应返回「{exp}」");
        } else {
            assert!(
                texts.is_empty(),
                "kind={kind:?} count={count} 连续监听应静默，got「{texts}」"
            );
        }
    }
    Ok(())
}

#[tokio::test]
#[traced_test]
#[ignore]
async fn test_chat_server_mcp() -> anyhow::Result<()> {
    test_chat_mcp(r#"Calculate the sum of 24.5 and 17.3 using the calculator service"#).await
}

async fn test_chat_mcp(text: &str) -> anyhow::Result<()> {
    let (container, state) = setup_database().await;
    Jwt::init(Arc::new(AuthConfig {
        access_token_secret: Some(String::from("test-secret")),
        access_token_expires_in: Some(28800),
        refresh_token_secret: Some(String::from("test-refresh-secret")),
        refresh_token_expires_in: Some(15897600),
        audience: Some(String::from("test-aud")),
        issuer: Some(String::from("test-iss")),
        ..Default::default()
    }));
    let admin_token = Jwt::global()
        .access_token_encode(&Principal {
            id: String::from("test-admin"),
            name: Some(String::from("root")),
            token_type: String::from("user"),
        })
        .expect("encode admin token");
    let router = OpenApiRouter::new();
    let ct = tokio_util::sync::CancellationToken::new();
    let router = setup_mcp(router, state.clone(), ct.child_token())
        .split_for_parts()
        .0;
    let mut config = StreamableHttpClientTransportConfig::with_uri("/mcp");
    config.auth_header = Some(admin_token);
    let client = RouterClient { router };
    let transport = StreamableHttpClientTransport::with_client(client, config);
    let client_info = InitializeRequestParams::new(
        ClientCapabilities::default(),
        Implementation::new("test sse client", "0.0.1"),
    );
    let client = client_info.serve(transport).await.inspect_err(|e| {
        tracing::error!("client error: {:?}", e);
    })?;
    let server_info = client.peer_info();
    tracing::info!("Connected to server: {server_info:#?}");

    let mut tools = vec![];
    let mut cursor = None;
    loop {
        let tools_result = client
            .list_tools(Some(PaginatedRequestParams::default().with_cursor(cursor)))
            .await?;
        for tool in tools_result.tools {
            tools.push(ToolDef {
                name: tool.name.to_string(),
                description: tool.description.unwrap_or_default().to_string(),
                input_schema: serde_json::to_value(tool.input_schema)?,
            });
        }
        if let Some(next_cursor) = tools_result.next_cursor {
            cursor = Some(next_cursor);
        } else {
            break;
        }
    }

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

    let device_list_tool: Vec<Tool> = serde_json::from_str(device_mcp_tools_list_response).unwrap();
    for tool in device_list_tool {
        tools.push(ToolDef {
            name: tool.name.to_string(),
            description: tool.description.unwrap_or_default().to_string(),
            input_schema: serde_json::to_value(tool.input_schema)?,
        });
    }
    tracing::info!("{:?}", tools);
    let config = create_llm_config();
    LlmManager::init(Arc::new(config.clone())).await;
    let model = LlmManager::create_model(&config);

    let mut has_next_step = true;
    let system_prompt = "".to_string();
    let mut messages = vec![Message {
        role: Role::User,
        parts: vec![ContentPart::Text(text.to_string())],
    }];

    let cancel = tokio_util::sync::CancellationToken::new();

    while has_next_step {
        let request = CompletionRequest {
            preamble: Some(system_prompt.clone()),
            messages: messages.clone(),
            tools: tools.clone(),
            temperature: Some(0.8),
            max_tokens: Some(999),
        };
        let stream = model
            .stream(request, InputState::Normal, cancel.child_token())
            .await;

        let new_messages = handle_response(stream, None).await?;
        has_next_step = false;
        for msg in &new_messages {
            messages.push(msg.clone());
            if matches!(msg.role, Role::Assistant) {
                for part in &msg.parts {
                    if let ContentPart::ToolCall {
                        id,
                        name,
                        arguments,
                    } = part
                    {
                        let params = match arguments.as_object().cloned() {
                            Some(args) => {
                                CallToolRequestParams::new(name.clone()).with_arguments(args)
                            }
                            None => CallToolRequestParams::new(name.clone()),
                        };
                        let result = client.call_tool(params).await?;
                        let content = &result.content;
                        let output = match content.len() {
                            0 => panic!("call tool result must be not empty"),
                            1 => {
                                let item = content.first().unwrap();
                                match item {
                                    ContentBlock::Text(t) => t.text.clone(),
                                    _ => panic!("unsupported tool result content type"),
                                }
                            }
                            _ => {
                                let texts: Vec<String> = content
                                    .iter()
                                    .map(|item| match item {
                                        ContentBlock::Text(t) => t.text.clone(),
                                        _ => panic!("unsupported tool result content type"),
                                    })
                                    .collect();
                                texts.join("\n")
                            }
                        };
                        messages.push(Message {
                            role: Role::User,
                            parts: vec![ContentPart::ToolResult {
                                id: id.clone(),
                                output,
                            }],
                        });
                        has_next_step = true;
                    }
                }
            }
        }
        info!("{:?}", messages);
    }
    let _ = &state.conn.close().await.unwrap();
    tear_down(container).await;
    Ok(())
}

async fn handle_response(
    mut stream: Pin<Box<dyn Stream<Item = Result<CompletionEvent, AppError>> + Send>>,
    tx: Option<Sender<Result<String, ModelError>>>,
) -> anyhow::Result<Vec<Message>> {
    let mut messages: Vec<Message> = vec![];
    let mut text_collector = String::new();
    let mut splitter = Splitter::new();
    while let Some(value) = stream.next().await {
        match value {
            Ok(CompletionEvent::Text(text)) => {
                info!("{:?}", text);
                text_collector.push_str(&text);
                if let Some(tx) = &tx {
                    let sentence_list = splitter.accept_token(&text);
                    for sentence in sentence_list {
                        tx.send(Ok(sentence.text)).await?;
                    }
                }
            }
            Ok(CompletionEvent::Final { .. }) => {
                info!("usage");
            }
            Ok(CompletionEvent::ToolCall {
                id,
                name,
                arguments,
            }) => {
                info!("tool call: {}", name);
                messages.push(Message {
                    role: Role::Assistant,
                    parts: vec![ContentPart::ToolCall {
                        id,
                        name,
                        arguments,
                    }],
                });
            }
            Ok(CompletionEvent::Reasoning(r)) => {
                info!("reasoning -> {:?}", r);
            }
            Ok(CompletionEvent::Error(e)) => {
                panic!("completion error: {:?}", e);
            }
            Err(e) => {
                panic!("has completion error: {:?}", e);
            }
        }
    }
    if let Some(tx) = &tx {
        let sentence_list = splitter.accept_final();
        for sentence in sentence_list {
            tx.send(Ok(sentence.text)).await?;
        }
    }
    if !text_collector.is_empty() {
        messages.push(Message {
            role: Role::Assistant,
            parts: vec![ContentPart::Text(text_collector)],
        });
    }
    Ok(messages)
}
