use std::{collections::VecDeque, sync::Arc, thread};

use crate::{
    common::ModelError,
    llm::{LlmEngine, chat::Chat, model::echo::Echo},
    mcp::mcp_host::McpHost,
};
use framework::id::gen_id;
use futures::StreamExt;
use futures::{Stream, executor::block_on};
use rig::{
    OneOrMany,
    completion::{CompletionError, CompletionRequest},
    message::{AssistantContent, Message, Reasoning, Text, ToolCall, UserContent},
    streaming::{StreamedAssistantContent, StreamingCompletionResponse},
};
use tokio::sync::{
    Mutex,
    mpsc::{Sender, channel},
};
use tokio_stream::wrappers::ReceiverStream;
use tokio_util::sync::CancellationToken;
use tracing::{Instrument, Level, error, span, trace};

#[derive(Clone)]
pub struct Client {
    session_id: Option<String>,
    model: Arc<Box<dyn LlmEngine>>,
    temperature: Option<f64>,
    max_tokens: Option<u64>,
    max_prompt_len: Option<u64>,
    history: Arc<Mutex<History>>,
    mcp_host: Option<Arc<Mutex<dyn McpHost>>>,
}

pub struct ChatRequest {
    pub message: Message,
}

pub struct History {
    pub preamble: Option<String>,
    pub chat_history: Vec<Message>,
}

