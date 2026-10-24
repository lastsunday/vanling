use crate::common::ModelError;
use async_trait::async_trait;
use framework::error::AppError;
use futures::{Stream, StreamExt};
use futures_channel::mpsc::channel;
use futures_util::SinkExt;
use rig_core::{
    completion::{
        CompletionRequest as RigCompletionRequest, ToolDefinition as RigToolDef,
        message::{
            AssistantContent, AssistantMessage, CallId, Message as RigMessage, ToolName,
            ToolResultContent, UserContent,
        },
    },
    driver::Model,
    providers::openai::{OpenAIConfig, wire::Chat},
    streaming::{Item, StreamEvent},
};
use service::component::llm::{
    CompletionEvent, CompletionRequest, ContentPart, InputState, Llm, Message, Role, ToolDef,
};
use std::pin::Pin;
use std::time::Duration;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, warn};

const LLM_STREAM_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Clone)]
pub struct OpenAiCompatible {
    model: Model<Chat>,
}

impl OpenAiCompatible {
    pub fn new(api_url: &str, api_key: &str, api_model: &str) -> Result<Self, ModelError> {
        let client = OpenAIConfig::new(api_key).with_base_url(api_url).client();

        Ok(Self {
            model: client.chat(api_model),
        })
    }
}

fn convert_messages_to_rig(messages: &[Message], preamble: &Option<String>) -> Vec<RigMessage> {
    let mut result = Vec::new();
    if let Some(p) = preamble {
        result.push(RigMessage::system(p));
    }
    for msg in messages {
        match msg.role {
            Role::User => {
                let mut contents = Vec::new();
                for part in &msg.parts {
                    match part {
                        ContentPart::Text(t) => contents.push(UserContent::text(t)),
                        ContentPart::ToolResult { id, name, output } => {
                            let Some(name) = ToolName::new(name.as_str()).ok() else {
                                continue;
                            };
                            contents.push(UserContent::tool_result(
                                CallId::from_wire(id.clone()),
                                name,
                                vec![ToolResultContent::text(output)],
                            ));
                        }
                        _ => {}
                    }
                }
                if !contents.is_empty() {
                    result.push(RigMessage::User { content: contents });
                }
            }
            Role::Assistant => {
                let mut contents = Vec::new();
                for part in &msg.parts {
                    match part {
                        ContentPart::Text(t) => contents.push(AssistantContent::text(t)),
                        ContentPart::ToolCall {
                            id,
                            name,
                            arguments,
                        } => {
                            if let Ok(name) = ToolName::new(name.as_str()) {
                                contents.push(AssistantContent::tool_call(
                                    id,
                                    name,
                                    arguments.clone(),
                                ));
                            }
                        }
                        ContentPart::Reasoning(text) => {
                            contents.push(AssistantContent::reasoning(text));
                        }
                        _ => {}
                    }
                }
                if !contents.is_empty() {
                    result.push(RigMessage::Assistant(AssistantMessage::new(contents)));
                }
            }
        }
    }
    result
}

fn convert_tools_to_rig(tools: &[ToolDef]) -> Vec<RigToolDef> {
    tools
        .iter()
        .filter_map(|t| {
            Some(RigToolDef::new(
                ToolName::new(t.name.as_str()).ok()?,
                &t.description,
                t.input_schema.clone(),
            ))
        })
        .collect()
}

fn build_rig_request(request: CompletionRequest) -> RigCompletionRequest {
    let mut messages = convert_messages_to_rig(&request.messages, &request.preamble);
    let prompt = if messages.is_empty() {
        RigMessage::user("")
    } else {
        messages.remove(0)
    };

    RigCompletionRequest::new(prompt)
        .messages(messages)
        .tools(convert_tools_to_rig(&request.tools))
        .temperature(request.temperature)
        .max_tokens(request.max_tokens)
}

