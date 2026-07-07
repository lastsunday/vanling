use api::{
    config::{LlmModel, llm::LlmConfig},
    llm::{
        LlmEngine, LlmFactory,
        client::{self, ChatRequest, ClientBuilder, History},
    },
    mcp::{client::server::ServerMcpClient, provider::McpProviderImpl},
    setup_mcp,
};
use rig::{
    OneOrMany,
    message::{AssistantContent, Message, Text, UserContent},
};
use rmcp::transport::{
    StreamableHttpClientTransport, streamable_http_client::StreamableHttpClientTransportConfig,
};
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio_stream::StreamExt;
use tokio_util::sync::CancellationToken;
use tracing::{error, info};
use tracing_test::traced_test;
use utoipa_axum::router::OpenApiRouter;
mod common;
use common::{setup_database, tear_down};

use crate::common::router_client::RouterClient;

fn create_model() -> Box<dyn LlmEngine> {
    let model_path = common::tts::ws_root()
        .join("data/llm/model/qwen3/0.6b/")
        .to_string_lossy()
        .into_owned();
    LlmFactory::create_model(&LlmConfig {
        model: Some(LlmModel::Qwen3),
        path: Some(model_path),
        variant: None,
    })
}

#[tokio::test]
#[traced_test]
async fn test_chat_echo() {
    let client = ClientBuilder::new()
        .with_model(Arc::new(LlmFactory::create_model(&LlmConfig {
            model: Some(LlmModel::Echo),
            path: None,
            variant: None,
        })))
        .build()
        .with_history(Arc::new(Mutex::new(History {
            preamble: None,
            chat_history: vec![],
        })));
    let mut output = client.chat(
        ChatRequest {
            message: Message::User {
                content: OneOrMany::<UserContent>::one(UserContent::Text(Text {
                    text: r#"Hello"#.to_string(),
                })),
            },
        },
        CancellationToken::new(),
    );
    let mut result = Vec::new();
    while let Some(text) = output.next().await {
        match text {
            Ok(text) => {
                result.push(text);
            }
            Err(e) => {
                error!("{}", e.to_string());
            }
        }
    }
    let result: String = result.into_iter().collect();
    assert_eq!(r#"Hello"#, result);
}

#[tokio::test]
#[traced_test]
#[ignore]
async fn test_chat_simple() {
    let model = create_model();
    let system_prompt = "你是一个助手，所有回答必须使用纯文本自然语言，禁止使用任何Markdown符号如#、-、*等并且数字使用中文字代替。".to_string();
    let hisotry = Arc::new(Mutex::new(History {
        preamble: Some(system_prompt),
        chat_history: vec![],
    }));
    let client = ClientBuilder::new()
        .with_model(Arc::new(model))
        .build()
        .with_history(hisotry);
    let request = ChatRequest {
        message: Message::User {
            content: OneOrMany::<UserContent>::one(UserContent::Text(Text {
                text: r#"静夜思的内容"#.to_string(),
            })),
        },
    };
    let mut output = client.chat(request, CancellationToken::new());
    let mut result = Vec::new();
    while let Some(text) = output.next().await {
        match text {
            Ok(text) => {
                result.push(text);
            }
            Err(e) => {
                error!("{}", e.to_string());
            }
        }
    }
    let result: String = result.into_iter().collect();
    info!("{}", result);
    assert_ne!(0, result.len());
}

#[tokio::test]
#[traced_test]
#[ignore]
async fn test_short_question() {
    let model = create_model();
    let system_prompt = "你是一个助手。".to_string();
    let history = Arc::new(Mutex::new(History {
        preamble: Some(system_prompt),
        chat_history: vec![],
    }));
    let client = ClientBuilder::new()
        .with_model(Arc::new(model))
        .build()
        .with_history(history);
    let request = ChatRequest {
        message: Message::User {
            content: OneOrMany::<UserContent>::one(UserContent::Text(Text {
                text: r#"1+1="#.to_string(),
            })),
        },
    };
    let mut output = client.chat(request, CancellationToken::new());
    let mut result = Vec::new();
    while let Some(text) = output.next().await {
        match text {
            Ok(text) => {
                result.push(text);
            }
            Err(e) => {
                error!("{}", e.to_string());
            }
        }
    }
    let result: String = result.into_iter().collect();
    info!("{}", result);
    assert_ne!(0, result.len());
}

#[tokio::test]
#[traced_test]
#[ignore]
async fn test_english_question() {
    let model = create_model();
    let system_prompt =
        "你是一个助手，所有回答必须使用纯文本自然语言，禁止使用任何Markdown符号如#、-、*等。"
            .to_string();
    let history = Arc::new(Mutex::new(History {
        preamble: Some(system_prompt),
        chat_history: vec![],
    }));
    let client = ClientBuilder::new()
        .with_model(Arc::new(model))
        .build()
        .with_history(history);
    let request = ChatRequest {
        message: Message::User {
            content: OneOrMany::<UserContent>::one(UserContent::Text(Text {
                text: r#"Who is Albert Einstein"#.to_string(),
            })),
        },
    };
    let mut output = client.chat(request, CancellationToken::new());
    let mut result = Vec::new();
    while let Some(text) = output.next().await {
        match text {
            Ok(text) => {
                result.push(text);
            }
            Err(e) => {
                error!("{}", e.to_string());
            }
        }
    }
    let result: String = result.into_iter().collect();
    info!("{}", result);
    assert_ne!(0, result.len());
}

#[tokio::test]
#[traced_test]
#[ignore]
async fn test_chat_history() {
    let model = create_model();
    let system_prompt = "你是一个助手，协助用户进行记录，查询和提供建议，所有回答必须使用纯文本自然语言，禁止使用任何Markdown符号如#、-、*等并且数字使用中文字代替。".to_string();
    let history = Arc::new(Mutex::new(History {
        preamble: Some(system_prompt),
        chat_history: vec![
            Message::User {
                content: OneOrMany::<UserContent>::one(UserContent::Text(Text {
                    text: r#"小小电话号码为12349876"#.to_string(),
                })),
            },
            Message::Assistant {
                id: None,
                content: OneOrMany::<AssistantContent>::one(AssistantContent::Text(Text {
                    text: r#"小小电话号码为12349876"#.to_string(),
                })),
            },
        ],
    }));
    let client = ClientBuilder::new()
        .with_model(Arc::new(model))
        .build()
        .with_history(history);
    let request = ChatRequest {
        message: Message::User {
            content: OneOrMany::<UserContent>::one(UserContent::Text(Text {
                text: r#"小小的电话号码是多少"#.to_string(),
            })),
        },
    };
    let mut output = client.chat(request, CancellationToken::new());
    let mut result = Vec::new();
    while let Some(text) = output.next().await {
        match text {
            Ok(text) => {
                result.push(text);
            }
            Err(e) => {
                error!("{}", e.to_string());
            }
        }
    }
    let result: String = result.into_iter().collect();
    assert_ne!(0, result.len());
    info!("{}", result);
    assert!(result.contains("12349876"));
}

#[tokio::test]
#[traced_test]
#[ignore]
async fn test_chat_mcp() -> anyhow::Result<()> {
    let model = create_model();
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

    let mut server_client = ServerMcpClient::new(transport).await?;
    server_client.init().await?;

    let mut mcp_impl = McpProviderImpl::new(framework::id::gen_id());
    let mcp_host = mcp_impl.mcp_host();
    mcp_impl.add_client(Box::new(server_client)).await;

    let client = client::ClientBuilder::new()
        .with_model(Arc::new(model))
        .with_mcp_host(mcp_host)
        .build();
    // let request = ChatRequest {
    //     message: Message::User {
    //         content: OneOrMany::<UserContent>::one(UserContent::Text(Text {
    //             text: r#"Calculate the sum of 24.5 and 17.3 using the calculator service"#
    //                 .to_string(),
    //         })),
    //     },
    // };
    let request = ChatRequest {
        message: Message::User {
            content: OneOrMany::<UserContent>::one(UserContent::Text(Text {
                // text: r#"What time is it now"#.to_string(),
                text: r#"现在几点"#.to_string(),
            })),
        },
    };
    let mut output = client.chat(request, CancellationToken::new());
    let mut result = Vec::new();
    while let Some(text) = output.next().await {
        match text {
            Ok(text) => {
                result.push(text);
            }
            Err(e) => {
                error!("{}", e.to_string());
            }
        }
    }
    let result: String = result.into_iter().collect();
    assert_ne!(0, result.len());
    info!("{}", result);

    let _ = &state.conn.close().await.unwrap();
    tear_down(container).await;
    Ok(())
}
