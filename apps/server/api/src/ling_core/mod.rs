pub mod splitter;
pub use splitter::Splitter;

use std::{collections::VecDeque, sync::Arc};

use crate::common::ModelError;
use futures::Stream;
use futures::StreamExt;
use service::ling::llm::{CompletionEvent, CompletionRequest, ContentPart, Llm, Message, Role};
use service::ling::mcp::McpRegistry;
use tokio::sync::{Mutex, mpsc::channel};
use tokio_stream::wrappers::ReceiverStream;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, trace, warn};

#[derive(Clone)]
pub struct LingCore {
    session_id: Option<String>,
    model: Arc<dyn Llm>,
    temperature: Option<f64>,
    max_tokens: Option<u64>,
    max_prompt_len: Option<u64>,
    history: Arc<Mutex<History>>,
    mcp_registry: Option<Arc<Mutex<McpRegistry>>>,
}

pub struct ChatRequest {
    pub message: Message,
}

pub struct History {
    pub preamble: Option<String>,
    pub chat_history: Vec<Message>,
}

impl LingCore {
    pub fn builder() -> LingCoreBuilder {
        LingCoreBuilder::new()
    }

    pub fn with_history(mut self, history: Arc<Mutex<History>>) -> Self {
        self.history = history;
        self
    }

    pub fn with_temperature(mut self, temperature: Option<f64>) -> Self {
        self.temperature = temperature;
        self
    }

    pub fn with_max_tokens(mut self, max_tokens: Option<u64>) -> Self {
        self.max_tokens = max_tokens;
        self
    }

    pub fn with_max_prompt_len(mut self, max_prompt_len: Option<u64>) -> Self {
        self.max_prompt_len = max_prompt_len;
        self
    }

