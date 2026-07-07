pub mod history;
pub mod round;

use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::Instant;

use chrono::Local;
use tokio::select;
use tokio::sync::Mutex;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

use crate::chobits::frame::Frame;
use crate::chobits::frame::{FrameResult, OutputMessage};
use crate::chobits::listener::{ListenInput, ListenResult, Listener};
use crate::chobits::llm::Llm;
use crate::chobits::mcp::Mcp;
use crate::chobits::message::hello::{AudioParam, HelloMessage};
use crate::chobits::message::{AudioFormat, Transport};
use crate::chobits::tts::Tts;

use framework::err;
use framework::prelude::error;

use round::{ChatParam, Command, Round};

#[derive(Debug, Clone, Copy)]
pub enum RoundStopReason {
    BargeIn,
    Upgrade,
    SessionEnd,
}

impl std::fmt::Display for RoundStopReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BargeIn => write!(f, "barge_in"),
            Self::Upgrade => write!(f, "upgrade"),
            Self::SessionEnd => write!(f, "session_end"),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum SessionEndReason {
    ClientClose,
    ClientDisconnect,
    ChannelClosed,
}

impl std::fmt::Display for SessionEndReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ClientClose => write!(f, "client_close"),
            Self::ClientDisconnect => write!(f, "client_disconnect"),
            Self::ChannelClosed => write!(f, "channel_closed"),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct SessionConfig {
    pub system_prompt: Option<String>,
    pub max_prompt_len: Option<u64>,
    pub silence_voice_timeout: Option<i64>,
    pub close_connection_no_voice_time: Option<i64>,
}

pub struct AudioConfig {
    pub output_sample_rate: u32,
    pub output_channel: u32,
    pub output_frame_duration: u64,
}

#[derive(Debug, Clone)]
pub enum Phase {
    Idle,
    Ready,
    Listening(ListeningParam),
    Speaking(SpeakingParam),
}

#[derive(Debug, Clone)]
pub struct ListeningParam {
    pub can_barge_in: bool,
}

#[derive(Debug, Clone)]
pub struct SpeakingParam {
    pub text: String,
    pub prob: f32,
}

#[derive(Default)]
pub struct SessionBuilder {
    id: Option<String>,
    listener: Option<Box<dyn Listener>>,
    llm: Option<Arc<dyn Llm>>,
    tts: Option<Arc<dyn Tts>>,
    mcp: Option<Arc<Mutex<dyn Mcp>>>,
    config: Option<SessionConfig>,
    audio_config: Option<AudioConfig>,
}

impl SessionBuilder {
    pub fn new() -> Self {
        Default::default()
    }

    pub fn with_id(mut self, id: String) -> Self {
        self.id = Some(id);
        self
    }

    pub fn with_listener(mut self, listener: Box<dyn Listener>) -> Self {
        self.listener = Some(listener);
        self
    }

    pub fn with_llm(mut self, llm: Arc<dyn Llm>) -> Self {
        self.llm = Some(llm);
        self
    }

    pub fn with_tts(mut self, tts: Arc<dyn Tts>) -> Self {
        self.tts = Some(tts);
        self
    }

    pub fn with_mcp(mut self, mcp: Arc<Mutex<dyn Mcp>>) -> Self {
        self.mcp = Some(mcp);
        self
    }

    pub fn with_config(mut self, config: SessionConfig) -> Self {
        self.config = Some(config);
        self
    }

    pub fn with_audio_config(mut self, config: AudioConfig) -> Self {
        self.audio_config = Some(config);
        self
    }

