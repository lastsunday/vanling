use crate::llm::client::{ChatRequest, Client};
use crate::record::observer::{FrameContext, FrameDirection};
use crate::tts::Tts;
use crate::util::llm::{EMOJI_MAP, analyze_emotion};
use crate::ws::WsErrorCode;
use anyhow::Context;
use core::result::Result;
use framework::err;
use framework::error::AppError;
use futures::StreamExt;
use rig::OneOrMany;
use rig::message::{Message, Text, UserContent};
use service::chobits::message::audio::AudioMessage;
use service::chobits::message::llm::LlmMessage;
use service::chobits::message::stt::SttMessage;
use service::chobits::message::tts::{TtsMessage, TtsState};
use service::ws::frame::FrameResult;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;
use tokio::sync::Mutex;
use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::mpsc::error::SendError;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tracing::{error, info, trace};

pub struct OutputMessage {
    pub epoch: u64,
    pub payload: Result<FrameResult, AppError>,
    pub frame_ctx: Option<FrameContext>,
}

pub struct Round {
    pub parent_id: String,
    pub id: String,
    tx: TracedSender,
    stop: Arc<AtomicBool>,
    client: Arc<Client>,
    tts: Arc<Box<dyn Tts>>,
    pub tts_state: Arc<Mutex<Option<TtsState>>>,
    pub cancel: CancellationToken,
    pub join_handle: Option<JoinHandle<()>>,
}

#[derive(Debug, Clone)]
pub enum Command<'a> {
    Chat(ChatParam<'a>),
}

#[derive(Clone, Debug)]
pub struct ChatParam<'a> {
    pub text: &'a str,
    pub prob: &'a f32,
}

#[derive(Clone)]
pub struct TracedSender {
    inner: UnboundedSender<OutputMessage>,
    round_id: Option<String>,
    session_id: Option<String>,
    seq: Arc<AtomicU64>,
    epoch: u64,
    round_started_at: Instant,
}

impl TracedSender {
    pub fn new(
        inner: UnboundedSender<OutputMessage>,
        round_id: Option<String>,
        session_id: Option<String>,
        seq: Arc<AtomicU64>,
        epoch: u64,
        round_started_at: Instant,
    ) -> Self {
        Self {
            inner,
            round_id,
            session_id,
            seq,
            epoch,
            round_started_at,
        }
    }

    pub async fn send(
        &self,
        item: Result<FrameResult, AppError>,
    ) -> Result<(), SendError<OutputMessage>> {
        let frame_ctx = self.round_id.as_ref().map(|round_id| {
            let detail = match &item {
                Ok(r) => format!("{r}"),
                Err(e) => format!("Err({e})"),
            };
            let seq = self.seq.fetch_add(1, Ordering::Relaxed);
            FrameContext {
                round_id: Some(round_id.clone()),
                session_id: self.session_id.clone(),
                seq,
                direction: FrameDirection::Outbound,
                detail,
                data: None,
                round_started_at: Some(self.round_started_at),
            }
        });
        self.inner
            .send(OutputMessage {
                epoch: self.epoch,
                payload: item,
                frame_ctx,
            })
            .map_err(|_| {
                SendError(OutputMessage {
                    epoch: self.epoch,
                    payload: Err(err!(WsErrorCode::InternalError)),
                    frame_ctx: None,
                })
            })
    }

    pub async fn send_audio(
        &self,
        item: Result<FrameResult, AppError>,
    ) -> Result<(), SendError<OutputMessage>> {
        let frame_ctx = self.round_id.as_ref().map(|round_id| {
            let detail = match &item {
                Ok(r) => format!("{r}"),
                Err(e) => format!("Err({e})"),
            };
            let data = match &item {
                Ok(FrameResult::AudioResult(msg)) => Some(msg.data.clone()),
                _ => None,
            };
            let seq = self.seq.fetch_add(1, Ordering::Relaxed);
            FrameContext {
                round_id: Some(round_id.clone()),
                session_id: self.session_id.clone(),
                seq,
                direction: FrameDirection::Outbound,
                detail,
                data,
                round_started_at: Some(self.round_started_at),
            }
        });
        self.inner
            .send(OutputMessage {
                epoch: self.epoch,
                payload: item,
                frame_ctx,
            })
            .map_err(|_| {
                SendError(OutputMessage {
                    epoch: self.epoch,
                    payload: Err(err!(WsErrorCode::InternalError)),
                    frame_ctx: None,
                })
            })
    }
}

async fn change_tts_state(tts_state: Arc<Mutex<Option<TtsState>>>, state: TtsState) {
    let mut tts_state = tts_state.lock().await;
    *tts_state = Some(state);
}

async fn send_tts_frame(
    tx: &TracedSender,
    session_id: String,
    state: TtsState,
    text: Option<String>,
) -> Result<(), SendError<OutputMessage>> {
    tx.send(Ok(FrameResult::TTSResult(TtsMessage::new(
        Some(session_id),
        Some(state),
        text,
    ))))
    .await?;
    Ok(())
}

