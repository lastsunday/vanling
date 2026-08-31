#[derive(Debug, Clone, PartialEq)]
pub struct Sentence {
    pub text: String,
    pub emotion: Option<String>,
}

/// 空输入语境的类型。由中枢（Session）辨别并赋予，供生成层按语境分级提示。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmptyKind {
    /// push-to-talk：按下但未检出语音。
    Manual,
    /// 唤醒词后首次监听的空输入。
    Wake,
    /// 免提 auto：说了话但 ASR 文本为空（没听清）。
    AutoSpoke,
    /// 免提 auto/realtime 完全静默（VAD 未触发，ASR 无流）。
    Silence,
    /// 回复后连续监听（realtime Speaking→Listening）下的空输入。
    Continuing,
}

pub enum Input {
    Text(String),
    Empty { kind: EmptyKind, count: u32 },
}

impl Input {
    pub fn text(text: impl Into<String>) -> Self {
        Self::Text(text.into())
    }
}

pub enum OutputBlock {
    Text(String),
    Sentence(Sentence),
}
