use std::collections::VecDeque;
use std::sync::{Arc, Mutex as StdMutex};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use service::ling::asr::Asr;
use service::ling::asr::AsrStream;
use service::ling::listener::{ListenInput, Listener, TurnOutput, TurnResult};
use service::ling::message::hello::AudioParam;
use service::ling::vad::Vad;

const PREFIX_SAMPLES_MAX: usize = 9600;

const MAX_OPUS_FRAME_MS: u64 = 120;

const DEFAULT_SAMPLE_RATE: u32 = 16000;

/// Silent audio (ms, counted from consumed samples rather than wall clock)
/// required after VAD reports non-speech before triggering ASR finish.
const VAD_SILENCE_CONFIRM_MS: u64 = 200;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum ListenerState {
    Idle,
    Listening { is_speech: bool },
}

pub struct DefaultListener {
    session_id: String,
    vad: Box<dyn Vad>,
    asr: Arc<dyn Asr>,
    stream: Option<Box<dyn AsrStream>>,
    decoder: StdMutex<ropus::Decoder>,
    state: ListenerState,
    silence_voice_timeout: Option<i64>,
    spoken: bool,
    silent_samples: u64,
    last_audio_received: Instant,
    client_input_sample_rate: u32,
    prefix_buffer: Vec<f32>,
    last_partial: Option<String>,
    queue: VecDeque<TurnOutput>,
    asr_pending: bool,
    audio_sample_count: u64,
}

impl DefaultListener {
    pub fn new(
        session_id: String,
        vad: Box<dyn Vad>,
        asr: Arc<dyn Asr>,
        silence_voice_timeout: Option<i64>,
    ) -> Self {
        Self {
            session_id,
            vad,
            asr,
            stream: None,
            decoder: StdMutex::new(
                ropus::Decoder::new(DEFAULT_SAMPLE_RATE, ropus::Channels::Mono).unwrap(),
            ),
            state: ListenerState::Idle,
            silence_voice_timeout,
            spoken: false,
            silent_samples: 0,
            last_audio_received: Instant::now(),
            client_input_sample_rate: DEFAULT_SAMPLE_RATE,
            prefix_buffer: Vec::with_capacity(PREFIX_SAMPLES_MAX),
            last_partial: None,
            queue: VecDeque::new(),
            asr_pending: false,
            audio_sample_count: 0,
        }
    }

    fn finish_stream(&mut self, skip_empty_check: bool) -> Option<TurnResult> {
        let stream = self.stream.as_ref()?;
        let t0 = Instant::now();
        let result = stream.finish();
        self.stream = None;
        self.audio_sample_count = 0;

        let result = result?;
        tracing::info!(
            component = "ASR", event = "asr_stream_finish",
            session_id = %self.session_id,
            finish_ms = t0.elapsed().as_millis(),
            text_len = result.text.len(),
            "asr: stream finish"
        );
        if !skip_empty_check && result.text.trim().is_empty() {
            tracing::debug!(
                component = "ASR", event = "asr_empty",
                session_id = %self.session_id,
                "asr: empty text, skipping"
            );
            return None;
        }
        Some(TurnResult {
            text: result.text,
            prob: result.prob,
            voice_data: Vec::new(),
        })
    }

    fn ms_to_samples(&self, ms: i64) -> u64 {
        ms as u64 * u64::from(self.client_input_sample_rate) / 1000
    }

    fn check_silence_timeout(&self) -> bool {
        if !self.spoken {
            return false;
        }
        if let Some(timeout) = self.silence_voice_timeout
            && self.silent_samples >= self.ms_to_samples(timeout)
        {
            return true;
        }
        self.silent_samples >= self.ms_to_samples(VAD_SILENCE_CONFIRM_MS as i64)
    }

