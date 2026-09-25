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

/// Boot-time config writes. The FT6336U persists changes in its touch-config
/// area, so earlier experiments could stick across reboots; one build = one
/// run, flip a bool to rebuild another. `RESTORE_FACTORY_DISTANCES` writes the
/// factory trigger distances back over persisted values; `FORCE_ACTIVE_MODE` /
/// `FORCE_POLLING_MODE` lock the work modes the freeze hunt implicated.
const RESTORE_FACTORY_DISTANCES: bool = true;
const FORCE_ACTIVE_MODE: bool = true;
const FORCE_POLLING_MODE: bool = true;

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

/// Identical-sample polls proving a slot is really parked (≈300 ms at the
/// 10 ms cadence); a continuing run re-marks every [`STALL_MARK_EVERY`] polls
/// so seconds-long freezes grow a counter instead of one line.
const STALL_MIN_POLLS: u16 = 30;
const STALL_MARK_EVERY: u16 = 60;

/// One decoded sample: normalized active contacts, valid count, raw count.
type TouchSample = ([(u16, u16); FT6336_MAX_POINTS], usize, u8);

/// FT6336 (FT5x06 family) capacitive touch controller, polled over a
/// board-provided I2C device on the shared bus. One failed read is tolerated
/// (log once per run, pulse `Ghost` on its rising edge, skip the sample) so a
/// wedged bus can neither stall the input pipeline nor hide from the
/// on-panel diagnostic. First poll lowers the touch threshold, applies the
/// run-C mode config, and dumps the boot registers for breadcrumbs; every
/// poll watches raw samples for a parked slot (stall watchdog) and pulses
/// [`InputEvent::ChipGesture`] when the controller's `GESTURE_ID` changes.
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
    /// The A/B mode config has been applied for this boot.
    mode_config_applied: bool,
    /// Last read `GESTURE_ID`; the poll pulses [`InputEvent::ChipGesture`] on
    /// change so the diagnostic overlay sees the controller's own engine.
    gesture_id: u8,
    /// The previous raw sample, for the stall watchdog to diff against.
    prev_points: [(u16, u16); FT6336_MAX_POINTS],
    prev_len: usize,
    prev_contacts: u8,
    /// Identical-coordinate samples per slot while contacts stay up; the stall
    /// watchdog grows these and logs on crossing [`STALL_MIN_POLLS`].
    slot_quiet: [u16; FT6336_MAX_POINTS],
    slot_stall_logged: [bool; FT6336_MAX_POINTS],
    /// How many still samples a logged stall had run, for its resume line.
    stall_len: [u16; FT6336_MAX_POINTS],
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
            prev_points: [(0, 0); FT6336_MAX_POINTS],
            prev_len: 0,
            prev_contacts: 0,
            slot_quiet: [0; FT6336_MAX_POINTS],
            slot_stall_logged: [false; FT6336_MAX_POINTS],
            stall_len: [0; FT6336_MAX_POINTS],
        }
    }

    /// Applies the touch threshold once at boot, tolerating a silent panel
    /// (log once, keep polling). Whether the write took is judged by behavior
    /// (fewer dropped presses, zero anomaly row), not the read-back — which is
    /// only breadcrumb evidence the controller accepted it.
    fn apply_sensitivity(&mut self) {
        if self
            .i2c
            .write(self.addr, &[REG_THGROUP, TOUCH_THRESHOLD])
            .is_err()
        {
            log::warn!("[TOUCH] threshold write failed");
            return;
        }
        let mut reg = [0u8; 1];
        match self.i2c.write_read(self.addr, &[REG_THGROUP], &mut reg) {
            Ok(()) => {
                log::info!(
                    "[TOUCH] threshold 0x{:02X} (wanted 0x{:02X})",
                    reg[0],
                    TOUCH_THRESHOLD
                );
            }
            Err(_) => log::warn!("[TOUCH] threshold read-back failed"),
        }
    }

    /// Applies the run-C mode config once at boot (see the three flags),
    /// tolerating a quiet chip (log once, keep polling). Each read-back is
    /// breadcrumb evidence the controller accepted it; distances staying at
    /// factory proves this part keeps its config across reboots.
    fn apply_mode_config(&mut self) {
        if RESTORE_FACTORY_DISTANCES {
            for (reg, wanted) in [
                (REG_DISTANCE_LEFT_RIGHT, GESTURE_DISTANCE_FACTORY_LR),
                (REG_DISTANCE_UP_DOWN, GESTURE_DISTANCE_FACTORY_UD),
                (REG_DISTANCE_ZOOM, GESTURE_DISTANCE_FACTORY_ZOOM),
            ] {
                if self.i2c.write(self.addr, &[reg, wanted]).is_err() {
                    log::warn!("[TOUCH] gesture distance write failed (0x{reg:02X})");
                    continue;
                }
                if let Some(reg_value) = self.read_register(reg) {
                    log::info!(
                        "[TOUCH] gesture distance 0x{reg:02X} = 0x{reg_value:02X} (wanted 0x{wanted:02X})"
                    );
                }
            }
        }
        if FORCE_ACTIVE_MODE {
            if self
                .i2c
                .write(self.addr, &[REG_CTRL_MODE, CTRL_KEEP_ACTIVE])
                .is_err()
            {
                log::warn!("[TOUCH] active-mode lock write failed");
            } else if let Some(ctrl) = self.read_register(REG_CTRL_MODE) {
                log::info!(
                    "[TOUCH] ctrl 0x{REG_CTRL_MODE:02X} = 0x{ctrl:02X} (wanted 0x{CTRL_KEEP_ACTIVE:02X})"
                );
            }
        }
        if FORCE_POLLING_MODE {
            if self
                .i2c
                .write(self.addr, &[REG_G_MODE, G_MODE_POLLING])
                .is_err()
            {
                log::warn!("[TOUCH] polling-mode lock write failed");
            } else if let Some(mode) = self.read_register(REG_G_MODE) {
                log::info!(
                    "[TOUCH] gmode 0x{REG_G_MODE:02X} = 0x{mode:02X} (wanted 0x{G_MODE_POLLING:02X})"
                );
            }
        }
    }

    /// Boot breadcrumb: dump the registers that identify the controller and the
    /// mode/config it powers on with. The FT6X36 and FT6336U document
    /// conflicting layouts around `0x94`–`0x96`, so the trace tells which map
    /// this part actually uses.
    fn dump_registers(&mut self) {
        const READS: &[(&str, u8)] = &[
            ("devmode", 0x00),
            ("gest", 0x01),
            ("tdst", 0x02),
            ("thgrp", 0x80),
            ("thdif", 0x85),
            ("ctrl", 0x86),
            ("enter-mon", 0x87),
            ("act-rate", 0x88),
            ("mon-rate", 0x89),
            ("dist-lr", 0x94),
            ("dist-ud", 0x95),
            ("dist-zoom", 0x96),
            ("libver-h", 0xA1),
            ("libver-l", 0xA2),
            ("chipid", 0xA3),
            ("g-mode", 0xA4),
            ("pwrmode", 0xA5),
            ("firmware-id", 0xA6),
            ("release", 0xAF),
            ("state", 0xBC),
        ];
        for (name, addr) in READS {
            let Some(value) = self.read_register(*addr) else {
                log::warn!("[TOUCH] reg 0x{addr:02X} ({name}) read failed");
                continue;
            };
            log::info!("[TOUCH] reg 0x{addr:02X} ({name}) = 0x{value:02X}");
        }
    }

    /// Stall watchdog: a live slot holding identical coordinates across many
    /// polls is the serial signature of the freeze. Snapshot each run once —
    /// with the other slot and `GESTURE_ID`/`CTRL`/`G_MODE` at that moment —
    /// re-mark every [`STALL_MARK_EVERY`] polls while it continues, and print a
    /// resume line with the total run length (`was=` polls → ~10 ms each).
    fn log_stall(&mut self, raw: &[(u16, u16)], n: usize, contacts: u8) {
        for slot in 0..FT6336_MAX_POINTS {
            let stays = n > slot
                && self.prev_len > slot
                && self.prev_contacts > 0
                && contacts > 0
                && raw[slot] == self.prev_points[slot];
            if stays {
                let quiet = self.slot_quiet[slot].saturating_add(1);
                self.slot_quiet[slot] = quiet;
                if quiet >= STALL_MIN_POLLS {
                    let mode_hex = self.read_register(REG_G_MODE).unwrap_or(u8::MAX);
                    if !self.slot_stall_logged[slot] {
                        self.slot_stall_logged[slot] = true;
                        self.stall_len[slot] = quiet;
                        let gid_hex = self.read_gesture_id().unwrap_or(u8::MAX);
                        let ctrl_hex = self.read_register(REG_CTRL_MODE).unwrap_or(u8::MAX);
                        let (x0, y0) = raw.first().copied().unwrap_or((0, 0));
                        let (x1, y1) = raw.get(1).copied().unwrap_or((0, 0));
                        log::info!(
                            "[TOUCH] stall slot{slot} p0=({x0},{y0}) p1=({x1},{y1}) ct={contacts} gid=0x{gid_hex:02X} ctrl=0x{ctrl_hex:02X} gmod=0x{mode_hex:02X} still={quiet}"
                        );
                    } else if quiet.is_multiple_of(STALL_MARK_EVERY) {
                        self.stall_len[slot] = quiet;
                        log::info!(
                            "[TOUCH] stall hold slot{slot} still={quiet} gmod=0x{mode_hex:02X}"
                        );
                    }
                }
            } else if self.slot_stall_logged[slot] {
                self.slot_stall_logged[slot] = false;
                let was = self.stall_len[slot];
                self.stall_len[slot] = 0;
                self.slot_quiet[slot] = 0;
                let (a, b) = raw.get(slot).copied().unwrap_or((0, 0));
                log::info!("[TOUCH] resume slot{slot} p0=({a},{b}) was={was}");
            } else {
                self.slot_quiet[slot] = 0;
            }
        }
        self.prev_points = [
            raw.first().copied().unwrap_or((0, 0)),
            raw.get(1).copied().unwrap_or((0, 0)),
        ];
        self.prev_len = n;
        self.prev_contacts = contacts;
    }

    fn read_register(&mut self, addr: u8) -> Option<u8> {
        let mut value = [0u8; 1];
        self.i2c.write_read(self.addr, &[addr], &mut value).ok()?;
        Some(value[0])
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
            self.dump_registers();
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
        self.log_stall(&raw[..n], n, contacts);
        self.continuity
            .update(&raw[..n], contacts)
            .map(InputEvent::Touch)
    }
}
