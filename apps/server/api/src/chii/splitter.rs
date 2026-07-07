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
        let break_char = ["。", "！", "？", "!", "?"];
        let break_char_array_str = break_char.concat();
        let regex = regex::Regex::new(&format!(
            "([^{}]*[{}])([\\s\\S]*)",
            break_char_array_str, break_char_array_str
        ))
        .unwrap();
        let regex_detect = regex::Regex::new(&format!("[{}]", break_char_array_str)).unwrap();
        let clone_text_collector = self.text_collector.clone();
        let mut source = clone_text_collector.as_str();
        while regex_detect.is_match(source) {
            let captures_result = regex.captures(source);
            match captures_result {
                Some(c) => {
                    let (_full, [sentence, other]) = c.extract();
                    result.push(sentence.to_string());
                    source = other;
                }
                None => {
                    break;
                }
            }
        }
        self.text_collector.clear();
        self.text_collector.push_str(source);
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

    use super::filter;

    #[tokio::test]
    #[traced_test]
    /// cargo test --package api --lib -- chat::splitter::tests::test_break_char --show-output
    async fn test_break_char() {
        let break_char = ["。", "！", "？", "!", "?"];
        let break_char_array_str = break_char.concat();
        let regex_str = format!("[{}]", break_char_array_str);
        let regex = regex::Regex::new(&regex_str).unwrap();
        let is_match = regex.is_match(r#"Hello World!1.2.3!"#);
        assert!(is_match);
        let is_match = regex.is_match(r#"Hello World1.2.3"#);
        assert!(!is_match);
        let is_match = regex.is_match(r#""#);
        assert!(!is_match);
    }

    #[tokio::test]
    #[traced_test]
    /// cargo test --package api --lib -- chat::splitter::tests::test_split_sentence --show-output
    async fn test_split_sentence() {
        let break_char = ["。", "！", "？", "!", "?"];
        let break_char_array_str = break_char.concat();
        let regex = regex::Regex::new(&format!(
            "([^{}]*[{}])([\\s\\S]*)",
            break_char_array_str, break_char_array_str
        ))
        .unwrap();
        let (_full, [sentence, other]) = regex
            .captures(r#"Hello World!1.2.3!456"#)
            .map(|caps| caps.extract())
            .unwrap();
        assert_eq!("Hello World!", sentence);
        assert_eq!("1.2.3!456", other);
        let result = regex.captures(r#""#);
        assert!(result.is_none());
    }

    #[tokio::test]
    #[traced_test]
    /// cargo test --package api --lib -- chat::splitter::tests::test_filter --show-output
    async fn test_filter() {
        let result = filter("1+1=2");
        assert_eq!(result, "1+1=2");
    }
}
