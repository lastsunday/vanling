use std::sync::Arc;
use std::sync::Mutex;

use chrono::{DateTime, FixedOffset, Local};
use service::chobits::message::listen::ListenState;
use service::ws::frame::Frame;
use tokio::sync::mpsc::UnboundedSender;

use crate::record::recorder::{Dir, EntryKind, FrameDetail, RecordEntry, Recorder};
use crate::ws::input_sender::InputSender;

struct InputState {
    input_active: bool,
    audio_buffer: Vec<Vec<u8>>,
    voice_start_time: Option<DateTime<FixedOffset>>,
}

pub struct InputProxy {
    session_id: String,
    recorder: Option<Arc<Recorder>>,
    input_tx: UnboundedSender<Frame>,
    state: Mutex<InputState>,
    input_frame_duration: u64,
    input_channels: u32,
}

impl InputProxy {
    pub fn new(
        session_id: String,
        recorder: Option<Arc<Recorder>>,
        input_tx: UnboundedSender<Frame>,
        input_frame_duration: u64,
        input_channels: u32,
    ) -> Self {
        Self {
            session_id,
            recorder,
            input_tx,
            state: Mutex::new(InputState {
                input_active: false,
                audio_buffer: Vec::new(),
                voice_start_time: None,
            }),
            input_frame_duration,
            input_channels,
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

    pub fn send(&self, msg: Frame) {
        let Some(ref recorder) = self.recorder else {
            let _ = self.input_tx.send(msg);
            return;
        };

        let now = Local::now().fixed_offset();
        self.ensure_session(recorder);

        match &msg {
            Frame::Hello(_) => {
                self.record_frame(recorder, now, FrameDetail::Hello, None);
            }
            Frame::Voice { data } => {
                self.record_frame(recorder, now, FrameDetail::Voice, Some(data.clone()));

                let mut state = self.state.lock().expect("state lock");
                if state.input_active {
                    if state.voice_start_time.is_none() {
                        state.voice_start_time = Some(now);
                        recorder.mark_voice_start(now);
                    }
                    state.audio_buffer.push(data.clone());
                }
            }
            Frame::Listen(listen) => {
                self.record_frame(recorder, now, FrameDetail::Listen, None);

                let mut state = self.state.lock().expect("state lock");
                match listen.state {
                    ListenState::Start => {
                        recorder.mark_voice_start(now);
                        state.input_active = true;
                        state.audio_buffer.clear();
                        state.voice_start_time = None;
                    }
                    ListenState::Stop => {
                        if state.input_active && !state.audio_buffer.is_empty() {
                            let frames = std::mem::take(&mut state.audio_buffer);
                            let first_frame_at = state.voice_start_time.take();
                            recorder.push_entry(RecordEntry {
                                received_at: now,
                                seq: None,
                                kind: EntryKind::InputAudio {
                                    frames,
                                    first_frame_at,
                                    frame_duration_ms: self.input_frame_duration,
                                    channels: self.input_channels as u8,
                                },
                            });
                        }
                        state.input_active = false;
                        state.audio_buffer.clear();
                        state.voice_start_time = None;
                    }
                    _ => {}
                }
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
            Frame::Close(_) => {
                self.record_frame(recorder, now, FrameDetail::Close, None);
            }
            Frame::Chat { text } => {
                self.record_frame(
                    recorder,
                    now,
                    FrameDetail::Chat,
                    Some(text.as_bytes().to_vec()),
                );
                recorder.push_entry(RecordEntry {
                    received_at: now,
                    seq: None,
                    kind: EntryKind::Text { text: text.clone() },
                });
            }
            Frame::UnknowText { data } => {
                self.record_frame(recorder, now, FrameDetail::UnknownText, Some(data.clone()));
            }
            Frame::Mcp(_) => {
                self.record_frame(recorder, now, FrameDetail::Mcp, None);
            }
            Frame::Ping { data } => {
                self.record_frame(recorder, now, FrameDetail::Ping, Some(data.clone()));
            }
            Frame::Pong { data } => {
                self.record_frame(recorder, now, FrameDetail::Pong, Some(data.clone()));
            }
        }

        let _ = self.input_tx.send(msg);
    }
}

impl InputSender for InputProxy {
    fn send(&self, msg: Frame) {
        InputProxy::send(self, msg);
    }
}
