use super::round::OutputMessage;
use chrono::Local;
use service::chobits::message::tts::TtsState;
use service::ws::frame::FrameResult;
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use tokio::sync::mpsc::{Sender, UnboundedReceiver};
use tokio::time::{Duration, Instant, MissedTickBehavior, interval_at};

pub struct OutputController {
    input_rx: UnboundedReceiver<OutputMessage>,
    output_tx: Sender<OutputMessage>,
    epoch: Arc<AtomicU64>,
    latest_activity_time: Arc<AtomicI64>,
    interval: Option<tokio::time::Interval>,
    frame_duration: u64,
}

impl OutputController {
    pub fn new(
        input_rx: UnboundedReceiver<OutputMessage>,
        output_tx: Sender<OutputMessage>,
        epoch: Arc<AtomicU64>,
        latest_activity_time: Arc<AtomicI64>,
        frame_duration: u64,
    ) -> Self {
        Self {
            input_rx,
            output_tx,
            epoch,
            latest_activity_time,
            interval: None,
            frame_duration,
        }
    }

    pub fn spawn(mut self) {
        tokio::spawn(async move {
            self.run().await;
        });
    }

    async fn run(&mut self) {
        while let Some(msg) = self.input_rx.recv().await {
            let current_epoch = self.epoch.load(Ordering::Acquire);
            if msg.epoch != 0 && msg.epoch < current_epoch {
                continue;
            }
            if let Ok(FrameResult::TTSResult(ref t)) = msg.payload
                && t.state == Some(TtsState::Start)
            {
                self.interval = None;
            }
            if let Ok(FrameResult::AudioResult(_)) = msg.payload {
                self.pace_audio().await;
            }
            self.latest_activity_time
                .store(Local::now().timestamp_millis(), Ordering::Release);
            if self.output_tx.send(msg).await.is_err() {
                break;
            }
        }
    }

    async fn pace_audio(&mut self) {
        if let Some(interval) = &mut self.interval {
            interval.tick().await;
        } else {
            let start = Instant::now() + Duration::from_millis(self.frame_duration);
            let mut intv = interval_at(start, Duration::from_millis(self.frame_duration));
            intv.set_missed_tick_behavior(MissedTickBehavior::Skip);
            self.interval = Some(intv);
        }
    }
}
