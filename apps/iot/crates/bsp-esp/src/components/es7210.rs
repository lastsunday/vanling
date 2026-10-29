use embedded_hal::i2c::I2c;

use super::register_seq::{self, Step};
use register_seq::{merge, write};

/// Datasheet 2.2: the seven-bit write address.
pub const ES7210_I2C_ADDR: u8 = 0x41;

const REG_RESET: u8 = 0x00;
const REG_CLOCK_OFF: u8 = 0x01;
const REG_MAINCLK: u8 = 0x02;
const REG_LRCK_DIVH: u8 = 0x04;
const REG_LRCK_DIVL: u8 = 0x05;
const REG_POWER_DOWN: u8 = 0x06;
const REG_OSR: u8 = 0x07;
const REG_MODE_CONFIG: u8 = 0x08;
const REG_TIME_CONTROL0: u8 = 0x09;
const REG_TIME_CONTROL1: u8 = 0x0A;
const REG_SDP_INTERFACE1: u8 = 0x11;
const REG_SDP_INTERFACE2: u8 = 0x12;
const REG_ADC34_FILTER_CLEAR: u8 = 0x20;
const REG_ADC34_FILTER_SET: u8 = 0x21;
const REG_ADC12_FILTER_SET: u8 = 0x22;
const REG_ADC12_FILTER_CLEAR: u8 = 0x23;
const REG_ANALOG_POWER: u8 = 0x40;
const REG_MIC12_BIAS: u8 = 0x41;
const REG_MIC34_BIAS: u8 = 0x42;
const REG_MIC1_GAIN: u8 = 0x43;
const REG_MIC2_GAIN: u8 = 0x44;
const REG_MIC3_GAIN: u8 = 0x45;
const REG_MIC4_GAIN: u8 = 0x46;
const REG_MIC1_POWER: u8 = 0x47;
const REG_MIC2_POWER: u8 = 0x48;
const REG_MIC3_POWER: u8 = 0x49;
const REG_MIC4_POWER: u8 = 0x4A;
const REG_MIC12_POWER: u8 = 0x4B;
const REG_MIC34_POWER: u8 = 0x4C;

const RESET_ALL: u8 = 0xFF;
const RESET_IDLE: u8 = 0x41;
const RESET_RUN: u8 = 0x71;

const CLOCK_OFF_BRINGUP: u8 = 0x3F;
const CLOCK_OFF_MIC12_MASK: u8 = 0x0B;
const POWER_DOWN_RUN: u8 = 0x00;

const MODE_SLAVE_BIT: u8 = 0x01;

/// The clock this part is configured from the table's row for, and the rate that row
/// produces.
pub use super::audio_clock::{MCLK_HZ, SAMPLE_RATE_HZ};

// The 12288000/48000 row of the datasheet clock table: ADC divide 1, the clock
// doubler on, the DLL on, OSR 0x20, and an LRCK divide of 256 (0x01:0x00) —
// 12.288 MHz over 256 is exactly 48 kHz.
const MAINCLK_48K: u8 = 0xC1;
const OSR_48K: u8 = 0x20;
const LRCK_DIVH_48K: u8 = 0x01;
const LRCK_DIVL_48K: u8 = 0x00;
const _: () = assert!(MCLK_HZ / SAMPLE_RATE_HZ == 256);

const FILTER_CLEAR: u8 = 0x0A;
const FILTER_SET: u8 = 0x2A;

const FILTER_CORNER_MASK: u8 = 0x07;

/// How many corner codes a filter register's corner field can take.
pub const HPF_CORNER_CODES: u8 = 8;

/// The corner the bring-up leaves configured.
pub const HPF_CORNER: u8 = FILTER_CLEAR & FILTER_CORNER_MASK;

const _: () = assert!(FILTER_SET ^ FILTER_CLEAR == 0x20);
const _: () = assert!(FILTER_SET & FILTER_CORNER_MASK == FILTER_CLEAR & FILTER_CORNER_MASK);

