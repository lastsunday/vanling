use std::sync::LazyLock;

use regex::Regex;

use super::{CompletionEvent, LlmError, ToolCall};

const THINK_OPEN: &str = " thinking";
const THINK_CLOSE: &str = " response";
const TOOL_CALL_OPEN: &str = "<tool_call>";
const TOOL_CALL_CLOSE: &str = "</tool_call>";
const MAX_TAG_NAME_LEN: usize = 9;

static START_THINK: LazyLock<Regex> = LazyLock::new(|| Regex::new(r" thinking([\s\S]*)").unwrap());
static START_TOOL_CALL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"<tool_call>([\s\S]*)").unwrap());
static END_THINK: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"([\s\S]*?) response+([\s]*[\s\S]*)").unwrap());
static END_TOOL_CALL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"([\s\S]*?)</tool_call>+([\s]*[\s\S]*)").unwrap());

#[derive(Default)]
pub struct TokenConverter {
    phase: Phase,
    text_collector: String,
}

impl TokenConverter {
    pub fn new() -> Self {
        Self {
            phase: Phase::Idle,
            text_collector: String::new(),
        }
    }

    fn skip_start_tag<'a>(text: &'a str, regex: &Regex) -> Result<&'a str, LlmError> {
        let (_full, [other]) =
            regex
                .captures(text)
                .map(|caps| caps.extract())
                .ok_or_else(|| {
                    LlmError::TokenConvertFailure(format!("start tag not exists, text = {text}"))
                })?;
        Ok(other)
    }

    fn skip_end_tag<'a>(text: &'a str, regex: &Regex) -> Result<(&'a str, &'a str), LlmError> {
        let (_full, [tag_content, other_content]) = regex
            .captures(text)
            .map(|caps| caps.extract())
            .ok_or_else(|| {
                LlmError::TokenConvertFailure(format!("end tag not found, text = {text}"))
            })?;
        Ok((tag_content, other_content))
    }

    fn analyse_text(&mut self) -> Result<Vec<CompletionEvent>, LlmError> {
        let mut result: Vec<CompletionEvent> = Vec::new();
        let text = self.text_collector.clone();
        match self.phase {
            Phase::Idle => {
                if self.text_collector.contains(THINK_OPEN) {
                    let other = TokenConverter::skip_start_tag(&text, &START_THINK)?;
                    self.text_collector.clear();
                    self.text_collector.push_str(other);
                    self.phase = Phase::Thinking;
                } else if self.text_collector.contains(TOOL_CALL_OPEN) {
                    let other = TokenConverter::skip_start_tag(&text, &START_TOOL_CALL)?;
                    self.text_collector.clear();
                    self.text_collector.push_str(other);
                    self.phase = Phase::ToolCall;
                } else {
                    self.phase = Phase::Text;
                }
            }
            Phase::Thinking => {
                if self.text_collector.contains(THINK_CLOSE) {
                    let (tag_content, other_content) =
                        TokenConverter::skip_end_tag(&text, &END_THINK)?;
                    result.push(CompletionEvent::Reasoning(tag_content.to_string()));
                    self.text_collector.clear();
                    self.text_collector.push_str(other_content);
                    self.phase = Phase::Idle;
                } else {
                    result.push(CompletionEvent::Reasoning(text));
                    self.text_collector.clear();
                }
            }
            Phase::ToolCall => {
                if self.text_collector.contains(TOOL_CALL_CLOSE) {
                    let (tag_content, other_content) =
                        TokenConverter::skip_end_tag(&text, &END_TOOL_CALL)?;
                    let tool_call: serde_json::error::Result<ToolCall> =
                        serde_json::from_str(tag_content);
                    match tool_call {
                        Ok(tool_call) => {
                            result.push(CompletionEvent::ToolCall {
                                id: framework::id::gen_id(),
                                name: tool_call.name,
                                arguments: tool_call.arguments,
                            });
                        }
                        Err(e) => {
                            return Err(LlmError::TokenConvertFailure(format!(
                                "{:?} : {}",
                                e, tag_content
                            )));
                        }
                    }
                    self.text_collector.clear();
                    self.text_collector.push_str(other_content);
                    self.phase = Phase::Idle;
                }
            }
            Phase::Text => {
                result.push(CompletionEvent::Text(self.text_collector.to_string()));
                self.text_collector.clear();
            }
        }
        let text = &self.text_collector;
        if text.contains(THINK_OPEN)
            || text.contains(THINK_CLOSE)
            || text.contains(TOOL_CALL_OPEN)
            || text.contains(TOOL_CALL_CLOSE)
        {
            result.append(&mut self.analyse_text()?);
        }
        Ok(result)
    }

    pub fn accept_text(&mut self, text: &str) -> Result<Vec<CompletionEvent>, LlmError> {
        self.text_collector.push_str(text);
        if self.text_collector.len() >= MAX_TAG_NAME_LEN + 2 {
            self.analyse_text()
        } else {
            Ok(vec![])
        }
    }

    pub fn accept_final_text(&mut self, text: &str) -> Result<Vec<CompletionEvent>, LlmError> {
        self.text_collector.push_str(text);
        self.analyse_text()
    }
}

#[derive(Default, PartialEq, Eq)]
enum Phase {
    #[default]
    Idle,
    Thinking,
    ToolCall,
    Text,
}

impl From<regex::Error> for LlmError {
    fn from(value: regex::Error) -> Self {
        LlmError::TokenConvertFailure(value.to_string())
    }
}
