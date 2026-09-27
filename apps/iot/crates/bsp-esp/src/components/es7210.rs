use embedded_hal::i2c::I2c;

/// Datasheet 2.2: the seven-bit write address. The part has no second address
/// line, so this is the only address it answers on.
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
const REG_ADC34_HPF2: u8 = 0x20;
const REG_ADC34_HPF1: u8 = 0x21;
const REG_ADC12_HPF1: u8 = 0x22;
const REG_ADC12_HPF2: u8 = 0x23;
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
/// Datasheet 4.9: bit3 is the analog power-down latch and bits 0-1 are the
/// per-pair clock gates. Clearing all three releases the MIC1/MIC2 path.
const CLOCK_OFF_MIC12_MASK: u8 = 0x0B;
const POWER_DOWN_RUN: u8 = 0x00;

/// Datasheet 5.1 bit0: 0 is slave. The host drives MCLK, BCLK and LRCK, so the
/// part must not be allowed to generate or divide them.
const MODE_SLAVE_BIT: u8 = 0x01;

/// The clock the host drives MCLK at. The part is configured from the clock
/// table's row for this rate, so the board that wires the clocks and the codec
/// that programs them have to agree on it.
pub const MCLK_HZ: u32 = 12_288_000;
/// The rate this driver programs. Fixed rather than a parameter: the clock table
/// row, the LRCK divider and the host's I2S configuration all have to move
/// together, and a per-instance rate would let them drift apart silently.
pub const SAMPLE_RATE_HZ: u32 = 48_000;

/// The 12288000/48000 row of the datasheet clock table: ADC divide 1, the clock
/// doubler on, the DLL on, OSR 0x20, and an LRCK divide of 256 (0x01:0x00) —
/// 12.288 MHz over 256 is exactly 48 kHz.
const MAINCLK_48K: u8 = 0xC1;
const OSR_48K: u8 = 0x20;
const LRCK_DIVH_48K: u8 = 0x01;
const LRCK_DIVL_48K: u8 = 0x00;
/// Pins the divider to the rate it is documented to produce, so retuning
/// `SAMPLE_RATE_HZ` without retuning the register bytes fails here instead of
/// landing on the host as a part running at the wrong rate.
const _: () = assert!(MCLK_HZ / SAMPLE_RATE_HZ == 256);

/// Datasheet 4.6: the two DC-blocking filters' cutoffs. The ADC12 pair gets the
/// voice-band corner this microphone path is tuned for; the ADC34 pair is
/// configured symmetrically even though the board reads MIC1 alone, because a
/// half-configured filter is not a documented state.
const ADC12_HPF2: u8 = 0x2A;
const ADC12_HPF1: u8 = 0x0A;
const ADC34_HPF2: u8 = 0x0A;
const ADC34_HPF1: u8 = 0x2A;

/// Datasheet 4.5: analog power with the internal 5 kOhm VMID, vdda 3.3 V and the
/// reference buffer on.
const ANALOG_POWER_RUN: u8 = 0x43;
/// Datasheet 4.5: the microphone bias rails, set for 2.87 V so an electret
/// capsule biased high still has headroom on a 3.3 V board.
const MIC_BIAS: u8 = 0x70;

/// Datasheet 4.8: bit4 enables a microphone's PGA and bits 0-3 hold its gain in
/// 3 dB steps. Index 10 is the 30 dB step.
const GAIN_ENABLE: u8 = 0x10;
const GAIN_FIELD: u8 = 0x0F;
const GAIN_30DB: u8 = 0x0A;
const MIC_POWER: u8 = 0x08;

/// Datasheet 3.3: bits 1-0 of the serial-port register select the frame; 0b00 is
/// standard I2S, which is what the host peripheral emits in Philips mode.
const SDP_FRAME_MASK: u8 = 0x03;
const SDP_I2S: u8 = 0x00;
/// Datasheet 3.3: bits 6-5 hold the word width; 0b01 is 16 bit. Bit 7 is written
/// alongside them because the width write also clears it. The field is spelled
/// out rather than reusing the vendor's `&= 0x1f`, which clears the frame bits too
/// and only looks harmless because the frame write before it wrote zero.
const SDP_WIDTH_MASK: u8 = 0xE0;
const SDP_16BIT: u8 = 0x60;
/// Datasheet 3.3: bit1 selects TDM. One microphone in standard I2S leaves it
/// clear, and it is written explicitly because the bit is only correct as a
/// consequence of the microphone count, not as a power-on value.
const SDP2_NO_TDM: u8 = 0x00;

