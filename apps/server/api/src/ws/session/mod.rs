use super::session::listener::Listener;
use super::session::round::{Command, OutputMessage, Round};
use crate::config::audio::AudioConfig;
use crate::config::session::SessionConfig;
use crate::llm::Model;
use crate::llm::client::{ClientBuilder, History};
use crate::mcp::client::device::{DeviceMcpClient, DeviceMcpPhase};
use crate::mcp::mcp_host::{McpHost, UnionMcpHost};
use crate::tts::Tts;
use crate::ws::WsErrorCode;
use crate::ws::session::round::ChatParam;
use chrono::Local;
use framework::prelude::err;
use rig::message::ToolResult;
use service::chobits::message::hello::{AudioParam, HelloMessage};
use service::chobits::message::{AudioFormat, Transport};
use service::ws::frame::{Frame, FrameResult};
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::Instant;

use tokio::select;
use tokio::sync::Mutex;
use tokio::sync::mpsc::{Sender, UnboundedSender, channel};
use tokio_util::sync::CancellationToken;
use tracing;

pub mod listener;
pub mod round;

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

#[derive(Default)]
pub struct SessionBuilder {
    id: Option<String>,
    listener: Option<Box<dyn Listener>>,
    model: Option<Arc<Box<dyn Model>>>,
    tts: Option<Arc<Box<dyn Tts>>>,
    mcp_host: Option<Arc<Mutex<UnionMcpHost>>>,
    config: Option<Arc<SessionConfig>>,
    audio_config: Option<Arc<AudioConfig>>,
}

impl SessionBuilder {
    pub fn new() -> Self {
        Default::default()
    }

    pub fn with_id(mut self, id: String) -> SessionBuilder {
        self.id = Some(id);
        self
    }

    pub fn with_listener(mut self, listener: Box<dyn Listener>) -> SessionBuilder {
        self.listener = Some(listener);
        self
    }

    pub fn with_model(mut self, model: Arc<Box<dyn Model>>) -> SessionBuilder {
        self.model = Some(model);
        self
    }

    pub fn with_tts(mut self, tts: Arc<Box<dyn Tts>>) -> SessionBuilder {
        self.tts = Some(tts);
        self
    }

    pub fn with_mcp_host(mut self, mcp_host: Arc<Mutex<UnionMcpHost>>) -> SessionBuilder {
        self.mcp_host = Some(mcp_host);
        self
    }

    pub fn with_config(mut self, config: Arc<SessionConfig>) -> SessionBuilder {
        self.config = Some(config);
        self
    }

    pub fn with_audio_config(mut self, config: Arc<AudioConfig>) -> SessionBuilder {
        self.audio_config = Some(config);
        self
    }

    pub fn build(
        self,
    ) -> (
        Session,
        tokio::sync::mpsc::UnboundedSender<Frame>,
        tokio::sync::mpsc::UnboundedReceiver<OutputMessage>,
    ) {
        let config = self.config.expect("config is required");
        let audio_config = self.audio_config.expect("audio is required");
        let system_prompt = config
            .system_prompt
            .as_ref()
            .expect("logic system prompt is empty");
        let (session_tx, session_rx) = tokio::sync::mpsc::unbounded_channel::<Frame>();
        let (output_tx, output_rx_inner) = tokio::sync::mpsc::unbounded_channel::<OutputMessage>();
        let (output_tx_outer, output_rx_outer) =
            tokio::sync::mpsc::unbounded_channel::<OutputMessage>();
        let session = Session {
            id: self.id.expect("id is required"),
            started_at: Instant::now(),
            round_count: 0,
            running_round: None,
            shadow_round: None,
            session_rx,
            output_tx,
            output_rx_inner,
            output_tx_outer,
            epoch: 1,
            phase: Phase::Idle,
            latest_activity_time: Arc::new(AtomicI64::new(0)),
            history: Arc::new(Mutex::new(History {
                preamble: Some(system_prompt.to_string()),
                chat_history: vec![],
            })),
            config,
            audio_config,
            listener: self.listener.expect("listener is required"),
            model: self.model.expect("model is required"),
            tts: self.tts.expect("tts is required"),
            mcp_host: self.mcp_host.expect("mcp host is required"),
            device_mcp_phase: DeviceMcpPhase::Initialize,
            device_mcp_call_tool_result_tx: None,
        };
        (session, session_tx, output_rx_outer)
    }
}