    pub fn build(
        self,
    ) -> (
        Session,
        UnboundedSender<Frame>,
        UnboundedReceiver<OutputMessage>,
    ) {
        let config = self.config.expect("config is required");
        let audio_config = self.audio_config.expect("audio is required");

        let (session_tx, session_rx) = tokio::sync::mpsc::unbounded_channel::<Frame>();
        let (inner_tx, inner_rx) = tokio::sync::mpsc::unbounded_channel::<OutputMessage>();
        let (outer_tx, outer_rx) = tokio::sync::mpsc::unbounded_channel::<OutputMessage>();

        let session = Session {
            id: self.id.expect("id is required"),
            started_at: Instant::now(),
            round_count: 0,
            running_round: None,
            shadow_round: None,
            session_rx,
            output_tx: inner_tx,
            output_rx_inner: inner_rx,
            output_tx_outer: outer_tx,
            epoch: 1,
            phase: Phase::Idle,
            latest_activity_time: Arc::new(AtomicI64::new(0)),
            config,
            audio_config,
            listener: self.listener.expect("listener is required"),
            llm: self.llm.expect("llm is required"),
            tts: self.tts.expect("tts is required"),
            mcp: self.mcp.expect("mcp is required"),
        };

        (session, session_tx, outer_rx)
    }
}

pub struct Session {
    pub id: String,
    started_at: Instant,
    round_count: u64,
    running_round: Option<Box<Round>>,
    shadow_round: Option<Box<Round>>,
    session_rx: UnboundedReceiver<Frame>,
    output_tx: UnboundedSender<OutputMessage>,
    output_rx_inner: UnboundedReceiver<OutputMessage>,
    output_tx_outer: UnboundedSender<OutputMessage>,
    epoch: u64,
    phase: Phase,
    latest_activity_time: Arc<AtomicI64>,

    config: SessionConfig,
    audio_config: AudioConfig,

    llm: Arc<dyn Llm>,
    tts: Arc<dyn Tts>,
    listener: Box<dyn Listener>,
    mcp: Arc<Mutex<dyn Mcp>>,
}

impl Session {
    pub async fn start(mut self) {
        let frame_duration = self.audio_config.output_frame_duration;
        let mut audio_pacer: Option<tokio::time::Interval> = None;

        loop {
            select! {
                msg = self.session_rx.recv() => {
                    match msg {
                        Some(Frame::Close(_)) | None => {
                            self.stop(SessionEndReason::ClientDisconnect).await;
                            break;
                        }
                        Some(msg) => self.accept_frame(&msg).await,
                    }
                }
                msg = self.output_rx_inner.recv() => {
                    let Some(msg) = msg else {
                        self.stop(SessionEndReason::ChannelClosed).await;
                        break;
                    };

                    if msg.epoch != 0 && msg.epoch < self.epoch {
                        continue;
                    }

                    if matches!(msg.payload, FrameResult::AudioResult(_)) {
                        audio_pacer.get_or_insert_with(|| {
                            tokio::time::interval_at(
                                tokio::time::Instant::now() + tokio::time::Duration::from_millis(frame_duration),
                                tokio::time::Duration::from_millis(frame_duration),
                            )
                        });
                        if let Some(pacer) = &mut audio_pacer {
                            pacer.tick().await;
                        }
                    } else {
                        audio_pacer = None;
                    }

                    if self.output_tx_outer.send(msg).is_err() {
                        self.stop(SessionEndReason::ChannelClosed).await;
                        break;
                    }
                }
            }
        }
    }

    pub async fn stop(&mut self, reason: SessionEndReason) {
        let duration_ms = self.started_at.elapsed().as_millis() as u64;
        self.stop_round(RoundStopReason::SessionEnd).await;
        tracing::info!(session_id = %self.id, rounds = self.round_count, duration_ms, %reason, "session ended");
        let _ = self.output_tx_outer.send(OutputMessage {
            epoch: self.epoch,
            round_id: None,
            session_id: self.id.clone(),
            payload: FrameResult::CloseResult,
        });
    }

