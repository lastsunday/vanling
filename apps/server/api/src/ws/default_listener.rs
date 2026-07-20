use std::sync::{Arc, Mutex as StdMutex};

use async_trait::async_trait;
use chrono::Local;
use framework::error::AppError;
use service::chobits::asr::Asr;
use service::chobits::listener::{ListenInput, ListenResult, ListenState, Listener};
use service::chobits::message::hello::AudioParam;
use service::chobits::vad::Vad;
use tokio::sync::Mutex;

/// Maximum prefix padding in samples (300ms at 16kHz).
const PREFIX_SAMPLES_MAX: usize = 4800;

/// Maximum Opus frame duration in ms (per spec, one packet ≤ 120ms).
const MAX_OPUS_FRAME_MS: u64 = 120;

/// Default input sample rate when client does not advertise audio_params.
const DEFAULT_SAMPLE_RATE: u32 = 16000;

pub struct DefaultListener {
    session_id: String,
    temp_voice_data: Vec<f32>,
    voice_data: Vec<f32>,
    vad: Box<dyn Vad>,
    asr: Arc<Mutex<Box<dyn Asr>>>,
    decoder: StdMutex<ropus::Decoder>,
    pub state: ListenState,
    silence_voice_timeout: Option<i64>,
    latest_speaking_time: Option<i64>,
    client_input_sample_rate: u32,
    prefix_buffer: Vec<f32>,
    prefix_flushed: bool,
    total_pcm: Vec<f32>,
    pending_text: Option<String>,
    speech_only_samples: usize,
}

impl DefaultListener {
    pub fn new(
        session_id: String,
        vad: Box<dyn Vad>,
        asr: Arc<Mutex<Box<dyn Asr>>>,
        silence_voice_timeout: Option<i64>,
    ) -> Self {
        Self {
            session_id,
            vad,
            asr,
            temp_voice_data: Vec::new(),
            voice_data: Vec::new(),
            decoder: StdMutex::new(
                ropus::Decoder::new(DEFAULT_SAMPLE_RATE, ropus::Channels::Mono).unwrap(),
            ),
            state: ListenState::Idle,
            silence_voice_timeout,
            latest_speaking_time: None,
            client_input_sample_rate: DEFAULT_SAMPLE_RATE,
            prefix_buffer: Vec::with_capacity(PREFIX_SAMPLES_MAX),
            prefix_flushed: false,
            total_pcm: Vec::new(),
            pending_text: None,
            speech_only_samples: 0,
        }
    }
}

#[async_trait]
impl Listener for DefaultListener {
    async fn accept(&mut self, input: ListenInput) {
        match input {
            ListenInput::Text(text) => {
                self.pending_text = Some(text);
            }
            ListenInput::Audio(data) => {
                if self.state == ListenState::Idle {
                    self.state = ListenState::Listening { is_speech: false };
                }
                if let ListenState::Listening { .. } = self.state {
                    let frame_size = ((self.client_input_sample_rate as u64 * MAX_OPUS_FRAME_MS)
                        / 1000) as usize;
                    let mut samples = vec![0f32; frame_size];
                    let len = match self.decoder.lock().unwrap().decode_float(
                        &data,
                        &mut samples,
                        ropus::DecodeMode::Normal,
                    ) {
                        Ok(len) => len,
                        Err(e) => {
                            tracing::warn!(
                                component = "vad", event = "opus_decode_error",
                                session_id = %self.session_id,
                                data_len = data.len(),
                                error = %e,
                                "opus decode error"
                            );
                            return;
                        }
                    };
                    for s in samples[..len].iter_mut() {
                        *s = s.clamp(-1.0, 1.0);
                    }
                    self.total_pcm.extend_from_slice(&samples[..len]);
                    self.temp_voice_data.append(&mut samples[..len].to_vec());
                    let window_size = self.vad.window_size();
                    while self.temp_voice_data.len() > window_size {
                        let window: Vec<f32> = self.temp_voice_data.drain(..window_size).collect();

                        self.prefix_buffer.extend(&window);
                        if self.prefix_buffer.len() > PREFIX_SAMPLES_MAX {
                            let excess = self.prefix_buffer.len() - PREFIX_SAMPLES_MAX;
                            self.prefix_buffer.drain(..excess);
                        }

                        if self.vad.accept_waveform(&window).is_err() {
                            return;
                        }

                        let was_speech =
                            matches!(self.state, ListenState::Listening { is_speech: true });
                        if self.vad.is_speech() {
                            self.state = ListenState::Listening { is_speech: true };
                            self.latest_speaking_time = Some(Local::now().timestamp_millis());
                        } else {
                            self.prefix_flushed = false;
                        }
                        if !was_speech
                            && matches!(self.state, ListenState::Listening { is_speech: true })
                        {
                            tracing::debug!(
                                component = "vad", event = "speech_started",
                                session_id = %self.session_id,
                                "listener: speech started"
                            );
                        }

                        if let ListenState::Listening { is_speech: true } = self.state {
                            if !self.prefix_flushed {
                                self.voice_data.append(&mut self.prefix_buffer);
                                self.prefix_buffer = Vec::with_capacity(PREFIX_SAMPLES_MAX);
                                self.prefix_flushed = true;
                            } else {
                                self.voice_data.extend_from_slice(&window);
                                self.speech_only_samples += window.len();
                            }
                        }
                    }
                }
                if let (Some(silence_voice_timeout), Some(latest_speaking_time)) =
                    (self.silence_voice_timeout, self.latest_speaking_time)
                {
                    let offset_time = Local::now().timestamp_millis() - latest_speaking_time;
                    if offset_time >= silence_voice_timeout {
                        tracing::debug!(
                            component = "vad", event = "silence_timeout",
                            session_id = %self.session_id,
                            silence_ms = offset_time,
                            "listener: silence timeout"
                        );
                        self.state = ListenState::End;
                    }
                }
            }
        }
    }

