use api::{
    config::{LlmProvider, llm::LlmConfig},
    ling_core::{ChatRequest, History, LingCoreBuilder},
    llm::{Llm, LlmManager},
    mcp::client::external::ExternalMcpClient,
    setup_mcp,
};
use rmcp::transport::{
    StreamableHttpClientTransport, streamable_http_client::StreamableHttpClientTransportConfig,
};
use service::ling::{
    llm::{ContentPart, Message, Role},
    mcp::McpRegistry,
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

fn create_model() -> Arc<dyn Llm> {
    let model_path = common::tts::ws_root()
        .join("data/llm/model/qwen3/0.6b/")
        .to_string_lossy()
        .into_owned();
    LlmManager::create_model(&LlmConfig {
        provider: Some(LlmProvider::LocalQwen3),
        path: Some(model_path),
        ..Default::default()
    })
}

#[tokio::test]
#[traced_test]
async fn test_chat_echo() {
    let client = LingCoreBuilder::new()
        .with_model(LlmManager::create_model(&LlmConfig {
            provider: Some(LlmProvider::LocalEcho),
            ..Default::default()
        }))
        .build()
        .with_history(Arc::new(Mutex::new(History {
            preamble: None,
            chat_history: vec![],
        })));
    let mut output = client.complete(
        ChatRequest {
            message: Message {
                role: Role::User,
                parts: vec![ContentPart::Text(r#"Hello"#.to_string())],
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
    let client = LingCoreBuilder::new()
        .with_model(model)
        .build()
        .with_history(hisotry);
    let request = ChatRequest {
        message: Message {
            role: Role::User,
            parts: vec![ContentPart::Text(r#"静夜思的内容"#.to_string())],
        },
    };
    let mut output = client.complete(request, CancellationToken::new());
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
    let client = LingCoreBuilder::new()
        .with_model(model)
        .build()
        .with_history(history);
    let request = ChatRequest {
        message: Message {
            role: Role::User,
            parts: vec![ContentPart::Text(r#"1+1="#.to_string())],
        },
    };
    let mut output = client.complete(request, CancellationToken::new());
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
    let client = LingCoreBuilder::new()
        .with_model(model)
        .build()
        .with_history(history);
    let request = ChatRequest {
        message: Message {
            role: Role::User,
            parts: vec![ContentPart::Text(r#"Who is Albert Einstein"#.to_string())],
        },
    };
    let mut output = client.complete(request, CancellationToken::new());
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
            Message {
                role: Role::User,
                parts: vec![ContentPart::Text(r#"小小电话号码为12349876"#.to_string())],
            },
            Message {
                role: Role::Assistant,
                parts: vec![ContentPart::Text(r#"小小电话号码为12349876"#.to_string())],
            },
        ],
    }));
    let client = LingCoreBuilder::new()
        .with_model(model)
        .build()
        .with_history(history);
    let request = ChatRequest {
        message: Message {
            role: Role::User,
            parts: vec![ContentPart::Text(r#"小小的电话号码是多少"#.to_string())],
        },
    };
    let mut output = client.complete(request, CancellationToken::new());
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
    let config = StreamableHttpClientTransportConfig::with_uri("/mcp");
    let client = RouterClient { router };
    let transport = StreamableHttpClientTransport::with_client(client, config);

    let mut external_client = ExternalMcpClient::new(transport).await?;
    external_client.init().await?;

    let mcp_registry = Arc::new(Mutex::new(McpRegistry::new(Some(framework::id::gen_id()))));
    mcp_registry
        .lock()
        .await
        .add_client(Arc::new(external_client))
        .await;

    let client = LingCoreBuilder::new()
        .with_model(model)
        .with_mcp_registry(mcp_registry)
        .build();
    let request = ChatRequest {
        message: Message {
            role: Role::User,
            parts: vec![ContentPart::Text(r#"现在几点"#.to_string())],
        },
    };
    let mut output = client.complete(request, CancellationToken::new());
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