/// One entry in a bring-up sequence: write `value` across `reg`, or merge it into
/// just the bits `mask` selects when a mask is given. A masked write is how the
/// shared registers stay intact — the serial-port register carries the frame, the
/// word width and the serial enable at once, and the gain register carries the
/// PGA enable beside its gain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Step {
    reg: u8,
    mask: u8,
    value: u8,
}

const fn write(reg: u8, value: u8) -> Step {
    Step {
        reg,
        mask: 0,
        value,
    }
}

const fn merge(reg: u8, mask: u8, value: u8) -> Step {
    Step { reg, mask, value }
}

/// Selects MIC1 alone: clears every PGA enable first so no unused channel can
/// hold one, gates both bias/ADC/PGA pairs off, then releases only the MIC1/MIC2
/// path and enables MIC1's PGA at 30 dB. Written once and referenced twice, since
/// a divergence between two copies would configure the part by start count.
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
    write(REG_SDP_INTERFACE2, SDP2_NO_TDM),
];

/// Reset, then the state-cycle timers and the two DC-blocking filters. The reset
/// is first because every later write is meaningless against a part still coming
/// out of it.
const BRING_UP_RESET: &[Step] = &[
    write(REG_RESET, RESET_ALL),
    write(REG_RESET, RESET_IDLE),
    write(REG_CLOCK_OFF, CLOCK_OFF_BRINGUP),
    write(REG_TIME_CONTROL0, 0x30),
    write(REG_TIME_CONTROL1, 0x30),
    write(REG_ADC12_HPF2, ADC12_HPF2),
    write(REG_ADC12_HPF1, ADC12_HPF1),
    write(REG_ADC34_HPF2, ADC34_HPF2),
    write(REG_ADC34_HPF1, ADC34_HPF1),
];

/// Slave mode, the analog rail, the microphone bias, and the 48 kHz clock table
/// row. Slave mode is set before the microphone selection so the part is not
/// driving a clock the host is also driving.
const BRING_UP_CLOCKS: &[Step] = &[
    merge(REG_MODE_CONFIG, MODE_SLAVE_BIT, 0x00),
    write(REG_ANALOG_POWER, ANALOG_POWER_RUN),
    write(REG_MIC12_BIAS, MIC_BIAS),
    write(REG_MIC34_BIAS, MIC_BIAS),
    write(REG_OSR, OSR_48K),
    write(REG_MAINCLK, MAINCLK_48K),
    // The vendor's slave-mode path leaves the LRCK divider at its power-on value,
    // which the datasheet does not document; the clock table's 12288000/48000 row
    // does, so the rate follows from a written divider rather than a reset value
    // that cannot be read back.
    write(REG_LRCK_DIVH, LRCK_DIVH_48K),
    write(REG_LRCK_DIVL, LRCK_DIVL_48K),
];

/// The serial port, before the microphone selection, because the start sequence
/// rewrites the clock registers and the frame is independent of the clocks.
const BRING_UP_SERIAL: &[Step] = &[
    merge(REG_SDP_INTERFACE1, SDP_FRAME_MASK, SDP_I2S),
    merge(REG_SDP_INTERFACE1, SDP_WIDTH_MASK, SDP_16BIT),
];

const BRING_UP: &[&[Step]] = &[
    BRING_UP_RESET,
    BRING_UP_CLOCKS,
    BRING_UP_SERIAL,
    MIC1_SELECT,
];

