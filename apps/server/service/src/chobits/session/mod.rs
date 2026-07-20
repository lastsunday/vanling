pub mod history;
pub mod round;

use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;
use tokio::select;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

use crate::chobits::chii::Chii;
use crate::chobits::frame::{Frame, InputMode};
use crate::chobits::frame::{FrameResult, OutputMessage};
use crate::chobits::listener::{ListenInput, ListenResult, ListenState, Listener};
use crate::chobits::message::hello::{AudioParam, HelloMessage};
use crate::chobits::message::tts::TtsState;
use crate::chobits::message::{AudioFormat, Transport};
use crate::chobits::tts::Tts;

use framework::err;
use framework::prelude::error;

use round::{ChatParam, Command, Round};

pub struct SessionContext {
    pub session: Session,
    pub input_tx: UnboundedSender<Frame>,
    pub output_rx: UnboundedReceiver<OutputMessage>,
}

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
    NoActivityTimeout,
}

impl std::fmt::Display for SessionEndReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ClientClose => write!(f, "client_close"),
            Self::ClientDisconnect => write!(f, "client_disconnect"),
            Self::ChannelClosed => write!(f, "channel_closed"),
            Self::NoActivityTimeout => write!(f, "no_activity_timeout"),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct SessionConfig {
    pub system_prompt: Option<String>,
    pub max_prompt_len: Option<u64>,
    pub silence_voice_timeout: Option<i64>,
    pub close_connection_no_activity_time: Option<i64>,
}

pub struct AudioConfig {
    pub output_sample_rate: u32,
    pub output_channel: u32,
    pub output_frame_duration: u64,
}

#[derive(Debug, Clone)]
pub enum Phase {
    Idle,
    Listening(ListeningParam),
    Speaking(SpeakingParam),
}

#[derive(Debug, Clone)]
pub struct ListeningParam {
    pub can_barge_in: bool,
    pub is_wake: bool,
    pub is_voice_break_detect: bool,
}

#[derive(Debug, Clone)]
pub struct SpeakingParam {
    pub text: String,
    pub prob: f32,
    pub can_barge_in: bool,
    pub is_wake: bool,
    pub is_voice_break_detect: bool,
}

impl std::fmt::Display for ListeningParam {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Listening(barge_in={},wake={},voice_break={})",
            self.can_barge_in, self.is_wake, self.is_voice_break_detect,
        )
    }
}

impl std::fmt::Display for SpeakingParam {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Speaking(text=\"{}\",prob={},barge_in={},wake={})",
            self.text, self.prob, self.can_barge_in, self.is_wake,
        )
    }
}

impl std::fmt::Display for Phase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Phase::Idle => write!(f, "Idle"),
            Phase::Listening(p) => std::fmt::Display::fmt(p, f),
            Phase::Speaking(p) => std::fmt::Display::fmt(p, f),
        }
    }
}

#[derive(Default)]
pub struct SessionBuilder {
    id: Option<String>,
    listener: Option<Box<dyn Listener>>,
    chii: Option<Arc<dyn Chii>>,
    tts: Option<Arc<dyn Tts>>,
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

    pub fn with_chii(mut self, chii: Arc<dyn Chii>) -> Self {
        self.chii = Some(chii);
        self
    }

