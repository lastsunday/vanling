use embedded_hal::i2c::I2c;
use iot_core::drivers::input::{
    GestureEvent, InputEvent, InputSource, TouchContinuity, TouchMap, ft6x06_point_is_live,
};

pub const FT6336_I2C_ADDR: u8 = 0x38;

/// Raw `GESTURE_ID` register of the FT5x06 family (`0x10` up / `0x14` left /
/// `0x18` down / `0x1C` right), read verbatim for the overlay so the chip's
/// built-in slide engine and the software classifier stay comparable. Never
/// used for classification.
const REG_GESTURE_ID: u8 = 0x01;
const REG_TD_STATUS: u8 = 0x02;
/// First point's `XH` register; each point occupies six contiguous bytes
/// (`XH XL YH YL` + weight + misc).
const POINTS_BASE_REG: u8 = 0x03;
const POINT_BYTES: usize = 6;
/// The FT6336 reports at most two self-capacitance contacts.
const FT6336_MAX_POINTS: usize = 2;
/// Touch-decision threshold (FT5x06 family system register), one step below
/// the default so a marginal press latches; the diagnostics verify it —
/// dropped presses shrink while the anomaly row must stay zero.
const REG_THGROUP: u8 = 0x80;
const TOUCH_THRESHOLD: u8 = 0x14;

/// FocalTech factory defaults for the slide/zoom trigger distances, restored
/// over the persisted 0xFF.
const GESTURE_DISTANCE_FACTORY_LR: u8 = 0x19;
const GESTURE_DISTANCE_FACTORY_UD: u8 = 0x19;
const GESTURE_DISTANCE_FACTORY_ZOOM: u8 = 0x32;

const REG_DISTANCE_LEFT_RIGHT: u8 = 0x94;
const REG_DISTANCE_UP_DOWN: u8 = 0x95;
const REG_DISTANCE_ZOOM: u8 = 0x96;
/// `CTRL` mode register: `0` keeps Active, `1` lapses to Monitor.
const REG_CTRL_MODE: u8 = 0x86;
const CTRL_KEEP_ACTIVE: u8 = 0x00;
/// Gesture/scan mode: `0` polls (continuous data refresh), `1` triggers
/// (republishes on detection change only).
const REG_G_MODE: u8 = 0xA4;
const G_MODE_POLLING: u8 = 0x00;

/// One decoded sample: normalized active contacts, valid count, raw count.
type TouchSample = ([(u16, u16); FT6336_MAX_POINTS], usize, u8);

/// FT6336 (FT5x06 family) capacitive touch controller, polled over a
/// board-provided I2C device on the shared bus. One failed read is tolerated
/// (log once per run, pulse `Ghost` on its rising edge, skip the sample) so a
/// wedged bus can neither stall the input pipeline nor hide from the
/// on-panel diagnostic. First poll lowers the touch threshold and applies the
/// mode config; every poll pulses [`InputEvent::ChipGesture`] when the
/// controller's `GESTURE_ID` changes.
///
/// [`Ghost`]: iot_core::drivers::input::GestureEvent::Ghost
pub struct Ft6336<D: I2c> {
    i2c: D,
    addr: u8,
    map: TouchMap,
    width: u16,
    height: u16,
    continuity: TouchContinuity,
    /// Deduplicates the failure log and tally pulse across a contiguous run.
    error_logged: bool,
    /// The touch threshold has been applied for this boot.
    sensitivity_applied: bool,
    /// The mode config has been applied for this boot.
    mode_config_applied: bool,
    /// Last read `GESTURE_ID`; the poll pulses [`InputEvent::ChipGesture`] on
    /// change so the diagnostic overlay sees the controller's own engine.
    gesture_id: u8,
}

impl<D: I2c> Ft6336<D> {
    pub fn new(i2c: D, addr: u8, map: TouchMap, width: u16, height: u16) -> Self {
        Self {
            i2c,
            addr,
            map,
            width,
            height,
            continuity: TouchContinuity::new(),
            error_logged: false,
            sensitivity_applied: false,
            mode_config_applied: false,
            gesture_id: 0,
        }
    }

    /// Applies the touch threshold once at boot, tolerating a silent panel
    /// (log once, keep polling). Whether the write took is judged by behavior
    /// (fewer dropped presses, zero anomaly row), not a read-back.
    fn apply_sensitivity(&mut self) {
        if self
            .i2c
            .write(self.addr, &[REG_THGROUP, TOUCH_THRESHOLD])
            .is_err()
        {
            log::warn!("[TOUCH] threshold write failed");
        }
    }