const ANALOG_POWER_RUN: u8 = 0x43;
const MIC_BIAS: u8 = 0x70;

// Datasheet 4.8: bit4 enables a microphone's PGA and bits 0-3 hold its gain in
// 3 dB steps. Index 10 is the 30 dB step.
const GAIN_ENABLE: u8 = 0x10;
const GAIN_FIELD: u8 = 0x0F;
const GAIN_30DB: u8 = 0x0A;
const MIC_POWER: u8 = 0x08;

/// How far this microphone's full scale sits above 0 dB SPL: 0 dBFS is 102 dB SPL.
///
/// From the two datasheets. ES7210: full scale is AVDD/3.3 Vrms, so at
/// `ANALOG_POWER_RUN` it is 1.0 Vrms, not the 2 Vrms "headroom" would suggest.
/// ZTS6216: −38 dBV/Pa is 12.59 mV/Pa, which through `GAIN_30DB` is 397.8 mV/Pa at
/// the converter — so full scale is 2.51 Pa, or 101.98 dB against the 20 µPa
/// reference.
///
/// Anchored against speech at 30 cm on 2026-09-28: a 66 dB SPL floor, peaks to 80.
/// To re-anchor when the capsule or a gain step changes, set `CAL_SPL_LOG` in the
/// capture driver and speak at the same distance.
pub const SPL_OFFSET_DECIBELS: i16 = 102;

const SDP_FRAME_MASK: u8 = 0x03;
const SDP_I2S: u8 = 0x00;
// Datasheet 3.3: bits 6-5 hold the word width; 0b01 is 16 bit. Bit 7 is written
// alongside them because the width write also clears it. The field is spelled
// out rather than reusing the vendor's `&= 0x1f`, which clears the frame bits too
// and only looks harmless because the frame write before it wrote zero.
const SDP_WIDTH_MASK: u8 = 0xE0;
const SDP_16BIT: u8 = 0x60;
const USE_MIC3_REFERENCE: bool = false;

const SDP2_TDMIN_PDN_ON: u8 = 0x00;

const SDP_DSP: u8 = 0x03;

const LRCK_RATE_MASK: u8 = 0xF0;
const LRCK_4_CHANNELS: u8 = 0x20;

const MIC1_SELECT: &[Step] = &[
    merge(REG_MIC1_GAIN, GAIN_ENABLE, 0x00),
    merge(REG_MIC2_GAIN, GAIN_ENABLE, 0x00),
    merge(REG_MIC3_GAIN, GAIN_ENABLE, 0x00),
    merge(REG_MIC4_GAIN, GAIN_ENABLE, 0x00),
    write(REG_MIC12_POWER, 0xFF),
    write(REG_MIC34_POWER, 0xFF),
    merge(REG_CLOCK_OFF, CLOCK_OFF_MIC12_MASK, 0x00),
    write(REG_MIC12_POWER, 0x00),
    merge(REG_MIC1_GAIN, GAIN_ENABLE, GAIN_ENABLE),
    merge(REG_MIC1_GAIN, GAIN_FIELD, GAIN_30DB),
    write(REG_SDP_INTERFACE2, SDP2_TDMIN_PDN_ON),
];

const BRING_UP_RESET: &[Step] = &[
    write(REG_RESET, RESET_ALL),
    write(REG_RESET, RESET_IDLE),
    write(REG_CLOCK_OFF, CLOCK_OFF_BRINGUP),
    write(REG_TIME_CONTROL0, 0x30),
    write(REG_TIME_CONTROL1, 0x30),
    write(REG_ADC12_FILTER_CLEAR, FILTER_CLEAR),
    write(REG_ADC12_FILTER_SET, FILTER_SET),
    write(REG_ADC34_FILTER_CLEAR, FILTER_CLEAR),
    write(REG_ADC34_FILTER_SET, FILTER_SET),
];

