use crate::{asr::Asr, common::ModelError, vad::Vad};
use async_trait::async_trait;
use chrono::Local;
use service::chobits::message::hello::AudioParam;
use std::sync::{Arc, Mutex as StdMutex};
use tokio::sync::Mutex;

/// Maximum prefix padding in samples (300ms at 16kHz).
const PREFIX_SAMPLES_MAX: usize = 4800;

/// Maximum Opus frame duration in ms (per spec, one packet ≤ 120ms).
const MAX_OPUS_FRAME_MS: u64 = 120;

/// Default input sample rate when client does not advertise audio_params.
const DEFAULT_SAMPLE_RATE: u32 = 16000;

#[derive(Debug, Clone)]
pub enum ListenInput {
    Text(String),
    Audio(Vec<u8>),
}

#[async_trait]
pub trait Listener: Send + Sync {
    async fn accept(&mut self, input: ListenInput);
    fn set_state(&mut self, state: ListenState);
    fn get_state(&self) -> ListenState;
    async fn reset(&mut self, silence_voice_timeout: Option<i64>);

    /// Reconfigure decoder and input parameters from client's hello audio_params.
    fn reconfigure(&mut self, params: &AudioParam);

    /// Extract voice data without running ASR (for parallel ASR path).
    async fn take_voice(&mut self) -> Vec<f32> {
        Vec::new()
    }

    async fn take_result(&mut self) -> (Vec<f32>, core::result::Result<ListenResult, ModelError>);

    fn clone_asr(&self) -> Option<Arc<Mutex<Box<dyn Asr>>>> {
        None
    }

    async fn get_raw_pcm(&mut self) -> Vec<f32> {
        Vec::new()
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ListenState {
    Idle,
    /// is_speech
    Listening(bool),
    End,
}

#[derive(Debug, Clone)]
pub enum ListenResult {
    Text(String),
    Audio { text: String, prob: f32 },
}

pub struct DefaultListener {
    temp_voice_data: Vec<f32>,
    voice_data: Vec<f32>,
    vad: Box<dyn Vad>,
    asr: Arc<Mutex<Box<dyn Asr>>>,
    decoder: StdMutex<opus::Decoder>,
    pub state: ListenState,
    silence_voice_timeout: Option<i64>,
    latest_speaking_time: Option<i64>,
    /// Sample rate from client hello (defaults to 16000).
    client_input_sample_rate: u32,
    /// Ring buffer for prefix padding (~300ms of raw audio).
    prefix_buffer: Vec<f32>,
    /// Whether prefix has been flushed for current speech turn.
    prefix_flushed: bool,
    /// Accumulates ALL decoded PCM (diagnostic only).
    total_pcm: Vec<f32>,
    pending_text: Option<String>,
}

impl DefaultListener {
    pub fn new(vad: Box<dyn Vad>, asr: Arc<Mutex<Box<dyn Asr>>>) -> Self {
        Self {
            vad,
            asr,
            temp_voice_data: Vec::new(),
            voice_data: Vec::new(),
            decoder: StdMutex::new(
                opus::Decoder::new(DEFAULT_SAMPLE_RATE, opus::Channels::Mono).unwrap(),
            ),
            state: ListenState::Idle,
            silence_voice_timeout: None,
            latest_speaking_time: None,
            client_input_sample_rate: DEFAULT_SAMPLE_RATE,
            prefix_buffer: Vec::with_capacity(PREFIX_SAMPLES_MAX),
            prefix_flushed: false,
            total_pcm: Vec::new(),
            pending_text: None,
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
                    self.state = ListenState::Listening(false);
                }
                if let ListenState::Listening(_) = self.state {
                    // Allocate buffer large enough for any Opus packet (max 120ms).
                    let frame_size = ((self.client_input_sample_rate as u64 * MAX_OPUS_FRAME_MS)
                        / 1000) as usize;
                    let mut samples = vec![0f32; frame_size];
                    let len =
                        match self
                            .decoder
                            .lock()
                            .unwrap()
                            .decode_float(&data, &mut samples, false)
                        {
                            Ok(len) => len,
                            Err(_) => return,
                        };
                    for s in samples[..len].iter_mut() {
                        *s = s.clamp(-1.0, 1.0);
                    }
                    self.total_pcm.extend_from_slice(&samples[..len]);
                    self.temp_voice_data.append(&mut samples[..len].to_vec());
                    let window_size = self.vad.window_size();
                    while self.temp_voice_data.len() > window_size {
                        let window: Vec<f32> = self.temp_voice_data.drain(..window_size).collect();

                        // 1. Maintain ring buffer for prefix padding.
                        self.prefix_buffer.extend(&window);
                        if self.prefix_buffer.len() > PREFIX_SAMPLES_MAX {
                            let excess = self.prefix_buffer.len() - PREFIX_SAMPLES_MAX;
                            self.prefix_buffer.drain(..excess);
                        }

                        // 2. VAD decision only (no longer accumulates audio internally).
                        if self.vad.accept_waveform(&window).is_err() {
                            return;
                        }

                        // 3. Audio management in Listener.
                        if self.vad.is_speech() {
                            self.state = ListenState::Listening(true);
                            self.latest_speaking_time = Some(Local::now().timestamp_millis());
                        } else {
                            self.prefix_flushed = false;
                        }

                        if self.state == ListenState::Listening(true) {
                            if !self.prefix_flushed {
                                // First speech frame in this turn — flush prefix (includes current window).
                                self.voice_data.append(&mut self.prefix_buffer);
                                self.prefix_buffer = Vec::with_capacity(PREFIX_SAMPLES_MAX);
                                self.prefix_flushed = true;
                            } else {
                                // Subsequent speech frames.
                                self.voice_data.extend_from_slice(&window);
                            }
                        }
                    }
                }
                if let (Some(silence_voice_timeout), Some(latest_speaking_time)) =
                    (self.silence_voice_timeout, self.latest_speaking_time)
                {
                    let offset_time = Local::now().timestamp_millis() - latest_speaking_time;
                    if offset_time >= silence_voice_timeout {
                        self.state = ListenState::End;
                    }
                }
            }
        }
    }

    fn reconfigure(&mut self, params: &AudioParam) {
        self.client_input_sample_rate = params.sample_rate;
        let mut dec = self.decoder.lock().unwrap();
        *dec = opus::Decoder::new(params.sample_rate, opus::Channels::Mono).unwrap();
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

    fn clone_asr(&self) -> Option<Arc<Mutex<Box<dyn Asr>>>> {
        Some(self.asr.clone())
    }

    async fn take_result(&mut self) -> (Vec<f32>, core::result::Result<ListenResult, ModelError>) {
        if let Some(text) = self.pending_text.take() {
            return (Vec::new(), Ok(ListenResult::Text(text)));
        }
        let voice_data = core::mem::take(&mut self.voice_data);
        if voice_data.is_empty() {
            return (
                voice_data,
                Ok(ListenResult::Audio {
                    text: String::new(),
                    prob: 1.0,
                }),
            );
        }
        let mut asr = self.asr.lock().await;
        let result = asr
            .transcribe(self.client_input_sample_rate, &voice_data)
            .await;
        match result {
            Ok(transcript) => (
                voice_data,
                Ok(ListenResult::Audio {
                    text: transcript.text,
                    prob: transcript.prob,
                }),
            ),
            Err(e) => (voice_data, Err(e)),
        }
    }

    async fn reset(&mut self, silence_voice_timeout: Option<i64>) {
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
    }

    async fn get_raw_pcm(&mut self) -> Vec<f32> {
        core::mem::take(&mut self.total_pcm)
    }
}