    /// Applies the mode config once at boot: restores the factory trigger
    /// distances over whichever the FT6336U persisted, keeps `CTRL` in Active
    /// (the freeze hunt implicated the Monitor lapse) and locks `G_MODE` to
    /// polling. Tolerates a quiet chip (log once per write, keep polling);
    /// whether the writes took is judged by the diagnostics, not a read-back.
    fn apply_mode_config(&mut self) {
        for (reg, wanted) in [
            (REG_DISTANCE_LEFT_RIGHT, GESTURE_DISTANCE_FACTORY_LR),
            (REG_DISTANCE_UP_DOWN, GESTURE_DISTANCE_FACTORY_UD),
            (REG_DISTANCE_ZOOM, GESTURE_DISTANCE_FACTORY_ZOOM),
        ] {
            if self.i2c.write(self.addr, &[reg, wanted]).is_err() {
                log::warn!("[TOUCH] gesture distance write failed (0x{reg:02X})");
            }
        }
        if self
            .i2c
            .write(self.addr, &[REG_CTRL_MODE, CTRL_KEEP_ACTIVE])
            .is_err()
        {
            log::warn!("[TOUCH] active-mode lock write failed");
        }
        if self
            .i2c
            .write(self.addr, &[REG_G_MODE, G_MODE_POLLING])
            .is_err()
        {
            log::warn!("[TOUCH] polling-mode lock write failed");
        }
    }

    /// Reads the raw `GESTURE_ID` register; `None` on any I2C hiccup (the touch
    /// sample's own error path already reports a wedged bus).
    fn read_gesture_id(&mut self) -> Option<u8> {
        let mut reg = [0u8; 1];
        self.i2c
            .write_read(self.addr, &[REG_GESTURE_ID], &mut reg)
            .ok()?;
        Some(reg[0])
    }

    fn read_sample(&mut self) -> Result<TouchSample, D::Error> {
        let mut status = [0u8; 1];
        self.i2c
            .write_read(self.addr, &[REG_TD_STATUS], &mut status)?;
        let contacts = status[0] & 0x0f;
        let count = core::cmp::min(contacts as usize, FT6336_MAX_POINTS);

        let mut raw = [(0u16, 0u16); FT6336_MAX_POINTS];
        let mut n = 0;
        if count > 0 {
            let mut buf = [0u8; FT6336_MAX_POINTS * POINT_BYTES];
            let len = count * POINT_BYTES;
            self.i2c
                .write_read(self.addr, &[POINTS_BASE_REG], &mut buf[..len])?;
            for i in 0..count {
                let base = i * POINT_BYTES;
                let x_h = buf[base];
                let x_l = buf[base + 1];
                let y_h = buf[base + 2];
                let y_l = buf[base + 3];
                if !ft6x06_point_is_live(x_h) {
                    continue;
                }
                let x = ((x_h & 0x0f) as u16) << 8 | x_l as u16;
                let y = ((y_h & 0x0f) as u16) << 8 | y_l as u16;
                let (x, y) = self.map.map((x, y), self.width, self.height);
                raw[n] = (x, y);
                n += 1;
            }
        }
        Ok((raw, n, contacts))
    }
}

impl<D: I2c> InputSource for Ft6336<D>
where
    D::Error: core::fmt::Debug,
{
    fn poll(&mut self, _now_ms: u64) -> Option<InputEvent> {
        if !self.sensitivity_applied {
            self.sensitivity_applied = true;
            self.apply_sensitivity();
        }
        if !self.mode_config_applied {
            self.mode_config_applied = true;
            self.apply_mode_config();
        }
        // A changed GESTURE_ID is a diagnostic pulse: it wins this poll's slot
        // and the touch sample rides on the next one, so the register
        // transition is never masked by a same-poll contact.
        if let Some(id) = self.read_gesture_id()
            && id != self.gesture_id
        {
            self.gesture_id = id;
            return Some(InputEvent::ChipGesture(id));
        }
        let (raw, n, contacts) = match self.read_sample() {
            Ok(sample) => {
                self.error_logged = false;
                sample
            }
            Err(e) => {
                if !self.error_logged {
                    log::warn!("[TOUCH] i2c read failed: {e:?}");
                    self.error_logged = true;
                    return Some(InputEvent::Gesture(GestureEvent::Ghost));
                }
                return None;
            }
        };
        self.continuity
            .update(&raw[..n], contacts)
            .map(InputEvent::Touch)
    }
}