    pub async fn new_round(&mut self) {
        self.round_count += 1;
        let tx = self.output_tx.clone();
        let round_id = framework::id::gen_id();
        let cancel = tokio_util::sync::CancellationToken::new();
        let epoch = self.epoch;
        tracing::debug!(session_id = %self.id, round_id = %round_id, epoch, "new round");
        self.shadow_round = Some(Box::new(Round::new(
            self.id.clone(),
            round_id,
            tx,
            epoch,
            self.llm.clone(),
            self.tts.clone(),
            cancel,
        )));
        if let Some(round) = &mut self.shadow_round {
            round.start().await;
        } else {
            panic!("current round is none");
        }
    }

    pub async fn stop_round(&mut self, reason: RoundStopReason) {
        if let Some(round) = &mut self.running_round {
            tracing::debug!(session_id = %self.id, round_id = %round.id, %reason, "round stopped");
            round.stop().await;
            round.join_handle.take();
        }
    }

    async fn upgrade_shadow_round(&mut self) {
        if self.running_round.is_some() {
            self.epoch += 1;
            if let Some(round) = &mut self.shadow_round {
                round.epoch = self.epoch;
                tracing::debug!(
                    session_id = %self.id, epoch = self.epoch, round_id = %round.id,
                    "round upgraded",
                );
            }
            self.stop_round(RoundStopReason::Upgrade).await;
        } else if let Some(round) = &self.shadow_round {
            tracing::debug!(
                session_id = %self.id, epoch = round.epoch, round_id = %round.id,
                "shadow round running",
            );
        }
        self.running_round = self.shadow_round.take();
    }

    pub async fn accept_frame(&mut self, frame: &Frame) {
        self._accept_frame(frame, false).await;
    }

    async fn forwarding_frame(&mut self, frame: &Frame) {
        self._accept_frame(frame, true).await;
    }

    async fn _accept_frame(&mut self, frame: &Frame, forwarding: bool) {
        match frame {
            Frame::Close(reason) => {
                tracing::debug!(session_id = %self.id, reason = reason.code, "close frame");
                self.stop(SessionEndReason::ClientClose).await;
                return;
            }
            Frame::Abort(_) => {
                tracing::debug!(session_id = %self.id, "abort received");
                return;
            }
            Frame::Ping { .. } | Frame::Pong { .. } => return,
            _ => {}
        }

        if let Frame::Mcp(message) = frame {
            let mut mcp = self.mcp.lock().await;
            mcp.handle_frame(message, &self.output_tx).await;
            return;
        }

        let phase = self.phase.clone();
        match phase {
            Phase::Idle => self.on_idle(frame).await,
            Phase::Ready => self.on_ready(frame, forwarding).await,
            Phase::Listening(ref param) => self.on_listening(frame, param).await,
            Phase::Speaking(ref param) => self.on_speaking(frame, param).await,
        }
    }

    async fn on_idle(&mut self, frame: &Frame) {
        self.new_round().await;
        if let Frame::Hello(hello_message) = frame {
            tracing::debug!(
                session_id = %self.id,
                version = hello_message.version,
                transport = ?hello_message.transport,
                "client hello"
            );
            let has_mcp = hello_message
                .features
                .as_ref()
                .and_then(|f| f.mcp)
                .unwrap_or(false);

            self.handle_connect(hello_message).await;
            self.phase = Phase::Ready;

            if has_mcp {
                let mut mcp = self.mcp.lock().await;
                mcp.handle_hello(hello_message, &self.output_tx).await;
            }
        }
    }

    async fn handle_connect(&mut self, hello_message: &HelloMessage) {
        if let Some(params) = &hello_message.audio_params {
            self.listener.reconfigure(params);
        }
        let audio_config = &self.audio_config;
        let data = HelloMessage {
            message: crate::chobits::message::Message {
                mtype: crate::chobits::message::Type::Hello,
            },
            transport: Some(Transport::Websocket),
            audio_params: Some(AudioParam {
                format: AudioFormat::Opus,
                sample_rate: audio_config.output_sample_rate,
                channels: audio_config.output_channel,
                frame_duration: audio_config.output_frame_duration,
            }),
            version: None,
            features: None,
            session_id: Some(self.id.clone()),
        };
        let _ = self.output_tx_outer.send(OutputMessage {
            epoch: 0,
            round_id: None,
            session_id: self.id.clone(),
            payload: FrameResult::HelloResult(data),
        });
    }

