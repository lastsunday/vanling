use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use service::ws::frame::Frame;
use tokio::sync::mpsc::UnboundedSender;

use crate::record::collector::RecordCollector;
use crate::record::observer::{FrameDirection, SessionEvent};
use crate::ws::input_sender::InputSender;

pub struct InputProxy {
    session_id: String,
    collector: Option<Arc<RecordCollector>>,
    seq: AtomicU64,
    input_tx: UnboundedSender<Frame>,
}

impl InputProxy {
    pub fn new(
        session_id: String,
        collector: Option<Arc<RecordCollector>>,
        input_tx: UnboundedSender<Frame>,
    ) -> Self {
        Self {
            session_id,
            collector,
            seq: AtomicU64::new(1),
            input_tx,
        }
    }

    pub fn send(&self, msg: Frame) {
        if let Frame::Voice { data } = &msg
            && let Some(ref collector) = self.collector
        {
            let seq = self.seq.fetch_add(1, Ordering::Relaxed);
            collector.handle_event(SessionEvent::Frame {
                round_id: None,
                session_id: Some(self.session_id.clone()),
                seq,
                direction: FrameDirection::Input,
                detail: "voice".to_string(),
                data: Some(data.clone()),
            });
        }
        let _ = self.input_tx.send(msg);
    }
}

impl InputSender for InputProxy {
    fn send(&self, msg: Frame) {
        InputProxy::send(self, msg);
    }
}