    pub fn with_tts(mut self, tts: Arc<dyn Tts>) -> Self {
        self.tts = Some(tts);
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

    pub fn build(self) -> SessionContext {
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
            idle_since: None,
            config,
            audio_config,
            listener: self.listener.expect("listener is required"),
            chii: self.chii.expect("chii is required"),
            tts: self.tts.expect("tts is required"),
        };

        SessionContext {
            session,
            input_tx: session_tx,
            output_rx: outer_rx,
        }
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
    idle_since: Option<Instant>,

    config: SessionConfig,
    audio_config: AudioConfig,

    chii: Arc<dyn Chii>,
    tts: Arc<dyn Tts>,
    listener: Box<dyn Listener>,
}

impl Session {
    pub async fn start(mut self) {
        let frame_duration = self.audio_config.output_frame_duration;
        let mut audio_pacer: Option<tokio::time::Interval> = None;
        let mut tick = tokio::time::interval(Duration::from_millis(100));
        tick.tick().await; // discard first immediate tick

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
                    if self.check_idle_timeout() {
                        self.stop(SessionEndReason::NoActivityTimeout).await;
                        break;
                    }
                }
                _ = tick.tick() => {
                    if let Phase::Listening(param) = &self.phase
                        && param.is_voice_break_detect
                        && self.listener.poll_timeout().is_some()
                    {
                        self.forwarding_frame(&Frame::ListenStop).await;
                    }

                    if self.check_idle_timeout() {
                        self.stop(SessionEndReason::NoActivityTimeout).await;
                        break;
                    }
                }
                msg = self.output_rx_inner.recv() => {
                    let Some(msg) = msg else {
                        self.stop(SessionEndReason::ChannelClosed).await;
                        break;
                    };

                    if msg.epoch != 0 && msg.epoch < self.epoch
                        && !matches!(
                            &msg.payload,
                            FrameResult::TTSResult(tts)
                                if matches!(tts.state, Some(TtsState::Stop) | Some(TtsState::SentenceEnd))
                        )
                    {
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

    fn check_idle_timeout(&mut self) -> bool {
        if let Phase::Listening(param) = &self.phase
            && !param.is_voice_break_detect
        {
            return false;
        }
        let round_idle = self.running_round.as_ref().is_none_or(|r| {
            r.tts_state
                .try_lock()
                .is_ok_and(|state| state.as_ref().is_some_and(|s| matches!(s, TtsState::Stop)))
        });
        let is_idle = round_idle
            && matches!(
                self.listener.get_state(),
                ListenState::Idle | ListenState::End | ListenState::Listening { is_speech: false }
            );
        if is_idle {
            if self.idle_since.is_none() {
                self.idle_since = Some(Instant::now());
                tracing::debug!(component = "session", event = "idle_started", session_id = %self.id, "idle started");
            }
            if let Some(timeout) = self.config.close_connection_no_activity_time
                && let Some(since) = self.idle_since
                && since.elapsed() >= Duration::from_millis(timeout as u64)
            {
                tracing::info!(
                    component = "session", event = "no_activity_timeout",
                    session_id = %self.id, timeout_ms = timeout,
                    "no activity timeout, closing connection"
                );
                return true;
            }
        } else if self.idle_since.take().is_some() {
            tracing::debug!(component = "session", event = "idle_cancelled", session_id = %self.id, "idle cancelled");
        }
        false
    }

    pub async fn stop(&mut self, reason: SessionEndReason) {
        let duration_ms = self.started_at.elapsed().as_millis() as u64;
        self.stop_round(RoundStopReason::SessionEnd).await;
        tracing::info!(component = "session", event = "session_ended", session_id = %self.id, rounds = self.round_count, duration_ms, %reason, "session ended");
        let _ = self.output_tx_outer.send(OutputMessage {
            epoch: self.epoch,
            round_id: None,
            session_id: self.id.clone(),
            payload: FrameResult::CloseResult,
        });
    }

    pub async fn new_shadow_round(&mut self) {
        self.round_count += 1;
        let tx = self.output_tx.clone();
        let round_id = framework::id::gen_id();
        let cancel = tokio_util::sync::CancellationToken::new();
        let epoch = self.epoch;
        tracing::debug!(component = "session", event = "new_round", session_id = %self.id, round_id = %round_id, epoch, "new round");
        self.shadow_round = Some(Box::new(Round::new(
            self.id.clone(),
            round_id,
            tx,
            epoch,
            self.chii.clone(),
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
            tracing::debug!(component = "session", event = "round_stopped", session_id = %self.id, round_id = %round.id, %reason, "round stopped");
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
                    component = "session", event = "round_upgraded",
                    session_id = %self.id, epoch = self.epoch, round_id = %round.id,
                    "round upgraded",
                );
            }
            self.stop_round(RoundStopReason::Upgrade).await;
        } else if let Some(round) = &self.shadow_round {
            tracing::debug!(
                component = "session", event = "shadow_round_running",
                session_id = %self.id, epoch = round.epoch, round_id = %round.id,
                "shadow round running",
            );
        }
        self.running_round = self.shadow_round.take();
    }

    pub async fn accept_frame(&mut self, frame: &Frame) {
        self._accept_frame(frame).await;
    }

    async fn forwarding_frame(&mut self, frame: &Frame) {
        self._accept_frame(frame).await;
    }

    async fn _accept_frame(&mut self, frame: &Frame) {
        match frame {
            Frame::Close(_reason) => {
                self.stop(SessionEndReason::ClientClose).await;
                return;
            }
            Frame::Abort(_) => {
                if self.running_round.is_some() {
                    self.epoch += 1;
                    self.stop_round(RoundStopReason::BargeIn).await;
                }
                return;
            }
            Frame::Ping { .. } | Frame::Pong { .. } => return,
            _ => {}
        }

        let phase = self.phase.clone();
        match phase {
            Phase::Idle => self.on_idle(frame).await,
            Phase::Listening(ref param) => self.on_listening(frame, param).await,
            Phase::Speaking(ref param) => self.on_speaking(frame, param).await,
        }
    }

    fn set_phase(&mut self, new_phase: Phase) {
        tracing::debug!(component = "session", event = "phase_changed", session_id = %self.id, from = %self.phase, to = %new_phase, "phase changed");
        self.phase = new_phase;
    }

    async fn on_idle(&mut self, frame: &Frame) {
        self.new_shadow_round().await;
        if let Frame::Hello(hello_message) = frame {
            self.handle_connect(hello_message).await;
            self.set_phase(Phase::Listening(ListeningParam {
                can_barge_in: false,
                is_wake: false,
                is_voice_break_detect: false,
            }));
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

    async fn on_listening(&mut self, frame: &Frame, param: &ListeningParam) {
        match frame {
            Frame::ListenStart {
                barge_in,
                is_voice_break_detect,
            } => {
                if !param.is_wake && self.running_round.is_some() {
                    self.epoch += 1;
                    self.stop_round(RoundStopReason::BargeIn).await;
                }
                self.set_phase(Phase::Listening(ListeningParam {
                    can_barge_in: *barge_in,
                    is_wake: param.is_wake,
                    is_voice_break_detect: is_voice_break_detect.to_owned(),
                }));
            }
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
                        tracing::warn!(component = "session", event = "asr_failed", session_id = %self.id, error = %e, "asr failed");
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
                if text.trim().is_empty() {
                    tracing::debug!(component = "session", event = "asr_empty_text", session_id = %self.id, "asr: empty text, skipping");
                    let silence_voice_timeout = self.config.silence_voice_timeout.unwrap_or(1200);
                    self.listener.reset(Some(silence_voice_timeout)).await;
                    return;
                }
                tracing::debug!(component = "session", event = "asr_result", session_id = %self.id, text = %text, prob, "asr result");
                self.set_phase(Phase::Speaking(SpeakingParam {
                    text,
                    prob,
                    can_barge_in: param.can_barge_in,
                    is_wake: param.is_wake,
                    is_voice_break_detect: param.is_voice_break_detect,
                }));
                self.upgrade_shadow_round().await;
                let silence_voice_timeout = self.config.silence_voice_timeout.unwrap_or(1200);
                self.listener.reset(Some(silence_voice_timeout)).await;
                Box::pin(self.forwarding_frame(frame)).await;
            }
            Frame::Input { text, mode } => {
                let is_wake = matches!(mode, InputMode::Wake);
                tracing::debug!(
                    component = "session", event = "input_received",
                    session_id = %self.id,
                    text_len = text.len(),
                    is_wake,
                    has_running_round = self.running_round.is_some(),
                    has_shadow_round = self.shadow_round.is_some(),
                    "input received, will interrupt: {}",
                    self.running_round.is_some(),
                );
                if is_wake {
                    let silence_voice_timeout = self.config.silence_voice_timeout.unwrap_or(1200);
                    self.listener.reset(Some(silence_voice_timeout)).await;
                }
                self.upgrade_shadow_round().await;
                self.set_phase(Phase::Speaking(SpeakingParam {
                    text: text.clone(),
                    prob: 1.0,
                    can_barge_in: true,
                    is_wake,
                    is_voice_break_detect: param.is_voice_break_detect,
                }));
                Box::pin(self.forwarding_frame(frame)).await;
            }
            Frame::Voice { data } => {
                let prev_state = self.listener.get_state();
                self.listener
                    .accept(ListenInput::Audio(data.to_vec()))
                    .await;
                let new_state = self.listener.get_state();
                if param.can_barge_in
                    && self.running_round.is_some()
                    && prev_state != (ListenState::Listening { is_speech: true })
                    && new_state == (ListenState::Listening { is_speech: true })
                {
                    tracing::debug!(component = "session", event = "barge_in", session_id = %self.id, "voice break detect: barge-in");
                    self.epoch += 1;
                    self.stop_round(RoundStopReason::BargeIn).await;
                    self.new_shadow_round().await;
                }
                if param.is_voice_break_detect && matches!(new_state, ListenState::End) {
                    Box::pin(self.forwarding_frame(&Frame::ListenStop)).await;
                }
            }
            _ => {}
        }
    }

    async fn on_speaking(&mut self, _frame: &Frame, param: &SpeakingParam) {
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
        self.set_phase(Phase::Listening(ListeningParam {
            can_barge_in: param.can_barge_in,
            is_wake: param.is_wake,
            is_voice_break_detect: param.is_voice_break_detect,
        }));
        self.new_shadow_round().await;
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
