#[cfg(test)]
mod tests {
    use api::chii::Splitter;
    use tracing::info;
    use tracing_test::traced_test;

    #[tokio::test]
    #[traced_test]
    async fn test_chat_sentence() {
        let mut splitter = Splitter::new();
        // Feed text as one token
        let sentences = splitter.accept_token("Hello,World!My name is");
        // The entire string is > 30 chars, so it should be emitted as first chunk
        assert!(!sentences.is_empty(), "should emit first chunk");

        let sentences = splitter.accept_token("lastsunday。I like rust。I want a chobits ");
        // Should split at 。 boundaries
        let total_sentences: usize = sentences.len();

        let final_sentences = splitter.accept_final();
        let total = total_sentences + final_sentences.len();
        assert!(total >= 2, "should split sentences: total={total}");
    }

    #[tokio::test]
    #[traced_test]
    async fn test_chat_sentence_new_line() {
        let mut splitter = Splitter::new();
        let sentences = splitter.accept_token("\n");
        assert_eq!(0, sentences.len());
        let sentences = splitter.accept_token("+1");
        assert_eq!(0, sentences.len());
        let sentences = splitter.accept_token("=2");
        assert_eq!(0, sentences.len());
        let sentences = splitter.accept_final();
        for msg in sentences.iter() {
            info!("{}", msg);
        }
    }
}
