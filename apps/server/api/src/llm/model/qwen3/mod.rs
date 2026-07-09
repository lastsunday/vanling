#[cfg(feature = "mkl")]
extern crate intel_mkl_src;

#[cfg(feature = "accelerate")]
extern crate accelerate_src;

pub mod quantized;

use super::token_output_stream::TokenOutputStream;
use crate::common::{ModelError, device};
use async_trait::async_trait;
use candle_core::{Device, Tensor, quantized::gguf_file};
use candle_transformers::generation::{LogitsProcessor, Sampling};
use framework::error::AppError;
use futures::{SinkExt, Stream, executor::block_on};
use futures_channel::mpsc::{Sender, channel};
use quantized::ModelWeights as Qwen3Model;
use service::chobits::llm::TokenConverter;
use service::chobits::llm::{
    CompletionEvent, CompletionRequest, ContentPart, Llm, Message, Role, ToolDef,
};
use std::pin::Pin;
use std::thread;
use tokenizers::Tokenizer;
use tokio_util::sync::CancellationToken;
use tracing::error;

#[derive(Clone)]
pub struct LlmQwen {
    model: Qwen3Model,
    tokenizer: Tokenizer,
    device: Device,
}

impl LlmQwen {
    pub fn new(path: &str) -> core::result::Result<Self, ModelError> {
        let model_path = format!("{}model.gguf", path);
        let token_path = format!("{}tokenizer.json", path);
        let mut file = std::fs::File::open(model_path.clone())
            .map_err(|_e| ModelError::ModelFileNotFound(model_path.clone()))?;
        let device = device(false)?;
        let model = {
            let model = gguf_file::Content::read(&mut file)
                .map_err(|_e| ModelError::ModelInitFailure(model_path.clone()))?;
            Qwen3Model::from_gguf(model, &mut file, &device)
                .map_err(|_e| ModelError::ModelInitFailure(model_path.clone()))?
        };
        let tokenizer = Tokenizer::from_file(token_path.clone())
            .map_err(|_e| ModelError::TokenInitFailure(token_path.clone()))?;
        Ok(Self {
            model,
            tokenizer,
            device: device.clone(),
        })
    }
}

fn create_system_prompt(system_prompt: &Option<String>) -> String {
    let mut prompt = String::new();
    prompt.push_str("<|im_start|>system\n");
    if let Some(text) = system_prompt {
        prompt.push_str(text);
    }
    prompt
}

