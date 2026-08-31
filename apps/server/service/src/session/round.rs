use std::collections::HashMap;
use std::sync::Arc;
use std::sync::LazyLock;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use futures::StreamExt;

use tokio::sync::Mutex;
use tokio::sync::mpsc::UnboundedSender;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::frame::{FrameResult, OutputMessage};
use crate::message::audio::AudioMessage;
use crate::message::llm::LlmMessage;
use crate::message::stt::SttMessage;
use crate::message::tts::{TtsMessage, TtsState};
use crate::pipeline::{EventSink, NodeChain, NodeContext, PipelineEvent, TapPoint};
use crate::session::SessionErrorCode;

use framework::err;

/// 输入侧回合事件 —— 由 Session 汇总后消费。
#[derive(Debug, Clone)]
pub enum TurnEvent {
    SpeechStarted,
    PartialTranscript(String),
    TurnComplete { text: String, prob: f32 },
}

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
    map.insert("confident", "😎");
    map.insert("sleepy", "😴");
    map.insert("silly", "😜");
    map.insert("confused", "🙄");
    map
});

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

/// Round 暴露给观察者（Session 注册订阅）的业务事件；Session 据此做生命周期决策。
#[derive(Debug, Clone)]
pub enum RoundEvent {
    /// 语音起始（已过 barge-in 锁定期）。
    SpeechStarted,
    /// 一轮识别完成（应升级 shadow→running、相位 Speaking、ListenStop）。
    TurnComplete { text: String, prob: f32 },
    /// 空输入完成（无有效语音）：与 TurnComplete 同理轮转出新的 shadow，但不产生 STT。
    EmptyTurn,
    /// TTS 全部结束（该 round 表达完成）。
    SpokenEnd,
}

pub struct Round {
    pub parent_id: String,
    pub id: String,
    output_tx: UnboundedSender<OutputMessage>,
    /// 共享可变的 epoch（Arc）：Session 升级时更新，observer 每帧读取最新值，避免旧 epoch 误弃活帧。
    epoch: Arc<AtomicU64>,
    stop: Arc<AtomicBool>,
    chain: NodeChain,
    pub tts_state: Arc<Mutex<Option<TtsState>>>,
    pub cancel: CancellationToken,
    pub join_handle: Option<JoinHandle<()>>,
    round_event_tx: tokio::sync::broadcast::Sender<RoundEvent>,
    round_event_rx: tokio::sync::broadcast::Receiver<RoundEvent>,
}

impl Round {
    pub fn new(
        parent_id: String,
        id: String,
        output_tx: UnboundedSender<OutputMessage>,
        epoch: u64,
        chain: NodeChain,
        cancel: CancellationToken,
    ) -> Self {
        let (round_event_tx, round_event_rx) = tokio::sync::broadcast::channel::<RoundEvent>(16);
        Self {
            parent_id,
            id,
            output_tx,
            epoch: Arc::new(AtomicU64::new(epoch)),
            stop: Arc::new(AtomicBool::new(false)),
            chain,
            tts_state: Arc::new(Mutex::new(None)),
            cancel,
            join_handle: None,
            round_event_tx,
            round_event_rx,
        }
    }

    pub fn epoch(&self) -> u64 {
        self.epoch.load(Ordering::Relaxed)
    }

    pub fn set_epoch(&self, e: u64) {
        self.epoch.store(e, Ordering::Relaxed);
    }

    /// 观察者接收端：Session 注册订阅（每 Round 唯一订阅者）。
    pub fn event_receiver(&mut self) -> tokio::sync::broadcast::Receiver<RoundEvent> {
        self.round_event_rx.resubscribe()
    }

    /// 单链投喂器：Session 注入音频/配置等事件到链首。
    pub fn chain(&self) -> &NodeChain {
        &self.chain
    }

