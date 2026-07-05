use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::mpsc::UnboundedReceiver;

use crate::error::AppError;
use crate::record::collector::RecordCollector;
use crate::record::observer::{FrameDirection, SessionEvent};
use crate::ws::output_sender::OutputSender;
use crate::ws::session::round::OutputMessage;
use service::ws::frame::FrameResult;

pub struct OutputProxy {
    output_rx: UnboundedReceiver<OutputMessage>,
    collector: Option<Arc<RecordCollector>>,
    session_id: String,
    seq: u64,
}

impl OutputProxy {
    pub fn new(
        output_rx: UnboundedReceiver<OutputMessage>,
        collector: Option<Arc<RecordCollector>>,
        session_id: String,
    ) -> Self {
        Self {
            output_rx,
            collector,
            session_id,
            seq: 0,
        }
    }

    fn record_frame(&mut self, msg: &OutputMessage) {
        let Some(ref collector) = self.collector else {
            return;
        };

        let detail = match &msg.payload {
            Ok(FrameResult::LLMResult(_)) => "llm",
            Ok(FrameResult::TTSResult(_)) => "tts",
            Ok(FrameResult::STTResult(_)) => "stt",
            Ok(FrameResult::AudioResult(_)) => "audio",
            Ok(FrameResult::HelloResult(_)) => return,
            Ok(FrameResult::CloseResult) => return,
            Ok(FrameResult::McpResult(_)) => return,
            Err(_) => "error",
        };
        let data = match &msg.payload {
            Ok(FrameResult::AudioResult(audio)) => Some(audio.data.clone()),
            _ => None,
        };

        collector.handle_event(SessionEvent::Frame {
            round_id: msg.round_id.clone(),
            session_id: Some(self.session_id.clone()),
            seq: self.seq,
            direction: FrameDirection::Output,
            detail: detail.to_string(),
            data,
        });
        self.seq += 1;
    }
}

#[async_trait]
impl OutputSender for OutputProxy {
    async fn recv(&mut self) -> Option<Result<FrameResult, AppError>> {
        let msg = self.output_rx.recv().await?;
        self.record_frame(&msg);
        Some(msg.payload)
    }
}
