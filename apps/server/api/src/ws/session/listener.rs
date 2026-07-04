use crate::{asr::Asr, common::ModelError, config::audio::AudioConfig, vad::Vad};
use async_trait::async_trait;
use chrono::Local;
use std::sync::{Arc, Mutex as StdMutex};
use tokio::sync::Mutex;

/// Maximum prefix padding in samples (300ms at 16kHz).
const PREFIX_SAMPLES_MAX: usize = 4800;

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
    audio_config: Arc<AudioConfig>,
    /// Ring buffer for prefix padding (~300ms of raw audio).
    prefix_buffer: Vec<f32>,
    /// Whether prefix has been flushed for current speech turn.
    prefix_flushed: bool,
    /// Accumulates ALL decoded PCM (diagnostic only).
    total_pcm: Vec<f32>,
    pending_text: Option<String>,
}

impl DefaultListener {
    pub fn new(
        vad: Box<dyn Vad>,
        asr: Arc<Mutex<Box<dyn Asr>>>,
        audio_config: Arc<AudioConfig>,
    ) -> Self {
        let sample_rate = audio_config
            .input_sample_rate
            .expect("input sample rate is empty");
        Self {
            vad,
            asr,
            temp_voice_data: Vec::new(),
            voice_data: Vec::new(),
            decoder: StdMutex::new(opus::Decoder::new(sample_rate, opus::Channels::Mono).unwrap()),
            state: ListenState::Idle,
            silence_voice_timeout: None,
            latest_speaking_time: None,
            audio_config,
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
                    let sample_rate = self
                        .audio_config
                        .input_sample_rate
                        .expect("input sample rate is empty");
                    let channel = self
                        .audio_config
                        .input_channel
                        .expect("input channel is empty");
                    let frame_duration = self
                        .audio_config
                        .input_frame_duration
                        .expect("input frame duration is empty");
                    let frame_size =
                        ((sample_rate as u64 * channel as u64 * frame_duration) / 1000) as usize;
                    let mut samples = vec![0f32; frame_size];
                    let len =
                        match self
                            .decoder
                            .lock()
                            .unwrap()
                            .decode_float(&data, &mut samples, false)
                        {
                            Ok(len) => len,
                            Err(e) => {
                                tracing::error!(
                                    "Opus decode error: {e}, data_len={}, first_bytes={:02x?}",
                                    data.len(),
                                    &data[..data.len().min(8)]
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

                        // 1. Maintain ring buffer for prefix padding.
                        self.prefix_buffer.extend(&window);
                        if self.prefix_buffer.len() > PREFIX_SAMPLES_MAX {
                            let excess = self.prefix_buffer.len() - PREFIX_SAMPLES_MAX;
                            self.prefix_buffer.drain(..excess);
                        }

                        // 2. VAD decision only (no longer accumulates audio internally).
                        if let Err(e) = self.vad.accept_waveform(&window) {
                            tracing::error!("accept_waveform error = {}", e.to_string());
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
        let sample_rate: u32 = self
            .audio_config
            .input_sample_rate
            .expect("input sample rate is empty");
        let mut asr = self.asr.lock().await;
        let result = asr.transcribe(sample_rate, &voice_data).await;
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
