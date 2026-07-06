use crate::llm::client::{ChatRequest, Client};
use crate::tts::Tts;
use crate::util::llm::{EMOJI_MAP, analyze_emotion};
use crate::ws::WsErrorCode;
use anyhow::Context;
use framework::err;
use futures::StreamExt;
use rig::OneOrMany;
use rig::message::{Message, Text, UserContent};
use service::chobits::message::audio::AudioMessage;
use service::chobits::message::llm::LlmMessage;
use service::chobits::message::stt::SttMessage;
use service::chobits::message::tts::{TtsMessage, TtsState};
use service::ws::frame::FrameResult;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::Mutex;
use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::mpsc::error::SendError;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tracing::{error, info, trace};

pub struct OutputMessage {
    pub epoch: u64,
    pub round_id: Option<String>,
    pub session_id: String,
    pub payload: FrameResult,
}

pub struct Round {
    pub parent_id: String,
    pub id: String,
    output_tx: UnboundedSender<OutputMessage>,
    pub epoch: u64,
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

async fn change_tts_state(tts_state: Arc<Mutex<Option<TtsState>>>, state: TtsState) {
    let mut tts_state = tts_state.lock().await;
    *tts_state = Some(state);
}

impl Round {
    pub fn new(
        parent_id: String,
        id: String,
        output_tx: UnboundedSender<OutputMessage>,
        epoch: u64,
        client: Arc<Client>,
        tts: Arc<Box<dyn Tts>>,
        cancel: CancellationToken,
    ) -> Self {
        Self {
            parent_id,
            id,
            output_tx,
            epoch,
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
        let output_tx = self.output_tx.clone();
        let epoch = self.epoch;
        let round_id = self.id.clone();
        let stop_me = self.stop.clone();
        let session_id = self.parent_id.clone();
        let client = self.client.clone();
        let tts = self.tts.clone();
        let tts_state_clone = self.tts_state.clone();
        let text = String::from(text);
        let cancel = self.cancel.clone();
        self.join_handle = Some(tokio::spawn(async move {
            let send = |payload: FrameResult| {
                output_tx
                    .send(OutputMessage {
                        epoch,
                        round_id: Some(round_id.clone()),
                        session_id: session_id.clone(),
                        payload,
                    })
                    .map_err(|_| {
                        Box::new(SendError(OutputMessage {
                            epoch,
                            round_id: Some(round_id.clone()),
                            session_id: session_id.clone(),
                            payload: FrameResult::Error(err!(WsErrorCode::InternalError)),
                        }))
                    })
            };

            if send(FrameResult::STTResult(SttMessage::new(
                Some(session_id.clone()),
                Some(text.to_string()),
            )))
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

            change_tts_state(tts_state_clone.clone(), TtsState::Start).await;
            if send(FrameResult::TTSResult(TtsMessage::new(
                Some(session_id.clone()),
                Some(TtsState::Start),
                None,
            )))
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
                        let text = text.clone();
                        let audio_data = result.audio;
                        let tts_state_clone = tts_state_clone.clone();
                        let stop_me_by_tts_packet = stop_me.clone();
                        let result: Result<(), anyhow::Error> = async {
                            let mut llm_msg = LlmMessage::new(
                                Some(session_id.to_string()),
                                Some(emotion.to_string()),
                                Some(EMOJI_MAP.get(emotion).map_or(r#"😶"#, |v| v).to_string()),
                            );
                            llm_msg.full_text = Some(text.clone());
                            send(FrameResult::LLMResult(llm_msg))
                                .context("send llm result failure")?;

                            change_tts_state(tts_state_clone.clone(), TtsState::SentenceStart)
                                .await;
                            send(FrameResult::TTSResult(TtsMessage::new(
                                Some(session_id.to_string()),
                                Some(TtsState::SentenceStart),
                                Some(text.to_string()),
                            )))
                            .context("send tts sentence start failure")?;

                            let audio_data = audio_data.unwrap_or_default();
                            let data = audio_data.into_iter();
                            for packet in data {
                                if stop_me_by_tts_packet.load(Ordering::Relaxed) {
                                    trace!("stop_me_by_tts_packet");
                                    break;
                                }
                                send(FrameResult::AudioResult(AudioMessage::new(
                                    Some(session_id.to_string()),
                                    packet,
                                )))
                                .context("send audio result failure")?;
                            }

                            change_tts_state(tts_state_clone.clone(), TtsState::SentenceEnd).await;
                            send(FrameResult::TTSResult(TtsMessage::new(
                                Some(session_id.to_string()),
                                Some(TtsState::SentenceEnd),
                                None,
                            )))
                            .context("send tts sentence end failure")?;

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
                        if let Err(e) = send(FrameResult::Error(
                            err!(WsErrorCode::TtsEncode).with_extra(e.to_string()),
                        )) {
                            error!(target:"round","{:?}", e);
                        }
                        stop_me.store(true, Ordering::Relaxed);
                        break;
                    }
                }
            }

            change_tts_state(tts_state_clone.clone(), TtsState::Stop).await;
            if send(FrameResult::TTSResult(TtsMessage::new(
                Some(session_id.clone()),
                Some(TtsState::Stop),
                None,
            )))
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
