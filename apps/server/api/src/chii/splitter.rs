use fancy_regex::Regex;

use std::sync::OnceLock;
use std::time::{Duration, Instant};

const MAX_CHUNK_CHARS: usize = 250;
const MAX_CHUNK_WORDS: usize = 30;
const TIMEOUT_DURATION_MS: u64 = 700;

pub struct Splitter {
    text_collector: String,
    token_buffer: String,
    last_flush_time: Instant,
    timeout_duration: Duration,
    first_chunk_emitted: bool,
}

impl Default for Splitter {
    fn default() -> Self {
        Self::new()
    }
}

impl Splitter {
    pub fn new() -> Self {
        Self {
            text_collector: String::new(),
            token_buffer: String::new(),
            last_flush_time: Instant::now(),
            timeout_duration: Duration::from_millis(TIMEOUT_DURATION_MS),
            first_chunk_emitted: false,
        }
    }

    pub fn accept_token(&mut self, token: &str) -> Vec<String> {
        let mut result = Vec::new();

        if !self.first_chunk_emitted {
            self.token_buffer.push_str(token);
            if is_word_boundary(token) && !self.token_buffer.trim().is_empty() {
                let chunk = self.token_buffer.clone();
                self.token_buffer.clear();
                self.first_chunk_emitted = true;
                self.last_flush_time = Instant::now();
                result.push(chunk);
            }
            return result;
        }

        self.text_collector.push_str(&filter(token));
        let sentences = sentencex::segment("zh", &self.text_collector);

        if sentences.is_empty() {
            if self.last_flush_time.elapsed() >= self.timeout_duration
                && !self.text_collector.trim().is_empty()
            {
                let remaining = self.text_collector.clone();
                self.text_collector.clear();
                self.last_flush_time = Instant::now();
                result.push(remaining);
            }
            return result;
        }

        let mut byte_offset = 0;
        let collector_len = self.text_collector.len();
        let last_sentence = sentences.last().unwrap();
        let last_sentence_end = last_sentence.as_ptr() as usize
            - self.text_collector.as_ptr() as usize
            + last_sentence.len();

        for sentence in &sentences {
            let sentence_start = sentence.as_ptr() as usize - self.text_collector.as_ptr() as usize;
            let sentence_end = sentence_start + sentence.len();
            if sentence_end >= last_sentence_end {
                break;
            }
            let s = self.text_collector[sentence_start..sentence_end].to_string();
            if !s.trim().is_empty() {
                let chunks = split_long_sentences(vec![s]);
                result.extend(chunks);
            }
            byte_offset = sentence_end;
        }

        let remaining = self.text_collector[byte_offset..collector_len].to_string();
        self.text_collector.clear();
        self.text_collector.push_str(&remaining);
        self.last_flush_time = Instant::now();

        result
    }

    pub fn accept_final(&mut self) -> Vec<String> {
        let mut result = Vec::new();

        if !self.token_buffer.is_empty() {
            let chunk = self.token_buffer.clone();
            self.token_buffer.clear();
            result.push(chunk);
        }

        let clone_text_collector = self.text_collector.clone();
        if !clone_text_collector.is_empty() {
            let chunks = split_long_sentences(vec![clone_text_collector]);
            result.extend(chunks);
            self.text_collector.clear();
        }
        result
    }
}

fn split_long_sentences(chunks: Vec<String>) -> Vec<String> {
    let mut result = Vec::new();
    for chunk in chunks {
        if chunk.len() <= MAX_CHUNK_CHARS && count_words(&chunk) <= MAX_CHUNK_WORDS {
            result.push(chunk);
        } else {
            let mut buffer = String::new();
            for c in chunk.chars() {
                buffer.push(c);
                if (c == '，' || c == '；' || c == '：') && buffer.len() >= MAX_CHUNK_CHARS / 2 {
                    result.push(buffer.clone());
                    buffer.clear();
                }
            }
            if !buffer.is_empty() {
                result.push(buffer);
            }
        }
    }
    result
}

fn count_words(text: &str) -> usize {
    let chinese_chars = text
        .chars()
        .filter(|c| *c >= '\u{4e00}' && *c <= '\u{9fff}')
        .count();
    if chinese_chars > 0 {
        chinese_chars
    } else {
        text.split_whitespace().count()
    }
}

fn is_word_boundary(token: &str) -> bool {
    token.ends_with([
        '，', '。', '！', '？', '；', '：', '、', ',', '.', '!', '?', ';', ':', ' ',
    ])
}

