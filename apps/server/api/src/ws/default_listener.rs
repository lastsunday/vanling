use std::collections::VecDeque;
use std::sync::{Arc, Mutex as StdMutex};

use async_trait::async_trait;
use chrono::Local;
use service::chobits::asr::Asr;
use service::chobits::listener::{ListenInput, Listener, TurnOutput, TurnResult};
use service::chobits::message::hello::AudioParam;
use service::chobits::vad::Vad;
use tokio::sync::Mutex;

const PREFIX_SAMPLES_MAX: usize = 9600;

const MAX_OPUS_FRAME_MS: u64 = 120;

const DEFAULT_SAMPLE_RATE: u32 = 16000;

const MIN_SPEECH_ONLY_SAMPLES: usize = 3200;

/// After VAD declares silence (is_speech()=false), wait this long before
/// triggering ASR. Combined with VAD's internal min_silence_duration (~550ms),
/// total silence before ASR ≈ 750ms, vs the previous fixed 1200ms timeout.
const VAD_SILENCE_CONFIRM_MS: i64 = 200;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum ListenerState {
    Idle,
    Listening { is_speech: bool },
}

pub struct DefaultListener {
    session_id: String,
    temp_voice_data: Vec<f32>,
    voice_data: Vec<f32>,
    vad: Box<dyn Vad>,
    asr: Arc<Mutex<Box<dyn Asr>>>,
    decoder: StdMutex<ropus::Decoder>,
    state: ListenerState,
    silence_voice_timeout: Option<i64>,
    latest_speaking_time: Option<i64>,
    vad_silent_since: Option<i64>,
    client_input_sample_rate: u32,
    prefix_buffer: Vec<f32>,
    prefix_flushed: bool,
    total_pcm: Vec<f32>,
    pending_text: Option<String>,
    speech_only_samples: usize,
    queue: VecDeque<TurnOutput>,
    asr_pending: bool,
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
            state: ListenerState::Idle,
            silence_voice_timeout,
            latest_speaking_time: None,
            vad_silent_since: None,
            client_input_sample_rate: DEFAULT_SAMPLE_RATE,
            prefix_buffer: Vec::with_capacity(PREFIX_SAMPLES_MAX),
            prefix_flushed: false,
            total_pcm: Vec::new(),
            pending_text: None,
            speech_only_samples: 0,
            queue: VecDeque::new(),
            asr_pending: false,
        }
    }

    async fn run_asr(&mut self) -> Option<TurnResult> {
        let voice_data = core::mem::take(&mut self.voice_data);
        let speech_only = self.speech_only_samples;
        self.speech_only_samples = 0;

        if voice_data.is_empty() || speech_only < MIN_SPEECH_ONLY_SAMPLES {
            tracing::debug!(
                component = "ASR", event = "speech_too_short",
                session_id = %self.session_id,
                speech_only,
                voice_data_len = voice_data.len(),
                "asr: speech too short, skipping"
            );
            return None;
        }

        let mut asr = self.asr.lock().await;
        tracing::debug!(
            component = "ASR", event = "asr_start",
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
                tracing::debug!(
                    component = "ASR", event = "asr_complete",
                    session_id = %self.session_id,
                    text = %text,
                    prob = transcript.prob,
                    "asr complete"
                );
                Some(TurnResult {
                    text,
                    prob: transcript.prob,
                    voice_data,
                })
            }
            Err(e) => {
                tracing::warn!(
                    component = "ASR", event = "asr_failed",
                    session_id = %self.session_id,
                    error = %e,
                    "asr failed"
                );
                None
            }
        }
    }

    fn check_silence_timeout(&self) -> bool {
        let now = Local::now().timestamp_millis();
        // Primary: fixed timeout from last speech frame (safety net)
        if let (Some(timeout), Some(last)) = (self.silence_voice_timeout, self.latest_speaking_time)
            && now - last >= timeout
        {
            return true;
        }
        // Early: VAD declared silence + short confirmation window
        if let Some(silent_since) = self.vad_silent_since
            && now - silent_since >= VAD_SILENCE_CONFIRM_MS
        {
            return true;
        }
        false
    }

    fn accumulate_audio(&mut self, data: &[u8]) {
        let frame_size =
            ((self.client_input_sample_rate as u64 * MAX_OPUS_FRAME_MS) / 1000) as usize;
        let mut samples = vec![0f32; frame_size];
        let len = match self.decoder.lock().unwrap().decode_float(
            data,
            &mut samples,
            ropus::DecodeMode::Normal,
        ) {
            Ok(len) => len,
            Err(e) => {
                tracing::warn!(
                    component = "VAD", event = "opus_decode_error",
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

            let was_speech = matches!(self.state, ListenerState::Listening { is_speech: true });
            if self.vad.is_speech() {
                self.state = ListenerState::Listening { is_speech: true };
                self.latest_speaking_time = Some(Local::now().timestamp_millis());
                self.vad_silent_since = None;
            } else {
                self.prefix_flushed = false;
                if was_speech {
                    self.vad_silent_since = Some(Local::now().timestamp_millis());
                }
            }

            if !was_speech && matches!(self.state, ListenerState::Listening { is_speech: true }) {
                tracing::debug!(
                    component = "VAD", event = "speech_started",
                    session_id = %self.session_id,
                    "listener: speech started"
                );
                self.queue.push_back(TurnOutput::SpeechStarted);
            }

            if let ListenerState::Listening { is_speech: true } = self.state {
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

    fn internal_reset(&mut self) {
        self.state = ListenerState::Idle;
        self.latest_speaking_time = None;
        self.vad_silent_since = None;
        self.temp_voice_data.clear();
        self.voice_data.clear();
        self.vad.clear();
        self.prefix_buffer.clear();
        self.prefix_flushed = false;
        self.speech_only_samples = 0;
        self.pending_text = None;
        self.asr_pending = false;
    }
}

#[async_trait]
impl Listener for DefaultListener {
    async fn accept(&mut self, input: ListenInput) {
        match input {
            ListenInput::Text(text) => {
                tracing::debug!(
                    component = "LISTENER", event = "text_input",
                    session_id = %self.session_id,
                    text_len = text.len(),
                    "listener: text input"
                );
                self.internal_reset();
                self.queue.push_back(TurnOutput::TurnComplete(TurnResult {
                    text,
                    prob: 1.0,
                    voice_data: Vec::new(),
                }));
            }
            ListenInput::Audio(data) => {
                if self.state == ListenerState::Idle {
                    self.state = ListenerState::Listening { is_speech: false };
                }
                if let ListenerState::Listening { .. } = self.state {
                    self.accumulate_audio(&data);

                    if let ListenerState::Listening { is_speech: true } = self.state
                        && self.check_silence_timeout()
                    {
                        tracing::debug!(
                            component = "VAD", event = "silence_timeout",
                            session_id = %self.session_id,
                            "listener: silence timeout"
                        );
                        self.asr_pending = true;
                    }
                }
            }
        }
    }

    async fn drain_outputs(&mut self) -> Vec<TurnOutput> {
        if self.asr_pending {
            self.asr_pending = false;
            let result = self.run_asr().await;
            self.internal_reset();
            let mut outputs: Vec<TurnOutput> = self.queue.drain(..).collect();
            if let Some(result) = result {
                outputs.push(TurnOutput::TurnComplete(result));
            }
            return outputs;
        }

        if matches!(self.state, ListenerState::Listening { is_speech: true })
            && self.check_silence_timeout()
        {
            let result = self.run_asr().await;
            self.internal_reset();
            let mut outputs: Vec<TurnOutput> = self.queue.drain(..).collect();
            if let Some(result) = result {
                outputs.push(TurnOutput::TurnComplete(result));
            }
            return outputs;
        }

        self.queue.drain(..).collect()
    }

    async fn flush(&mut self) -> Option<TurnResult> {
        let pending: Vec<TurnOutput> = self.queue.drain(..).collect();
        for event in pending {
            if let TurnOutput::TurnComplete(result) = event {
                self.internal_reset();
                return Some(result);
            }
        }

        let result = self.run_asr().await;
        self.internal_reset();
        result
    }

    fn has_active_speech(&self) -> bool {
        matches!(self.state, ListenerState::Listening { is_speech: true })
    }

    fn reconfigure(&mut self, params: &AudioParam) {
        self.client_input_sample_rate = params.sample_rate;
        let mut dec = self.decoder.lock().unwrap();
        *dec = ropus::Decoder::new(params.sample_rate, ropus::Channels::Mono).unwrap();
    }

    async fn reset(&mut self, silence_voice_timeout: Option<i64>) {
        self.internal_reset();
        self.silence_voice_timeout = silence_voice_timeout;
        self.total_pcm.clear();
        self.queue.clear();
    }
}
