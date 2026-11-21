use embedded_hal::i2c::I2c;
use iot_core::drivers::input::{
    GestureEvent, InputEvent, InputSource, RawPoint, TouchContinuity, TouchMap,
    ft6x06_point_is_live,
};

pub const FT6336_I2C_ADDR: u8 = 0x38;

/// The frame this digitizer reports in on the 2.0" module it is fitted to.
pub const LANDSCAPE_MAP: TouchMap = TouchMap {
    swap_xy: true,
    mirror_x: false,
    mirror_y: true,
};

// Raw `GESTURE_ID` register of the FT5x06 family, read verbatim for the
// overlay so the chip's built-in slide engine and the software classifier stay
// comparable. Never used for classification. The reported codes are `0x10` up /
// `0x14` left / `0x18` down / `0x1C` right [据称] — the register only carries the
// code, the tests pin that it passes through unmodified and never check the
// directions themselves.
const REG_GESTURE_ID: u8 = 0x01;
const REG_TD_STATUS: u8 = 0x02;
const POINT_BYTES: usize = 6;
const FT6336_MAX_POINTS: usize = 2;
const REG_THGROUP: u8 = 0x80;
const TOUCH_THRESHOLD: u8 = 0x14;

const GESTURE_DISTANCE_FACTORY_LR: u8 = 0x19;
const GESTURE_DISTANCE_FACTORY_UD: u8 = 0x19;
const GESTURE_DISTANCE_FACTORY_ZOOM: u8 = 0x32;

const REG_DISTANCE_LEFT_RIGHT: u8 = 0x94;
const REG_DISTANCE_UP_DOWN: u8 = 0x95;
const REG_DISTANCE_ZOOM: u8 = 0x96;
const REG_CTRL_MODE: u8 = 0x86;
const CTRL_KEEP_ACTIVE: u8 = 0x00;
const REG_G_MODE: u8 = 0xA4;
const G_MODE_POLLING: u8 = 0x00;

struct TouchSample {
    points: [RawPoint; FT6336_MAX_POINTS],
    /// Points with live coordinates; a slot in its lift frame is skipped.
    len: usize,
}

/// FT6336 (FT5x06 family) capacitive touch controller, polled over a board-provided I2C
/// device on the shared bus.
pub struct Ft6336<D: I2c> {
    i2c: D,
    addr: u8,
    map: TouchMap,
    width: u16,
    height: u16,
    continuity: TouchContinuity,
    error_logged: bool,
    sensitivity_applied: bool,
    mode_config_applied: bool,
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

    fn apply_sensitivity(&mut self) {
        if self
            .i2c
            .write(self.addr, &[REG_THGROUP, TOUCH_THRESHOLD])
            .is_err()
        {
            log::warn!("[INPUT] threshold write failed");
        }
    }

    fn apply_mode_config(&mut self) {
        for (reg, wanted) in [
            (REG_DISTANCE_LEFT_RIGHT, GESTURE_DISTANCE_FACTORY_LR),
            (REG_DISTANCE_UP_DOWN, GESTURE_DISTANCE_FACTORY_UD),
            (REG_DISTANCE_ZOOM, GESTURE_DISTANCE_FACTORY_ZOOM),
        ] {
            if self.i2c.write(self.addr, &[reg, wanted]).is_err() {
                log::warn!("[INPUT] gesture distance write failed (0x{reg:02X})");
            }
        }
        if self
            .i2c
            .write(self.addr, &[REG_CTRL_MODE, CTRL_KEEP_ACTIVE])
            .is_err()
        {
            log::warn!("[INPUT] active-mode lock write failed");
        }
        if self
            .i2c
            .write(self.addr, &[REG_G_MODE, G_MODE_POLLING])
            .is_err()
        {
            log::warn!("[INPUT] polling-mode lock write failed");
        }
    }

    fn read_gesture_id(&mut self) -> Option<u8> {
        let mut reg = [0u8; 1];
        self.i2c
            .write_read(self.addr, &[REG_GESTURE_ID], &mut reg)
            .ok()?;
        Some(reg[0])
    }

