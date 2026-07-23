use fancy_regex::Regex;

use std::sync::OnceLock;

#[derive(Default)]
pub struct Splitter {
    text_collector: String,
}

impl Splitter {
    pub fn new() -> Self {
        Self {
            text_collector: String::new(),
        }
    }

    pub fn accept_text(&mut self, text: &str) -> Vec<String> {
        let mut result = Vec::new();
        self.text_collector.push_str(&filter(text));

        let sentences = sentencex::segment("en", &self.text_collector);

        if sentences.is_empty() {
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
                result.push(s);
            }
            byte_offset = sentence_end;
        }

        let remaining = self.text_collector[byte_offset..collector_len].to_string();
        self.text_collector.clear();
        self.text_collector.push_str(&remaining);

        result
    }

    pub fn accept_final(&mut self) -> Vec<String> {
        let mut result = Vec::new();
        let clone_text_collector = self.text_collector.clone();
        if !clone_text_collector.is_empty() {
            result.push(clone_text_collector);
            self.text_collector.clear();
        }
        result
    }
}

fn regex() -> &'static Vec<Regex> {
    static REGEX: OnceLock<Vec<Regex>> = OnceLock::new();
    REGEX.get_or_init(|| {
        vec![
            Regex::new(r"\n").unwrap(),                         //换行
            Regex::new(r"```.*?```").unwrap(),                  //代码块
            Regex::new(r"^#+\s*").unwrap(),                     //标题
            Regex::new(r"(\*\*|__)(.*?)\1").unwrap(),           //粗体
            Regex::new(r"(\*|_)(?=\S)(.*?)(?<=\S)\1").unwrap(), //斜体
            Regex::new(r"!\[.*?\]\(.*?\)").unwrap(),            //图片
            Regex::new(r"\[(.*?)\]\(.*?\)").unwrap(),           //链接
            Regex::new(r"^\s*>+\s*").unwrap(),                  //引用
            Regex::new(r"\$\$.*?\$\$").unwrap(),                //块级公式
                                                                // TODO: 列表
        ]
    })
}

pub fn filter(text: &str) -> String {
    let mut content = String::from(text);
    let regex = regex().iter();
    for r in regex {
        content = String::from(r.replace_all(&content, ""));
    }
    content.to_string()
}

#[cfg(test)]
mod tests {
    use tracing_test::traced_test;

    use super::Splitter;

    #[test]
    #[traced_test]
    fn test_splitter_abbreviation_not_split() {
        let mut splitter = Splitter::new();
        let r1 = splitter.accept_text("Dr. Smith said hello.");
        let r2 = splitter.accept_final();
        let all: Vec<&str> = r1
            .iter()
            .chain(r2.iter())
            .flat_map(|s| s.split('\n'))
            .collect();
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
        let r1 = splitter.accept_text("你好世界。今天天气不错！");
        let r2 = splitter.accept_final();
        let all: Vec<String> = r1.into_iter().chain(r2.into_iter()).collect();
        let count = all.len();
        assert!(count >= 2, "should split Chinese sentences: {all:?}");
    }

    #[test]
    #[traced_test]
    fn test_splitter_english_sentences() {
        let mut splitter = Splitter::new();
        let r1 = splitter.accept_text("Hello world. How are you? I am fine!");
        let r2 = splitter.accept_final();
        let all: Vec<String> = r1.into_iter().chain(r2.into_iter()).collect();
        let count = all.len();
        assert!(count >= 3, "should split English sentences: {all:?}");
    }

    #[test]
    #[traced_test]
    fn test_splitter_mixed_content() {
        let mut splitter = Splitter::new();
        let r1 = splitter.accept_text("2024年5月11号，拨打110。");
        let r2 = splitter.accept_final();
        let all: Vec<String> = r1.into_iter().chain(r2.into_iter()).collect();
        assert!(!all.is_empty(), "should produce output from mixed content");
        let joined = all.join("");
        assert!(joined.contains("2024"), "should preserve content: {joined}");
    }

    #[test]
    #[traced_test]
    fn test_splitter_empty() {
        let mut splitter = Splitter::new();
        let r1 = splitter.accept_text("");
        let r2 = splitter.accept_final();
        assert!(r1.is_empty());
        assert!(r2.is_empty());
    }

    #[test]
    #[traced_test]
    fn test_splitter_streaming_accumulation() {
        let mut splitter = Splitter::new();
        let r1 = splitter.accept_text("你好");
        assert!(r1.is_empty(), "partial text should not yield sentences");
        let r2 = splitter.accept_text("世界。");
        let r3 = splitter.accept_final();
        let all: Vec<String> = r2.into_iter().chain(r3.into_iter()).collect();
        assert!(!all.is_empty(), "should yield after full sentence arrives");
    }
}