fn completion_event(item: Item<StreamEvent>) -> Option<CompletionEvent> {
    match item {
        Item::Event(StreamEvent::Text { text, .. }) => Some(CompletionEvent::Text(text)),
        Item::Event(StreamEvent::Reasoning { text, .. }) => Some(CompletionEvent::Reasoning(text)),
        Item::Event(StreamEvent::End {
            content: AssistantContent::ToolCall(call),
            ..
        }) => Some(CompletionEvent::ToolCall {
            id: call.id.to_string(),
            name: call.function.name.to_string(),
            arguments: serde_json::Value::Object(call.function.arguments),
        }),
        Item::Event(event) => {
            debug!(
                component = "OPENAI_MODEL",
                event = "unknown_stream_event",
                variant = event.name(),
                "unhandled streaming event skipped"
            );
            None
        }
        Item::Unknown(payload) => {
            debug!(
                component = "OPENAI_MODEL", event = "unknown_stream_payload",
                payload = ?payload,
                "unhandled streaming payload skipped"
            );
            None
        }
    }
}

#[async_trait]
impl Llm for OpenAiCompatible {
    async fn stream(
        &self,
        request: CompletionRequest,
        _state: InputState,
        cancel: CancellationToken,
    ) -> Pin<Box<dyn Stream<Item = Result<CompletionEvent, AppError>> + Send>> {
        let (mut tx, rx) = channel::<Result<CompletionEvent, AppError>>(16);
        let rig_request = build_rig_request(request);
        let rig_model = self.model.clone();

        tokio::spawn(async move {
            let stream = match rig_model.stream(rig_request) {
                Ok(s) => s,
                Err(e) => {
                    error!(component = "OPENAI_MODEL", event = "stream_init_error", error = %e, "LLM stream init failed");
                    let _ = tx.send(Ok(CompletionEvent::Error(e.to_string()))).await;
                    return;
                }
            };

            let mut stream = Box::pin(stream);
            loop {
                tokio::select! {
                    item = tokio::time::timeout(LLM_STREAM_TIMEOUT, stream.next()) => {
                        match item {
                            Ok(Some(Ok(item))) => {
                                let Some(event) = completion_event(item) else {
                                    continue;
                                };
                                if tx.send(Ok(event)).await.is_err() {
                                    break;
                                }
                            }
                            Ok(Some(Err(e))) => {
                                let _ = tx.send(Ok(CompletionEvent::Error(e.to_string()))).await;
                                break;
                            }
                            Ok(None) => {
                                let usage = stream.partial().usage;
                                let prompt_tokens = usage.input_tokens.unwrap_or(0);
                                let total_tokens = usage.total_tokens.unwrap_or_else(|| {
                                    prompt_tokens + usage.output_tokens.unwrap_or(0)
                                });
                                let _ = tx
                                    .send(Ok(CompletionEvent::Final {
                                        prompt_tokens: prompt_tokens as usize,
                                        total_tokens: total_tokens as usize,
                                    }))
                                    .await;
                                break;
                            }
                            Err(_) => {
                                // Timeout: no event received within LLM_STREAM_TIMEOUT
                                warn!(
                                    component = "OPENAI_MODEL", event = "stream_timeout",
                                    timeout_secs = LLM_STREAM_TIMEOUT.as_secs(),
                                    "LLM stream timeout: no response received"
                                );
                                let _ = tx.send(Ok(CompletionEvent::Error(
                                    format!("LLM stream timeout: no response within {} seconds", LLM_STREAM_TIMEOUT.as_secs())
                                ))).await;
                                break;
                            }
                        }
                    }
                    _ = cancel.cancelled() => {
                        info!(
                            component = "OPENAI_MODEL", event = "stream_cancelled",
                            "LLM stream cancelled"
                        );
                        break;
                    }
                }
            }
        });

        Box::pin(rx)
    }

    fn calculate_system_prompt_len(&self, _system_prompt: &Option<String>) -> u64 {
        0
    }

    fn calculate_tools_prompt_len(&self, _tools: &[ToolDef]) -> u64 {
        0
    }

    fn calculate_message_prompt_len(&self, _message: &Message) -> u64 {
        0
    }
}