    /// 启动观察者任务：消费单链广播 + 链尾，统一处理 barge-in / 说话推进 / 逐句转发 / TTS 状态机。
    pub async fn start(&mut self) {
        let (sink, mut tap_rx) = EventSink::channel();
        let ctx = NodeContext::with_emit(self.cancel.clone(), sink, self.parent_id.clone());
        let mut tail = self.chain.stream(&ctx);

        let session_id = self.parent_id.clone();
        let round_id = self.id.clone();
        let epoch_ptr = self.epoch.clone();
        let output_tx = self.output_tx.clone();
        let stop_me = self.stop.clone();
        let cancel = self.cancel.clone();
        let round_event_tx = self.round_event_tx.clone();
        let tts_state_clone = self.tts_state.clone();
        let chain_finish = self.chain.clone();

        self.join_handle = Some(tokio::spawn(async move {
            let round_start = std::time::Instant::now();
            let mut status = RoundStatus::Ok;
            let mut sentence_count: u32 = 0;

            let send = |payload: FrameResult| {
                output_tx
                    .send(OutputMessage {
                        epoch: epoch_ptr.load(Ordering::Relaxed),
                        round_id: Some(round_id.clone()),
                        session_id: session_id.clone(),
                        payload,
                    })
                    .map_err(Box::new)
            };

            let mark_failed = |status: &mut RoundStatus, ok: bool| -> bool {
                if ok {
                    return false;
                }
                *status = RoundStatus::Error;
                stop_me.store(true, Ordering::Relaxed);
                true
            };

            async fn change_state(tts_state: &Arc<Mutex<Option<TtsState>>>, state: TtsState) {
                let mut ts = tts_state.lock().await;
                *ts = Some(state);
            }

            let mut current_text: Option<String> = None;
            let mut current_emotion: Option<String> = None;
            let mut tts_sentence_active = false;
            let mut tts_started = false;
            let mut turn_decided = false;

            loop {
                tokio::select! {
                    () = cancel.cancelled() => {
                        status = RoundStatus::Ok;
                        break;
                    }
                    tapped = tap_rx.recv() => {
                        let Some(tapped) = tapped else { continue };
                        match (tapped.point, &tapped.event) {
                            (TapPoint::Before, PipelineEvent::SpeechStarted) => {
                                let _ = round_event_tx.send(RoundEvent::SpeechStarted);
                            }
                            (TapPoint::After, PipelineEvent::TextChunk { text, emotion }) => {
                                current_text = Some(text.clone());
                                current_emotion = emotion.clone();
                            }
                            (TapPoint::After, PipelineEvent::TurnComplete { text, prob })
                                if !turn_decided =>
                            {
                                turn_decided = true;
                                if mark_failed(
                                    &mut status,
                                    send(FrameResult::STTResult(SttMessage::new(
                                        Some(session_id.clone()),
                                        Some(text.clone()),
                                    )))
                                    .is_ok(),
                                ) {
                                    break;
                                }
                                let _ = round_event_tx.send(RoundEvent::TurnComplete {
                                    text: text.clone(),
                                    prob: *prob,
                                });
                            }
                            (TapPoint::After, PipelineEvent::EmptyInput) => {
                                // 空输入轮转 shadow（升级 + 新建），避免死 shadow 吞后续输入。
                                let _ = round_event_tx.send(RoundEvent::EmptyTurn);
                            }
                            (TapPoint::After, PipelineEvent::AudioOut { audio, is_first, is_last }) => {
                                if stop_me.load(Ordering::Relaxed) {
                                    break;
                                }
                                if !tts_started {
                                    tts_started = true;
                                    change_state(&tts_state_clone, TtsState::Start).await;
                                    if mark_failed(
                                        &mut status,
                                        send(FrameResult::TTSResult(TtsMessage::new(
                                            Some(session_id.clone()),
                                            Some(TtsState::Start),
                                            None,
                                        )))
                                        .is_ok(),
                                    ) {
                                        break;
                                    }
                                }
                                let text = current_text.take().unwrap_or_default();
                                let emotion = current_emotion.take().unwrap_or_else(|| "neutral".to_string());
                                let audio_len: usize = audio.iter().map(|a| a.len()).sum();

                                if *is_first {
                                    sentence_count += 1;
                                    tracing::debug!(
                                        component = "ROUND", event = "sentence",
                                        session_id = %session_id, round_id = %round_id,
                                        sentence = sentence_count, text = %text, emotion, audio_bytes = audio_len,
                                        "sentence"
                                    );
                                    let mut llm_msg = LlmMessage::new(
                                        Some(session_id.to_string()),
                                        Some(emotion.clone()),
                                        Some(EMOJI_MAP.get(emotion.as_str()).map_or("😶", |v| v).to_string()),
                                    );
                                    llm_msg.full_text = Some(text.clone());
                                    if mark_failed(
                                        &mut status,
                                        send(FrameResult::LLMResult(llm_msg)).is_ok(),
                                    ) {
                                        break;
                                    }
                                    change_state(&tts_state_clone, TtsState::SentenceStart).await;
                                    if mark_failed(
                                        &mut status,
                                        send(FrameResult::TTSResult(TtsMessage::new(
                                            Some(session_id.to_string()),
                                            Some(TtsState::SentenceStart),
                                            Some(text.clone()),
                                        )))
                                        .is_ok(),
                                    ) {
                                        break;
                                    }
                                    tts_sentence_active = true;
                                }
                                for packet_data in audio {
                                    if stop_me.load(Ordering::Relaxed) {
                                        break;
                                    }
                                    if send(FrameResult::AudioResult(AudioMessage::new(
                                        Some(session_id.to_string()),
                                        packet_data.clone(),
                                    ))).is_err() {
                                        break;
                                    }
                                }
                                if *is_last && tts_sentence_active {
                                    change_state(&tts_state_clone, TtsState::SentenceEnd).await;
                                    if mark_failed(
                                        &mut status,
                                        send(FrameResult::TTSResult(TtsMessage::new(
                                            Some(session_id.to_string()),
                                            Some(TtsState::SentenceEnd),
                                            None,
                                        )))
                                        .is_ok(),
                                    ) {
                                        break;
                                    }
                                    // 表达完成：闭合链尾，避免残留 Stop/SentenceEnd 长尾帧污染下一轮。
                                    break;
                                }
                            }
                            _ => {}
                        }
                    }
                    result = tokio::time::timeout(TTS_TIMEOUT, tail.next()) => {
                        match result {
                            Ok(Some(Ok(_other))) => {}
                            Ok(Some(Err(e))) => {
                                status = RoundStatus::Error;
                                if sentence_count == 0 {
                                    tracing::warn!(
                                        component = "ROUND", event = "llm_no_usable_output",
                                        session_id = %session_id, round_id = %round_id,
                                        error = %e, "LLM completed with no usable output"
                                    );
                                    let _ = send(FrameResult::Error(
                                        err!(SessionErrorCode::LlmNoUsableOutput)
                                            .with_extra(e.to_string()),
                                    ));
                                } else {
                                    tracing::warn!(
                                        component = "ROUND", event = "tts_encode_error",
                                        session_id = %session_id, round_id = %round_id,
                                        error = %e, "tts encode error"
                                    );
                                    let _ = send(FrameResult::Error(
                                        err!(SessionErrorCode::TtsEncode).with_extra(e.to_string()),
                                    ));
                                }
                                stop_me.store(true, Ordering::Relaxed);
                                break;
                            }
                            Ok(None) => {
                                break;
                            }
                            Err(_) => {
                                // 空轮询 shadow 保持存活等待输入；仅已开始表达（`tts_started`）才算卡死。
                                if !tts_started {
                                    continue;
                                }
                                tracing::warn!(component = "ROUND", event = "tts_timeout", session_id = %session_id, round_id = %round_id, "tts timeout");
                                status = RoundStatus::Error;
                                break;
                            }
                        }
                    }
                }
            }

            // 仅当本轮确实表达过 TTS 才广播 Stop，避免空轮询 shadow 的 Stop 污染下一轮输出。
            if tts_started {
                change_state(&tts_state_clone, TtsState::Stop).await;
                mark_failed(
                    &mut status,
                    send(FrameResult::TTSResult(TtsMessage::new(
                        Some(session_id.clone()),
                        Some(TtsState::Stop),
                        None,
                    )))
                    .is_ok(),
                );
            }
            let _ = round_event_tx.send(RoundEvent::SpokenEnd);
            // 整轮收尾：Deferred 节点（VAD/ASR）统一 `on_release`。
            chain_finish.finish();
            let duration_ms = round_start.elapsed().as_millis() as u64;
            tracing::debug!(
                component = "ROUND", event = "round_complete",
                session_id = %session_id, round_id = %round_id,
                duration_ms, sentences = sentence_count, %status,
                "round complete"
            );
        }));
    }

    pub async fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
        self.cancel.cancel();
    }
}
