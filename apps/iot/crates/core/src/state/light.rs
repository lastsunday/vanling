//! The light surfaces as the render loop reads them: the breathing schedule and the
//! behavior a page's snapshot carries.
use super::*;

/// A breathing schedule: brightness envelope plus the hue path it travels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Breath {
    pub period_ms: u32,
    pub hue_period_ms: u32,
    /// Hue sweep width; `0` means a static standalone color. Ignored when
    /// `group_len > 0`.
    pub hue_span: u8,
    /// Trough value of the brightness envelope. A nonzero floor keeps the
    /// breathing glow lit instead of fading fully to black.
    pub min_brightness: u8,
    pub max_brightness: u8,
    pub saturation: u8,
    /// Base hue the sweep starts from (or holds when `hue_span == 0`). Ignored
    /// when `group_len > 0`.
    pub hue: u8,
    /// Color waypoints on the 256-step hue wheel. `group_len` picks the
    /// trajectory: `0` = the `hue`/`hue_span` sweep above; `1` = a static
    /// standalone color, `group[0]`; `>=2` = rotate through the waypoints,
    /// walking each `group[i] -> group[i+1]` segment over
    /// `hue_period_ms / group_len` and wrapping `group[last] -> group[0]`.
    pub group: [u8; Self::MAX_GROUP],
    pub group_len: u8,
}

impl Breath {
    pub const MAX_GROUP: usize = GROUP_CAPACITY;
}

/// Light behavior, read by the render loop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LightState {
    Off,
    Solid { color: Rgb, brightness: u8 },
    Breath(Breath),
}

impl LightState {
    pub const fn boot() -> Self {
        LightState::Breath(Breath {
            period_ms: DeviceManager::DEFAULT_PERIOD_MS,
            hue_period_ms: DeviceManager::DEFAULT_HUE_PERIOD_MS,
            hue_span: DeviceManager::DEFAULT_HUE_SPAN,
            min_brightness: DeviceManager::DEFAULT_MIN_BRIGHTNESS,
            max_brightness: DeviceManager::DEFAULT_MAX_BRIGHTNESS,
            saturation: DeviceManager::DEFAULT_SATURATION,
            hue: DeviceManager::DEFAULT_HUE,
            group: DeviceManager::DEFAULT_GROUP,
            group_len: DeviceManager::DEFAULT_GROUP_LEN,
        })
    }
}

impl Default for LightState {
    fn default() -> Self {
        LightState::boot()
    }
}