    fn read_sample(&mut self) -> Result<TouchSample, D::Error> {
        // Status and both contact slots in one burst: a second read would let the chip move
        // on and report a count that no longer matches the points that follow it.
        const BURST: usize = 1 + FT6336_MAX_POINTS * POINT_BYTES;
        let mut buf = [0u8; BURST];
        self.i2c.write_read(self.addr, &[REG_TD_STATUS], &mut buf)?;
        let contacts = usize::from(buf[0] & 0x0f);
        let mut sample = TouchSample {
            points: [RawPoint::new(0, 0, RawPoint::NO_ID); FT6336_MAX_POINTS],
            len: 0,
        };
        if contacts > FT6336_MAX_POINTS {
            return Ok(sample);
        }
        for slot in 0..contacts {
            let base = 1 + slot * POINT_BYTES;
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
            // `Pn_YH[7:4]` is the controller's slot number, `0x0F` when it has none. This
            // panel reorders its two points between samples, so the slot is a hint for the
            // tracker, never an identity; a value past the two-point ceiling is no hint.
            let slot = (y_h >> 4) & 0x0f;
            let hw_id = if usize::from(slot) < FT6336_MAX_POINTS {
                slot
            } else {
                RawPoint::NO_ID
            };
            sample.points[sample.len] = RawPoint::new(x, y, hw_id);
            sample.len += 1;
        }
        Ok(sample)
    }
}

impl<D: I2c> InputSource for Ft6336<D>
where
    D::Error: core::fmt::Debug,
{
    fn name(&self) -> &'static str {
        "touch"
    }

    fn poll(&mut self, _now_ms: u64) -> Option<InputEvent> {
        if !self.sensitivity_applied {
            self.sensitivity_applied = true;
            self.apply_sensitivity();
        }
        if !self.mode_config_applied {
            self.mode_config_applied = true;
            self.apply_mode_config();
        }
        // The chip's slide engine is a hint for the overlay, never for classification: a
        // gesture change must not cost this tick its sample.
        let gesture = self.read_gesture_id();
        let sample = match self.read_sample() {
            Ok(sample) => {
                self.error_logged = false;
                sample
            }
            Err(e) => {
                if !self.error_logged {
                    log::warn!("[INPUT] i2c read failed: {e:?}");
                    self.error_logged = true;
                    return Some(InputEvent::Gesture(GestureEvent::Ghost));
                }
                return None;
            }
        };
        if let Some(event) = self.continuity.update(&sample.points[..sample.len]) {
            return Some(InputEvent::Touch(event));
        }
        let id = gesture?;
        (id != self.gesture_id).then(|| {
            self.gesture_id = id;
            InputEvent::ChipGesture(id)
        })
    }
}