    fn decode_opus(&self, data: &[u8]) -> Option<(Vec<f32>, usize)> {
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
                return None;
            }
        };
        for s in samples[..len].iter_mut() {
            *s = s.clamp(-1.0, 1.0);
        }
        Some((samples, len))
    }

    fn process_audio(&mut self, data: &[u8]) {
        let Some((samples, len)) = self.decode_opus(data) else {
            return;
        };
        self.last_audio_received = Instant::now();

        if self.stream.is_none() {
            self.audio_sample_count = 0;
        }
        self.audio_sample_count += len as u64;

        let window_size = self.vad.window_size();
        let mut pos = 0;
        while pos < len {
            let chunk = if len - pos < window_size {
                &samples[pos..len]
            } else {
                &samples[pos..pos + window_size]
            };
            pos += chunk.len();

            self.prefix_buffer.extend_from_slice(chunk);
            if self.prefix_buffer.len() > PREFIX_SAMPLES_MAX {
                let excess = self.prefix_buffer.len() - PREFIX_SAMPLES_MAX;
                self.prefix_buffer.drain(..excess);
            }

            if self.vad.accept_waveform(chunk).is_err() {
                return;
            }

            let was_speech = matches!(self.state, ListenerState::Listening { is_speech: true });
            let is_speech = self.vad.is_speech();

            if is_speech {
                self.state = ListenerState::Listening { is_speech: true };
                self.spoken = true;
                self.silent_samples = 0;

                if self.stream.is_none() {
                    let new_stream = self.asr.create_stream();
                    new_stream.accept_waveform(&self.prefix_buffer);
                    self.prefix_buffer = Vec::with_capacity(PREFIX_SAMPLES_MAX);
                    self.stream = Some(new_stream);
                }

                if let Some(ref s) = self.stream {
                    s.accept_waveform(chunk);
                    s.decode();

                    if let Some(partial) = s.get_partial()
                        && self.last_partial.as_deref() != Some(&partial)
                    {
                        self.last_partial = Some(partial.clone());
                        self.queue.push_back(TurnOutput::PartialTranscript(partial));
                    }

                    if s.is_endpoint() {
                        tracing::debug!(
                            component = "ASR", event = "endpoint_detected",
                            session_id = %self.session_id,
                            "asr endpoint detected"
                        );
                        self.asr_pending = true;
                    }
                }
            } else if self.spoken {
                self.silent_samples += chunk.len() as u64;
            }

            if !was_speech && is_speech {
                tracing::debug!(
                    component = "VAD", event = "speech_started",
                    session_id = %self.session_id,
                    "listener: speech started"
                );
                self.queue.push_back(TurnOutput::SpeechStarted);
            }
        }
    }

    fn reset_state(&mut self, full: bool) {
        self.state = ListenerState::Idle;
        self.spoken = false;
        self.silent_samples = 0;
        self.last_audio_received = Instant::now();
        if full {
            self.vad.clear();
            self.prefix_buffer.clear();
        }
        self.last_partial = None;
        self.asr_pending = false;
        self.stream = None;
        self.audio_sample_count = 0;
    }

    async fn finish_turn(&mut self) -> Vec<TurnOutput> {
        tracing::info!(
            component = "ASR", event = "finish_turn",
            session_id = %self.session_id,
            audio_ms = self.audio_sample_count / 16,
            "asr: finish turn"
        );
        let result = self.finish_stream(false);
        if result.is_some() {
            self.reset_state(true);
        } else {
            self.reset_state(false);
        }
        let mut outputs: Vec<TurnOutput> = self.queue.drain(..).collect();
        if let Some(result) = result {
            outputs.push(TurnOutput::TurnComplete(result));
        }
        outputs
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
                self.reset_state(true);
                self.queue.push_back(TurnOutput::TurnComplete(TurnResult {
                    text,
                    prob: 1.0,
                    voice_data: Vec::new(),
                }));
            }
            ListenInput::Audio(data) => {
                if matches!(self.state, ListenerState::Idle) {
                    self.state = ListenerState::Listening { is_speech: false };
                }
                if let ListenerState::Listening { .. } = self.state {
                    self.process_audio(&data);

                    if let ListenerState::Listening { is_speech: true } = self.state
                        && self.check_silence_timeout()
                        && !self.asr_pending
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
            return self.finish_turn().await;
        }

        if let Some(timeout) = self.silence_voice_timeout
            && self.spoken
            && self.stream.is_some()
            && !self.asr_pending
            && self.last_audio_received.elapsed() >= Duration::from_millis(timeout as u64)
        {
            tracing::debug!(
                component = "LISTENER", event = "transport_stall",
                session_id = %self.session_id,
                idle_ms = self.last_audio_received.elapsed().as_millis(),
                "listener: transport stall, no audio received"
            );
            return self.finish_turn().await;
        }

        if matches!(self.state, ListenerState::Listening { is_speech: true })
            && self.check_silence_timeout()
            && !self.asr_pending
            && self.stream.is_some()
        {
            return self.finish_turn().await;
        }

        self.queue.drain(..).collect()
    }

    async fn flush(&mut self) -> Option<TurnResult> {
        let pending: Vec<TurnOutput> = self.queue.drain(..).collect();
        for event in pending {
            if let TurnOutput::TurnComplete(result) = event {
                self.reset_state(true);
                return Some(result);
            }
        }

        let result = self.finish_stream(true);
        self.reset_state(true);
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
        self.reset_state(true);
        self.silence_voice_timeout = silence_voice_timeout;
        self.queue.clear();
    }
}
