use std::collections::VecDeque;
use std::sync::Arc;

use futures::StreamExt;

use crate::component::tts::Tts;
use crate::pipeline::{EventStream, Node, NodeCapability, NodeContext, PipelineEvent, ReleaseMode};
use framework::error::AppError;

/// 声节点：将上游 LLM 文本流（`TextChunk`）经 TTS 引擎变换为 `AudioOut` 事件流。
/// emotion 以 FIFO 顺序与 TTS 句子配对；上游 `Err` 与 TTS 编码错误均透传。
pub struct TtsNode {
    tts: Arc<dyn Tts>,
}

impl TtsNode {
    pub fn new(tts: Arc<dyn Tts>) -> Self {
        Self { tts }
    }
}

impl Node for TtsNode {
    fn new_instance(&self) -> Arc<dyn Node> {
        Arc::new(TtsNode::new(self.tts.clone()))
    }

    fn capabilities(&self) -> Vec<Box<NodeCapability>> {
        vec![Box::new(self.tts.audio_spec())]
    }

    fn release_mode(&self) -> ReleaseMode {
        ReleaseMode::Immediate
    }

    /// 流式变换：消费上游事件流（LLM `TextChunk` 或透传），产出 `TextChunk` + `AudioOut`。
    fn stream(&self, upstream: EventStream, ctx: &NodeContext) -> EventStream {
        let cancel = ctx.cancel.clone();
        let chunks = upstream;
        let (mut feed_tx, feed_rx) = futures_channel::mpsc::channel::<String>(1024);
        let (out_tx, out_rx) =
            futures_channel::mpsc::channel::<Result<PipelineEvent, AppError>>(1024);

        let tts = self.tts.clone();
        let cancel_for_a = cancel.clone();

        tokio::spawn(async move {
            let mut tts_out = tts.stream(Box::pin(feed_rx), cancel_for_a).await;
            let mut emotion_queue: VecDeque<Option<String>> = VecDeque::new();
            let mut current_text: Option<String> = None;
            let mut current_emotion: Option<Option<String>> = None;
            let mut feeding = true;

            let mut chunks = chunks;
            loop {
                tokio::select! {
                    biased;
                    maybe = chunks.next(), if feeding => {
                        match maybe {
                            Some(Ok(PipelineEvent::TextChunk { text, emotion })) => {
                                emotion_queue.push_back(emotion);
                                if feed_tx.clone().try_send(text).is_err() {
                                    feeding = false;
                                }
                            }
                            Some(Ok(other)) => {
                                if out_tx.clone().try_send(Ok(other)).is_err() {
                                    break;
                                }
                            }
                            Some(Err(e)) => {
                                let _ = out_tx.clone().try_send(Err(e));
                                feeding = false;
                            }
                            None => {
                                feeding = false;
                            }
                        }
                        if !feeding {
                            feed_tx.close_channel();
                        }
                    }
                    maybe_packet = tts_out.next() => {
                        let Some(packet) = maybe_packet else { break };
                        if cancel.is_cancelled() {
                            break;
                        }
                        match packet {
                            Ok(packet) => {
                                let text: String = packet.text.to_string();
                                if current_text.as_deref() != Some(text.as_str()) {
                                    current_text = Some(text.clone());
                                    current_emotion = emotion_queue.pop_front();
                                }
                                let emotion = current_emotion.clone().flatten();
                                if out_tx
                                    .clone()
                                    .try_send(Ok(PipelineEvent::TextChunk {
                                        text: text.clone(),
                                        emotion: emotion.clone(),
                                    }))
                                    .is_err()
                                {
                                    break;
                                }
                                if out_tx
                                    .clone()
                                    .try_send(Ok(PipelineEvent::AudioOut {
                                        audio: packet.audio,
                                        is_first: packet.is_first,
                                        is_last: packet.is_last,
                                    }))
                                    .is_err()
                                {
                                    break;
                                }
                            }
                            Err(e) => {
                                let _ = out_tx.clone().try_send(Err(e));
                                break;
                            }
                        }
                    }
                }
            }
        });

        Box::pin(out_rx)
    }
}
