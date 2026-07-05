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
use service::chobits::message::listen::ListenState;
use service::chobits::message::{AudioFormat, Transport};
use service::ws::frame::{Frame, FrameResult};
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};

use tokio::select;
use tokio::sync::Mutex;
use tokio::sync::mpsc::{Sender, UnboundedSender, channel};
use tokio_util::sync::CancellationToken;
use tracing::{error, info};

pub mod listener;
pub mod round;

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
    audio: Option<Vec<f32>>,
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
        info!("session start");

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
                            self.stop().await;
                            break;
                        }
                        Some(msg) => self.accept_frame(&msg).await,
                    }
                }
                msg = self.output_rx_inner.recv() => {
                    let Some(msg) = msg else { break };

                    // epoch filter: skip stale messages from old rounds
                    if msg.epoch != 0 && msg.epoch < self.epoch {
                        continue;
                    }

                    // audio pacing
                    if matches!(msg.payload, Ok(FrameResult::AudioResult(_))) {
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
                        break;
                    }
                }
            }
        }
    }

    pub async fn stop(&mut self) {
        self.stop_round().await;
        let _ = self.output_tx_outer.send(OutputMessage {
            epoch: self.epoch,
            round_id: None,
            session_id: self.id.clone(),
            payload: Ok(FrameResult::CloseResult),
        });
        info!("session stop");
    }

    pub async fn new_round(&mut self) {
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
        info!("new round");
    }

    pub async fn stop_round(&mut self) {
        if let Some(round) = &mut self.running_round {
            round.stop().await;
            round.join_handle.take();
        }
        info!("stop round");
    }

    pub async fn accept_frame(&mut self, frame: &Frame) {
        self._accept_frame(frame, false).await;
    }

    async fn forwarding_frame(&mut self, frame: &Frame) {
        self._accept_frame(frame, true).await;
    }

    async fn _accept_frame(&mut self, frame: &Frame, forwarding: bool) {
        match frame {
            Frame::Close(_) => {
                info!(target:"session","close");
                self.stop().await;
                return;
            }
            Frame::Abort(_) => {
                info!(target:"session","abort");
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
                    } else {
                        error!("mcp device client not exists");
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
        info!("on_idle");
        self.new_round().await;
        match frame {
            Frame::Hello(hello_message) => {
                let mut has_mcp = false;
                if let Some(features) = &hello_message.features
                    && let Some(mcp) = features.mcp
                {
                    has_mcp = mcp;
                }
                self.handle_connect(hello_message).await;
                self.phase = Phase::Ready;
                if has_mcp {
                    //init Device MCP client
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
            _ => {
                error!(
                    "invalid frame in phase = {:?},frame = {:?}",
                    self.phase, frame
                );
            }
        }
    }

    async fn handle_connect(&mut self, _hello_message: &HelloMessage) {
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
            payload: Ok(FrameResult::HelloResult(data)),
        });
    }

    async fn on_ready(&mut self, frame: &Frame, forwarding: bool) {
        info!("on_ready");
        if forwarding {
            self.new_round().await;
        } else {
            match frame {
                Frame::Listen(listen_message) => {
                    let state = &listen_message.state;
                    match state {
                        ListenState::Start => {
                            if self.running_round.is_some() {
                                self.epoch += 1;
                                self.stop_round().await;
                            }
                            let mode = &listen_message.mmod;
                            if let Some(mode) = mode {
                                match mode {
                                    service::chobits::message::listen::ListenMode::Auto => {
                                        self.phase = Phase::Listening(ListeningParam {
                                            can_barge_in: false,
                                        });
                                    }
                                    service::chobits::message::listen::ListenMode::Manual => {
                                        self.phase =
                                            Phase::Listening(ListeningParam { can_barge_in: true });
                                    }
                                    service::chobits::message::listen::ListenMode::RealTime => {
                                        self.phase =
                                            Phase::Listening(ListeningParam { can_barge_in: true });
                                    }
                                }
                                self.new_round().await;
                            } else {
                                error!(
                                    "invalid frame in phase = {:?},frame = {:?}, state = {:?}",
                                    self.phase, frame, state
                                );
                            }
                        }
                        ListenState::Detect => {
                            let text = &listen_message.text;
                            match text {
                                Some(text) => {
                                    if self.running_round.is_some() {
                                        self.epoch += 1;
                                        self.stop_round().await;
                                    }
                                    self.running_round = self.shadow_round.take();
                                    self.phase = Phase::Speaking(SpeakingParam {
                                        audio: None,
                                        text: text.to_string(),
                                        prob: 1.0,
                                    });
                                    Box::pin(self.forwarding_frame(frame)).await;
                                }
                                None => {
                                    error!(
                                        "invalid frame in phase = {:?},frame = {:?}, text = {:?}",
                                        self.phase, frame, text
                                    );
                                }
                            }
                        }
                        _ => {
                            error!(
                                "invalid frame in phase = {:?},frame = {:?}, state = {:?}",
                                self.phase, frame, state
                            );
                        }
                    }
                }
                Frame::Voice { data } => {
                    self.listener
                        .accept(listener::ListenInput::Audio(data.to_vec()))
                        .await;
                }
                _ => {
                    error!(
                        "invalid frame in phase = {:?},frame = {:?}",
                        self.phase, frame
                    );
                }
            }
        }
    }

    async fn on_listening(&mut self, frame: &Frame, param: &ListeningParam) {
        info!("on_listening,{:?}", param);
        let ListeningParam { can_barge_in } = param;
        match frame {
            Frame::Listen(listen_message) => {
                let state = &listen_message.state;
                match state {
                    ListenState::Stop => {
                        self.listener
                            .set_state(crate::ws::session::listener::ListenState::End);
                        let (audio, result) = self.listener.take_result().await;
                        let audio = if audio.is_empty() { None } else { Some(audio) };
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
                                    payload: Err(
                                        err!(WsErrorCode::AsrFailure).with_extra(e.to_string())
                                    ),
                                });
                                return;
                            }
                        };
                        self.phase = Phase::Speaking(SpeakingParam { audio, text, prob });
                        self.running_round = self.shadow_round.take();
                        let silence_voice_timeout = self
                            .config
                            .silence_voice_timeout
                            .expect("logic silence voice timeout is empty");
                        self.listener.reset(Some(silence_voice_timeout)).await;
                        // forwarding frame
                        Box::pin(self.forwarding_frame(frame)).await;
                    }
                    _ => {
                        error!(
                            "invalid frame in phase = {:?},frame = {:?}",
                            self.phase, frame
                        );
                    }
                }
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
            _ => {
                error!(
                    "invalid frame in phase = {:?},frame = {:?}",
                    self.phase, frame
                );
            }
        }
    }

    async fn on_speaking(&mut self, frame: &Frame, param: &SpeakingParam) {
        let SpeakingParam { audio, text, prob } = param;
        if let Some(audio) = audio {
            info!("on_speaking, audio len = {}", audio.len());
        }
        info!("on_speaking,text = {},prob = {}", text, prob);
        if let Some(round) = &mut self.running_round {
            round
                .accept_command(Command::Chat(ChatParam { text, prob }))
                .await;
        } else {
            panic!("current round is none");
        }
        self.phase = Phase::Ready;
        // forwarding frame
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
