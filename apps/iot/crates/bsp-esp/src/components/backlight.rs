use esp_hal::ledc::LowSpeed;
use esp_hal::ledc::channel::{Channel, ChannelIFace};

/// Panel backlight driven by an LEDC PWM channel. The BL bus is active-low
/// (on = pulled low), so the channel duty is the software inversion of the
/// requested level: `level_pct` 100 → 0% duty (bright), 0 → 100% duty (dark).
pub struct Backlight {
    channel: Channel<'static, LowSpeed>,
}

impl Backlight {
    pub fn new(channel: Channel<'static, LowSpeed>) -> Self {
        Self { channel }
    }

    /// Sets the backlight to `level_pct` percent of full brightness.
    pub fn set_level_pct(&mut self, level_pct: u8) {
        let level = u8::min(level_pct, 100);
        let duty_pct = 100 - level;
        if let Err(e) = self.channel.set_duty(duty_pct) {
            log::error!("[DISPLAY] set_duty failed: {e:?}");
        }
    }
}