    pub fn complete(
        &self,
        request: ChatRequest,
        cancel: CancellationToken,
    ) -> impl Stream<Item = core::result::Result<String, ModelError>> + Unpin + Send + 'static {
        let (tx, rx) = channel::<core::result::Result<String, ModelError>>(10);
        let session_id = self.session_id.clone();
        let model = self.model.clone();
        let mcp_registry = self.mcp_registry.clone();
        let tx_main = tx.clone();
        let clone_history = self.history.clone();
        let temperature = self.temperature;
        let max_tokens = self.max_tokens;
        let max_prompt_len = self.max_prompt_len;
        tokio::spawn(async move {
            if cancel.is_cancelled() {
                return;
            }
            let result: Result<(), anyhow::Error> = async {
                    let tools = {
                        if let Some(mcp_registry) = &mcp_registry {
                            let mcp_registry = mcp_registry.lock().await;
                            mcp_registry.get_tool().await?
                        } else {
                            vec![]
                        }
                    };
                    let mut has_next_step = true;
                    let history = clone_history.clone();
                    let mut history = history.lock().await;
                    if let Some(max_prompt_len) = max_prompt_len {
                        let mut current_len: u64 = 0;
                        if let Some(item) = &history.preamble {
                            current_len += item.len() as u64;
                        }
                        current_len += model.calculate_tools_prompt_len(&tools);
                        let mut target_message_list = VecDeque::new();
                        let chat_history: Vec<_> =
                            history.chat_history.clone().into_iter().rev().collect();
                        for message in chat_history {
                            let len = model.calculate_message_prompt_len(&message);
                            current_len += len;
                            if current_len <= max_prompt_len {
                                target_message_list.push_front(message);
                            } else {
                                break;
                            }
                        }
                        trace!(current_len, max_prompt_len, ?history.chat_history, ?target_message_list, "truncation history");
                        history.chat_history.clear();
                        history
                            .chat_history
                            .append(&mut Vec::from(target_message_list));
                    }
                    history.chat_history.push(request.message.clone());
                    drop(history);
                    if cancel.is_cancelled() {
                        return Err(anyhow::anyhow!("cancelled"));
                    }
                    let mut loop_count = 0u32;
                    while has_next_step {
                        loop_count += 1;
                        debug!(
                            component = "LING", event = "loop_iteration",
                            session_id = ?session_id,
                            iteration = loop_count,
                            "LLM complete loop iteration"
                        );
                        if cancel.is_cancelled() {
                            break;
                        }
                        let history = clone_history.clone();
                        let history = history.lock().await;
                        let preamble = history.preamble.clone();
                        let messages = history.chat_history.clone();
                        drop(history);
                        let req = CompletionRequest {
                            preamble: preamble.clone(),
                            messages: messages.clone(),
                            tools: tools.clone(),
                            temperature,
                            max_tokens,
                        };
                        trace!(?req, "[REQUEST]");
                        let mut stream = model.stream(req, cancel.clone()).await;
                        let mut text_collector = String::new();
                        let mut assistant_events: Vec<CompletionEvent> = vec![];
                        let mut splitter = Splitter::new();
                        while let Some(event) = stream.next().await {
                            if cancel.is_cancelled() {
                                break;
                            }
                            match event {
                                Ok(CompletionEvent::Text(text)) => {
                                    trace!(
                                        component = "LING", event = "text_event",
                                        session_id = ?session_id,
                                        text_len = text.len(),
                                        "text event received"
                                    );
                                    text_collector.push_str(&text);

                                    // Single entry point: accept tokens
                                    let sentence_list = splitter.accept_token(&text);
                                    for sentence in sentence_list {
                                        tx.send(Ok(sentence)).await?;
                                    }
                                }
                                Ok(CompletionEvent::ToolCall {
                                    id,
                                    name,
                                    arguments,
                                }) => {
                                    info!(
                                        component = "LING", event = "tool_call",
                                        session_id = ?session_id,
                                        tool_name = %name,
                                        "tool call received"
                                    );
                                    assistant_events.push(CompletionEvent::ToolCall {
                                        id,
                                        name,
                                        arguments,
                                    });
                                }
                                Ok(CompletionEvent::Reasoning(text)) => {
                                    debug!(
                                        component = "LING", event = "reasoning_event",
                                        session_id = ?session_id,
                                        text_len = text.len(),
                                        "reasoning event received"
                                    );
                                    assistant_events.push(CompletionEvent::Reasoning(text));
                                }
                                Ok(CompletionEvent::Final { .. }) => {
                                debug!(
                                    component = "LING", event = "final_event",
                                    session_id = ?session_id,
                                    text_collector_len = text_collector.len(),
                                    "final event received"
                                );
                                    if !text_collector.is_empty() {
                                        let sentence_list = splitter.accept_final();
                                        for sentence in sentence_list {
                                            tx.send(Ok(sentence)).await?;
                                        }
                                        assistant_events.push(CompletionEvent::Text(
                                            text_collector.clone(),
                                        ));
                                    }
                                }
                                Ok(CompletionEvent::Error(e)) => {
                                    error!(component = "LING", event = "llm_stream_event_error", error = %e, "LLM stream event error");
                                }
                                Err(e) => {
                                    error!(component = "LING", event = "llm_stream_error", error = %e, "LLM stream error");
                                    let _ = tx.send(Err(ModelError::Chat(e.to_string()))).await;
                                }
                            }
                        }
                        trace!(?assistant_events, "[RESPONSE]");
                        let mut has_tool_call = false;
                        for event in &assistant_events {
                            match event {
                                CompletionEvent::Text(text) => {
                                    let history = clone_history.clone();
                                    let mut history = history.lock().await;
                                    history
                                        .chat_history
                                        .push(Message {
                                            role: Role::Assistant,
                                            parts: vec![ContentPart::Text(text.clone())],
                                        });
                                    drop(history);
                                }
                                CompletionEvent::ToolCall {
                                    id,
                                    name,
                                    arguments,
                                } => {
                                    has_tool_call = true;
                                    let history = clone_history.clone();
                                    let mut history = history.lock().await;
                                    history
                                        .chat_history
                                        .push(Message {
                                            role: Role::Assistant,
                                            parts: vec![ContentPart::ToolCall {
                                                id: id.clone(),
                                                name: name.clone(),
                                                arguments: arguments.clone(),
                                            }],
                                        });
                                    drop(history);
                                    if let Some(mcp_registry) = mcp_registry.clone() {
                                        let mcp_registry = mcp_registry.lock().await;
                                        match mcp_registry
                                            .call_tool(name, arguments.clone())
                                            .await
                                        {
                                            Ok(result) => {
                                                info!(
                                                    component = "LING", event = "tool_result",
                                                    session_id = ?session_id,
                                                    tool_name = %name,
                                                    result_len = result.len(),
                                                    "tool call result received"
                                                );
                                                let history = clone_history.clone();
                                                let mut history = history.lock().await;
                                                history
                                                    .chat_history
                                                    .push(Message {
                                                        role: Role::User,
                                                        parts: vec![ContentPart::ToolResult {
                                                            id: id.clone(),
                                                            output: result,
                                                        }],
                                                    });
                                                drop(history);
                                            }
                                            Err(e) => {
                                                error!(
                                                    component = "LING", event = "tool_error",
                                                    session_id = ?session_id,
                                                    tool_name = %name,
                                                    error = %e,
                                                    "tool call failed"
                                                );
                                                let _ = tx
                                                    .send(Err(ModelError::Chat(e.to_string())))
                                                    .await;
                                            }
                                        }
                                    }
                                }
                                CompletionEvent::Reasoning(text) => {
                                    let history = clone_history.clone();
                                    let mut history = history.lock().await;
                                    history
                                        .chat_history
                                        .push(Message {
                                            role: Role::Assistant,
                                            parts: vec![ContentPart::Reasoning(text.clone())],
                                        });
                                    drop(history);
                                }
                                _ => {}
                            }
                        }
                        if text_collector.is_empty() && !has_tool_call {
                            warn!(
                                component = "LING", event = "llm_no_usable_output",
                                session_id = ?session_id,
                                "LLM completed with no usable output"
                            );
                            let _ = tx
                                .send(Err(ModelError::Chat(
                                    "LLM completed with no usable output".to_string(),
                                )))
                                .await;
                        }
                        has_next_step = has_tool_call;
                    }
                    drop(tx);
                    anyhow::Ok(())
                }
                .await;
            match result {
                Ok(_) => drop(tx_main),
                Err(e) => {
                    let _ = tx_main.send(Err(ModelError::Chat(e.to_string()))).await;
                    drop(tx_main);
                }
            }
        });
        ReceiverStream::new(rx)
    }
}

