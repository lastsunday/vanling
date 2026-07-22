use framework::error::AppError;
use framework::error::critical_code::CriticalErrorCode;
use nnnoiseless::DenoiseState;
use rubato::audioadapter_buffers::direct::InterleavedSlice;
use rubato::{Fft, FixedSync, Resampler};

const INPUT_FRAME_16KHZ: usize = 160;
const INPUT_FRAME_48KHZ: usize = 480;
const SCALE: f32 = 32768.0;
const RNNOISE_BLEND: f32 = 1.0;

pub struct RnnoisePreprocessor {
    denoiser: Box<DenoiseState<'static>>,
    upsampler: Fft<f32>,
    downsampler: Fft<f32>,
    upsampled_buffer: [f32; INPUT_FRAME_48KHZ],
    rnnoise_input_buffer: [f32; INPUT_FRAME_48KHZ],
    rnnoise_output_buffer: [f32; INPUT_FRAME_48KHZ],
    decimated_buffer: [f32; INPUT_FRAME_16KHZ],
}

impl RnnoisePreprocessor {
    pub fn new() -> Result<Self, AppError> {
        let upsampler = Fft::<f32>::new(16000, 48000, INPUT_FRAME_16KHZ, 2, 1, FixedSync::Input)
            .map_err(|e| {
                AppError::from_code(CriticalErrorCode::InternalError)
                    .with_extra(format!("Failed to create upsampler: {e}"))
            })?;

        let downsampler = Fft::<f32>::new(48000, 16000, INPUT_FRAME_48KHZ, 2, 1, FixedSync::Input)
            .map_err(|e| {
                AppError::from_code(CriticalErrorCode::InternalError)
                    .with_extra(format!("Failed to create downsampler: {e}"))
            })?;

        Ok(Self {
            denoiser: DenoiseState::new(),
            upsampler,
            downsampler,
            upsampled_buffer: [0.0; INPUT_FRAME_48KHZ],
            rnnoise_input_buffer: [0.0; INPUT_FRAME_48KHZ],
            rnnoise_output_buffer: [0.0; INPUT_FRAME_48KHZ],
            decimated_buffer: [0.0; INPUT_FRAME_16KHZ],
        })
    }

    pub fn process(
        &mut self,
        input_16k: &[f32; INPUT_FRAME_16KHZ],
        out: &mut [f32; INPUT_FRAME_16KHZ],
    ) -> Result<(), AppError> {
        let buffer_in =
            InterleavedSlice::new(&input_16k[..], 1, INPUT_FRAME_16KHZ).map_err(|e| {
                AppError::from_code(CriticalErrorCode::InternalError)
                    .with_extra(format!("Adapter error: {e}"))
            })?;

        let mut buffer_out =
            InterleavedSlice::new_mut(&mut self.upsampled_buffer[..], 1, INPUT_FRAME_48KHZ)
                .map_err(|e| {
                    AppError::from_code(CriticalErrorCode::InternalError)
                        .with_extra(format!("Adapter error: {e}"))
                })?;

        self.upsampler
            .process_into_buffer(&buffer_in, &mut buffer_out, None)
            .map_err(|e| {
                AppError::from_code(CriticalErrorCode::InternalError)
                    .with_extra(format!("Upsample failed: {e}"))
            })?;

        self.rnnoise_input_buffer
            .iter_mut()
            .zip(self.upsampled_buffer.iter())
            .for_each(|(d, s)| *d = s * SCALE);

        self.denoiser
            .process_frame(&mut self.rnnoise_output_buffer, &self.rnnoise_input_buffer);

        self.rnnoise_output_buffer
            .iter_mut()
            .for_each(|s| *s /= SCALE);

        for (denoised, original) in self
            .rnnoise_output_buffer
            .iter_mut()
            .zip(self.upsampled_buffer.iter())
        {
            *denoised = *denoised * RNNOISE_BLEND + *original * (1.0 - RNNOISE_BLEND);
        }

        let down_input =
            InterleavedSlice::new(&self.rnnoise_output_buffer[..], 1, INPUT_FRAME_48KHZ).map_err(
                |e| {
                    AppError::from_code(CriticalErrorCode::InternalError)
                        .with_extra(format!("Adapter error: {e}"))
                },
            )?;

        let mut down_output =
            InterleavedSlice::new_mut(&mut self.decimated_buffer[..], 1, INPUT_FRAME_16KHZ)
                .map_err(|e| {
                    AppError::from_code(CriticalErrorCode::InternalError)
                        .with_extra(format!("Adapter error: {e}"))
                })?;

        self.downsampler
            .process_into_buffer(&down_input, &mut down_output, None)
            .map_err(|e| {
                AppError::from_code(CriticalErrorCode::InternalError)
                    .with_extra(format!("Downsample failed: {e}"))
            })?;

        out.copy_from_slice(&self.decimated_buffer);

        Ok(())
    }

    pub fn reset(&mut self) {
        self.denoiser = DenoiseState::new();
        self.upsampler.reset();
        self.downsampler.reset();
    }
}