impl<D: I2c> crate::hardware::SelfCheck for Ft6336<D> {
    fn name(&self) -> &'static str {
        "touch"
    }

    fn id(&self) -> Option<crate::hardware::Address> {
        Some(crate::hardware::Address::new(self.addr))
    }

    fn check(&mut self) -> crate::hardware::Status {
        match self.read_gesture_id() {
            Some(_) => crate::hardware::Status::answered(),
            None => crate::hardware::Status::silent(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::mock_i2c::MockI2c;

    const PANEL: (u16, u16) = (320, 240);
    const REGISTERS: usize = 0xA5;

    fn touching(framebuffer: (u16, u16)) -> Ft6336<MockI2c<REGISTERS>> {
        let (raw_x, raw_y) = (PANEL.1 - 1 - framebuffer.1, framebuffer.0);
        let i2c = MockI2c::<REGISTERS>::new(|registers| {
            registers[REG_TD_STATUS as usize] = 1;
            registers[REG_TD_STATUS as usize + 1] = (raw_x >> 8) as u8;
            registers[REG_TD_STATUS as usize + 2] = raw_x as u8;
            registers[REG_TD_STATUS as usize + 3] = (raw_y >> 8) as u8;
            registers[REG_TD_STATUS as usize + 4] = raw_y as u8;
        });
        Ft6336::new(i2c, FT6336_I2C_ADDR, LANDSCAPE_MAP, PANEL.0, PANEL.1)
    }

    fn raw(framebuffer: (u16, u16)) -> (u16, u16) {
        (PANEL.1 - 1 - framebuffer.1, framebuffer.0)
    }

    fn write_point(registers: &mut [u8], slot: usize, framebuffer: (u16, u16), event: u8, id: u8) {
        let (raw_x, raw_y) = raw(framebuffer);
        let base = REG_TD_STATUS as usize + 1 + slot * POINT_BYTES;
        registers[base] = event | (raw_x >> 8) as u8;
        registers[base + 1] = raw_x as u8;
        registers[base + 2] = (id << 4) | (raw_y >> 8) as u8;
        registers[base + 3] = raw_y as u8;
    }

    #[test]
    fn two_contacts_come_out_of_one_burst_with_their_slot_numbers() {
        let i2c = MockI2c::<REGISTERS>::new(|registers| {
            registers[REG_TD_STATUS as usize] = 2;
            write_point(registers, 0, (96, 133), 0, 1);
            write_point(registers, 1, (200, 60), 0, 0);
        });
        let mut touch = Ft6336::new(i2c, FT6336_I2C_ADDR, LANDSCAPE_MAP, PANEL.0, PANEL.1);
        let sample = touch.read_sample().expect("a readable sample");
        assert_eq!(sample.len, 2);
        assert_eq!(
            (sample.points[0], sample.points[1]),
            (RawPoint::new(96, 133, 1), RawPoint::new(200, 60, 0))
        );
    }

    #[test]
    fn a_lifted_slot_is_skipped_not_reported_at_its_last_point() {
        let i2c = MockI2c::<REGISTERS>::new(|registers| {
            registers[REG_TD_STATUS as usize] = 2;
            write_point(registers, 0, (96, 133), 0, 0);
            write_point(registers, 1, (200, 60), 0x40, 1);
        });
        let mut touch = Ft6336::new(i2c, FT6336_I2C_ADDR, LANDSCAPE_MAP, PANEL.0, PANEL.1);
        let sample = touch.read_sample().expect("a readable sample");
        assert_eq!((sample.points[0].x, sample.len), (96, 1));
    }

    #[test]
    fn a_finger_lands_where_the_panel_shows_it() {
        let mut touch = touching((96, 133));
        let sample = touch.read_sample().expect("a readable sample");
        assert_eq!((sample.points[0].x, sample.len), (96, 1));
    }

    #[test]
    fn a_status_beyond_the_two_slots_is_rejected_rather_than_read_short() {
        let mut touch = touching((96, 133));
        touch.i2c.registers[REG_TD_STATUS as usize] = 3;
        let sample = touch.read_sample().expect("a readable sample");
        assert_eq!(sample.len, 0, "a three-contact frame is not trusted");
    }

    #[test]
    fn a_gesture_change_does_not_swallow_the_sample() {
        let mut touch = touching((96, 133));
        touch.i2c.registers[REG_GESTURE_ID as usize] = 0x10;
        assert!(
            matches!(touch.poll(0), Some(InputEvent::Touch(_))),
            "the first sample of a chip gesture is still sample news"
        );
        touch.i2c.registers[REG_GESTURE_ID as usize] = 0x14;
        assert_eq!(
            touch.poll(20),
            Some(InputEvent::ChipGesture(0x14)),
            "the id is still reported once no contact moved"
        );
    }

    #[test]
    fn the_panels_own_frame_is_transposed_and_mirrored() {
        for (raw, framebuffer) in [
            ((0, 0), (0, 239)),
            ((239, 0), (0, 0)),
            ((0, 319), (319, 239)),
            ((239, 319), (319, 0)),
        ] {
            assert_eq!(
                LANDSCAPE_MAP.map(raw, PANEL.0, PANEL.1),
                framebuffer,
                "raw {raw:?} does not come out at {framebuffer:?}"
            );
        }
    }

    #[test]
    fn a_self_check_names_the_part_and_the_address_it_answers_on() {
        use crate::hardware::{Address, SelfCheck};

        let touch = touching((96, 133));
        assert_eq!(iot_core::drivers::input::InputSource::name(&touch), "touch");
        assert_eq!(touch.id(), Some(Address::new(FT6336_I2C_ADDR)));
    }

    #[test]
    fn a_self_check_reads_one_register_and_calls_silence_absent() {
        use crate::components::mock_i2c::Unresponsive;
        use crate::hardware::{SelfCheck, Status};

        let mut touch = touching((96, 133));
        assert_eq!(touch.check(), Status::answered());

        let mut absent = Ft6336::new(
            Unresponsive,
            FT6336_I2C_ADDR,
            LANDSCAPE_MAP,
            PANEL.0,
            PANEL.1,
        );
        assert_eq!(absent.check(), Status::silent());
    }
}
