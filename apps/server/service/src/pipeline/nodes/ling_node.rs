use std::pin::Pin;
use std::sync::Arc;

use futures::{Stream, StreamExt};

use crate::ling::Ling;
use crate::pipeline::{EventStream, Node, NodeContext, PipelineEvent, ReleaseMode};
use crate::types::{Input, OutputBlock};
use framework::error::AppError;
use tokio_util::sync::CancellationToken;

/// 灵节点：由 `TurnComplete`/`TurnText`（文本直达）触发 LLM 流，流式产出 `TextChunk`。
/// LLM 运行错误以 `Err` 透传，供 Round 判定无可用输出。
pub struct LingNode {
    ling: Arc<dyn Ling>,
}

impl LingNode {
    pub fn new(ling: Arc<dyn Ling>) -> Self {
        Self { ling }
    }

    fn ask_llm(ling: Arc<dyn Ling>, cancel: CancellationToken, input: Input) -> EventStream {
        type LlmStream =
            Pin<Box<dyn Stream<Item = Result<OutputBlock, AppError>> + Send + 'static>>;
        let inner: Option<LlmStream> = None;
        Box::pin(futures::stream::unfold(
            (cancel, inner, ling, Some(input)),
            |(cancel, mut inner, ling, mut input)| async move {
                loop {
                    if let Some(stream) = inner.as_mut() {
                        match stream.next().await {
                            Some(Ok(OutputBlock::Sentence(sentence))) => {
                                return Some((
                                    Ok(PipelineEvent::TextChunk {
                                        text: sentence.text,
                                        emotion: sentence.emotion,
                                    }),
                                    (cancel, inner, ling, input),
                                ));
                            }
                            Some(Ok(OutputBlock::Text(s))) => {
                                return Some((
                                    Ok(PipelineEvent::TextChunk {
                                        text: s,
                                        emotion: None,
                                    }),
                                    (cancel, inner, ling, input),
                                ));
                            }
                            Some(Err(e)) => {
                                return Some((Err(e), (cancel, inner, ling, input)));
                            }
                            None => return None,
                        }
                    } else if cancel.is_cancelled() {
                        return None;
                    } else {
                        let input = input.take().expect("input consumed once");
                        let stream = ling.ask(input, cancel.clone()).await;
                        inner = Some(stream);
                    }
                }
            },
        ))
    }
}

impl Node for LingNode {
    fn new_instance(&self) -> Arc<dyn Node> {
        Arc::new(LingNode::new(self.ling.clone()))
    }

    fn release_mode(&self) -> ReleaseMode {
        ReleaseMode::Immediate
    }

    fn stream(&self, upstream: EventStream, ctx: &NodeContext) -> EventStream {
        let cancel = ctx.cancel.clone();
        let ling = self.ling.clone();
        Box::pin(upstream.flat_map(move |r| -> EventStream {
            match r {
                Ok(PipelineEvent::TurnComplete { text, .. })
                | Ok(PipelineEvent::TurnText { text, .. }) => {
                    Self::ask_llm(ling.clone(), cancel.clone(), Input::Text(text))
                }
                // 对话 Act：中枢注入的提示语指令；原始 EmptyInput 仅透传。
                Ok(PipelineEvent::Prompt { kind, count }) => {
                    Self::ask_llm(ling.clone(), cancel.clone(), Input::Empty { kind, count })
                }
                Ok(other) => Box::pin(futures::stream::iter([Ok(other)])),
                Err(e) => Box::pin(futures::stream::iter([Err(e)])),
            }
        }))
    }
}