use async_trait::async_trait;
use framework::error::AppError;
use service::ling::core::{ContentBlock, Input, Ling, OutputBlock};
use std::pin::Pin;

#[async_trait]
impl Ling for LingCore {
    async fn ask(
        &self,
        input: Input,
        cancel: CancellationToken,
    ) -> Pin<Box<dyn Stream<Item = Result<OutputBlock, AppError>> + Send + 'static>> {
        let text = match input.content.first() {
            Some(ContentBlock::Text(t)) => t.clone(),
            _ => String::new(),
        };
        let request = ChatRequest {
            message: Message {
                role: Role::User,
                parts: vec![ContentPart::Text(text)],
            },
        };
        let stream = self.complete(request, cancel);
        let mapped = stream.map(|item| item.map(OutputBlock::Text).map_err(AppError::from));
        Box::pin(mapped)
    }
}

#[derive(Default)]
pub struct LingCoreBuilder {
    session_id: Option<String>,
    model: Option<Arc<dyn Llm>>,
    mcp_registry: Option<Arc<Mutex<McpRegistry>>>,
    preamble: Option<String>,
}

impl LingCoreBuilder {
    pub fn new() -> Self {
        Default::default()
    }

    pub fn with_session_id(mut self, session_id: Option<String>) -> LingCoreBuilder {
        self.session_id = session_id;
        self
    }

    pub fn with_model(mut self, model: Arc<dyn Llm>) -> LingCoreBuilder {
        self.model = Some(model);
        self
    }

    pub fn with_mcp_registry(mut self, mcp_registry: Arc<Mutex<McpRegistry>>) -> LingCoreBuilder {
        self.mcp_registry = Some(mcp_registry);
        self
    }

    pub fn with_preamble(mut self, preamble: Option<String>) -> LingCoreBuilder {
        self.preamble = preamble;
        self
    }

    pub fn build(self) -> LingCore {
        LingCore {
            session_id: self.session_id,
            model: self.model.expect("model is required"),
            temperature: None,
            max_tokens: None,
            max_prompt_len: Some(6000),
            history: Arc::new(Mutex::new(History {
                preamble: self.preamble,
                chat_history: vec![],
            })),
            mcp_registry: self.mcp_registry,
        }
    }
}
