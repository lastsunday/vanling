use std::collections::HashMap;
use std::sync::Arc;
use std::sync::LazyLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use futures::StreamExt;

use tokio::sync::Mutex;
use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::watch;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::chobits::chii::{Chii, Input, OutputBlock};
use crate::chobits::frame::{FrameResult, OutputMessage};
use crate::chobits::message::audio::AudioMessage;
use crate::chobits::message::llm::LlmMessage;
use crate::chobits::message::stt::SttMessage;
use crate::chobits::message::tts::{TtsMessage, TtsState};
use crate::chobits::session::SessionErrorCode;
use crate::chobits::tts::Tts;

use framework::err;

const TTS_TIMEOUT: Duration = Duration::from_secs(35);

pub static EMOJI_MAP: LazyLock<HashMap<&str, &str>> = LazyLock::new(|| {
    let mut map: HashMap<&str, &str> = HashMap::new();
    map.insert("neutral", "😶");
    map.insert("happy", "🙂");
    map.insert("laughing", "😆");
    map.insert("funny", "😂");
    map.insert("sad", "😔");
    map.insert("angry", "😠");
    map.insert("crying", "😭");
    map.insert("loving", "😍");
    map.insert("embarrassed", "😳");
    map.insert("surprised", "😲");
    map.insert("shocked", "😱");
    map.insert("thinking", "🤔");
    map.insert("winking", "😉");
    map.insert("cool", "😎");
    map.insert("relaxed", "😌");
    map.insert("delicious", "🤤");
    map.insert("kissy", "😘");
    map.insert("confident", "😏");
    map.insert("sleepy", "😴");
    map.insert("silly", "😜");
    map.insert("confused", "🙄");
    map
});

pub fn analyze_emotion(_text: &str) -> &str {
    "happy"
}

#[derive(Debug, Clone, Copy)]
pub enum RoundStatus {
    Ok,
    Error,
}

impl std::fmt::Display for RoundStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Ok => write!(f, "ok"),
            Self::Error => write!(f, "error"),
        }
    }
}

pub struct Round {
    pub parent_id: String,
    pub id: String,
    output_tx: UnboundedSender<OutputMessage>,
    pub epoch: u64,
    stop: Arc<AtomicBool>,
    chii: Arc<dyn Chii>,
    tts: Arc<dyn Tts>,
    pub tts_state: Arc<Mutex<Option<TtsState>>>,
    pub cancel: CancellationToken,
    pub join_handle: Option<JoinHandle<()>>,
}

#[derive(Debug, Clone)]
pub struct ChatParam {
    pub text: String,
    pub prob: f32,
}

#[derive(Debug, Clone)]
pub enum Command {
    Chat(ChatParam),
}

impl Round {
    pub fn new(
        parent_id: String,
        id: String,
        output_tx: UnboundedSender<OutputMessage>,
        epoch: u64,
        chii: Arc<dyn Chii>,
        tts: Arc<dyn Tts>,
        cancel: CancellationToken,
    ) -> Self {
        Self {
            parent_id,
            id,
            output_tx,
            epoch,
            stop: Arc::new(AtomicBool::new(false)),
            chii,
            tts,
            tts_state: Arc::new(Mutex::new(None)),
            cancel,
            join_handle: None,
        }
    }

    pub async fn start(&self) {}

    pub async fn accept_command(&mut self, command: Command) {
        match command {
            Command::Chat(chat_param) => {
                self.chat_tts_handle(chat_param.text, chat_param.prob).await;
            }
        }
    }

