pub mod history;
pub mod round;

use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;
use tokio::select;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

use crate::frame::{Frame, InputMode};
use crate::frame::{FrameResult, OutputMessage};
use crate::message::hello::{AudioParam, HelloMessage};
use crate::message::tts::TtsState;
use crate::message::{AudioFormat, Transport};
use crate::pipeline::{AudioSpec, Node, NodeChain, PipelineEvent, compose_chain};
use crate::session::round::{Round, RoundEvent};
use crate::types::EmptyKind;

use framework::prelude::error;

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

const BARGE_IN_LOCKOUT_DEFAULT_MS: u64 = 250;

#[derive(Debug, Clone, Default)]
pub struct SessionConfig {
    pub silence_voice_timeout: Option<i64>,
    pub close_connection_no_activity_time: Option<i64>,
    /// TTS 起止后的 barge-in 锁定期（限制打断），默认 250ms。
    pub barge_in_lockout_ms: Option<u64>,
}

impl SessionConfig {
    pub fn silence_voice_timeout_ms(&self) -> i64 {
        self.silence_voice_timeout.unwrap_or(1200)
    }
}

#[derive(Debug, Clone)]
pub enum Phase {
    Idle,
    Listening(ListeningParam),
    Speaking(SpeakingParam),
}

#[derive(Debug, Clone, Default)]
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

impl From<&ListeningParam> for ListeningParam {
    /// 重新监听：仅清除唤醒标记，保留 barge-in / 断音检测。
    fn from(p: &ListeningParam) -> Self {
        Self {
            can_barge_in: p.can_barge_in,
            is_wake: false,
            is_voice_break_detect: p.is_voice_break_detect,
        }
    }
}

impl From<&SpeakingParam> for ListeningParam {
    /// 表达结束转回监听：清唤醒标记，保留 barge-in / 断音检测。
    fn from(p: &SpeakingParam) -> Self {
        Self {
            can_barge_in: p.can_barge_in,
            is_wake: false,
            is_voice_break_detect: p.is_voice_break_detect,
        }
    }
}

impl ListeningParam {
    /// 监听达成（TurnComplete）转表达，继承全部监听特征。
    fn to_speaking(&self, text: String, prob: f32) -> SpeakingParam {
        SpeakingParam {
            text,
            prob,
            can_barge_in: self.can_barge_in,
            is_wake: self.is_wake,
            is_voice_break_detect: self.is_voice_break_detect,
        }
    }
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
    node_templates: Option<Vec<Arc<dyn Node>>>,
    config: Option<SessionConfig>,
}

impl SessionBuilder {
    pub fn new() -> Self {
        Default::default()
    }

    pub fn with_id(mut self, id: String) -> Self {
        self.id = Some(id);
        self
    }

    pub fn with_node_templates(mut self, node_templates: Vec<Arc<dyn Node>>) -> Self {
        self.node_templates = Some(node_templates);
        self
    }

    pub fn with_config(mut self, config: SessionConfig) -> Self {
        self.config = Some(config);
        self
    }