fn create_tools_prompt(tools: &[ToolDef]) -> String {
    let mut prompt = String::new();
    let mut tools_str = String::new();
    for tool in tools.iter() {
        let tool_json_str = serde_json::to_string(tool).unwrap();
        tools_str.push_str(&format!("{}\n", tool_json_str));
    }
    let mut tools_prompt = String::new();
    tools_prompt.push_str(&format!(
        "# Tools\n
        You may call one or more functions to assist with the user query.\n
        You are provided with function signatures within <tools></tools> XML tags:\n
        <tools>\n
        {}\n
        </tools>\n
        For each function call, return a json object with function name and arguments within <tool_call></tool_call> XML tags:\n
        <tool_call>\n
        {{\"name\": <function-name>, \"arguments\": <args-json-object>}}\n
        </tool_call>\n
        ",
        tools_str
    ));
    prompt.push_str(&tools_prompt);
    prompt
}

fn create_message_prompt(message: &Message) -> String {
    let mut prompt = String::new();
    for part in &message.parts {
        match part {
            ContentPart::Text(text) => match message.role {
                Role::User => {
                    prompt.push_str(&format!("<|im_start|>user\n{}<|im_end|>\n", text));
                }
                Role::Assistant => {
                    prompt.push_str(&format!("<|im_start|>assistant\n{}<|im_end|>\n", text));
                }
            },
            ContentPart::ToolCall {
                name, arguments, ..
            } => {
                let tool_call_text = serde_json::to_string(&serde_json::json!({
                    "name": name,
                    "arguments": arguments,
                }))
                .unwrap();
                prompt.push_str(&format!(
                    "<|im_start|>assistant\n<tool_call>{}</tool_call><|im_end|>\n",
                    tool_call_text
                ));
            }
            ContentPart::ToolResult { output, .. } => {
                prompt.push_str(&format!(
                    "<|im_start|>user\n<tool_response>{}</tool_response><|im_end|>\n",
                    output
                ));
            }
        }
    }
    prompt
}

fn convert_request_to_prompt(request: &CompletionRequest) -> String {
    let mut prompt = String::new();
    prompt.push_str(&create_system_prompt(&request.preamble));

    if !request.tools.is_empty() {
        prompt.push_str(&create_tools_prompt(&request.tools));
    }
    prompt.push_str("<|im_end|>\n");
    for message in request.messages.iter() {
        prompt.push_str(&create_message_prompt(message));
    }
    prompt.push_str("<|im_start|>assistant\n");
    prompt
}

async fn handle(
    request: &CompletionRequest,
    tokenizer: Tokenizer,
    mut model: Qwen3Model,
    device: Device,
    mut tx: Sender<Result<CompletionEvent, AppError>>,
    cancel: CancellationToken,
) -> Result<(), AppError> {
    if cancel.is_cancelled() {
        return Ok(());
    }

    let mut tos = TokenOutputStream::new(tokenizer);
    let prompt_str = convert_request_to_prompt(request);

    let tokens = tos
        .tokenizer()
        .encode(prompt_str, true)
        .map_err(|e| ModelError::Chat(format!("tokenizer encode error {}", e)))?;
    let tokens = tokens.get_ids();

    let to_sample = request.max_tokens.unwrap_or(32768) as usize;
    let temperature = request.temperature.unwrap_or(0.8);
    let seed = 299792458;
    let repeat_last_n = 64;
    let repeat_penalty = 1.1;

    let mut all_tokens = vec![];
    let mut token_converter = TokenConverter::new();

    let mut logits_processor = LogitsProcessor::from_sampling(seed, Sampling::All { temperature });

    let input = Tensor::new(tokens, &device)
        .map_err(|e| ModelError::Chat(format!("tensor create error {}", e)))?
        .unsqueeze(0)
        .map_err(|e| ModelError::Chat(format!("tensor create unsqueeze error {}", e)))?;
    let logits = model
        .forward(&input, 0)
        .map_err(|e| ModelError::Chat(format!("model forward error {}", e)))?;
    let logits = logits
        .squeeze(0)
        .map_err(|e| ModelError::Chat(format!("tensor squeeze error {}", e)))?;
    let mut next_token = logits_processor
        .sample(&logits)
        .map_err(|e| ModelError::Chat(format!("tensor processor sample error {}", e)))?;

    all_tokens.push(next_token);
    if let Some(t) = tos
        .next_token(next_token)
        .map_err(|e| ModelError::Chat(format!("tensor encoding error {}", e)))?
    {
        let messages = token_converter.accept_text(&t)?;
        for message in messages.iter() {
            if let Err(e) = tx.send(Ok(message.clone())).await {
                error!(error = %e, "send text error");
            }
        }
    }

    let eos_token = *tos
        .tokenizer()
        .get_vocab(true)
        .get("<|im_end|>")
        .ok_or_else(|| ModelError::Chat("tensor can't get eos_token error ".to_string()))?;

    let mut sampled = 0;
    for index in 0..to_sample {
        if cancel.is_cancelled() {
            break;
        }
        let input = Tensor::new(&[next_token], &device)
            .map_err(|e| ModelError::Chat(format!("tensor create error {}", e)))?
            .unsqueeze(0)
            .map_err(|e| ModelError::Chat(format!("tensor create unsqueeze error {}", e)))?;
        let logits = model
            .forward(&input, tokens.len() + index)
            .map_err(|e| ModelError::Chat(format!("model forward error {}", e)))?;
        let logits = logits
            .squeeze(0)
            .map_err(|e| ModelError::Chat(format!("tensor squeeze error {}", e)))?;
        let logits = {
            let start_at = all_tokens.len().saturating_sub(repeat_last_n);
            candle_transformers::utils::apply_repeat_penalty(
                &logits,
                repeat_penalty,
                &all_tokens[start_at..],
            )
            .map_err(|e| ModelError::Chat(format!("tensor apply repeat penalty error {}", e)))?
        };
        next_token = logits_processor
            .sample(&logits)
            .map_err(|e| ModelError::Chat(format!("tensor processor sample error {}", e)))?;

        all_tokens.push(next_token);

        if let Some(t) = tos
            .next_token(next_token)
            .map_err(|e| ModelError::Chat(format!("tensor encoding error {}", e)))?
        {
            let messages = token_converter.accept_text(&t)?;
            for message in messages.iter() {
                if let Err(e) = tx.send(Ok(message.clone())).await {
                    error!(error = %e, "send text error");
                    break;
                }
            }
        }
        sampled += 1;
        if next_token == eos_token {
            break;
        };
    }

    if let Some(rest) = tos
        .decode_rest()
        .map_err(|e| ModelError::Chat(format!("tensor decode rest error {}", e)))?
    {
        let messages = token_converter.accept_final_text(&rest)?;
        for message in messages.iter() {
            if let Err(e) = tx.send(Ok(message.clone())).await {
                error!(error = %e, "send text error");
            }
        }
    }

    let _ = tx
        .send(Ok(CompletionEvent::Final {
            prompt_tokens: tokens.len(),
            total_tokens: sampled,
        }))
        .await;
    drop(tx);
    Ok(())
}

#[async_trait]
impl Llm for LlmQwen {
    async fn stream(
        &self,
        request: CompletionRequest,
        cancel: CancellationToken,
    ) -> Pin<Box<dyn Stream<Item = Result<CompletionEvent, AppError>> + Send>> {
        let tokenizer = self.tokenizer.clone();
        let model = self.model.clone();
        let device = self.device.clone();
        let (mut tx, rx) = channel::<Result<CompletionEvent, AppError>>(10);
        thread::spawn(move || {
            block_on(async move {
                if let Err(e) = handle(&request, tokenizer, model, device, tx.clone(), cancel).await
                    && let Err(e) = tx.send(Err(e)).await
                {
                    error!(error = %e, "chat llm error send error");
                };
                drop(tx);
            })
        });
        Box::pin(rx)
    }

    fn calculate_system_prompt_len(&self, system_prompt: &Option<String>) -> u64 {
        create_system_prompt(system_prompt).len() as u64
    }

    fn calculate_tools_prompt_len(&self, tools: &[ToolDef]) -> u64 {
        create_tools_prompt(tools).len() as u64
    }

    fn calculate_message_prompt_len(&self, message: &Message) -> u64 {
        create_message_prompt(message).len() as u64
    }
}

#[cfg(test)]
mod tests {
    use service::chobits::llm::{CompletionRequest, ContentPart, Message, Role, ToolDef};
    use tracing_test::traced_test;

    use crate::llm::model::qwen3::convert_request_to_prompt;

    #[tokio::test]
    #[traced_test]
    /// cargo test --package api --lib -- llm::model::qwen3::tests::test_convert_request_to_prompt_chat_history --show-output
    async fn test_convert_request_to_prompt_chat_history() {
        let system_prompt = "你是一个助手，协助用户进行记录，查询和提供建议，所有回答必须使用纯文本自然语言，禁止使用任何Markdown符号如#、-、*等并且数字使用中文字代替。".to_string();
        let chat_history = vec![
            Message {
                role: Role::User,
                parts: vec![ContentPart::Text(
                    r#"记录一下，小小的电话号码为12349876"#.to_string(),
                )],
            },
            Message {
                role: Role::Assistant,
                parts: vec![ContentPart::Text(r#"小小电话号码为12349876"#.to_string())],
            },
            Message {
                role: Role::User,
                parts: vec![ContentPart::Text(r#"告诉我小小的电话号码"#.to_string())],
            },
        ];
        let request = CompletionRequest {
            preamble: Some(system_prompt),
            messages: chat_history,
            tools: vec![],
            temperature: Some(0.8),
            max_tokens: Some(999),
        };
        let result = convert_request_to_prompt(&request);
        let expect = format!(
            "{}",
            "<|im_start|>system\n你是一个助手，协助用户进行记录，查询和提供建议，所有回答必须使用纯文本自然语言，禁止使用任何Markdown符号如#、-、*等并且数字使用中文字代替。<|im_end|>\n<|im_start|>user\n记录一下，小小的电话号码为12349876<|im_end|>\n<|im_start|>assistant\n小小电话号码为12349876<|im_end|>\n<|im_start|>user\n告诉我小小的电话号码<|im_end|>\n<|im_start|>assistant\n"
        );
        assert_eq!(expect, result);
    }

    #[tokio::test]
    #[traced_test]
    /// cargo test --package api --lib -- ws::llm::models::qwen3::tests::test_convert_request_to_prompt_mcp --show-output
    async fn test_convert_request_to_prompt_mcp() {
        let system_prompt = String::new();
        let tools: Vec<ToolDef> = vec![ToolDef {
            name: "get_current_weather".to_string(),
            description: "Get the current weather in a given location".to_string(),
            input_schema: serde_json::from_str(
                r#"
                {
                    "type": "object",
                    "properties": {
                        "location": {
                            "type": "string",
                            "description":"The city and state, e.g. San Francisco, CA"
                        },
                        "unit": {
                            "type": "string",
                            "enum": ["celsius", "fahrenheit"]
                        }
                    },
                    "required": ["location"]
                }
                "#,
            )
            .unwrap(),
        }];
        let chat_history = vec![Message {
            role: Role::User,
            parts: vec![ContentPart::Text(
                r#"What's the weather like in San Francisco?"#.to_string(),
            )],
        }];
        let request = CompletionRequest {
            preamble: Some(system_prompt),
            messages: chat_history,
            tools,
            temperature: Some(0.8),
            max_tokens: Some(999),
        };
        let _result = convert_request_to_prompt(&request);
    }
}