    async fn chat_tts_handle(&mut self, text: String, _prob: f32) {
        let output_tx = self.output_tx.clone();
        let epoch = self.epoch;
        let round_id = self.id.clone();
        let stop_me = self.stop.clone();
        let session_id = self.parent_id.clone();
        let chii = self.chii.clone();
        let tts = self.tts.clone();
        let tts_state_clone = self.tts_state.clone();
        let cancel = self.cancel.clone();

        self.join_handle = Some(tokio::spawn(async move {
            let round_start = std::time::Instant::now();
            let mut status = RoundStatus::Ok;
            let mut sentence_count: u32 = 0;

            tracing::debug!(component = "round", event = "round_start", session_id = %session_id, round_id = %round_id, text = %text, "round start");

            let send = |payload: FrameResult| {
                output_tx
                    .send(OutputMessage {
                        epoch,
                        round_id: Some(round_id.clone()),
                        session_id: session_id.clone(),
                        payload,
                    })
                    .map_err(Box::new)
            };

            if send(FrameResult::STTResult(SttMessage::new(
                Some(session_id.clone()),
                Some(text.clone()),
            )))
            .is_err()
            {
                return;
            }
            tracing::debug!(component = "round", event = "stt_result_sent", session_id = %session_id, round_id = %round_id, "stt result sent");

            tracing::debug!(component = "round", event = "llm_start", session_id = %session_id, round_id = %round_id, "llm start");
            let chii_stream = chii.ask(Input::text(text), cancel.clone()).await;
            let sid = session_id.clone();
            let rid = round_id.clone();
            let (llm_err_tx, mut llm_err_rx) = watch::channel::<Option<String>>(None);
            let chat_text_stream = chii_stream.filter_map(move |r| {
                let sid = sid.clone();
                let rid = rid.clone();
                let llm_err_tx = llm_err_tx.clone();
                async move {
                    match r {
                        Ok(block) => match block {
                            OutputBlock::Text(s) => Some(s),
                        },
                        Err(e) => {
                            tracing::warn!(component = "round", event = "llm_error", session_id = %sid, round_id = %rid, error = %e, "llm error");
                            let _ = llm_err_tx.send(Some(e.to_string()));
                            None
                        }
                    }
                }
            });
            tracing::debug!(component = "round", event = "tts_start", session_id = %session_id, round_id = %round_id, "tts start");
            let tts_output = tts.stream(Box::pin(chat_text_stream), cancel.clone()).await;

            async fn change_state(tts_state: &Arc<Mutex<Option<TtsState>>>, state: TtsState) {
                let mut ts = tts_state.lock().await;
                *ts = Some(state);
            }

            change_state(&tts_state_clone, TtsState::Start).await;
            if send(FrameResult::TTSResult(TtsMessage::new(
                Some(session_id.clone()),
                Some(TtsState::Start),
                None,
            )))
            .is_err()
            {
                stop_me.store(true, Ordering::Relaxed);
            }

            let mut tts_output = tts_output;
            let mut llm_error_detected = false;
            loop {
                tokio::select! {
                    result = tokio::time::timeout(TTS_TIMEOUT, tts_output.next()) => {
                        match result {
                            Ok(Some(result)) => match result {
                                Ok(packet) => {
                                    if stop_me.load(Ordering::Relaxed) {
                                        break;
                                    }
                                    let text = packet.text;
                                    let emotion = analyze_emotion(&text);
                                    let audio_data = packet.audio;
                                    let audio_len: usize = audio_data.iter().map(|a| a.len()).sum();
                                    sentence_count += 1;
                                    tracing::debug!(
                                        component = "round", event = "sentence",
                                        session_id = %session_id, round_id = %round_id,
                                        sentence = sentence_count, text = %text, emotion, audio_bytes = audio_len,
                                        "sentence"
                                    );
                                    let tts_state_c = tts_state_clone.clone();
                                    let stop_by_tts = stop_me.clone();

                                    let mut llm_msg = LlmMessage::new(
                                        Some(session_id.to_string()),
                                        Some(emotion.to_string()),
                                        Some(EMOJI_MAP.get(emotion).map_or("😶", |v| v).to_string()),
                                    );
                                    llm_msg.full_text = Some(text.clone());
                                    if send(FrameResult::LLMResult(llm_msg)).is_err() {
                                        status = RoundStatus::Error;
                                        stop_me.store(true, Ordering::Relaxed);
                                        break;
                                    }

                                    change_state(&tts_state_c, TtsState::SentenceStart).await;
                                    if send(FrameResult::TTSResult(TtsMessage::new(
                                        Some(session_id.to_string()),
                                        Some(TtsState::SentenceStart),
                                        Some(text.to_string()),
                                    )))
                                    .is_err()
                                    {
                                        status = RoundStatus::Error;
                                        stop_me.store(true, Ordering::Relaxed);
                                        break;
                                    }

                                    for packet_data in audio_data.into_iter() {
                                        if stop_by_tts.load(Ordering::Relaxed) {
                                            break;
                                        }
                                        if send(FrameResult::AudioResult(AudioMessage::new(
                                            Some(session_id.to_string()),
                                            packet_data,
                                        )))
                                        .is_err()
                                        {
                                            break;
                                        }
                                    }

                                    change_state(&tts_state_c, TtsState::SentenceEnd).await;
                                    if send(FrameResult::TTSResult(TtsMessage::new(
                                        Some(session_id.to_string()),
                                        Some(TtsState::SentenceEnd),
                                        None,
                                    )))
                                    .is_err()
                                    {
                                        status = RoundStatus::Error;
                                        stop_me.store(true, Ordering::Relaxed);
                                        break;
                                    }
                                }
                                Err(e) => {
                                    status = RoundStatus::Error;
                                    tracing::warn!(component = "round", event = "tts_encode_error", session_id = %session_id, round_id = %round_id, error = %e, "tts encode error");
                                    let _ = send(FrameResult::Error(
                                        err!(SessionErrorCode::TtsEncode).with_extra(e.to_string()),
                                    ));
                                    stop_me.store(true, Ordering::Relaxed);
                                    break;
                                }
                            },
                            Ok(None) => {
                                tracing::debug!(component = "round", event = "tts_stream_ended", session_id = %session_id, round_id = %round_id, "tts: stream ended");
                                break;
                            }
                            Err(_) => {
                                tracing::warn!(component = "round", event = "tts_timeout", session_id = %session_id, round_id = %round_id, "tts timeout");
                                status = RoundStatus::Error;
                                break;
                            }
                        }
                    }
                    _ = llm_err_rx.changed() => {
                        if let Some(e) = llm_err_rx.borrow().clone() {
                            tracing::warn!(
                                component = "round", event = "llm_error_detected",
                                session_id = %session_id, round_id = %round_id,
                                "LLM error detected, aborting TTS"
                            );
                            status = RoundStatus::Error;
                            llm_error_detected = true;
                            let _ = send(FrameResult::Error(
                                err!(SessionErrorCode::LlmNoUsableOutput)
                                    .with_extra(e),
                            ));
                            stop_me.store(true, Ordering::Relaxed);
                            break;
                        }
                    }
                }
            }

            // Fallback: check if LLM error arrived after TTS loop ended (race condition safety net)
            if !llm_error_detected
                && sentence_count == 0
                && let Some(e) = llm_err_rx.borrow().clone()
            {
                status = RoundStatus::Error;
                tracing::warn!(
                    component = "round", event = "llm_no_usable_output",
                    session_id = %session_id, round_id = %round_id,
                    "LLM completed with no usable output"
                );
                let _ = send(FrameResult::Error(
                    err!(SessionErrorCode::LlmNoUsableOutput).with_extra(e),
                ));
            }

            change_state(&tts_state_clone, TtsState::Stop).await;
            if send(FrameResult::TTSResult(TtsMessage::new(
                Some(session_id.clone()),
                Some(TtsState::Stop),
                None,
            )))
            .is_err()
            {
                status = RoundStatus::Error;
                stop_me.store(true, Ordering::Relaxed);
            }
            let duration_ms = round_start.elapsed().as_millis() as u64;
            tracing::debug!(
                component = "round", event = "round_complete",
                session_id = %session_id, round_id = %round_id,
                duration_ms, sentences = sentence_count, %status,
                "round complete"
            );
        }));
    }

    pub async fn is_speaking(&self) -> bool {
        let state = self.tts_state.lock().await;
        if let Some(state) = &*state {
            !matches!(state, TtsState::Stop)
        } else {
            false
        }
    }

    pub async fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
        self.cancel.cancel();
    }
}