const BRING_UP_CLOCKS: &[Step] = &[
    merge(REG_MODE_CONFIG, MODE_SLAVE_BIT, 0x00),
    write(REG_ANALOG_POWER, ANALOG_POWER_RUN),
    write(REG_MIC12_BIAS, MIC_BIAS),
    write(REG_MIC34_BIAS, MIC_BIAS),
    write(REG_OSR, OSR_48K),
    write(REG_MAINCLK, MAINCLK_48K),
    write(REG_LRCK_DIVH, LRCK_DIVH_48K),
    write(REG_LRCK_DIVL, LRCK_DIVL_48K),
];

const BRING_UP_SERIAL: &[Step] = &[
    merge(REG_SDP_INTERFACE1, SDP_FRAME_MASK, SDP_I2S),
    merge(REG_SDP_INTERFACE1, SDP_WIDTH_MASK, SDP_16BIT),
];

const MIC1_AND_MIC3: &[Step] = &[
    merge(REG_MIC1_GAIN, GAIN_ENABLE, GAIN_ENABLE),
    merge(REG_MIC1_GAIN, GAIN_FIELD, GAIN_30DB),
    merge(REG_MIC3_GAIN, GAIN_ENABLE, GAIN_ENABLE),
    merge(REG_MIC3_GAIN, GAIN_FIELD, GAIN_30DB),
    merge(REG_MODE_CONFIG, LRCK_RATE_MASK, LRCK_4_CHANNELS),
    merge(REG_SDP_INTERFACE1, SDP_FRAME_MASK, SDP_DSP),
    write(REG_SDP_INTERFACE2, SDP2_TDMIN_PDN_ON),
];

const SELECT: &[Step] = if USE_MIC3_REFERENCE {
    MIC1_AND_MIC3
} else {
    MIC1_SELECT
};

const BRING_UP: &[&[Step]] = &[BRING_UP_RESET, BRING_UP_CLOCKS, BRING_UP_SERIAL, SELECT];

const START: &[&[Step]] = &[
    &[
        write(REG_POWER_DOWN, POWER_DOWN_RUN),
        write(REG_ANALOG_POWER, ANALOG_POWER_RUN),
        write(REG_MIC1_POWER, MIC_POWER),
        write(REG_MIC2_POWER, MIC_POWER),
        write(REG_MIC3_POWER, MIC_POWER),
        write(REG_MIC4_POWER, MIC_POWER),
    ],
    SELECT,
    &[
        write(REG_ANALOG_POWER, ANALOG_POWER_RUN),
        write(REG_RESET, RESET_RUN),
        write(REG_RESET, RESET_IDLE),
    ],
];

#[derive(Debug, PartialEq, Eq)]
pub enum Es7210Error<E> {
    Bus(E),
}

impl<E> From<E> for Es7210Error<E> {
    fn from(error: E) -> Self {
        Self::Bus(error)
    }
}

/// What a capture needs from a filter's driver to walk its corner, and nothing more.
pub trait HpfCorner {
    type CornerError;

    fn set_hpf_corner(&mut self, corner: u8) -> Result<(), Self::CornerError>;
}

impl<D: I2c> HpfCorner for Es7210<D> {
    type CornerError = Es7210Error<D::Error>;

    fn set_hpf_corner(&mut self, corner: u8) -> Result<(), Es7210Error<D::Error>> {
        Es7210::write_hpf_corner(self, corner)
    }
}

pub struct Es7210<D> {
    i2c: D,
    addr: u8,
    running: bool,
}

impl<D: I2c> Es7210<D> {
    pub const fn new(i2c: D, addr: u8) -> Self {
        Self {
            i2c,
            addr,
            running: false,
        }
    }

