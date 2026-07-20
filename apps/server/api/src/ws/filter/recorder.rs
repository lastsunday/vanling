use std::sync::Arc;
use std::sync::Mutex;

use async_trait::async_trait;
use chrono::{DateTime, FixedOffset, Local};
use service::chobits::frame::{Frame, FrameResult, InputMode, OutputMessage};
use service::chobits::message::tts::TtsState;

use crate::record::recorder::{Dir, EntryKind, FrameDetail, RecordEntry, Recorder, RoundStatus};
use crate::ws::filter::{FilterAction, FilterCtx, InputFilter, OutputFilter};

pub struct RecorderInputFilter {
    recorder: Option<Arc<Recorder>>,
    session_id: String,
}

impl RecorderInputFilter {
    pub fn new(recorder: Option<Arc<Recorder>>, session_id: String) -> Self {
        Self {
            recorder,
            session_id,
        }
    }

    fn ensure_session(&self, recorder: &Recorder) {
        recorder.ensure_session(&self.session_id);
    }

    fn record_frame(
        &self,
        recorder: &Recorder,
        now: DateTime<FixedOffset>,
        detail: FrameDetail,
        data: Option<Vec<u8>>,
    ) {
        recorder.push_entry(RecordEntry {
            received_at: now,
            seq: None,
            kind: EntryKind::Frame {
                dir: Dir::Input,
                detail,
                data,
                session_id: Some(self.session_id.clone()),
            },
        });
    }
}

#[async_trait]
impl InputFilter for RecorderInputFilter {
    async fn process(&self, _ctx: &FilterCtx, frame: Frame) -> FilterAction<Frame> {
        let Some(ref recorder) = self.recorder else {
            return FilterAction::Continue(frame);
        };

        let now = Local::now().fixed_offset();
        self.ensure_session(recorder);

        match &frame {
            Frame::Hello(hello) => {
                if let Some(params) = &hello.audio_params {
                    recorder.set_input_params(params.frame_duration, params.channels);
                }
                self.record_frame(recorder, now, FrameDetail::Hello, None);
            }
            Frame::Voice { data } => {
                self.record_frame(recorder, now, FrameDetail::Voice, Some(data.clone()));
            }
            Frame::ListenStart { .. } => {
                self.record_frame(recorder, now, FrameDetail::ListenStart, None);
            }
            Frame::ListenStop => {
                self.record_frame(recorder, now, FrameDetail::ListenStop, None);
            }
            Frame::Abort(_) => {
                self.record_frame(recorder, now, FrameDetail::Abort, None);
                recorder.interrupt_round();
            }
            Frame::Error { code: _, message } => {
                self.record_frame(
                    recorder,
                    now,
                    FrameDetail::Error,
                    Some(message.as_bytes().to_vec()),
                );
            }
            Frame::Close(reason) => {
                tracing::debug!(
                    component = "recorder", event = "close_frame_received",
                    session_id = %self.session_id,
                    reason = reason.code,
                    "close frame received",
                );
                self.record_frame(recorder, now, FrameDetail::Close, None);
            }
            Frame::Input { text, mode } => {
                self.record_frame(
                    recorder,
                    now,
                    FrameDetail::Input,
                    Some(text.as_bytes().to_vec()),
                );
                let mode_str = match mode {
                    InputMode::Wake => "wake".to_string(),
                    InputMode::Normal => "normal".to_string(),
                };
                recorder.push_entry(RecordEntry {
                    received_at: now,
                    seq: None,
                    kind: EntryKind::Text {
                        text: text.clone(),
                        mode: mode_str,
                    },
                });
            }
            Frame::UnknownText { data } => {
                self.record_frame(recorder, now, FrameDetail::UnknownText, Some(data.clone()));
            }
            Frame::Mcp(_) => {}
            Frame::Ping { data } => {
                self.record_frame(recorder, now, FrameDetail::Ping, Some(data.clone()));
            }
            Frame::Pong { data } => {
                self.record_frame(recorder, now, FrameDetail::Pong, Some(data.clone()));
            }
        }

        FilterAction::Continue(frame)
    }
}

