use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use chrono::{DateTime, FixedOffset, Local};
use sea_orm::entity::prelude::*;
use sea_orm::{ActiveValue::Set, DatabaseConnection, TransactionTrait};

use entity::frame;
use entity::round;
use entity::round_data;
use entity::session;
use framework::id::gen_id;

use super::ogg::mux_opus_to_ogg;

const FRAME_KIND: &str = "frame";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dir {
    Input,
    Output,
}

impl Dir {
    pub fn as_str(&self) -> &'static str {
        match self {
            Dir::Input => "input",
            Dir::Output => "output",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameDetail {
    Voice,
    STTResult,
    LLMResult,
    TTSResult,
    AudioResult,
    Error,
    Hello,
    ListenStart,
    ListenStop,
    Input,
    UnknownText,
    Abort,
    Close,
    Mcp,
    Ping,
    Pong,
}

impl FrameDetail {
    pub fn as_str(&self) -> &'static str {
        match self {
            FrameDetail::Voice => "Voice",
            FrameDetail::STTResult => "STTResult",
            FrameDetail::LLMResult => "LLMResult",
            FrameDetail::TTSResult => "TTSResult",
            FrameDetail::AudioResult => "AudioResult",
            FrameDetail::Error => "Error",
            FrameDetail::Hello => "Hello",
            FrameDetail::ListenStart => "ListenStart",
            FrameDetail::ListenStop => "ListenStop",
            FrameDetail::Input => "Input",
            FrameDetail::UnknownText => "UnknownText",
            FrameDetail::Abort => "Abort",
            FrameDetail::Close => "Close",
            FrameDetail::Mcp => "Mcp",
            FrameDetail::Ping => "Ping",
            FrameDetail::Pong => "Pong",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataType {
    InputAudio,
    Llm,
    Tts,
    Text,
}

impl DataType {
    pub fn as_str(&self) -> &'static str {
        match self {
            DataType::InputAudio => "input_audio",
            DataType::Llm => "llm",
            DataType::Tts => "tts",
            DataType::Text => "text",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoundStatus {
    Completed,
    Interrupted,
}

impl RoundStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            RoundStatus::Completed => "completed",
            RoundStatus::Interrupted => "interrupted",
        }
    }
}

#[derive(Debug, Clone)]
pub struct RecordEntry {
    pub received_at: DateTime<FixedOffset>,
    pub seq: Option<u64>,
    pub kind: EntryKind,
}

#[derive(Debug, Clone)]
pub enum EntryKind {
    Frame {
        dir: Dir,
        detail: FrameDetail,
        data: Option<Vec<u8>>,
        session_id: Option<String>,
    },
    InputAudio {
        frames: Vec<Vec<u8>>,
        first_frame_at: Option<DateTime<FixedOffset>>,
        frame_duration_ms: u64,
        channels: u8,
    },
    LlmText {
        text: String,
    },
    TtsText {
        text: String,
    },
    Text {
        text: String,
    },
}

struct RoundInfo {
    round_id: String,
    session_id: Option<String>,
    started_at: DateTime<FixedOffset>,
}

pub struct Recorder {
    conn: DatabaseConnection,
    entries: Arc<Mutex<Vec<RecordEntry>>>,
    round_info: Arc<Mutex<Option<RoundInfo>>>,
    known_sessions: Arc<Mutex<HashSet<String>>>,
    next_seq: AtomicU64,
    voice_start: Mutex<Option<DateTime<FixedOffset>>>,
}

impl Recorder {
    pub fn new(conn: DatabaseConnection) -> Self {
        Self {
            conn,
            entries: Arc::new(Mutex::new(Vec::new())),
            round_info: Arc::new(Mutex::new(None)),
            known_sessions: Arc::new(Mutex::new(HashSet::new())),
            next_seq: AtomicU64::new(1),
            voice_start: Mutex::new(None),
        }
    }

    pub fn push_entry(&self, mut entry: RecordEntry) {
        if matches!(entry.kind, EntryKind::Frame { .. }) {
            entry.seq = Some(self.next_seq.fetch_add(1, Ordering::Relaxed));
        }
        if let Ok(mut entries) = self.entries.lock() {
            entries.push(entry);
        }
    }

    pub fn has_active_round(&self) -> bool {
        self.round_info.lock().is_ok_and(|info| info.is_some())
    }

    pub fn interrupt_round(&self) {
        let entries = self
            .entries
            .lock()
            .map_or_else(|_| Vec::new(), |mut e| std::mem::take(&mut *e));
        let round_info = self
            .round_info
            .lock()
            .map_or_else(|_| None, |mut r| r.take());
        if let Ok(mut vs) = self.voice_start.lock() {
            *vs = None;
        }
        if round_info.is_some() && !entries.is_empty() {
            let conn = self.conn.clone();
            tokio::spawn(async move {
                Self::flush(
                    &conn,
                    &entries,
                    round_info.as_ref(),
                    RoundStatus::Interrupted,
                )
                .await;
            });
        }
    }

    pub fn mark_voice_start(&self, now: DateTime<FixedOffset>) {
        if let Ok(mut vs) = self.voice_start.lock() {
            *vs = Some(vs.map(|v| v.min(now)).unwrap_or(now));
        }
    }

    pub fn start_round(&self, round_id: String, session_id: Option<String>) {
        let now = Local::now().fixed_offset();
        let voice_start = self.voice_start.lock().map_or(None, |mut vs| vs.take());
        let started_at = voice_start.map(|vs| vs.min(now)).unwrap_or(now);
        if let Ok(mut info) = self.round_info.lock() {
            *info = Some(RoundInfo {
                round_id,
                session_id,
                started_at,
            });
        }
    }

    pub fn ensure_session(&self, session_id: &str) {
        if let Ok(mut known) = self.known_sessions.lock()
            && known.insert(session_id.to_string())
        {
            let conn = self.conn.clone();
            let sid = session_id.to_string();
            tokio::spawn(async move {
                let _ = session::ActiveModel {
                    id: Set(sid),
                    ..Default::default()
                }
                .insert(&conn)
                .await;
            });
        }
    }

    pub async fn end_round(&self, status: RoundStatus) {
        let entries = self
            .entries
            .lock()
            .map_or_else(|_| Vec::new(), |mut e| std::mem::take(&mut *e));
        let round_info = self
            .round_info
            .lock()
            .map_or_else(|_| None, |mut r| r.take());
        Self::flush(&self.conn, &entries, round_info.as_ref(), status).await;
    }

    async fn flush(
        conn: &DatabaseConnection,
        entries: &[RecordEntry],
        round_info: Option<&RoundInfo>,
        status: RoundStatus,
    ) {
        if entries.is_empty() && round_info.is_none() {
            return;
        }

        let txn = match conn.begin().await {
            Ok(txn) => txn,
            Err(e) => {
                tracing::error!("flush begin transaction error: {e}");
                return;
            }
        };

        if let Some(info) = round_info {
            let now = Local::now().fixed_offset();
            if (round::ActiveModel {
                id: Set(info.round_id.clone()),
                session_id: Set(info.session_id.clone().unwrap_or_default()),
                client_info: Set(None),
                status: Set(Some(status.as_str().to_string())),
                create_datetime: Set(Some(info.started_at)),
                update_datetime: Set(Some(now)),
            })
            .insert(&txn)
            .await
            .is_err()
            {
                return;
            }
        }

        let tts_ogg = {
            let mut audio_result_packets: Vec<Vec<u8>> = Vec::new();
            let mut has_tts_text = false;
            for entry in entries.iter() {
                match &entry.kind {
                    EntryKind::Frame {
                        detail: FrameDetail::AudioResult,
                        data: Some(p),
                        ..
                    } => {
                        audio_result_packets.push(p.clone());
                    }
                    EntryKind::TtsText { .. } => {
                        has_tts_text = true;
                    }
                    _ => {}
                }
            }
            if has_tts_text && !audio_result_packets.is_empty() {
                match mux_opus_to_ogg(&audio_result_packets, 20, 1, 16000) {
                    Ok(data) => Some((data, audio_result_packets.len() as u64 * 20)),
                    Err(e) => {
                        tracing::error!("tts ogg mux error: {e}");
                        None
                    }
                }
            } else {
                None
            }
        };

        for entry in entries {
            let elapsed_us = round_info
                .and_then(|info| (entry.received_at - info.started_at).num_microseconds());

            match &entry.kind {
                EntryKind::Frame {
                    dir,
                    detail,
                    data,
                    session_id: sid,
                } => {
                    let seq = entry.seq.expect("Frame entry missing seq");
                    if (frame::ActiveModel {
                        round_id: Set(round_info.map(|r| r.round_id.clone())),
                        session_id: Set(sid.clone()),
                        seq: Set(seq as i32),
                        dir: Set(dir.as_str().to_string()),
                        kind: Set(FRAME_KIND.to_string()),
                        detail: Set(Some(detail.as_str().to_string())),
                        data: Set(data.clone()),
                        elapsed_us: Set(elapsed_us),
                        ..Default::default()
                    })
                    .insert(&txn)
                    .await
                    .is_err()
                    {
                        return;
                    }
                }
                EntryKind::InputAudio {
                    frames,
                    first_frame_at,
                    frame_duration_ms,
                    channels,
                } => {
                    let elapsed_ms = round_info
                        .and_then(|info| {
                            first_frame_at.map(|t| (t - info.started_at).num_milliseconds())
                        })
                        .unwrap_or(0);
                    let num_frames = frames.len();
                    let audio_duration_ms = num_frames as u64 * frame_duration_ms;
                    let ogg_data =
                        match mux_opus_to_ogg(frames, *frame_duration_ms, *channels, 16000) {
                            Ok(data) => data,
                            Err(e) => {
                                tracing::error!("ogg mux error: {e}");
                                return;
                            }
                        };
                    if (round_data::ActiveModel {
                        id: Set(gen_id()),
                        round_id: Set(round_info.map(|r| r.round_id.clone()).unwrap_or_default()),
                        data_type: Set(DataType::InputAudio.as_str().to_string()),
                        data: Set(Some(ogg_data)),
                        text: Set(None),
                        metadata: Set(Some(serde_json::json!({
                            "elapsed_ms": elapsed_ms,
                            "audio_duration_ms": audio_duration_ms,
                            "format": "ogg",
                        }))),
                        ..Default::default()
                    })
                    .insert(&txn)
                    .await
                    .is_err()
                    {
                        return;
                    }
                }
                EntryKind::LlmText { text } => {
                    let elapsed_ms = elapsed_us.map(|u| u / 1000).unwrap_or(0);
                    if (round_data::ActiveModel {
                        id: Set(gen_id()),
                        round_id: Set(round_info.map(|r| r.round_id.clone()).unwrap_or_default()),
                        data_type: Set(DataType::Llm.as_str().to_string()),
                        data: Set(None),
                        text: Set(Some(text.clone())),
                        metadata: Set(Some(serde_json::json!({
                            "elapsed_ms": elapsed_ms,
                        }))),
                        ..Default::default()
                    })
                    .insert(&txn)
                    .await
                    .is_err()
                    {
                        return;
                    }
                }
                EntryKind::Text { text } => {
                    let elapsed_ms = elapsed_us.map(|u| u / 1000).unwrap_or(0);
                    if (round_data::ActiveModel {
                        id: Set(gen_id()),
                        round_id: Set(round_info.map(|r| r.round_id.clone()).unwrap_or_default()),
                        data_type: Set(DataType::Text.as_str().to_string()),
                        data: Set(None),
                        text: Set(Some(text.clone())),
                        metadata: Set(Some(serde_json::json!({
                            "elapsed_ms": elapsed_ms,
                        }))),
                        ..Default::default()
                    })
                    .insert(&txn)
                    .await
                    .is_err()
                    {
                        return;
                    }
                }
                EntryKind::TtsText { text } => {
                    let elapsed_ms = elapsed_us.map(|u| u / 1000).unwrap_or(0);
                    let (data, audio_duration_ms) = tts_ogg
                        .as_ref()
                        .map(|(d, dur)| (Some(d.clone()), Some(*dur as i64)))
                        .unwrap_or((None, None));
                    if (round_data::ActiveModel {
                        id: Set(gen_id()),
                        round_id: Set(round_info.map(|r| r.round_id.clone()).unwrap_or_default()),
                        data_type: Set(DataType::Tts.as_str().to_string()),
                        data: Set(data),
                        text: Set(Some(text.clone())),
                        metadata: Set(Some(serde_json::json!({
                            "elapsed_ms": elapsed_ms,
                            "audio_duration_ms": audio_duration_ms,
                            "format": "ogg",
                        }))),
                        ..Default::default()
                    })
                    .insert(&txn)
                    .await
                    .is_err()
                    {
                        return;
                    }
                }
            }
        }

        if let Err(e) = txn.commit().await {
            tracing::error!("flush commit error: {e}");
        }
    }
}
