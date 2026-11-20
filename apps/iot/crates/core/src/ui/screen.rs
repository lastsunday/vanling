//! What the panel draws with that is not the picture.

/// Whose surface it is, the frame rate to report, and the microphone's calibration.
#[derive(Clone, Copy, Debug, Default)]
pub struct Screen {
    pub instance: u8,
    pub fps: u8,
    pub debug: bool,
    pub spl_offset_db: i16,
    /// The platform's stack reading, taken where the paint path is deepest.
    pub probe: Option<fn()>,
}
