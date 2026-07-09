#[derive(Debug, Clone)]
pub enum ChatMessage {
    User(String),
    Assistant(String),
    ToolResult(String),
}

#[derive(Debug, Clone)]
pub struct History {
    pub preamble: Option<String>,
    pub messages: Vec<ChatMessage>,
}

impl History {
    pub fn new(preamble: Option<String>) -> Self {
        Self {
            preamble,
            messages: Vec::new(),
        }
    }

    pub fn add_user(&mut self, text: String) {
        self.messages.push(ChatMessage::User(text));
    }

    pub fn add_assistant(&mut self, text: String) {
        self.messages.push(ChatMessage::Assistant(text));
    }

    pub fn add_tool_result(&mut self, text: String) {
        self.messages.push(ChatMessage::ToolResult(text));
    }
}