impl Client {
    pub fn builder() -> ClientBuilder {
        ClientBuilder::new()
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

    pub fn chat(
        &self,
        request: ChatRequest,
        cancel: CancellationToken,
    ) -> impl Stream<Item = core::result::Result<String, ModelError>> + Unpin + Send + 'static {
        let (tx, rx) = channel::<core::result::Result<String, ModelError>>(10);
        let session_id = self.session_id.clone();
        let model = self.model.clone();
        let mcp_host = self.mcp_host.clone();
        let tx_main = tx.clone();
        let clone_history = self.history.clone();
        let temperature = self.temperature;
        let max_tokens = self.max_tokens;
        let max_prompt_len = self.max_prompt_len;
        let span = span!(parent:None,Level::DEBUG, "llm_client", session_id=%session_id.unwrap_or_default());
        thread::spawn(move || {
            if cancel.is_cancelled() {
                return;
            }
            let output = block_on(
                async move {
                    let tools = {
                        if let Some(mcp_host) = &mcp_host {
                            let mcp_host = mcp_host.lock().await;
                            mcp_host.get_tool().await?
                        } else {
                            vec![]
                        }
                    };
                    let mut has_next_step = true;
                    let history = clone_history.clone();
                    let mut history = history.lock().await;
                    if let Some(max_prompt_len) = max_prompt_len {
                        // cut prompt
                        let mut current_len: u64 = 0;
                        if let Some(item) = &history.preamble {
                            current_len += item.len() as u64;
                        }
                        current_len += model.calculate_tools_prompt_len(&tools);
                        let mut target_message_list = VecDeque::new();
                        // TODO: remove clone?
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
                    while has_next_step {
                        if cancel.is_cancelled() {
                            break;
                        }
                        let history = clone_history.clone();
                        let history = history.lock().await;
                        let preamble = history.preamble.clone();
                        let chat_history = OneOrMany::many(history.chat_history.clone()).unwrap();
                        drop(history);
                        let request = CompletionRequest {
                            preamble: preamble.clone(),
                            chat_history: chat_history.clone(),
                            documents: vec![],
                            tools: tools.clone(),
                            temperature,
                            max_tokens,
                            tool_choice: None,
                            additional_params: None,
                        };
                        trace!(?request, "[REQUEST]");
                        let response = model.stream(request).await;
                        let messages =
                            handle_response(response, Some(tx.clone()), cancel.clone()).await;
                        trace!(?messages, "[RESPONSE]");
                        match messages {
                            Ok(messages) => {
                                has_next_step = false;
                                for message in &messages {
                                    let history = clone_history.clone();
                                    let mut history = history.lock().await;
                                    history.chat_history.push(message.clone());
                                    drop(history);
                                    match message {
                                        Message::User { content: _content } => {
                                            //skip
                                        }
                                        Message::Assistant { id: _id, content } => {
                                            for item in content.iter() {
                                                match item {
                                                    AssistantContent::ToolCall(ToolCall {
                                                        id,
                                                        call_id,
                                                        function,
                                                        signature,
                                                        additional_params,
                                                    }) => {
                                                        if let Some(mcp_host) = mcp_host.clone() {
                                                            let mcp_host = mcp_host.lock().await;
                                                            let result = mcp_host
                                                                .call_tool(ToolCall {
                                                                    id: id.clone(),
                                                                    call_id: call_id.clone(),
                                                                    function: function.clone(),
                                                                    signature: signature.clone(),
                                                                    additional_params:
                                                                        additional_params.clone(),
                                                                })
                                                                .await?;
                                                            let history = clone_history.clone();
                                                            let mut history = history.lock().await;
                                                            history
                                                                .chat_history
                                                                .push(Message::User {
                                                                content:
                                                                    OneOrMany::<UserContent>::one(
                                                                        UserContent::ToolResult(
                                                                            result,
                                                                        ),
                                                                    ),
                                                            });
                                                            drop(history);
                                                            has_next_step = true;
                                                        }
                                                    }
                                                    _ => {
                                                        //skip
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                            Err(e) => {
                                tracing::error!(error = %e, "LLM stream error");
                                break;
                            }
                        }
                    }
                    drop(tx);
                    anyhow::Ok(())
                }
                .instrument(span),
            );
            match output {
                Ok(_) => {
                    drop(tx_main);
                }
                Err(e) => {
                    block_on(async move {
                        if let Err(e) = tx_main.send(Err(ModelError::Chat(e.to_string()))).await {
                            error!(error = %e, "send response error");
                        }
                        drop(tx_main);
                    });
                }
            }
        });
        ReceiverStream::new(rx)
    }
}

pub async fn handle_response(
    response: Result<
        StreamingCompletionResponse<rig::providers::openai::streaming::StreamingCompletionResponse>,
        CompletionError,
    >,
    tx: Option<Sender<Result<String, ModelError>>>,
    cancel: CancellationToken,
) -> anyhow::Result<Vec<Message>> {
    let mut messages: Vec<Message> = vec![];
    let mut text_collector = String::new();
    let mut chat = Chat::new();
    match response {
        Ok(mut stream) => {
            while let Some(value) = stream.next().await {
                if cancel.is_cancelled() {
                    break;
                }
                match value {
                    Ok(StreamedAssistantContent::Text(text)) => {
                        text_collector.push_str(&text.text);
                        if let Some(tx) = &tx {
                            let sentence_list = chat.accept_text(&text.text);
                            let sentence_iter = sentence_list.iter();
                            for sentence in sentence_iter {
                                tx.send(Ok(sentence.to_string())).await?;
                            }
                        }
                    }
                    Ok(StreamedAssistantContent::Final(
                        rig::providers::openai::StreamingCompletionResponse { usage: _usage },
                    )) => {}
                    Ok(StreamedAssistantContent::ToolCall {
                        tool_call,
                        internal_call_id: _internal_call_id,
                    }) => {
                        messages.push(Message::Assistant {
                            id: Some(tool_call.id.clone()),
                            content: OneOrMany::<AssistantContent>::one(
                                AssistantContent::ToolCall(ToolCall {
                                    id: tool_call.id.clone(),
                                    call_id: tool_call.call_id.clone(),
                                    function: tool_call.function,
                                    signature: tool_call.signature.clone(),
                                    additional_params: tool_call.additional_params.clone(),
                                }),
                            ),
                        });
                    }
                    Ok(StreamedAssistantContent::ToolCallDelta {
                        id: _id,
                        internal_call_id: _internal_call_id,
                        content,
                    }) => {
                        // TODO:
                        trace!(?content, "tool call delta");
                    }
                    Ok(StreamedAssistantContent::Reasoning(Reasoning {
                        id: _id,
                        reasoning,
                        ..
                    })) => {
                        // TODO:
                        trace!(?reasoning, "reasoning");
                    }
                    Ok(StreamedAssistantContent::ReasoningDelta { id: _id, reasoning }) => {
                        trace!(?reasoning, "reasoning delta");
                    }
                    Err(e) => {
                        if let Some(tx) = &tx {
                            tx.send(Err(ModelError::ModelCompletionError(e.to_string())))
                                .await?;
                        }
                    }
                }
            }
            if let Some(tx) = &tx {
                let sentence_list = chat.accept_final();
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
        Err(e) => Err(anyhow::anyhow!(e.to_string())),
    }
}

pub struct ClientBuilder {
    session_id: Option<String>,
    model: Arc<Box<dyn LlmEngine>>,
    mcp_host: Option<Arc<Mutex<dyn McpHost>>>,
}

impl ClientBuilder {
    pub fn new() -> Self {
        Default::default()
    }

    pub fn with_session_id(mut self, session_id: Option<String>) -> ClientBuilder {
        self.session_id = session_id;
        self
    }

    pub fn with_model(mut self, model: Arc<Box<dyn LlmEngine>>) -> ClientBuilder {
        self.model = model;
        self
    }

    pub fn with_mcp_host(mut self, mcp_host: Arc<Mutex<dyn McpHost>>) -> ClientBuilder {
        self.mcp_host = Some(mcp_host);
        self
    }

    pub fn build(self) -> Client {
        Client {
            session_id: self.session_id,
            model: self.model,
            temperature: None,
            max_tokens: None,
            max_prompt_len: Some(6000),
            history: Arc::new(Mutex::new(History {
                preamble: None,
                chat_history: vec![],
            })),
            mcp_host: self.mcp_host,
        }
    }
}

impl Default for ClientBuilder {
    fn default() -> Self {
        Self {
            session_id: None,
            model: Arc::new(Box::new(Echo::default())),
            mcp_host: None,
        }
    }
}

use async_trait::async_trait;
use framework::error::AppError;
use service::chobits::llm::Llm;
use std::pin::Pin;

#[async_trait]
impl Llm for Client {
    async fn chat(
        &self,
        text: String,
        cancel: CancellationToken,
    ) -> Pin<Box<dyn Stream<Item = Result<String, AppError>> + Send + 'static>> {
        let request = ChatRequest {
            message: Message::User {
                content: OneOrMany::one(UserContent::Text(Text { text })),
            },
        };
        let stream = self.chat(request, cancel);
        let mapped = stream.map(|item| item.map_err(AppError::from));
        Box::pin(mapped)
    }
}