    fn reconfigure(&mut self, params: &AudioParam) {
        tracing::debug!(
            component = "listener", event = "listener_reconfigure",
            session_id = %self.session_id,
            sample_rate = params.sample_rate,
            "listener reconfigure"
        );
        self.client_input_sample_rate = params.sample_rate;
        let mut dec = self.decoder.lock().unwrap();
        *dec = ropus::Decoder::new(params.sample_rate, ropus::Channels::Mono).unwrap();
    }

    fn set_state(&mut self, state: ListenState) {
        self.state = state;
    }

    fn get_state(&self) -> ListenState {
        self.state
    }

    async fn take_voice(&mut self) -> Vec<f32> {
        core::mem::take(&mut self.voice_data)
    }

    async fn take_result(&mut self) -> (Vec<f32>, Result<ListenResult, AppError>) {
        if let Some(text) = self.pending_text.take() {
            return (Vec::new(), Ok(ListenResult::Text(text)));
        }
        let voice_data = core::mem::take(&mut self.voice_data);
        let speech_only = self.speech_only_samples;
        self.speech_only_samples = 0;
        if voice_data.is_empty() || speech_only < 3200 {
            tracing::debug!(
                component = "asr", event = "speech_too_short",
                session_id = %self.session_id,
                speech_only,
                voice_data_len = voice_data.len(),
                "asr: speech too short, skipping"
            );
            return (
                voice_data,
                Ok(ListenResult::Audio {
                    text: String::new(),
                    prob: 1.0,
                }),
            );
        }
        let mut asr = self.asr.lock().await;
        tracing::debug!(
            component = "asr", event = "asr_start",
            session_id = %self.session_id,
            speech_only_samples = speech_only,
            "asr start"
        );
        let result = asr
            .transcribe(self.client_input_sample_rate, &voice_data)
            .await;
        match result {
            Ok(transcript) => {
                let text = transcript.text;
                let cleaned: String = text
                    .chars()
                    .filter(|c| !c.is_ascii_punctuation() && !is_cjk_punctuation(*c))
                    .collect();
                if cleaned.trim().is_empty() {
                    tracing::debug!(component = "asr", event = "asr_no_meaningful_text", session_id = %self.session_id, %text, "asr: no meaningful text, skipping");
                    return (
                        voice_data,
                        Ok(ListenResult::Audio {
                            text: String::new(),
                            prob: transcript.prob,
                        }),
                    );
                }
                tracing::debug!(
                    component = "asr", event = "asr_complete",
                    session_id = %self.session_id,
                    text = %cleaned,
                    prob = transcript.prob,
                    "asr complete"
                );
                (
                    voice_data,
                    Ok(ListenResult::Audio {
                        text,
                        prob: transcript.prob,
                    }),
                )
            }
            Err(e) => (voice_data, Err(e)),
        }
    }

    async fn reset(&mut self, silence_voice_timeout: Option<i64>) {
        tracing::debug!(component = "listener", event = "listener_reset", session_id = %self.session_id, "listener reset");
        self.state = ListenState::Idle;
        self.silence_voice_timeout = silence_voice_timeout;
        self.latest_speaking_time = None;
        self.temp_voice_data.clear();
        self.voice_data.clear();
        self.vad.clear();
        self.prefix_buffer.clear();
        self.prefix_flushed = false;
        self.total_pcm.clear();
        self.pending_text = None;
        self.speech_only_samples = 0;
    }

    async fn get_raw_pcm(&mut self) -> Vec<f32> {
        core::mem::take(&mut self.total_pcm)
    }

    fn poll_timeout(&mut self) -> Option<()> {
        match self.state {
            ListenState::Listening { is_speech: true } => {
                if let (Some(timeout), Some(last)) =
                    (self.silence_voice_timeout, self.latest_speaking_time)
                    && Local::now().timestamp_millis() - last >= timeout
                {
                    self.state = ListenState::End;
                    return Some(());
                }
                None
            }
            _ => None,
        }
    }
}

fn is_cjk_punctuation(c: char) -> bool {
    matches!(
        c,
        '。' | '，'
            | '、'
            | '；'
            | '：'
            | '？'
            | '！'
            | '（'
            | '）'
            | '【'
            | '】'
            | '《'
            | '》'
            | '「'
            | '」'
            | '『'
            | '』'
            | '〝'
            | '〞'
            | '─'
            | '—'
            | '…'
            | '～'
            | '·'
    )
}