async fn send_tts_frame_and_change_state(
    tts_state: Arc<Mutex<Option<TtsState>>>,
    tx: &TracedSender,
    session_id: String,
    state: TtsState,
    text: Option<String>,
) -> Result<(), SendError<OutputMessage>> {
    change_tts_state(tts_state, state.clone()).await;
    send_tts_frame(tx, session_id, state, text).await?;
    Ok(())
}

impl Round {
    pub fn new(
        parent_id: String,
        id: String,
        tx: TracedSender,
        client: Arc<Client>,
        tts: Arc<Box<dyn Tts>>,
        cancel: CancellationToken,
    ) -> Self {
        Self {
            parent_id,
            id,
            tx,
            stop: Arc::new(AtomicBool::new(false)),
            client,
            tts,
            tts_state: Arc::new(Mutex::new(None)),
            cancel,
            join_handle: None,
        }
    }

    pub async fn start(&self) {
        info!("round start");
    }

    pub async fn accept_command<'a>(&mut self, command: Command<'a>) {
        info!("accept_command {:?}", command);
        match command {
            Command::Chat(chat_param) => {
                self.llm_tts_handle(chat_param.text, chat_param.prob).await
            }
        }
    }

    async fn llm_tts_handle(&mut self, text: &str, _prob: &f32) {
        let tx = self.tx.clone();
        let stop_me = self.stop.clone();
        let session_id = self.parent_id.clone();
        let client = self.client.clone();
        let tts = self.tts.clone();
        let tts_state_clone = self.tts_state.clone();
        let text = String::from(text);
        let cancel = self.cancel.clone();
        self.join_handle = Some(tokio::spawn(async move {
            if tx
                .send(Ok(FrameResult::STTResult(SttMessage::new(
                    Some(session_id.clone()),
                    Some(text.to_string()),
                ))))
                .await
                .is_err()
            {
                info!(target:"round","send stt result failure");
                return;
            }
            let request = ChatRequest {
                message: Message::User {
                    content: OneOrMany::one(UserContent::Text(Text { text: text.clone() })),
                },
            };
            let llm_output = client.chat(request, cancel.clone());
            let mut tts_output = tts.stream(Box::pin(llm_output), cancel).await;
            let stop_me = stop_me.clone();
            if send_tts_frame_and_change_state(
                tts_state_clone.clone(),
                &tx,
                session_id.clone(),
                TtsState::Start,
                None,
            )
            .await
            .is_err()
            {
                info!(target:"round","send tts state start failure");
                stop_me.store(true, Ordering::Relaxed);
            }
            while let Some(result) = tts_output.next().await {
                match result {
                    Ok(result) => {
                        if stop_me.load(Ordering::Relaxed) {
                            trace!("stop_me");
                            break;
                        }
                        let text = result.text;
                        let emotion = analyze_emotion(&text);
                        let session_id = session_id.clone();
                        let tx = tx.clone();
                        let text = text.clone();
                        let audio_data = result.audio;
                        let tts_state_clone = tts_state_clone.clone();
                        let stop_me_by_tts_packet = stop_me.clone();
                        let result: Result<(), anyhow::Error> = async {
                            tx.send(Ok(FrameResult::LLMResult(LlmMessage::new(
                                Some(session_id.to_string()),
                                Some(emotion.to_string()),
                                Some(EMOJI_MAP.get(emotion).map_or(r#"😶"#, |v| v).to_string()),
                            ))))
                            .await
                            .context("send llm result failure")?;
                            send_tts_frame_and_change_state(
                                tts_state_clone.clone(),
                                &tx,
                                session_id.clone(),
                                TtsState::SentenceStart,
                                Some(text.to_string()),
                            )
                            .await?;
                            let audio_data = audio_data.unwrap_or_default();
                            let data = audio_data.into_iter();
                            for packet in data {
                                if stop_me_by_tts_packet.load(Ordering::Relaxed) {
                                    trace!("stop_me_by_tts_packet");
                                    break;
                                }
                                tx.send_audio(Ok(FrameResult::AudioResult(AudioMessage::new(
                                    Some(session_id.to_string()),
                                    packet,
                                ))))
                                .await
                                .context("send audio result failure")?;
                            }
                            send_tts_frame_and_change_state(
                                tts_state_clone.clone(),
                                &tx,
                                session_id.clone(),
                                TtsState::SentenceEnd,
                                None,
                            )
                            .await?;
                            Ok(())
                        }
                        .await;
                        if let Err(e) = result {
                            error!(target:"round","{:?}", e);
                            stop_me.store(true, Ordering::Relaxed);
                            break;
                        }
                    }
                    Err(e) => {
                        error!(target:"round","{:?}", e);
                        if let Err(e) = tx
                            .send(Err(err!(WsErrorCode::TtsEncode).with_extra(e.to_string())))
                            .await
                        {
                            error!(target:"round","{:?}", e);
                        }
                        stop_me.store(true, Ordering::Relaxed);
                        break;
                    }
                }
            }
            if send_tts_frame_and_change_state(
                tts_state_clone.clone(),
                &tx,
                session_id.clone(),
                TtsState::Stop,
                None,
            )
            .await
            .is_err()
            {
                stop_me.store(true, Ordering::Relaxed);
            }
            info!(target:"round","end");
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
        info!("round stop");
    }
}