    /// Brings the part up and starts it capturing from MIC1.
    pub fn init(&mut self) -> Result<(), Es7210Error<D::Error>> {
        if self.running {
            return Ok(());
        }
        register_seq::run(&mut self.i2c, self.addr, BRING_UP)?;
        let mode = register_seq::read_reg(&mut self.i2c, self.addr, REG_MODE_CONFIG)?;
        let serial = register_seq::read_reg(&mut self.i2c, self.addr, REG_SDP_INTERFACE1)?;
        let interface = register_seq::read_reg(&mut self.i2c, self.addr, REG_SDP_INTERFACE2)?;
        let width_ok = serial & SDP_WIDTH_MASK == SDP_16BIT;
        let frame = match serial & SDP_FRAME_MASK {
            SDP_I2S => "I2S",
            SDP_DSP => "DSP",
            _ => "unknown framing",
        };
        let frame_ok = frame != "unknown framing";
        log::log!(
            if width_ok && frame_ok {
                log::Level::Info
            } else {
                log::Level::Warn
            },
            "[AUDIO] ES7210 serial: {} word, {frame}, LRCK rate {}, interface 0x{interface:02X}",
            if width_ok { "16-bit" } else { "NOT 16-bit" },
            (mode & LRCK_RATE_MASK) >> 4,
        );
        self.start()?;
        self.running = true;
        Ok(())
    }

    /// Separate from `init` so a part that lost its clocks can be restarted without a full
    /// re-init.
    pub fn start(&mut self) -> Result<(), Es7210Error<D::Error>> {
        let clock_off = register_seq::read_reg(&mut self.i2c, self.addr, REG_CLOCK_OFF)?;
        self.i2c
            .write(self.addr, &[REG_CLOCK_OFF, clock_off])
            .map_err(Es7210Error::Bus)?;
        Ok(register_seq::run(&mut self.i2c, self.addr, START)?)
    }

    /// Moves every input pair's DC-blocking corner to `corner`, leaving every other bit of
    /// the register exactly as the part holds it.
    pub fn write_hpf_corner(&mut self, corner: u8) -> Result<(), Es7210Error<D::Error>> {
        for reg in [
            REG_ADC12_FILTER_CLEAR,
            REG_ADC12_FILTER_SET,
            REG_ADC34_FILTER_CLEAR,
            REG_ADC34_FILTER_SET,
        ] {
            let current = register_seq::read_reg(&mut self.i2c, self.addr, reg)?;
            let value = (current & !FILTER_CORNER_MASK) | (corner & FILTER_CORNER_MASK);
            self.i2c.write(self.addr, &[reg, value])?;
        }
        Ok(())
    }
}

impl<D: I2c> crate::hardware::Named for Es7210<D> {
    const NAME: &'static str = "mic";
}