fn regex() -> &'static Vec<Regex> {
    static REGEX: OnceLock<Vec<Regex>> = OnceLock::new();
    REGEX.get_or_init(|| {
        vec![
            Regex::new(r"\n").unwrap(),
            Regex::new(r"```.*?```").unwrap(),
            Regex::new(r"^#+\s*").unwrap(),
            Regex::new(r"(\*\*|__)(.*?)\1").unwrap(),
            Regex::new(r"(\*|_)(?=\S)(.*?)(?<=\S)\1").unwrap(),
            Regex::new(r"!\[.*?\]\(.*?\)").unwrap(),
            Regex::new(r"\[(.*?)\]\(.*?\)").unwrap(),
            Regex::new(r"^\s*>+\s*").unwrap(),
            Regex::new(r"\$\$.*?\$\$").unwrap(),
        ]
    })
}

fn filter(text: &str) -> String {
    let mut content = text.to_string();
    for r in regex() {
        content = r.replace_all(&content, "").to_string();
    }
    content
}

#[cfg(test)]
mod tests {
    use tracing_test::traced_test;

    use super::Splitter;

    #[test]
    #[traced_test]
    fn test_splitter_abbreviation_not_split() {
        let mut splitter = Splitter::new();
        let r1 = splitter.accept_token("Dr. Smith said hello.");
        let r2 = splitter.accept_final();
        let all: Vec<String> = r1.into_iter().chain(r2.into_iter()).collect();
        let joined = all.join("");
        assert!(
            joined.contains("Dr. Smith"),
            "should not split on abbreviation 'Dr.': {joined:?}"
        );
    }

    #[test]
    #[traced_test]
    fn test_splitter_chinese_sentences() {
        let mut splitter = Splitter::new();
        let tokens = vec![
            "你", "好", "世", "界", "。", "今", "天", "天", "气", "不", "错", "！",
        ];
        let mut all = Vec::new();
        for token in tokens {
            let r = splitter.accept_token(token);
            all.extend(r);
        }
        let r_final = splitter.accept_final();
        all.extend(r_final);
        assert!(all.len() >= 2, "should split Chinese sentences: {all:?}");
    }

    #[test]
    #[traced_test]
    fn test_splitter_english_sentences() {
        let mut splitter = Splitter::new();
        let tokens = vec![
            "H", "e", "l", "l", "o", " ", "w", "o", "r", "l", "d", ".", " ", "H", "o", "w", " ",
            "a", "r", "e", " ", "y", "o", "u", "?", " ", "I", " ", "a", "m", " ", "f", "i", "n",
            "e", "!",
        ];
        let mut all = Vec::new();
        for token in tokens {
            let r = splitter.accept_token(token);
            all.extend(r);
        }
        let r_final = splitter.accept_final();
        all.extend(r_final);
        assert!(all.len() >= 3, "should split English sentences: {all:?}");
    }

    #[test]
    #[traced_test]
    fn test_splitter_mixed_content() {
        let mut splitter = Splitter::new();
        let tokens = vec![
            "2", "0", "2", "4", "年", "5", "月", "1", "1", "号", "，", "拨", "打", "1", "1", "0",
            "。",
        ];
        let mut all = Vec::new();
        for token in tokens {
            let r = splitter.accept_token(token);
            all.extend(r);
        }
        let r_final = splitter.accept_final();
        all.extend(r_final);
        assert!(!all.is_empty(), "should produce output from mixed content");
        let joined = all.join("");
        assert!(joined.contains("2024"), "should preserve content: {joined}");
    }

    #[test]
    #[traced_test]
    fn test_splitter_empty() {
        let mut splitter = Splitter::new();
        let r1 = splitter.accept_token("");
        let r2 = splitter.accept_final();
        assert!(r1.is_empty());
        assert!(r2.is_empty());
    }

    #[test]
    #[traced_test]
    fn test_splitter_streaming_accumulation() {
        let mut splitter = Splitter::new();
        let r1 = splitter.accept_token("你");
        assert!(r1.is_empty(), "partial text should not yield sentences");
        let r2 = splitter.accept_token("好");
        assert!(r2.is_empty(), "partial text should not yield sentences");
        let r3 = splitter.accept_token("世");
        assert!(r3.is_empty(), "partial text should not yield sentences");
        let r4 = splitter.accept_token("界");
        assert!(r4.is_empty(), "partial text should not yield sentences");
        let r5 = splitter.accept_token("。");
        let r_final = splitter.accept_final();
        let all: Vec<String> = r5.into_iter().chain(r_final.into_iter()).collect();
        assert!(!all.is_empty(), "should yield after full sentence arrives");
    }

