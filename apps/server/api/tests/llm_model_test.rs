use std::sync::Arc;

use api::{
    common::ModelError,
    config::{LlmModel, llm::LlmConfig},
    {chii::Splitter, llm::LlmFactory},
};
use framework::id::gen_id;
use rig::{
    OneOrMany,
    completion::{CompletionError, CompletionRequest, ToolDefinition},
    message::{
        AssistantContent, Message, Reasoning, Text, ToolCall, ToolResult, ToolResultContent,
        UserContent,
    },
    streaming::{StreamedAssistantContent, StreamingCompletionResponse},
};
use tokio::sync::mpsc::Sender;
use tokio_stream::StreamExt;
use tracing::info;
use tracing_test::traced_test;

use api::setup_mcp;
use rmcp::{
    ServiceExt as _rmcp_ServiceExt,
    model::{
        CallToolRequestParams, ClientCapabilities, ClientInfo, Implementation,
        PaginatedRequestParams, Tool,
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
        model: Some(LlmModel::Qwen3),
        path: Some(model_path),
        variant: None,
    }
}

#[tokio::test]
#[traced_test]
async fn test_llm_model_echo() -> anyhow::Result<()> {
    let model = LlmFactory::create_model(&LlmConfig {
        model: Some(LlmModel::Echo),
        ..Default::default()
    });
    let chat_history = OneOrMany::<Message>::one(Message::User {
        content: OneOrMany::<UserContent>::one(UserContent::Text(Text {
            text: String::from("Hello"),
        })),
    });
    let request = CompletionRequest {
        preamble: None,
        chat_history: chat_history.clone(),
        documents: vec![],
        tools: vec![],
        temperature: None,
        max_tokens: None,
        tool_choice: None,
        additional_params: None,
    };
    let mut response = model.stream(request).await?;
    let response = response.next().await;
    let response = response.expect("has value")?;
    match response {
        StreamedAssistantContent::Text(text) => {
            assert_eq!("Hello", text.text);
        }
        _ => {
            panic!("assistant content not text")
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
    let router = OpenApiRouter::new();
    let ct = tokio_util::sync::CancellationToken::new();
    let router = setup_mcp(router, state.clone(), ct.child_token())
        .split_for_parts()
        .0;
    let config = StreamableHttpClientTransportConfig {
        uri: "/mcp".into(),
        ..Default::default()
    };
    let client = RouterClient { router };
    let transport = StreamableHttpClientTransport::with_client(client, config);
    let client_info = ClientInfo {
        meta: None,
        protocol_version: Default::default(),
        capabilities: ClientCapabilities::default(),
        client_info: Implementation {
            name: "test sse client".to_string(),
            title: None,
            version: "0.0.1".to_string(),
            website_url: None,
            icons: None,
            description: None,
        },
    };
    let client = client_info.serve(transport).await.inspect_err(|e| {
        tracing::error!("client error: {:?}", e);
    })?;
    // Initialize
    let server_info = client.peer_info();
    tracing::info!("Connected to server: {server_info:#?}");

    let mut tools = vec![];
    let mut cursor = None;
    loop {
        // List tools
        let tools_result = client
            .list_tools(Some(PaginatedRequestParams { meta: None, cursor }))
            .await?;
        for tool in tools_result.tools {
            tools.push(ToolDefinition {
                name: tool.name.to_string(),
                description: tool.description.unwrap_or_default().to_string(),
                parameters: serde_json::to_value(tool.input_schema)?,
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
        tools.push(ToolDefinition {
            name: tool.name.to_string(),
            description: tool.description.unwrap_or_default().to_string(),
            parameters: serde_json::to_value(tool.input_schema)?,
        });
    }
    tracing::info!("{:?}", tools);
    let config = create_llm_config();
    LlmFactory::init(Arc::new(config.clone())).await;
    let model = LlmFactory::create_model(&config);

    let mut has_next_step = true;

    let system_prompt = "".to_string();
    let mut chat_history = OneOrMany::<Message>::one(Message::User {
        content: OneOrMany::<UserContent>::one(UserContent::Text(Text {
            text: text.to_string(),
        })),
    });

    while has_next_step {
        let request = CompletionRequest {
            preamble: Some(system_prompt.clone()),
            chat_history: chat_history.clone(),
            documents: vec![],
            tools: tools.clone(),
            temperature: Some(0.8),
            max_tokens: Some(999),
            tool_choice: None,
            additional_params: None,
        };
        let response = model.stream(request).await;

        let messages = handle_response(response, None).await?;
        has_next_step = false;
        for message in &messages {
            chat_history.push(message.clone());
            match message {
                Message::User { .. } => {
                    //skip
                }
                Message::Assistant { id: _id, content } => {
                    for item in content.iter() {
                        match item {
                            AssistantContent::ToolCall(ToolCall {
                                id,
                                call_id,
                                function,
                                signature: _signature,
                                additional_params: _additional_params,
                            }) => {
                                let function_json_text = serde_json::to_string(&function)?;
                                let param: CallToolRequestParams =
                                    serde_json::from_str(function_json_text.as_str())?;
                                let result = client.call_tool(param).await?;
                                let content = &result.content;
                                let content = {
                                    match &content.len() {
                                        0 => {
                                            panic!("call tool result must be not empty")
                                        }
                                        1 => {
                                            let item = content.first().unwrap();
                                            match &item.raw {
                                                rmcp::model::RawContent::Text(raw_text_content) => {
                                                    OneOrMany::<UserContent>::one(
                                                        UserContent::ToolResult(ToolResult {
                                                            id: id.clone(),
                                                            call_id: call_id.clone(),
                                                            content: OneOrMany::one(
                                                                ToolResultContent::Text(Text {
                                                                    text: raw_text_content
                                                                        .text
                                                                        .clone(),
                                                                }),
                                                            ),
                                                        }),
                                                    )
                                                }
                                                rmcp::model::RawContent::Image(..) => {
                                                    // TODO:
                                                    panic!(
                                                        "tool call image result not supported yet"
                                                    )
                                                }
                                                rmcp::model::RawContent::Resource(..) => {
                                                    // TODO:
                                                    panic!(
                                                        "tool call resource result not supported yet"
                                                    )
                                                }
                                                rmcp::model::RawContent::Audio(..) => {
                                                    // TODO:
                                                    panic!(
                                                        "tool call audio result not supported yet"
                                                    )
                                                }
                                                rmcp::model::RawContent::ResourceLink(..) => {
                                                    // TODO:
                                                    panic!(
                                                        "tool call resource link result not supported yet"
                                                    )
                                                }
                                            }
                                        }
                                        _ => {
                                            let items: Vec<UserContent> =
                                        content.iter().map(|item| {
                                                match &item.raw {
                                                    rmcp::model::RawContent::Text(raw_text_content) => {
                                                        UserContent::ToolResult(
                                                            ToolResult {
                                                                id: id.clone(),
                                                                call_id:call_id.clone(),
                                                                content: OneOrMany::one(
                                                                    ToolResultContent::Text(Text {
                                                                        text: raw_text_content.text.clone(),
                                                                    }),
                                                                ),
                                                            },
                                                        )
                                                    }
                                                    rmcp::model::RawContent::Image(..) => {
                                                        // TODO:
                                                        panic!("tool call image result not supported yet")
                                                    }
                                                    rmcp::model::RawContent::Resource(
                                                        ..
                                                    ) => {
                                                        // TODO:
                                                        panic!("tool call resource result not supported yet")
                                                    }
                                                    rmcp::model::RawContent::Audio(..) => {
                                                        // TODO:
                                                        panic!("tool call audio result not supported yet")
                                                    }
                                                    rmcp::model::RawContent::ResourceLink(..) => {
                                                        // TODO:
                                                        panic!(
                                                            "tool call resource link result not supported yet"
                                                        )
                                                    }
                                                }
                                            })
                                            .collect();
                                            OneOrMany::<UserContent>::many(items).unwrap()
                                        }
                                    }
                                };
                                chat_history.push(Message::User { content });
                                has_next_step = true;
                            }
                            _ => {
                                //skip
                            }
                        }
                    }
                }
            }
        }

        info!("{:?}", chat_history);
    }
    let _ = &state.conn.close().await.unwrap();
    tear_down(container).await;
    Ok(())
}

async fn handle_response(
    response: Result<
        StreamingCompletionResponse<rig::providers::openai::streaming::StreamingCompletionResponse>,
        CompletionError,
    >,
    tx: Option<Sender<Result<String, ModelError>>>,
) -> anyhow::Result<Vec<Message>> {
    let mut messages: Vec<Message> = vec![];
    let mut text_collector = String::new();
    let mut splitter = Splitter::new();
    match response {
        Ok(mut stream) => {
            // TODO:
            while let Some(value) = stream.next().await {
                match value {
                    Ok(StreamedAssistantContent::Text(text)) => {
                        info!("{:?}", text);
                        text_collector.push_str(&text.text);
                        if let Some(tx) = &tx {
                            let sentence_list = splitter.accept_text(&text.text);
                            let sentence_iter = sentence_list.iter();
                            for sentence in sentence_iter {
                                tx.send(Ok(sentence.to_string())).await?;
                            }
                        }
                    }
                    Ok(StreamedAssistantContent::Final(
                        rig::providers::openai::StreamingCompletionResponse { usage },
                    )) => {
                        info!("{:?}", usage);
                    }
                    Ok(StreamedAssistantContent::ToolCall {
                        tool_call,
                        internal_call_id: _internal_call_id,
                    }) => {
                        info!("{:?}", tool_call.function);
                        messages.push(Message::Assistant {
                            id: Some(tool_call.id.clone()),
                            content: OneOrMany::<AssistantContent>::one(
                                AssistantContent::ToolCall(ToolCall {
                                    id: tool_call.id.clone(),
                                    call_id: tool_call.call_id.clone(),
                                    function: tool_call.function,
                                    signature: None,
                                    additional_params: None,
                                }),
                            ),
                        });
                    }
                    Ok(StreamedAssistantContent::ToolCallDelta { .. }) => {
                        // TODO:
                    }
                    Ok(StreamedAssistantContent::Reasoning(Reasoning {
                        id: _id,
                        reasoning,
                        ..
                    })) => {
                        info!("reasoning -> {:?}", reasoning);
                    }
                    Ok(StreamedAssistantContent::ReasoningDelta { id: _id, reasoning }) => {
                        info!("reasoning -> {:?}", reasoning);
                    }
                    Err(e) => {
                        panic!("has completion error: {:?}", e);
                    }
                }
            }
            if let Some(tx) = &tx {
                let sentence_list = splitter.accept_final();
                let sentence_iter = sentence_list.iter();
                for sentence in sentence_iter {
                    tx.send(Ok(sentence.to_string())).await?;
                }
            }
            if !text_collector.is_empty() {
                messages.push(Message::Assistant {
                    id: Some(gen_id()),
                    content: OneOrMany::<AssistantContent>::one(AssistantContent::Text(Text {
                        text: text_collector,
                    })),
                });
            }
            Ok(messages)
        }
        Err(_e) => {
            panic!("has completion error");
        }
    }
}
