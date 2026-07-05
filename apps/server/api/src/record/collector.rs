use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex as StdMutex;

use sea_orm::{ActiveValue::Set, DatabaseConnection, entity::prelude::*};
use tokio::sync::mpsc;

use super::observer::*;
use entity::frame;
use entity::round;
use entity::round_data;
use entity::session;
use framework::id::gen_id;

const MAX_FRAMES_PER_ROUND: usize = 5000;

struct FrameEntry {
    seq: u64,
    is_input: bool,
    detail: String,
    session_id: Option<String>,
    data: Option<Vec<u8>>,
}

struct PendingRound {
    round_id: String,
    session_id: Option<String>,
    llm_text: String,
    tts_text: String,
    frames: Vec<FrameEntry>,
}

struct FlushEvent {
    pending: PendingRound,
    reason: RoundEndReason,
}

pub struct RecordCollector {
    conn: DatabaseConnection,
    pending: Arc<StdMutex<HashMap<String, PendingRound>>>,
    known_sessions: Arc<StdMutex<std::collections::HashSet<String>>>,
    flush_tx: mpsc::UnboundedSender<FlushEvent>,
    _flush_handle: tokio::task::JoinHandle<()>,
}

async fn insert_round_data(
    conn: &DatabaseConnection,
    round_id: &str,
    data_type: &str,
    text: Option<String>,
    data: Option<Vec<u8>>,
) -> Result<(), anyhow::Error> {
    round_data::ActiveModel {
        id: Set(gen_id()),
        round_id: Set(round_id.to_string()),
        data_type: Set(data_type.to_string()),
        data: Set(data),
        text: Set(text),
        metadata: Set(Some(serde_json::json!({}))),
        ..Default::default()
    }
    .insert(conn)
    .await?;
    Ok(())
}

impl RecordCollector {
    pub fn new(conn: DatabaseConnection) -> Self {
        let (flush_tx, mut flush_rx) = mpsc::unbounded_channel::<FlushEvent>();
        let bg_conn = conn.clone();
        let _flush_handle = tokio::spawn(async move {
            while let Some(event) = flush_rx.recv().await {
                if let Err(e) = Self::flush_to_db(&bg_conn, &event.pending, event.reason).await {
                    tracing::error!("DB flush error: {e}");
                }
            }
        });
        Self {
            conn,
            pending: Arc::new(StdMutex::new(HashMap::new())),
            known_sessions: Arc::new(StdMutex::new(std::collections::HashSet::new())),
            flush_tx,
            _flush_handle,
        }
    }

    pub fn handle_event(&self, event: SessionEvent) {
        match event {
            SessionEvent::RoundStart {
                round_id,
                session_id,
                client_info: _,
            } => {
                if let Some(ref sid) = session_id {
                    let mut known = self.known_sessions.lock().expect("known lock");
                    if known.insert(sid.clone()) {
                        let conn = self.conn.clone();
                        let sid = sid.clone();
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
                let mut pending = self.pending.lock().expect("pending lock");
                pending.insert(
                    round_id.clone(),
                    PendingRound {
                        round_id,
                        session_id,
                        llm_text: String::new(),
                        tts_text: String::new(),
                        frames: Vec::new(),
                    },
                );
            }
            SessionEvent::LlmDelta { round_id, text } => {
                let mut pending = self.pending.lock().expect("pending lock");
                if let Some(buf) = pending.get_mut(&round_id) {
                    buf.llm_text.push_str(&text);
                }
            }
            SessionEvent::TtsDelta { round_id, text } => {
                let mut pending = self.pending.lock().expect("pending lock");
                if let Some(buf) = pending.get_mut(&round_id) {
                    buf.tts_text.push_str(&text);
                }
            }
            SessionEvent::Frame {
                round_id,
                session_id,
                seq,
                direction,
                detail,
                data,
            } => {
                let is_input = matches!(direction, FrameDirection::Input);
                if let Some(ref rid) = round_id {
                    let mut pending = self.pending.lock().expect("pending lock");
                    if let Some(buf) = pending.get_mut(rid) {
                        if buf.frames.len() < MAX_FRAMES_PER_ROUND {
                            buf.frames.push(FrameEntry {
                                seq,
                                is_input,
                                detail,
                                session_id,
                                data,
                            });
                        }
                        return;
                    }
                }
                // No pending round — write directly to DB
                let conn = self.conn.clone();
                let dir_str = if is_input { "input" } else { "output" };
                tokio::spawn(async move {
                    let _ = frame::ActiveModel {
                        round_id: Set(round_id),
                        session_id: Set(session_id),
                        seq: Set(seq as i32),
                        dir: Set(dir_str.to_string()),
                        kind: Set("frame".to_string()),
                        detail: Set(Some(detail)),
                        data: Set(data),
                        elapsed_us: Set(None),
                        ..Default::default()
                    }
                    .insert(&conn)
                    .await;
                });
            }
            SessionEvent::RoundEnd { round_id, reason } => {
                let buf = {
                    let mut pending = self.pending.lock().expect("pending lock");
                    pending.remove(&round_id)
                };
                if let Some(buf) = buf {
                    let _ = self.flush_tx.send(FlushEvent {
                        pending: buf,
                        reason,
                    });
                }
            }
        }
    }

    async fn flush_to_db(
        conn: &DatabaseConnection,
        pending: &PendingRound,
        reason: RoundEndReason,
    ) -> Result<(), anyhow::Error> {
        let now = chrono::Local::now().fixed_offset();
        let status = match reason {
            RoundEndReason::Completed => Some("completed".to_string()),
            RoundEndReason::Interrupted => Some("interrupted".to_string()),
        };

        round::ActiveModel {
            id: Set(pending.round_id.clone()),
            session_id: Set(pending.session_id.clone().unwrap_or_default()),
            client_info: Set(None),
            mode: Set("auto".to_string()),
            status: Set(status),
            create_datetime: Set(Some(now)),
            update_datetime: Set(Some(now)),
        }
        .insert(conn)
        .await?;

        if !pending.llm_text.is_empty() {
            insert_round_data(
                conn,
                &pending.round_id,
                "llm",
                Some(pending.llm_text.clone()),
                None,
            )
            .await?;
        }

        if !pending.tts_text.is_empty() {
            insert_round_data(
                conn,
                &pending.round_id,
                "tts",
                Some(pending.tts_text.clone()),
                None,
            )
            .await?;
        }

        for entry in &pending.frames {
            let dir_str = if entry.is_input { "input" } else { "output" };
            frame::ActiveModel {
                round_id: Set(Some(pending.round_id.clone())),
                session_id: Set(entry.session_id.clone()),
                seq: Set(entry.seq as i32),
                dir: Set(dir_str.to_string()),
                kind: Set("frame".to_string()),
                detail: Set(Some(entry.detail.clone())),
                data: Set(entry.data.clone()),
                elapsed_us: Set(None),
                ..Default::default()
            }
            .insert(conn)
            .await?;
        }

        Ok(())
    }
}