pub struct RecorderOutputFilter {
    recorder: Option<Arc<Recorder>>,
    session_id: String,
    current_round_id: Mutex<Option<String>>,
}

impl RecorderOutputFilter {
    pub fn new(recorder: Option<Arc<Recorder>>, session_id: String) -> Self {
        Self {
            recorder,
            session_id,
            current_round_id: Mutex::new(None),
        }
    }

    fn record_frame(
        &self,
        recorder: &Recorder,
        now: DateTime<FixedOffset>,
        detail: FrameDetail,
        data: Option<Vec<u8>>,
    ) {
        recorder.push_entry(RecordEntry {
            received_at: now,
            seq: None,
            kind: EntryKind::Frame {
                dir: Dir::Output,
                detail,
                data,
                session_id: Some(self.session_id.clone()),
            },
        });
    }
}

#[async_trait]
impl OutputFilter for RecorderOutputFilter {
    async fn process(&self, _ctx: &FilterCtx, msg: OutputMessage) -> FilterAction<OutputMessage> {
        let Some(ref recorder) = self.recorder else {
            return FilterAction::Continue(msg);
        };

        let now = Local::now().fixed_offset();
        let payload = &msg.payload;

        let Some(rid) = &msg.round_id else {
            match &payload {
                FrameResult::HelloResult(hello) => {
                    if let Some(params) = &hello.audio_params {
                        recorder.set_tts_params(
                            params.frame_duration,
                            params.channels as u8,
                            params.sample_rate,
                        );
                    }
                    self.record_frame(recorder, now, FrameDetail::Hello, None);
                }
                FrameResult::CloseResult => {
                    self.record_frame(recorder, now, FrameDetail::Close, None);
                }
                _ => {}
            }
            return FilterAction::Continue(msg);
        };

        let prev_round_id = self
            .current_round_id
            .lock()
            .expect("round id lock")
            .replace(rid.clone());

        if prev_round_id.as_deref() != Some(rid) {
            if prev_round_id.is_some() && recorder.has_active_round() {
                recorder.end_round(RoundStatus::Interrupted).await;
            }
            recorder.start_round(rid.clone(), Some(self.session_id.clone()));
        }

        match &payload {
            FrameResult::STTResult(stt) => {
                self.record_frame(
                    recorder,
                    now,
                    FrameDetail::STTResult,
                    stt.text.as_ref().map(|t| t.as_bytes().to_vec()),
                );
            }
            FrameResult::LLMResult(llm) => {
                if let Some(text) = &llm.full_text {
                    recorder.push_entry(RecordEntry {
                        received_at: now,
                        seq: None,
                        kind: EntryKind::LlmText { text: text.clone() },
                    });
                }
                self.record_frame(recorder, now, FrameDetail::LLMResult, None);
            }
            FrameResult::TTSResult(tts) => {
                if tts.state == Some(TtsState::Stop) {
                    self.record_frame(recorder, now, FrameDetail::TTSResult, None);
                    recorder.end_round(RoundStatus::Completed).await;
                    self.current_round_id.lock().expect("round id lock").take();
                    return FilterAction::Continue(msg);
                }

                if let Some(text) = &tts.text {
                    recorder.push_entry(RecordEntry {
                        received_at: now,
                        seq: None,
                        kind: EntryKind::TtsText { text: text.clone() },
                    });
                }
                self.record_frame(recorder, now, FrameDetail::TTSResult, None);
            }
            FrameResult::AudioResult(audio) => {
                self.record_frame(
                    recorder,
                    now,
                    FrameDetail::AudioResult,
                    Some(audio.data.clone()),
                );
            }
            FrameResult::HelloResult(_) | FrameResult::CloseResult | FrameResult::McpResult(_) => {}
            FrameResult::Error(_) => {
                self.record_frame(recorder, now, FrameDetail::Error, None);
            }
        }

        FilterAction::Continue(msg)
    }
}
