use crate::common::ModelError;
use async_trait::async_trait;
use framework::error::AppError;
use futures::{Stream, StreamExt};
use futures_channel::mpsc::channel;
use futures_util::SinkExt;
use rig_core::{
    OneOrMany,
    client::CompletionClient,
    completion::{
        CompletionModel, CompletionRequest as RigCompletionRequest, ToolDefinition as RigToolDef,
        message::{AssistantContent, Message as RigMessage, ToolResultContent, UserContent},
    },
    providers::openai::CompletionsClient,
    streaming::StreamedAssistantContent,
};
use service::ling::llm::{
    CompletionEvent, CompletionRequest, ContentPart, Llm, Message, Role, ToolDef,
};
use std::pin::Pin;
use std::time::Duration;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, warn};

const LLM_STREAM_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Clone)]
pub struct OpenAiCompatible {
    model: rig_core::providers::openai::completion::CompletionModel,
}

impl OpenAiCompatible {
    pub fn new(api_url: &str, api_key: &str, api_model: &str) -> Result<Self, ModelError> {
        let client = CompletionsClient::builder()
            .base_url(api_url)
            .api_key(api_key)
            .build()
            .map_err(|e| {
                ModelError::ModelInitFailure(format!(
                    "failed to build OpenAI-compatible client: {e}"
                ))
            })?;

        let model = client.completion_model(api_model.to_owned());

        Ok(Self { model })
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
                        ContentPart::Text(t) => {
                            contents.push(UserContent::text(t));
                        }
                        ContentPart::ToolResult { id, output } => {
                            contents.push(UserContent::tool_result(
                                id.clone(),
                                OneOrMany::one(ToolResultContent::text(output)),
                            ));
                        }
                        _ => {}
                    }
                }
                if let Ok(content) = OneOrMany::many(contents) {
                    result.push(RigMessage::User { content });
                }
            }
            Role::Assistant => {
                let mut contents = Vec::new();
                for part in &msg.parts {
                    match part {
                        ContentPart::Text(t) => {
                            contents.push(AssistantContent::text(t));
                        }
                        ContentPart::ToolCall {
                            id,
                            name,
                            arguments,
                        } => {
                            contents.push(AssistantContent::tool_call(id, name, arguments.clone()));
                        }
                        ContentPart::Reasoning(text) => {
                            contents.push(AssistantContent::reasoning(text));
                        }
                        _ => {}
                    }
                }
                if let Ok(content) = OneOrMany::many(contents) {
                    result.push(RigMessage::Assistant { id: None, content });
                }
            }
        }
    }
    result
}

fn convert_tools_to_rig(tools: &[ToolDef]) -> Vec<RigToolDef> {
    tools
        .iter()
        .map(|t| RigToolDef {
            name: t.name.clone(),
            description: t.description.clone(),
            parameters: t.input_schema.clone(),
        })
        .collect()
}

fn build_rig_request(request: CompletionRequest) -> RigCompletionRequest {
    let messages = convert_messages_to_rig(&request.messages, &request.preamble);
    let chat_history =
        OneOrMany::many(messages).unwrap_or_else(|_| OneOrMany::one(RigMessage::user("")));
    let tools = convert_tools_to_rig(&request.tools);

    RigCompletionRequest {
        model: None,
        preamble: None,
        chat_history,
        documents: vec![],
        tools,
        temperature: request.temperature,
        max_tokens: request.max_tokens,
        tool_choice: None,
        additional_params: None,
        output_schema: None,
    }
}

#[async_trait]
impl Llm for OpenAiCompatible {
    async fn stream(
        &self,
        request: CompletionRequest,
        cancel: CancellationToken,
    ) -> Pin<Box<dyn Stream<Item = Result<CompletionEvent, AppError>> + Send>> {
        let (mut tx, rx) = channel::<Result<CompletionEvent, AppError>>(16);
        let rig_request = build_rig_request(request);
        let rig_model = self.model.clone();

        tokio::spawn(async move {
            let stream = match rig_model.stream(rig_request).await {
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
                            Ok(Some(item)) => {
                                let event = match item {
                                    Ok(StreamedAssistantContent::Text(text)) => {
                                        Ok(CompletionEvent::Text(text.text))
                                    }
                                    Ok(StreamedAssistantContent::ReasoningDelta { reasoning, .. }) => {
                                        Ok(CompletionEvent::Reasoning(reasoning))
                                    }
                                    Ok(StreamedAssistantContent::Reasoning(reasoning)) => {
                                        Ok(CompletionEvent::Reasoning(reasoning.display_text()))
                                    }
                                    Ok(StreamedAssistantContent::ToolCall { tool_call, .. }) => {
                                        Ok(CompletionEvent::ToolCall {
                                            id: tool_call.id,
                                            name: tool_call.function.name,
                                            arguments: tool_call.function.arguments,
                                        })
                                    }
                                    Ok(StreamedAssistantContent::Final(response)) => Ok(CompletionEvent::Final {
                                        prompt_tokens: response.usage.prompt_tokens,
                                        total_tokens: response.usage.total_tokens,
                                    }),
                                    Ok(other) => {
                                        debug!(
                                            component = "OPENAI_MODEL", event = "unknown_stream_event",
                                            variant = std::any::type_name_of_val(&other),
                                            "unhandled streaming event skipped"
                                        );
                                        continue;
                                    }
                                    Err(e) => Ok(CompletionEvent::Error(e.to_string())),
                                };
                                if tx.send(event).await.is_err() {
                                    break;
                                }
                            }
                            Ok(None) => {
                                // Stream ended normally
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