/// Releases the part into capture. The clock register is written by `start`
/// itself from the byte it read back, ahead of these steps, because the vendor
/// driver round-trips it: bit7 records that the external clock is present, and
/// writing a literal would clobber that readback.
const START: &[&[Step]] = &[
    &[
        write(REG_POWER_DOWN, POWER_DOWN_RUN),
        write(REG_ANALOG_POWER, ANALOG_POWER_RUN),
        write(REG_MIC1_POWER, MIC_POWER),
        write(REG_MIC2_POWER, MIC_POWER),
        write(REG_MIC3_POWER, MIC_POWER),
        write(REG_MIC4_POWER, MIC_POWER),
    ],
    MIC1_SELECT,
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

    /// Brings the part up and starts it capturing from MIC1. There is no identity
    /// check to make — the register map exposes no ID register, so a bus error is
    /// the only failure this can report, which is why the tests check the sequence
    /// as a whole rather than trusting each write.
    pub fn init(&mut self) -> Result<(), Es7210Error<D::Error>> {
        if self.running {
            return Ok(());
        }
        self.run(BRING_UP)?;
        self.start()?;
        self.running = true;
        Ok(())
    }

    /// Releases the part into capture, reading the clock register first so the
    /// start hands back the byte the part actually holds. Separate from `init` so
    /// a part that lost its clocks can be restarted without a full re-init.
    pub fn start(&mut self) -> Result<(), Es7210Error<D::Error>> {
        let clock_off = self.read_reg(REG_CLOCK_OFF)?;
        self.i2c
            .write(self.addr, &[REG_CLOCK_OFF, clock_off])
            .map_err(Es7210Error::Bus)?;
        self.run(START)
    }

    fn run(&mut self, stages: &[&[Step]]) -> Result<(), Es7210Error<D::Error>> {
        for stage in stages {
            for step in *stage {
                if step.mask == 0 {
                    self.i2c
                        .write(self.addr, &[step.reg, step.value])
                        .map_err(Es7210Error::Bus)?;
                } else {
                    self.update(*step)?;
                }
            }
        }
        Ok(())
    }

    fn update(&mut self, step: Step) -> Result<(), Es7210Error<D::Error>> {
        let current = self.read_reg(step.reg)?;
        let merged = (current & !step.mask) | (step.value & step.mask);
        self.i2c
            .write(self.addr, &[step.reg, merged])
            .map_err(Es7210Error::Bus)
    }

    fn read_reg(&mut self, reg: u8) -> Result<u8, D::Error> {
        let mut value = [0u8; 1];
        self.i2c.write(self.addr, &[reg])?;
        self.i2c.read(self.addr, &mut value)?;
        Ok(value[0])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;
    use alloc::vec::Vec;
    use embedded_hal::i2c::{ErrorKind, ErrorType, NoAcknowledgeSource, Operation};

    /// The fault the mock bus injects, in the vocabulary the embedded-hal trait
    /// defines rather than a bespoke type, so a test that asserts on it reads the
    /// same as a real peripheral failure.
    const NACK: ErrorKind = ErrorKind::NoAcknowledge(NoAcknowledgeSource::Data);

    struct MockI2c {
        registers: [u8; 0x4D],
        pointer: u8,
        /// Every write in order, as (register, value). Masked writes record the
        /// merged byte the bus actually saw, so a wrong mask shows up as a wrong
        /// trace rather than as nothing at all.
        trace: Vec<(u8, u8)>,
        /// Index into `trace` at which the bus starts failing, so a fault can be
        /// placed at one specific write of a long sequence.
        fail_at: Option<usize>,
    }

    impl MockI2c {
        fn new() -> Self {
            let mut registers = [0u8; 0x4D];
            // Seed the power-on values the sequence has to correct. The serial
            // port matters most: a bring-up that only merged the word width
            // would leave the TDM bit and the frame bits standing.
            registers[REG_SDP_INTERFACE1 as usize] = 0x83;
            registers[REG_SDP_INTERFACE2 as usize] = 0x02;
            registers[REG_MODE_CONFIG as usize] = 0x01;
            registers[REG_MIC1_GAIN as usize] = GAIN_ENABLE;
            registers[REG_CLOCK_OFF as usize] = 0xBF;
            Self {
                registers,
                pointer: 0,
                trace: Vec::new(),
                fail_at: None,
            }
        }

        fn apply(&mut self, write: &[u8]) -> Result<(), ErrorKind> {
            match write {
                [reg] => {
                    self.pointer = *reg;
                    Ok(())
                }
                [reg, value] => {
                    if self.fail_at == Some(self.trace.len()) {
                        return Err(NACK);
                    }
                    self.registers[*reg as usize] = *value;
                    self.trace.push((*reg, *value));
                    Ok(())
                }
                _ => Ok(()),
            }
        }

        fn writes_of(&self, reg: u8) -> Vec<u8> {
            self.trace
                .iter()
                .filter(|&&(written, _)| written == reg)
                .map(|&(_, value)| value)
                .collect()
        }
    }

    impl ErrorType for MockI2c {
        type Error = ErrorKind;
    }

    impl I2c for MockI2c {
        fn read(&mut self, _address: u8, read: &mut [u8]) -> Result<(), Self::Error> {
            for (offset, byte) in read.iter_mut().enumerate() {
                *byte = self.registers[(self.pointer as usize + offset).min(0x4C)];
            }
            Ok(())
        }

        fn transaction(
            &mut self,
            address: u8,
            operations: &mut [Operation<'_>],
        ) -> Result<(), Self::Error> {
            for operation in operations {
                match operation {
                    Operation::Read(read) => self.read(address, read)?,
                    Operation::Write(write) => self.apply(write)?,
                }
            }
            Ok(())
        }
    }

    fn settle() -> Es7210<MockI2c> {
        let mut codec = Es7210::new(MockI2c::new(), ES7210_I2C_ADDR);
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
        let codec = settle();
        let sdp = codec.i2c.registers[REG_SDP_INTERFACE1 as usize];
        assert_eq!(sdp & SDP_FRAME_MASK, SDP_I2S, "standard I2S framing");
        assert_eq!(sdp & SDP_WIDTH_MASK, SDP_16BIT, "16-bit words");
        assert_eq!(sdp, 0x60, "the power-on frame and TDM bits are gone");
        assert_eq!(
            codec.i2c.registers[REG_SDP_INTERFACE2 as usize] & 0x02,
            0,
            "not TDM"
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
    fn a_masked_write_never_clobbers_a_neighbouring_field() {
        // Bit7 of the gain register is outside every mask the sequence uses, so
        // it has to survive being written twice — once to clear the enable and
        // once to set the gain.
        let mut i2c = MockI2c::new();
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
        let mut codec = Es7210::new(MockI2c::new(), ES7210_I2C_ADDR);
        codec.init().expect("init");
        // The bring-up leaves the clock register wherever its masked writes put
        // it, so a start has to write that byte back rather than a literal.
        let held = codec.i2c.registers[REG_CLOCK_OFF as usize];
        let before = codec.i2c.trace.len();
        codec.start().expect("start");
        assert_eq!(codec.i2c.trace[before], (REG_CLOCK_OFF, held));
    }

    #[test]
    fn the_microphone_selection_is_the_same_whether_it_is_the_first_or_a_later_start() {
        // The vendor issues the selection during bring-up and again on every
        // start. Both have to leave the same register state, or a part that was
        // started twice would be configured differently from a fresh one.
        let mut first = Es7210::new(MockI2c::new(), ES7210_I2C_ADDR);
        first.init().expect("init");
        let once = first.i2c.registers;

        let mut second = Es7210::new(MockI2c::new(), ES7210_I2C_ADDR);
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
        let mut codec = Es7210::new(MockI2c::new(), ES7210_I2C_ADDR);
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
            let mut i2c = MockI2c::new();
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
        // The clock register is the one the start round-trips, so it has to be
        // settled before the start reads it — a start reading it before the
        // selection cleared its gate bits would latch the gated value.
        let codec = settle();
        // The power-on value stands in for whatever the host's clock divider
        // leaves behind; the bring-up overwrites it, then each of the two
        // microphone selections clears its gate bits, then the start hands back
        // the byte it read.
        let gated = codec.i2c.registers[REG_CLOCK_OFF as usize] & !CLOCK_OFF_MIC12_MASK;
        assert_eq!(
            codec.i2c.writes_of(REG_CLOCK_OFF),
            vec![CLOCK_OFF_BRINGUP, gated, gated, gated],
            "bring-up, both selections' masked clears, then the start's read-back"
        );
    }
}