pub struct Session {
    pub id: String,
    started_at: Instant,
    round_count: u64,
    running_round: Option<Box<Round>>,
    shadow_round: Option<Box<Round>>,
    session_rx: tokio::sync::mpsc::UnboundedReceiver<Frame>,
    output_tx: UnboundedSender<OutputMessage>,
    output_rx_inner: tokio::sync::mpsc::UnboundedReceiver<OutputMessage>,
    output_tx_outer: UnboundedSender<OutputMessage>,
    epoch: u64,
    phase: Phase,
    latest_activity_time: Arc<AtomicI64>,
    history: Arc<Mutex<History>>,

    config: Arc<SessionConfig>,
    audio_config: Arc<AudioConfig>,

    model: Arc<Box<dyn Model>>,
    tts: Arc<Box<dyn Tts>>,
    listener: Box<dyn Listener>,
    mcp_host: Arc<Mutex<UnionMcpHost>>,
    device_mcp_phase: DeviceMcpPhase,
    device_mcp_call_tool_result_tx: Option<Sender<anyhow::Result<ToolResult>>>,
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
    can_barge_in: bool,
}

#[derive(Debug, Clone)]
pub struct SpeakingParam {
    text: String,
    prob: f32,
}

impl Session {
    pub async fn start(mut self) {
        let (result_tx, result_rx) = channel::<anyhow::Result<ToolResult>>(1);
        self.device_mcp_call_tool_result_tx = Some(result_tx);
        let device = DeviceMcpClient::new(
            Some(self.id.clone()),
            self.output_tx.clone(),
            Arc::new(Mutex::new(result_rx)),
        );
        self.mcp_host
            .lock()
            .await
            .set_device_client(Arc::new(Mutex::new(device)))
            .await;

        let frame_duration = self
            .audio_config
            .output_frame_duration
            .expect("output frame duration is empty");

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

                    // epoch filter: skip stale messages from old rounds
                    if msg.epoch != 0 && msg.epoch < self.epoch {
                        continue;
                    }

                    // audio pacing
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
        let client = ClientBuilder::new()
            .with_session_id(Some(self.id.clone()))
            .with_model(self.model.clone())
            .with_mcp_host(self.mcp_host.clone())
            .build()
            .with_history(self.history.clone())
            .with_max_prompt_len(self.config.max_prompt_len);
        let round_id = framework::id::gen_id();
        let cancel_token = CancellationToken::new();
        let epoch = self.epoch;
        tracing::debug!(session_id = %self.id, round_id = %round_id, epoch, "new round");
        self.shadow_round = Some(Box::new(Round::new(
            self.id.clone(),
            round_id,
            tx,
            epoch,
            Arc::new(client),
            self.tts.clone(),
            cancel_token,
        )));
        if let Some(round) = &mut self.shadow_round {
            round.start().await;
        } else {
            panic!("current round is none");
        }
        tracing::debug!(
            session_id = %self.id,
            running_round_id = ?self.running_round.as_ref().map(|r| &r.id),
            shadow_round_id = ?self.shadow_round.as_ref().map(|r| &r.id),
            "round state",
        );
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
        tracing::debug!(
            session_id = %self.id,
            running_round_id = ?self.running_round.as_ref().map(|r| &r.id),
            shadow_round_id = ?self.shadow_round.as_ref().map(|r| &r.id),
            "round state",
        );
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
            match self.device_mcp_phase {
                DeviceMcpPhase::ToolCall => {
                    let result = DeviceMcpClient::handle_mcp_tool_call_result(message).await;
                    let device_mcp_call_tool_result_tx = self
                        .device_mcp_call_tool_result_tx
                        .clone()
                        .expect("device mcp call tool result tx not exists");
                    if let Err(ex) = device_mcp_call_tool_result_tx.send(result).await {
                        panic!("can't send device mcp call tool result {:?}", ex);
                    }
                }
                _ => {
                    let mcp_host = self.mcp_host.clone();
                    let mut mcp_host = mcp_host.lock().await;
                    let device_mcp_client = mcp_host.get_device_client().await;
                    let device_mcp_client = device_mcp_client.clone();
                    if let Some(device_mcp_client) = device_mcp_client {
                        let mut device_mcp_client = device_mcp_client.lock().await;
                        self.device_mcp_phase = device_mcp_client.handle_mcp(message).await.clone();
                    }
                }
            }
            return;
        }

