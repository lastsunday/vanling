use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, FixedOffset, Local};
use service::chobits::message::tts::TtsState;
use tokio::sync::mpsc::UnboundedReceiver;

use crate::error::AppError;
use crate::record::recorder::{Dir, EntryKind, FrameDetail, RecordEntry, Recorder, RoundStatus};
use crate::ws::output_sender::OutputSender;
use crate::ws::session::round::OutputMessage;
use service::ws::frame::FrameResult;

pub struct OutputProxy {
    output_rx: UnboundedReceiver<OutputMessage>,
    recorder: Option<Arc<Recorder>>,
    session_id: String,
    current_round_id: Option<String>,
}

impl OutputProxy {
    pub fn new(
        output_rx: UnboundedReceiver<OutputMessage>,
        recorder: Option<Arc<Recorder>>,
        session_id: String,
    ) -> Self {
        Self {
            output_rx,
            recorder,
            session_id,
            current_round_id: None,
        }
    }

    fn record_frame(
        recorder: &Recorder,
        now: DateTime<FixedOffset>,
        detail: FrameDetail,
        data: Option<Vec<u8>>,
        session_id: &str,
    ) {
        recorder.push_entry(RecordEntry {
            received_at: now,
            seq: None,
            kind: EntryKind::Frame {
                dir: Dir::Output,
                detail,
                data,
                session_id: Some(session_id.to_string()),
            },
        });
    }

    async fn record_output(&mut self, msg: &OutputMessage) {
        let Some(ref recorder) = self.recorder else {
            return;
        };

        let now: DateTime<FixedOffset> = Local::now().fixed_offset();

        let Some(rid) = &msg.round_id else {
            // Pre-round output frames (no round_id)
            match &msg.payload {
                Ok(FrameResult::HelloResult(_)) => {
                    Self::record_frame(recorder, now, FrameDetail::Hello, None, &self.session_id);
                }
                Ok(FrameResult::CloseResult) => {
                    Self::record_frame(recorder, now, FrameDetail::Close, None, &self.session_id);
                }
                Ok(FrameResult::McpResult(_)) => {
                    Self::record_frame(recorder, now, FrameDetail::Mcp, None, &self.session_id);
                }
                _ => {}
            }
            return;
        };

        if self.current_round_id.as_deref() != Some(rid) {
            if self.current_round_id.is_some() && recorder.has_active_round() {
                recorder.end_round(RoundStatus::Interrupted).await;
            }
            recorder.start_round(rid.clone(), Some(self.session_id.clone()));
            self.current_round_id = Some(rid.clone());
        }

        match &msg.payload {
            Ok(FrameResult::STTResult(stt)) => {
                Self::record_frame(
                    recorder,
                    now,
                    FrameDetail::STTResult,
                    stt.text.as_ref().map(|t| t.as_bytes().to_vec()),
                    &self.session_id,
                );
            }
            Ok(FrameResult::LLMResult(llm)) => {
                if let Some(text) = &llm.full_text {
                    recorder.push_entry(RecordEntry {
                        received_at: now,
                        seq: None,
                        kind: EntryKind::LlmText { text: text.clone() },
                    });
                }
                Self::record_frame(
                    recorder,
                    now,
                    FrameDetail::LLMResult,
                    None,
                    &self.session_id,
                );
            }
            Ok(FrameResult::TTSResult(tts)) => {
                if tts.state == Some(TtsState::Stop) {
                    Self::record_frame(
                        recorder,
                        now,
                        FrameDetail::TTSResult,
                        None,
                        &self.session_id,
                    );
                    recorder.end_round(RoundStatus::Completed).await;
                    self.current_round_id = None;
                    return;
                }

                if let Some(text) = &tts.text {
                    recorder.push_entry(RecordEntry {
                        received_at: now,
                        seq: None,
                        kind: EntryKind::TtsText { text: text.clone() },
                    });
                }
                Self::record_frame(
                    recorder,
                    now,
                    FrameDetail::TTSResult,
                    None,
                    &self.session_id,
                );
            }
            Ok(FrameResult::AudioResult(audio)) => {
                Self::record_frame(
                    recorder,
                    now,
                    FrameDetail::AudioResult,
                    Some(audio.data.clone()),
                    &self.session_id,
                );
            }
            Ok(
                FrameResult::HelloResult(_) | FrameResult::CloseResult | FrameResult::McpResult(_),
            ) => {
                // Handled above for pre-round; should not reach here with round_id
            }
            Err(_) => {
                Self::record_frame(recorder, now, FrameDetail::Error, None, &self.session_id);
            }
        }
    }
}

#[async_trait]
impl OutputSender for OutputProxy {
    async fn recv(&mut self) -> Option<Result<FrameResult, AppError>> {
        let msg = self.output_rx.recv().await?;
        self.record_output(&msg).await;
        Some(msg.payload)
    }
}