    pub fn build(self) -> SessionContext {
        let config = self.config.expect("config is required");
        let silence_voice_timeout = config.silence_voice_timeout;

        let node_templates = self.node_templates.expect("node_templates is required");

        let audio_spec = node_templates.iter().find_map(|n| {
            n.capabilities()
                .into_iter()
                .find_map(|c| c.downcast_ref::<AudioSpec>().cloned())
        });

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
            last_tts_state_change: None,
            config,
            audio_spec,
            node_templates,
            active_events: None,
            last_audio_received: Instant::now(),
            silence_voice_timeout,
            speech_active: false,
            empty_kind: None,
            empty_count: 0,
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
    /// Last time TTS state changed (started or stopped).
    /// Used for barge-in lockout to prevent immediate interruption.
    last_tts_state_change: Option<Instant>,

    config: SessionConfig,
    /// 下行音频输出能力（从模板 look up）。无 TTS 节点为 None —— 握手不声明、pacer 不构造。
    audio_spec: Option<AudioSpec>,

    node_templates: Vec<Arc<dyn Node>>,
    /// 当前 active Round（shadow）的 RoundEvent 观察者订阅端。
    active_events: Option<tokio::sync::broadcast::Receiver<RoundEvent>>,
    /// 最近一次收到音频的时间（transport-stall 判定）。
    last_audio_received: Instant,
    silence_voice_timeout: Option<i64>,
    /// 当前是否有活动语音（SpeechStarted 置位，TurnComplete 清除）。
    speech_active: bool,
    /// 当前监听的空输入语境（中枢判别）。进入 Listening 时按模式 + 前一 turn 判定。
    empty_kind: Option<EmptyKind>,
    /// 空输入重试计数（Rule of three）；成功 TurnComplete（真实输入）后复位。
    empty_count: u32,
}

impl Session {
    pub async fn start(mut self) {
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
                    self.check_transport_stall().await;
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

                    // 丢弃已让位的旧 round 残留帧（非当前 running/shadow）。
                    // 用 round_id 判断活跃性而非 epoch 数值，避免升级/投喂竞态下误弃活 round 的表达帧。
                    let active_round_ids = [
                        self.running_round.as_ref().map(|r| r.id.clone()),
                        self.shadow_round.as_ref().map(|r| r.id.clone()),
                    ];
                    let is_active = active_round_ids
                        .iter()
                        .flatten()
                        .any(|id| Some(id) == msg.round_id.as_ref());
                    if !is_active
                        && msg.epoch != 0
                        && msg.epoch < self.epoch
                        && !matches!(
                            &msg.payload,
                            FrameResult::TTSResult(tts)
                                if matches!(tts.state, Some(TtsState::Stop) | Some(TtsState::SentenceEnd))
                        )
                    {
                        continue;
                    }

                    // Track TTS state changes for barge-in lockout
                    if let FrameResult::TTSResult(tts) = &msg.payload
                        && matches!(tts.state, Some(TtsState::Start)) {
                            self.last_tts_state_change = Some(Instant::now());
                        }

                    // Forwarding round output (incl. audio pacing) counts as activity,
                    // so the connection isn't treated idle while a response is still playing.
                    self.idle_since = None;

                    if matches!(msg.payload, FrameResult::AudioResult(_)) {
                        if let Some(frame_duration) =
                            self.audio_spec.as_ref().map(|s| s.frame_duration_ms)
                        {
                            let pacer = audio_pacer.get_or_insert_with(|| {
                                tokio::time::interval_at(
                                    tokio::time::Instant::now()
                                        + tokio::time::Duration::from_millis(frame_duration),
                                    tokio::time::Duration::from_millis(frame_duration),
                                )
                            });
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
                ev = async {
                    match &mut self.active_events {
                        Some(rx) => rx.recv().await,
                        None => std::future::pending().await,
                    }
                } => {
                    if let Ok(ev) = ev {
                        self.handle_round_event(ev).await;
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
        let is_idle = round_idle && !self.speech_active;
        if is_idle {
            if self.idle_since.is_none() {
                self.idle_since = Some(Instant::now());
                tracing::debug!(component = "SESSION", event = "listening_for_input", session_id = %self.id, "listening for input");
            }
            if let Some(timeout) = self.config.close_connection_no_activity_time
                && let Some(since) = self.idle_since
                && since.elapsed() >= Duration::from_millis(timeout as u64)
            {
                tracing::info!(
                    component = "SESSION", event = "no_activity_timeout",
                    session_id = %self.id, timeout_ms = timeout,
                    "no activity timeout, closing connection"
                );
                return true;
            }
        } else {
            self.idle_since = None;
        }
        false
    }

    /// transport-stall：说话中但长时间无音频（静音超时）→ 让本轮链立即 finish。
    async fn check_transport_stall(&mut self) {
        let Some(timeout) = self.silence_voice_timeout else {
            return;
        };
        // 按键录音（manual）：由设备 ListenStop 显式结束，绝不因静音超时抢先 finish。
        if let Phase::Listening(param) = &self.phase
            && !param.is_voice_break_detect
        {
            return;
        }
        if self.speech_active
            && self.last_audio_received.elapsed() >= Duration::from_millis(timeout as u64)
        {
            tracing::debug!(
                component = "LISTENER", event = "transport_stall",
                session_id = %self.id,
                idle_ms = self.last_audio_received.elapsed().as_millis(),
                "input: transport stall, no audio received"
            );
            if let Some(round) = &self.shadow_round {
                round.chain().feed(PipelineEvent::FinishTurn);
            }
        }
    }

    pub async fn stop(&mut self, reason: SessionEndReason) {
        let duration_ms = self.started_at.elapsed().as_millis() as u64;
        self.stop_round(RoundStopReason::SessionEnd).await;
        tracing::info!(component = "SESSION", event = "session_ended", session_id = %self.id, rounds = self.round_count, duration_ms, %reason, "session ended");
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
        tracing::debug!(component = "SESSION", event = "new_round", session_id = %self.id, round_id = %round_id, epoch, "new round");
        let chain = {
            let instances: Vec<Arc<dyn Node>> = self
                .node_templates
                .iter()
                .map(|t| t.new_instance())
                .collect();
            let leaves = instances.clone();
            let head = compose_chain(instances).expect("at least one node template is required");
            let chain = NodeChain::new(head, leaves);
            chain.begin();
            chain
        };
        let mut round = Box::new(Round::new(
            self.id.clone(),
            round_id,
            tx,
            epoch,
            chain,
            cancel,
        ));
        let events = round.event_receiver();
        round.start().await;
        self.active_events = Some(events);
        self.shadow_round = Some(round);
    }

    /// 消费 current Round 的 RoundEvent（Session 作为外层观察者），做生命周期决策。
    async fn handle_round_event(&mut self, ev: RoundEvent) {
        match ev {
            RoundEvent::SpeechStarted => {
                self.speech_active = true;
                if let Phase::Listening(param) = &self.phase
                    && param.can_barge_in
                    && self.running_round.is_some()
                {
                    let lockout_ms = self
                        .config
                        .barge_in_lockout_ms
                        .unwrap_or(BARGE_IN_LOCKOUT_DEFAULT_MS);
                    let is_in_lockout = self
                        .last_tts_state_change
                        .is_some_and(|t| t.elapsed() < Duration::from_millis(lockout_ms));
                    if !is_in_lockout {
                        tracing::debug!(component = "SESSION", event = "barge_in_detected", session_id = %self.id, "barge-in detected");
                        self.next_round_epoch();
                        self.stop_round(RoundStopReason::BargeIn).await;
                    } else {
                        tracing::debug!(
                            component = "SESSION", event = "barge_in_suppressed",
                            session_id = %self.id, lockout_ms,
                            "barge-in suppressed during lockout period"
                        );
                    }
                }
            }
            RoundEvent::TurnComplete { text, prob } => {
                if let Phase::Listening(param) = &self.phase {
                    let param = param.clone();
                    self.speech_active = false;
                    self.on_turn_complete(text, prob, &param).await;
                }
            }
            RoundEvent::EmptyTurn => {
                // 空输入：判定对话 Act 决定是否注入提示语，并轮转（升级 shadow + 新建）避免死 shadow。
                self.speech_active = false;
                if let Phase::Listening(param) = &self.phase {
                    let param = param.clone();
                    let kind = match self.empty_kind.take() {
                        Some(k) => k,
                        None if param.is_voice_break_detect => EmptyKind::AutoSpoke,
                        None => EmptyKind::Manual,
                    };
                    self.empty_count += 1;
                    match self.decide_empty_act(kind) {
                        Some((kind, count)) => {
                            // Manual 事件驱动：提示后归零，不受 Rule of three 限次。
                            self.empty_count = if kind == EmptyKind::Manual { 0 } else { count };
                            if let Some(round) = &self.shadow_round {
                                round.chain().feed(PipelineEvent::Prompt { kind, count });
                            }
                        }
                        None => {
                            // Silence/GiveUp：不注入表达，静默等待或回 idle。
                        }
                    }
                    self.upgrade_shadow_round().await;
                    self.new_shadow_round().await;
                    self.phase = Phase::Listening(ListeningParam::from(&param));
                }
            }
            RoundEvent::SpokenEnd => {
                tracing::debug!(
                    component = "ROUND", event = "spoken_end",
                    session_id = %self.id,
                    "round finished speaking"
                );
            }
        }
    }

    pub async fn stop_round(&mut self, reason: RoundStopReason) {
        // take() 使被打断 round 不再是 active，epoch 过滤随即丢弃其残留音频帧（立即静音）。
        let Some(mut round) = self.running_round.take() else {
            return;
        };
        tracing::debug!(component = "SESSION", event = "round_stopped", session_id = %self.id, round_id = %round.id, %reason, "round stopped");
        round.stop().await;
        if let Some(handle) = round.join_handle.take() {
            let round_id = &round.id;
            match tokio::time::timeout(Duration::from_secs(5), handle).await {
                Ok(Ok(())) => {}
                Ok(Err(e)) => {
                    if e.is_panic() {
                        tracing::error!(
                            component = "SESSION", event = "round_task_panic",
                            session_id = %self.id, round_id = %round_id,
                            "round task panicked"
                        );
                    }
                }
                Err(_) => {
                    tracing::warn!(
                        component = "SESSION", event = "round_task_timeout",
                        session_id = %self.id, round_id = %round_id,
                        "round task did not finish within timeout, proceeding"
                    );
                }
            }
        }
    }

    fn next_round_epoch(&mut self) -> u64 {
        self.epoch += 1;
        self.epoch
    }

    async fn upgrade_shadow_round(&mut self) {
        if self.running_round.is_some() {
            let epoch = self.next_round_epoch();
            if let Some(round) = &mut self.shadow_round {
                round.set_epoch(epoch);
                tracing::debug!(
                    component = "SESSION", event = "round_upgraded",
                    session_id = %self.id, epoch = self.epoch, round_id = %round.id,
                    "round upgraded",
                );
            }
            self.stop_round(RoundStopReason::Upgrade).await;
        }
        self.running_round = self.shadow_round.take();
        self.active_events = None;
    }

    pub async fn accept_frame(&mut self, frame: &Frame) {
        match frame {
            Frame::Abort(_) => {
                if self.running_round.is_some() {
                    self.next_round_epoch();
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

    /// TurnComplete 后：升级 shadow→running，并为其后续表达创建下一轮 shadow。
    async fn on_turn_complete(&mut self, text: String, prob: f32, param: &ListeningParam) {
        // 成功回合（真实输入）：复位空输入计数与语境。
        self.empty_count = 0;
        self.empty_kind = None;
        self.phase = Phase::Speaking(param.to_speaking(text, prob));
        self.upgrade_shadow_round().await;
        // 该 round 已升级为 running 自动表达；新建下一轮 shadow 监听后续输入。
        self.new_shadow_round().await;
        self.phase = Phase::Listening(ListeningParam::from(param));
    }

    async fn on_idle(&mut self, frame: &Frame) {
        self.new_shadow_round().await;
        if let Frame::Hello(hello_message) = frame {
            self.handle_connect(hello_message).await;
            self.phase = Phase::Listening(ListeningParam::default());
        }
    }

    async fn handle_connect(&mut self, hello_message: &HelloMessage) {
        if let Some(params) = &hello_message.audio_params {
            for template in &self.node_templates {
                template.on_configure(&PipelineEvent::Configure(params.clone()));
            }
            // 首轮实例已在 `on_idle` 由默认模板创建，故需补一次流内投喂。
            if let Some(round) = &self.shadow_round {
                round.chain().feed(PipelineEvent::Configure(params.clone()));
            }
        }
        let audio_params = self.audio_spec.as_ref().map(|spec| AudioParam {
            format: AudioFormat::Opus,
            sample_rate: spec.sample_rate,
            channels: spec.channel,
            frame_duration: spec.frame_duration_ms,
        });
        let data = HelloMessage {
            message: crate::message::Message {
                mtype: crate::message::Type::Hello,
            },
            transport: Some(Transport::Websocket),
            audio_params,
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
                    self.next_round_epoch();
                    self.stop_round(RoundStopReason::BargeIn).await;
                }
                self.phase = Phase::Listening(ListeningParam {
                    can_barge_in: *barge_in,
                    is_voice_break_detect: *is_voice_break_detect,
                    ..Default::default()
                });
                if let Some(round) = &self.shadow_round {
                    round.chain().feed(PipelineEvent::ListenMode {
                        streaming: *is_voice_break_detect,
                    });
                }
            }
            Frame::ListenStop => {
                if let Some(round) = &self.shadow_round {
                    round.chain().feed(PipelineEvent::FinishTurn);
                    // 连续监听下保持静默、不重复引导。
                    if self.empty_kind == Some(EmptyKind::Continuing) {
                        return;
                    }
                    // 空输入类型判定：无人声 manual=Manual / 断音卷轴=Silence（均补注入 EmptyInput 触发提示），检出语音=AutoSpoke。
                    if !self.speech_active && !param.is_voice_break_detect {
                        self.empty_kind = Some(EmptyKind::Manual);
                        round.chain().feed(PipelineEvent::EmptyInput);
                    } else if !self.speech_active && param.is_voice_break_detect {
                        self.empty_kind = Some(EmptyKind::Silence);
                        round.chain().feed(PipelineEvent::EmptyInput);
                    } else if self.speech_active {
                        self.empty_kind = Some(EmptyKind::AutoSpoke);
                    }
                }
            }
            Frame::Input { text, mode } => {
                let is_wake = matches!(mode, InputMode::Wake);
                // 唤醒词后：为下次监听标记 Wake 语境（唤醒后首次监听的空输入按引导式处理）。
                if is_wake {
                    self.empty_kind = Some(EmptyKind::Wake);
                    self.empty_count = 0;
                }
                tracing::debug!(
                    component = "SESSION", event = "input_received",
                    session_id = %self.id,
                    text_len = text.len(),
                    is_wake,
                    has_running_round = self.running_round.is_some(),
                    has_shadow_round = self.shadow_round.is_some(),
                    "input received",
                );
                if let Some(round) = &self.shadow_round {
                    round.chain().feed(PipelineEvent::TurnText {
                        text: text.clone(),
                        prob: 1.0,
                    });
                }
            }
            Frame::Voice { data } => {
                self.last_audio_received = Instant::now();
                if let Some(round) = &self.shadow_round {
                    round.chain().feed(PipelineEvent::AudioFrame(data.to_vec()));
                }
            }
            _ => {}
        }
    }

    async fn on_speaking(&mut self, frame: &Frame, param: &SpeakingParam) {
        if let Frame::ListenStart {
            barge_in,
            is_voice_break_detect,
        } = frame
        {
            // 语音播放中收到 listen(start)：按新监听配置切回监听，并同步流内监听模式。
            if let Some(round) = &self.shadow_round {
                round.chain().feed(PipelineEvent::ListenMode {
                    streaming: *is_voice_break_detect,
                });
            }
            // realtime/auto 打断说话继续监听：标记连续监听语境。
            if *is_voice_break_detect && *barge_in {
                self.empty_kind = Some(EmptyKind::Continuing);
            }
            self.phase = Phase::Listening(ListeningParam {
                can_barge_in: *barge_in,
                is_voice_break_detect: *is_voice_break_detect,
                ..Default::default()
            });
            return;
        }
        // realtime 回复后继续监听（Speaking→Listening）标记连续监听语境。
        if param.is_voice_break_detect && param.can_barge_in {
            self.empty_kind = Some(EmptyKind::Continuing);
        }
        self.phase = Phase::Listening(ListeningParam::from(param));
    }

    /// 决策空输入的对话 Act。返回 (kind, count) 表示应注入 `Prompt` 渲染提示语；
    /// `None` 表示静默/收尾（`Continuing` 不提示、超上限回 idle 不再提示）。
    fn decide_empty_act(&self, kind: EmptyKind) -> Option<(EmptyKind, u32)> {
        let count = self.empty_count;
        match kind {
            EmptyKind::Continuing => None,
            EmptyKind::Manual => {
                // push-to-talk 事件驱动：每次按键无人声都提示，不受 Rule of three 限次。
                Some((EmptyKind::Manual, 1))
            }
            // Rule of three：Wake/AutoSpoke/Silence 逐级提示至第 3 次，之后回 idle 不再提示。
            _ if count <= 3 => Some((kind, count)),
            _ => None,
        }
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
    LlmNoUsableOutput = 504007,
}
