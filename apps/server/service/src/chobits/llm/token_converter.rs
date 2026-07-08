use super::{CompletionEvent, LlmError};

use super::ToolCall;

const THINK_TAG_NAME: &str = r#"think"#;
const TOOL_CALL_TAG_NAME: &str = r#"tool_call"#;
const MAX_TAG_NAME_LEN: usize = 9;

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

    fn skip_start_tag<'a>(
        text: &'a str,
        tag_name: &'a str,
    ) -> core::result::Result<&'a str, LlmError> {
        let regex = regex::Regex::new(&format!("<{}>([\\s\\S]*)", tag_name))?;
        let (_full, [other]) = regex.captures(text).map(|caps| caps.extract()).ok_or(
            LlmError::TokenConvertFailure(format!(
                "start tag not exists,tag name = {}, text = {}",
                tag_name, text
            )),
        )?;
        Ok(other)
    }

    fn skip_end_tag_and_get_content<'a>(
        text: &'a str,
        tag_name: &'a str,
    ) -> core::result::Result<(&'a str, &'a str), LlmError> {
        let regex = regex::Regex::new(&format!("([\\s\\S]*?)</{}>+([\\s]*[\\s\\S]*)", tag_name))?;
        let (_full, [tag_content, other_content]) = regex
            .captures(text)
            .map(|caps| caps.extract())
            .ok_or(LlmError::TokenConvertFailure(format!(
                "start tag not exists,tag name = {}, text = {}",
                tag_name, text
            )))?;
        Ok((tag_content, other_content))
    }

    fn analyse_text(&mut self) -> core::result::Result<Vec<CompletionEvent>, LlmError> {
        let mut result: Vec<CompletionEvent> = Vec::new();
        let text = self.text_collector.clone();
        match self.phase {
            Phase::Idle => {
                if self
                    .text_collector
                    .contains(&format!("<{}>", THINK_TAG_NAME))
                {
                    let other = TokenConverter::skip_start_tag(&text, THINK_TAG_NAME)?;
                    self.text_collector.clear();
                    self.text_collector.push_str(other);
                    self.phase = Phase::Thinking;
                } else if self
                    .text_collector
                    .contains(&format!("<{}>", TOOL_CALL_TAG_NAME))
                {
                    let other = TokenConverter::skip_start_tag(&text, TOOL_CALL_TAG_NAME)?;
                    self.text_collector.clear();
                    self.text_collector.push_str(other);
                    self.phase = Phase::ToolCall;
                } else {
                    self.phase = Phase::Text;
                }
            }
            Phase::Thinking => {
                if self
                    .text_collector
                    .contains(&format!("</{}>", THINK_TAG_NAME))
                {
                    let (tag_content, other_content) =
                        TokenConverter::skip_end_tag_and_get_content(&text, THINK_TAG_NAME)?;
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
                if self
                    .text_collector
                    .contains(&format!("</{}>", TOOL_CALL_TAG_NAME))
                {
                    let (tag_content, other_content) =
                        TokenConverter::skip_end_tag_and_get_content(&text, TOOL_CALL_TAG_NAME)?;
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
                } else {
                    //skip
                }
            }
            Phase::Text => {
                result.push(CompletionEvent::Text(self.text_collector.to_string()));
                self.text_collector.clear();
            }
        }
        let text = &self.text_collector;
        if text.contains(&format!("<{}>", THINK_TAG_NAME))
            || text.contains(&format!("</{}>", THINK_TAG_NAME))
            || text.contains(&format!("<{}>", TOOL_CALL_TAG_NAME))
            || text.contains(&format!("</{}>", TOOL_CALL_TAG_NAME))
        {
            result.append(&mut self.analyse_text()?);
        }
        Ok(result)
    }

    pub fn accept_text(
        &mut self,
        text: &str,
    ) -> core::result::Result<Vec<CompletionEvent>, LlmError> {
        self.text_collector.push_str(text);
        if self.text_collector.len() >= MAX_TAG_NAME_LEN + 2 {
            self.analyse_text()
        } else {
            Ok(vec![])
        }
    }

    pub fn accept_final_text(
        &mut self,
        text: &str,
    ) -> core::result::Result<Vec<CompletionEvent>, LlmError> {
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