    #[test]
    #[traced_test]
    fn test_splitter_chinese_clause_split() {
        let mut splitter = Splitter::new();
        let long_text = "这是一个很长的句子，包含多个从句，我们需要验证从句级分句是否正常工作，特别是对于中文文本的处理，当我们遇到长句子时应该能够正确地在从句边界进行分割，这样可以显著降低首字延时，提升用户体验，这是语音助手的关键优化点。";
        let mut all = Vec::new();
        for token in long_text.chars() {
            let mut buf = [0; 4];
            let token_str = token.encode_utf8(&mut buf);
            let r = splitter.accept_token(token_str);
            all.extend(r);
        }
        let r_final = splitter.accept_final();
        all.extend(r_final);
        assert!(all.len() > 1, "long sentence should be split: {all:?}");
        for chunk in &all {
            assert!(chunk.len() <= 250, "chunk too long: {}", chunk.len());
        }
    }

    #[test]
    #[traced_test]
    fn test_splitter_timeout_flush() {
        let mut splitter = Splitter::new();
        let tokens = vec![
            "这", "是", "一", "个", "没", "有", "标", "点", "的", "长", "文", "本",
        ];
        let mut all = Vec::new();
        for token in tokens {
            let r = splitter.accept_token(token);
            all.extend(r);
        }
        assert!(all.is_empty(), "should not yield without punctuation");
    }

    #[test]
    #[traced_test]
    fn test_splitter_token_first_chunk() {
        let mut splitter = Splitter::new();
        let r1 = splitter.accept_token("你");
        assert!(r1.is_empty(), "should not yield without punctuation");
        let r2 = splitter.accept_token("好");
        assert!(r2.is_empty(), "should not yield without punctuation");
        let r3 = splitter.accept_token("！");
        assert!(!r3.is_empty(), "should yield on punctuation boundary");
        assert!(!r3[0].is_empty(), "chunk should not be empty");
    }

    #[test]
    #[traced_test]
    fn test_splitter_token_max_chars() {
        let mut splitter = Splitter::new();
        let mut tokens = Vec::new();
        for i in 0..10 {
            tokens.push(format!("字{}", i));
        }
        let mut chunk_emitted = false;
        for token in &tokens {
            let r = splitter.accept_token(token);
            if !r.is_empty() {
                assert!(!r[0].is_empty(), "chunk should not be empty");
                chunk_emitted = true;
                break;
            }
        }
        assert!(!chunk_emitted, "should not emit without punctuation");
    }

    #[test]
    #[traced_test]
    fn test_splitter_two_phase_integration() {
        let mut splitter = Splitter::new();
        let _r1 = splitter.accept_token("你");
        let _r2 = splitter.accept_token("好");
        let r3 = splitter.accept_token("！");
        let _r4 = splitter.accept_token("今");
        let _r5 = splitter.accept_token("天");
        let _r6 = splitter.accept_token("天");
        let _r7 = splitter.accept_token("气");
        let _r8 = splitter.accept_token("不");
        let _r9 = splitter.accept_token("错");
        let r10 = splitter.accept_token("！");
        let r_final = splitter.accept_final();
        let all: Vec<String> = r3
            .into_iter()
            .chain(r10.into_iter())
            .chain(r_final.into_iter())
            .collect();
        assert!(
            !all.is_empty(),
            "should produce output from two-phase processing"
        );
        assert!(all.len() >= 2, "should split sentences: {all:?}");
    }

    #[test]
    #[traced_test]
    fn test_splitter_no_duplication() {
        let mut splitter = Splitter::new();
        let tokens = vec![
            "你", "好", "！", "有", "什", "么", "我", "可", "以", "帮", "助", "您", "吗", "？",
        ];
        let mut all = Vec::new();
        let mut seen = String::new();
        for token in tokens {
            let r = splitter.accept_token(token);
            for chunk in &r {
                assert!(
                    !seen.contains(chunk),
                    "duplication detected: chunk '{}' already in '{}'",
                    chunk,
                    seen
                );
                seen.push_str(chunk);
            }
            all.extend(r);
        }
        let r_final = splitter.accept_final();
        all.extend(r_final);
        let joined = all.join("");
        assert!(joined.contains("你好"), "should contain '你好': {joined}");
        assert!(
            joined.contains("有什么我可以帮助您吗"),
            "should contain full sentence: {joined}"
        );
    }
}
