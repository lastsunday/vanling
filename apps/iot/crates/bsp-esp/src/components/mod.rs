/// The clock row both codecs are programmed from, and the rate the DMA fills
/// its ring at. Ungated and tiny: it is arithmetic, and a board that wires
/// both halves of the audio peripheral has to be able to name it from either
/// side without pulling in a driver that may not be fitted.
pub mod audio_clock;
#[cfg(feature = "backlight")]
pub mod backlight;
#[cfg(feature = "button")]
pub mod button;
#[cfg(feature = "es7210")]
pub mod es7210;
#[cfg(feature = "es8311")]
pub mod es8311;
#[cfg(feature = "ft6336")]
pub mod ft6336;
#[cfg(feature = "gc2145")]
pub mod gc2145;
#[cfg(test)]
pub mod mock_i2c;
#[cfg(feature = "pca9557")]
pub mod pca9557;
#[cfg(feature = "qmi8658")]
pub mod qmi8658;
/// Only the two codecs' register sequences need it, and each of those features
/// already pulls in `embedded-hal`, so gating on either keeps the module
/// compiling standalone per feature.
#[cfg(any(feature = "es7210", feature = "es8311"))]
pub mod register_seq;
#[cfg(feature = "st7789")]
pub mod st7789;
#[cfg(feature = "ws2812")]
pub mod ws2812;