    async fn on_ready(&mut self, frame: &Frame, forwarding: bool) {
        if forwarding {
            self.new_round().await;
        } else {
            match frame {
                Frame::ListenStart { barge_in } => {
                    if self.running_round.is_some() {
                        self.epoch += 1;
                        self.stop_round(RoundStopReason::BargeIn).await;
                    }
                    self.phase = Phase::Listening(ListeningParam {
                        can_barge_in: *barge_in,
                    });
                    self.new_round().await;
                }
                Frame::Input { text } => {
                    self.upgrade_shadow_round().await;
                    self.phase = Phase::Speaking(SpeakingParam {
                        text: text.clone(),
                        prob: 1.0,
                    });
                    Box::pin(self.forwarding_frame(frame)).await;
                }
                Frame::Voice { data } => {
                    self.listener
                        .accept(ListenInput::Audio(data.to_vec()))
                        .await;
                }
                _ => {}
            }
        }
    }

    async fn on_listening(&mut self, frame: &Frame, param: &ListeningParam) {
        match frame {
            Frame::ListenStop => {
                self.listener
                    .set_state(crate::chobits::listener::ListenState::End);
                let (_, result) = self.listener.take_result().await;
                let (text, prob) = match result {
                    Ok(result) => match result {
                        ListenResult::Text(text) => (text, 1.0),
                        ListenResult::Audio { text, prob } => (text, prob),
                    },
                    Err(e) => {
                        let _ = self.output_tx.send(OutputMessage {
                            epoch: self.epoch,
                            round_id: None,
                            session_id: self.id.clone(),
                            payload: FrameResult::Error(
                                err!(SessionErrorCode::AsrFailure).with_extra(e.to_string()),
                            ),
                        });
                        return;
                    }
                };
                self.phase = Phase::Speaking(SpeakingParam { text, prob });
                self.upgrade_shadow_round().await;
                let silence_voice_timeout = self
                    .config
                    .silence_voice_timeout
                    .expect("silence voice timeout is empty");
                self.listener.reset(Some(silence_voice_timeout)).await;
                Box::pin(self.forwarding_frame(frame)).await;
            }
            Frame::Voice { data } => {
                let is_speaking = match &self.running_round {
                    Some(round) => round.is_speaking().await,
                    None => false,
                };
                if param.can_barge_in || !is_speaking {
                    self.listener
                        .accept(ListenInput::Audio(data.to_vec()))
                        .await;
                }
            }
            _ => {}
        }
    }

    async fn on_speaking(&mut self, frame: &Frame, param: &SpeakingParam) {
        if let Some(round) = &mut self.running_round {
            round
                .accept_command(Command::Chat(ChatParam {
                    text: param.text.clone(),
                    prob: param.prob,
                }))
                .await;
        } else {
            panic!("current round is none");
        }
        self.phase = Phase::Ready;
        Box::pin(self.forwarding_frame(frame)).await;
    }

    pub fn update_latest_activity_time(&self) {
        self.latest_activity_time
            .store(Local::now().timestamp_millis(), Ordering::Release);
    }

    pub fn get_latest_activity_time(&self) -> Option<i64> {
        let time = self.latest_activity_time.load(Ordering::Acquire);
        if time == 0 { None } else { Some(time) }
    }
}

#[error]
pub enum SessionErrorCode {
    ListenFailure = 504001,
    TtsEncode = 504002,
    TtsText = 504003,
    AsrFailure = 504004,
    LlmFailure = 504005,
    InternalError = 504006,
}