        let phase = self.phase.clone();
        match phase {
            Phase::Idle => self.on_idle(frame).await,
            Phase::Ready => self.on_ready(frame, forwarding).await,
            Phase::Listening(listening_param) => self.on_listening(frame, &listening_param).await,
            Phase::Speaking(speaking_param) => self.on_speaking(frame, &speaking_param).await,
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
            let mut has_mcp = false;
            if let Some(features) = &hello_message.features
                && let Some(mcp) = features.mcp
            {
                has_mcp = mcp;
            }
            self.handle_connect(hello_message).await;
            self.phase = Phase::Ready;
            if has_mcp {
                let mut mcp_host = self.mcp_host.lock().await;
                let device_mcp_client = mcp_host
                    .get_device_client()
                    .await
                    .clone()
                    .expect("device mcp not exists");
                let mut device_mcp_client = device_mcp_client.lock().await;
                device_mcp_client
                    .request_mcp_initialize(hello_message)
                    .await;
            }
        }
    }

    async fn handle_connect(&mut self, hello_message: &HelloMessage) {
        if let Some(params) = &hello_message.audio_params {
            self.listener.reconfigure(params);
        }
        let audio_config = &self.audio_config;
        let data = HelloMessage {
            message: service::chobits::message::Message {
                mtype: service::chobits::message::Type::Hello,
            },
            transport: Some(Transport::Websocket),
            audio_params: Some(AudioParam {
                format: AudioFormat::Opus,
                sample_rate: audio_config
                    .output_sample_rate
                    .expect("output sample rate is empty"),
                channels: audio_config
                    .output_channel
                    .expect("output channel is empty"),
                frame_duration: audio_config
                    .output_frame_duration
                    .expect("output frame duration is empty"),
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
                        .accept(listener::ListenInput::Audio(data.to_vec()))
                        .await;
                }
                _ => {}
            }
        }
    }

    async fn on_listening(&mut self, frame: &Frame, param: &ListeningParam) {
        let ListeningParam { can_barge_in } = param;
        match frame {
            Frame::ListenStop => {
                self.listener.set_state(listener::ListenState::End);
                let (_, result) = self.listener.take_result().await;
                let (text, prob) = match result {
                    Ok(result) => match result {
                        listener::ListenResult::Text(text) => (text, 1.0),
                        listener::ListenResult::Audio { text, prob } => (text, prob),
                    },
                    Err(e) => {
                        let _ = self.output_tx.send(OutputMessage {
                            epoch: self.epoch,
                            round_id: None,
                            session_id: self.id.clone(),
                            payload: FrameResult::Error(
                                err!(WsErrorCode::AsrFailure).with_extra(e.to_string()),
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
                    .expect("logic silence voice timeout is empty");
                self.listener.reset(Some(silence_voice_timeout)).await;
                Box::pin(self.forwarding_frame(frame)).await;
            }
            Frame::Voice { data } => {
                let is_speaking = match &self.running_round {
                    Some(round) => round.is_speaking().await,
                    None => false,
                };
                if *can_barge_in || !is_speaking {
                    self.listener
                        .accept(listener::ListenInput::Audio(data.to_vec()))
                        .await;
                }
            }
            _ => {}
        }
    }

    async fn on_speaking(&mut self, frame: &Frame, param: &SpeakingParam) {
        let SpeakingParam { text, prob } = param;
        if let Some(round) = &mut self.running_round {
            round
                .accept_command(Command::Chat(ChatParam { text, prob }))
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