impl<D: I2c> crate::hardware::SelfCheck for Es7210<D> {
    fn name(&self) -> &'static str {
        "mic"
    }

    fn id(&self) -> Option<crate::hardware::Address> {
        Some(crate::hardware::Address::new(self.addr))
    }

    fn check(&mut self) -> crate::hardware::Status {
        use crate::hardware::Status;
        match register_seq::read_reg(&mut self.i2c, self.addr, REG_MODE_CONFIG) {
            Err(_) => Status::silent(),
            Ok(_) if self.running => Status::answered(),
            Ok(_) => Status::refused("not running"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iot_core::drivers::audio::SCOPE_FLOOR_DECIBELS;

    use crate::components::mock_i2c::{MockI2c, NACK};

    type Mock = MockI2c<0x4D>;

    fn mock() -> Mock {
        Mock::new(|registers| {
            registers[REG_SDP_INTERFACE1 as usize] = 0x83;
            registers[REG_SDP_INTERFACE2 as usize] = 0x02;
            registers[REG_MODE_CONFIG as usize] = 0x01;
            registers[REG_MIC1_GAIN as usize] = GAIN_ENABLE;
            registers[REG_CLOCK_OFF as usize] = 0xBF;
        })
    }

    fn settle() -> Es7210<Mock> {
        let mut codec = Es7210::new(mock(), ES7210_I2C_ADDR);
        codec.init().expect("init");
        codec
    }

    #[test]
    fn bring_up_resets_the_part_before_anything_else() {
        let codec = settle();
        assert_eq!(codec.i2c.trace[0], (REG_RESET, RESET_ALL));
        assert_eq!(codec.i2c.trace[1], (REG_RESET, RESET_IDLE));
    }

    #[test]
    fn the_part_runs_as_a_slave_of_the_host_clocks() {
        let codec = settle();
        assert_eq!(
            codec.i2c.registers[REG_MODE_CONFIG as usize] & MODE_SLAVE_BIT,
            0,
            "the host owns MCLK, BCLK and LRCK"
        );
    }

    #[test]
    fn the_clock_table_row_for_48k_over_12288k_is_the_one_written() {
        let codec = settle();
        assert_eq!(codec.i2c.registers[REG_MAINCLK as usize], MAINCLK_48K);
        assert_eq!(codec.i2c.registers[REG_OSR as usize], OSR_48K);
        // 0x01:0x00 is a divide of 256, and 12.288 MHz over 256 is 48 kHz.
        let divider = u32::from(codec.i2c.registers[REG_LRCK_DIVH as usize]) << 8
            | u32::from(codec.i2c.registers[REG_LRCK_DIVL as usize]);
        assert_eq!(divider, 256);
        assert_eq!(MCLK_HZ / divider, 48_000);
    }

    #[test]
    fn the_serial_port_ends_up_in_standard_i2s_at_16_bits() {
        // **Standard framing, and this is the one thing standing between this
        // board and a hardware echo reference.** MIC3 is a TDM slot, so reading
        // it needs TDM — but flipping `REG_SDP_INTERFACE2`'s TDM bit on its own
        // stops the part driving the data line, and the capture goes silent.
        // `[实测]` The reference therefore stays dark until TDM can be enabled
        // properly; see [`USE_MIC3_REFERENCE`].
        let codec = settle();
        let sdp = codec.i2c.registers[REG_SDP_INTERFACE1 as usize];
        assert_eq!(sdp & SDP_FRAME_MASK, SDP_I2S, "standard I2S framing");
        assert_eq!(sdp & SDP_WIDTH_MASK, SDP_16BIT, "16-bit words");
        assert_eq!(
            codec.i2c.registers[REG_SDP_INTERFACE2 as usize] & 0x02,
            0,
            "not TDM — enabling it alone silences the capture"
        );
    }

    #[test]
    fn only_mic1_is_enabled_at_30db() {
        let codec = settle();
        assert_eq!(
            codec.i2c.registers[REG_MIC1_GAIN as usize],
            GAIN_ENABLE | GAIN_30DB
        );
        for reg in [REG_MIC2_GAIN, REG_MIC3_GAIN, REG_MIC4_GAIN] {
            assert_eq!(
                codec.i2c.registers[reg as usize] & GAIN_ENABLE,
                0,
                "0x{reg:02X} must not hold a live PGA"
            );
        }
    }

    #[test]
    fn both_input_filter_pairs_carry_the_reference_values() {
        let codec = settle();
        for (set, clear) in [
            (REG_ADC12_FILTER_SET, REG_ADC12_FILTER_CLEAR),
            (REG_ADC34_FILTER_SET, REG_ADC34_FILTER_CLEAR),
        ] {
            assert_eq!(
                codec.i2c.registers[set as usize], FILTER_SET,
                "0x{set:02X} is the bit5-set register"
            );
            assert_eq!(
                codec.i2c.registers[clear as usize], FILTER_CLEAR,
                "0x{clear:02X} is the bit5-clear register"
            );
        }
    }

    #[test]
    fn the_bring_up_leaves_the_documented_corner() {
        let codec = settle();
        for reg in [
            REG_ADC12_FILTER_SET,
            REG_ADC12_FILTER_CLEAR,
            REG_ADC34_FILTER_SET,
            REG_ADC34_FILTER_CLEAR,
        ] {
            assert_eq!(
                codec.i2c.registers[reg as usize] & FILTER_CORNER_MASK,
                HPF_CORNER,
                "0x{reg:02X} must start on the corner a walk returns to"
            );
        }
    }

    #[test]
    fn setting_a_corner_leaves_bit5_and_the_rest_of_the_register_alone() {
        let mut codec = settle();
        const SEED: u8 = 0xEA;
        const UNTOUCHED: u8 = SEED & !FILTER_CORNER_MASK;
        for reg in [
            REG_ADC12_FILTER_SET,
            REG_ADC12_FILTER_CLEAR,
            REG_ADC34_FILTER_SET,
            REG_ADC34_FILTER_CLEAR,
        ] {
            codec.i2c.registers[reg as usize] = SEED;
        }
        codec.set_hpf_corner(0x05).expect("corner");
        for reg in [
            REG_ADC12_FILTER_SET,
            REG_ADC12_FILTER_CLEAR,
            REG_ADC34_FILTER_SET,
            REG_ADC34_FILTER_CLEAR,
        ] {
            let value = codec.i2c.registers[reg as usize];
            assert_eq!(value & FILTER_CORNER_MASK, 0x05, "0x{reg:02X} corner moved");
            assert_eq!(
                value & !FILTER_CORNER_MASK,
                UNTOUCHED,
                "0x{reg:02X} kept its bits"
            );
        }
    }

    #[test]
    fn a_corner_walk_returns_to_where_it_started() {
        let mut codec = settle();
        let before = [
            REG_ADC12_FILTER_SET,
            REG_ADC12_FILTER_CLEAR,
            REG_ADC34_FILTER_SET,
            REG_ADC34_FILTER_CLEAR,
        ]
        .map(|reg| codec.i2c.registers[reg as usize]);
        for corner in 0..HPF_CORNER_CODES {
            codec.set_hpf_corner(corner).expect("corner");
            for reg in [
                REG_ADC12_FILTER_SET,
                REG_ADC12_FILTER_CLEAR,
                REG_ADC34_FILTER_SET,
                REG_ADC34_FILTER_CLEAR,
            ] {
                assert_eq!(
                    codec.i2c.registers[reg as usize] & FILTER_CORNER_MASK,
                    corner,
                    "corner {corner} did not reach 0x{reg:02X}"
                );
            }
        }
        codec.set_hpf_corner(HPF_CORNER).expect("restore");
        for (reg, value) in [
            REG_ADC12_FILTER_SET,
            REG_ADC12_FILTER_CLEAR,
            REG_ADC34_FILTER_SET,
            REG_ADC34_FILTER_CLEAR,
        ]
        .into_iter()
        .zip(before)
        {
            assert_eq!(codec.i2c.registers[reg as usize], value, "0x{reg:02X}");
        }
    }

    #[test]
    fn a_corner_write_reports_a_bus_failure_at_every_register() {
        for fail_at in 0..4 {
            let mut i2c = mock();
            i2c.fail_at = Some(fail_at);
            let mut codec = Es7210::new(i2c, ES7210_I2C_ADDR);
            assert_eq!(
                codec.set_hpf_corner(0x03),
                Err(Es7210Error::Bus(NACK)),
                "write {fail_at} of the walk is where the bus gave up"
            );
        }
    }

    #[test]
    fn the_spl_offset_lands_the_window_where_a_room_is() {
        let window_floor = i32::from(SPL_OFFSET_DECIBELS) - SCOPE_FLOOR_DECIBELS;
        assert!(
            window_floor < 30,
            "the window floor reads {window_floor} dB SPL, above the quiet room it must show"
        );
        assert!(
            SPL_OFFSET_DECIBELS <= 130,
            "full scale reads {SPL_OFFSET_DECIBELS} dB SPL, above a shout"
        );
    }

    #[test]
    fn a_masked_write_never_clobbers_a_neighbouring_field() {
        let mut i2c = mock();
        i2c.registers[REG_MIC1_GAIN as usize] = 0x80;
        let mut codec = Es7210::new(i2c, ES7210_I2C_ADDR);
        codec.init().expect("init");
        assert_eq!(
            codec.i2c.registers[REG_MIC1_GAIN as usize],
            0x80 | GAIN_ENABLE | GAIN_30DB
        );
    }

    #[test]
    fn start_hands_the_clock_register_back_the_value_it_read() {
        let mut codec = Es7210::new(mock(), ES7210_I2C_ADDR);
        codec.init().expect("init");
        let held = codec.i2c.registers[REG_CLOCK_OFF as usize];
        let before = codec.i2c.trace.len();
        codec.start().expect("start");
        assert_eq!(codec.i2c.trace[before], (REG_CLOCK_OFF, held));
    }

    #[test]
    fn the_microphone_selection_is_the_same_whether_it_is_the_first_or_a_later_start() {
        let mut first = Es7210::new(mock(), ES7210_I2C_ADDR);
        first.init().expect("init");
        let once = first.i2c.registers;

        let mut second = Es7210::new(mock(), ES7210_I2C_ADDR);
        second.init().expect("init");
        second.start().expect("start");
        assert_eq!(
            second.i2c.registers, once,
            "a restart is idempotent, so a wedged part can recover without a re-init"
        );
    }

    #[test]
    fn start_releases_the_power_down_latch_and_the_microphone_rail() {
        let codec = settle();
        assert_eq!(codec.i2c.registers[REG_POWER_DOWN as usize], POWER_DOWN_RUN);
        assert_eq!(
            codec.i2c.registers[REG_ANALOG_POWER as usize],
            ANALOG_POWER_RUN
        );
        assert_eq!(codec.i2c.registers[REG_MIC1_POWER as usize], MIC_POWER);
    }

    #[test]
    fn init_is_idempotent() {
        let mut codec = Es7210::new(mock(), ES7210_I2C_ADDR);
        codec.init().expect("init");
        let after_init = codec.i2c.trace.len();
        codec.init().expect("second init");
        assert_eq!(
            codec.i2c.trace.len(),
            after_init,
            "an already-running part is not re-initialised"
        );
    }

    #[test]
    fn a_bus_failure_during_bring_up_is_reported_and_leaves_the_part_not_running() {
        for fail_at in 0..12 {
            let mut i2c = mock();
            i2c.fail_at = Some(fail_at);
            let mut codec = Es7210::new(i2c, ES7210_I2C_ADDR);
            assert_eq!(
                codec.init(),
                Err(Es7210Error::Bus(NACK)),
                "write {fail_at} of the sequence is where the bus gave up"
            );
            assert!(
                !codec.running,
                "a part that never finished bring-up must not claim to be running"
            );
        }
    }

    #[test]
    fn the_sequence_writes_the_clock_register_before_the_microphone_rail() {
        let codec = settle();
        let gated = codec.i2c.registers[REG_CLOCK_OFF as usize] & !CLOCK_OFF_MIC12_MASK;
        assert_eq!(
            codec.i2c.writes_of(REG_CLOCK_OFF).first().copied(),
            Some(CLOCK_OFF_BRINGUP),
            "the rail's gate is set before anything un-gates it"
        );
        assert!(
            codec
                .i2c
                .writes_of(REG_CLOCK_OFF)
                .iter()
                .all(|w| *w == CLOCK_OFF_BRINGUP || *w == gated),
            "bring-up then the selection's masked clear, then the start's read-back: {:?}",
            codec.i2c.writes_of(REG_CLOCK_OFF)
        );
    }

    #[test]
    fn a_self_check_names_the_part_and_the_address_it_answers_on() {
        use crate::hardware::{Address, SelfCheck};

        let codec = Es7210::new(mock(), ES7210_I2C_ADDR);
        assert_eq!(codec.name(), "mic");
        assert_eq!(codec.id(), Some(Address::new(ES7210_I2C_ADDR)));
    }

    #[test]
    fn a_self_check_reports_a_part_it_cannot_configure_and_a_bus_nobody_answers_on() {
        use crate::components::mock_i2c::Unresponsive;
        use crate::hardware::{SelfCheck, Status};

        assert_eq!(
            Es7210::new(mock(), ES7210_I2C_ADDR).check(),
            Status::refused("not running")
        );
        assert_eq!(settle().check(), Status::answered());
        assert_eq!(
            Es7210::new(Unresponsive, ES7210_I2C_ADDR).check(),
            Status::silent()
        );
    }
}
