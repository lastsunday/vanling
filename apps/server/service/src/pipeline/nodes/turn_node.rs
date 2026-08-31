use std::sync::Arc;

use crate::pipeline::{EventStream, Node, NodeContext, PipelineEvent, ReleaseMode};

/// 回合判定收尾节点：把 ASR 产出的 `TurnText` 包装为 `TurnComplete`（回合边界关闭标记）。
/// 透传其余事件（`SpeechStarted` 供 barge-in 上报、`PartialTranscript` 供前向展示）。
/// 回合边界判定（静音确认 / 静音超时 / transport stall / ListenStop）已由 ASR 节点内部 finish
/// 或 `FinishTurn` 控制事件完成；本节点作为链中显式的边界收尾节点存在。
pub struct TurnNode;

impl TurnNode {
    pub fn new() -> Self {
        Self
    }
}

impl Default for TurnNode {
    fn default() -> Self {
        Self::new()
    }
}

impl Node for TurnNode {
    fn new_instance(&self) -> Arc<dyn Node> {
        Arc::new(TurnNode)
    }

    fn release_mode(&self) -> ReleaseMode {
        ReleaseMode::Immediate
    }

    fn stream(&self, upstream: EventStream, _ctx: &NodeContext) -> EventStream {
        use futures::stream::StreamExt as _;
        Box::pin(upstream.map(|r| match r {
            Ok(PipelineEvent::TurnText { text, prob }) => {
                Ok(PipelineEvent::TurnComplete { text, prob })
            }
            other => other,
        }))
    }
}
