use embedded_hal::i2c::I2c;

/// Datasheet 2.1: the seven-bit write address. The part has a single address
/// pin, so a board that straps it the other way lands on
/// [`ES8311_I2C_ADDR_ALT`]; the board probes both rather than trusting a
/// schematic it cannot read back.
pub const ES8311_I2C_ADDR: u8 = 0x18;
/// The same part with its address pin strapped high.
pub const ES8311_I2C_ADDR_ALT: u8 = 0x19;

const REG_RESET: u8 = 0x00;
const REG_CLK_ENABLE: u8 = 0x01;
const REG_CLK_DIV_MULT: u8 = 0x02;
const REG_CLK_ADC: u8 = 0x03;
const REG_CLK_DAC_OSR: u8 = 0x04;
const REG_CLK_ADC_DAC_DIV: u8 = 0x05;
const REG_CLK_BCLK: u8 = 0x06;
const REG_CLK_LRCK_H: u8 = 0x07;
const REG_CLK_LRCK_L: u8 = 0x08;
const REG_SDP_DAC: u8 = 0x09;
const REG_SYSTEM_0B: u8 = 0x0B;
const REG_SYSTEM_0C: u8 = 0x0C;
const REG_SYSTEM_0D: u8 = 0x0D;
const REG_SYSTEM_0E: u8 = 0x0E;
const REG_SYSTEM_10: u8 = 0x10;
const REG_SYSTEM_11: u8 = 0x11;
const REG_SYSTEM_12: u8 = 0x12;
const REG_SYSTEM_13: u8 = 0x13;
const REG_SYSTEM_14: u8 = 0x14;
const REG_ADC_15: u8 = 0x15;
const REG_ADC_16: u8 = 0x16;
const REG_ADC_17: u8 = 0x17;
const REG_ADC_1B: u8 = 0x1B;
const REG_ADC_1C: u8 = 0x1C;
const REG_DAC_MUTE: u8 = 0x31;
const REG_DAC_VOLUME: u8 = 0x32;
const REG_DAC_RAMP: u8 = 0x37;
const REG_GPIO: u8 = 0x44;
const REG_GP: u8 = 0x45;
const REG_CHIP_ID1: u8 = 0xFD;
const REG_CHIP_ID2: u8 = 0xFE;

/// The clock and rate this part is configured from the table's row for. Declared
/// once for the whole board and re-exported here, because the capture codec is
/// programmed from the same row and a second copy of either value would be a
/// capture and a speaker a few hertz apart with nothing to say so.
pub use super::audio_clock::{MCLK_HZ, SAMPLE_RATE_HZ};

/// Reset register: bit 7 releases the digital block, bit 6 selects master. Bit 7
/// is written as part of the bring-up rather than as a reset pulse, so the part
/// comes up configured; bit 6 is read-modify-written because the host drives
/// MCLK, BCLK and LRCK and the part must not be allowed to generate them.
const RESET_DIGITAL: u8 = 0x80;
const MASTER_BIT: u8 = 0x40;

/// Clock manager: 0x3F is every clock gate open, and the two top bits are the
/// MCLK source and MCLK inversion. Both are cleared — MCLK comes from the pin
/// and is not inverted — which is why the bring-up writes the whole byte here
/// and every later clock write goes through a mask that preserves it.
const CLOCKS_ALL_RUN: u8 = 0x3F;
const MCLK_SOURCE_MASK: u8 = 0x80;
const MCLK_INVERT_MASK: u8 = 0x40;
/// BCLK inversion, cleared: the host emits BCLK rising-edge aligned.
const BCLK_INVERT_MASK: u8 = 0x20;
/// The BCLK divider's own field, which the reference driver's mask leaves
/// entirely to the divider write.
const BCLK_DIV_MASK: u8 = 0x1F;
/// The part of `0x03` and `0x04` the clock table owns: the fractional-speed bit
/// pair and the OSR the row names. Every row the reference table holds is an OSR
/// of `0x10` or `0x20`, so the low nibble is the same zero the table's values
/// leave there — writing it outright is what lets a retune actually land instead
/// of OR-ing a new OSR onto the one the part is already running.
const CLOCK_FIELD_MASK: u8 = 0xF0;

/// The 12288000/48000 row of the reference driver's clock table: pre-divider 1,
/// pre-multiplier 1, ADC divide 1, DAC divide 1, no fractional part, LRCK
/// divider 0x00:0xFF, BCLK divide 4, and an OSR of 0x10 on both converters. With
/// a 12.288 MHz MCLK that is exactly 48 kHz, and it is the same row the capture
/// half programs, so both codecs come up on one clock.
const COEFF_48K: Coeff = Coeff {
    pre_div: 1,
    pre_multi: 1,
    adc_div: 1,
    dac_div: 1,
    fs_mode: 0,
    lrck_h: 0,
    lrck_l: 0xFF,
    bclk_div: 4,
    adc_osr: 0x10,
    dac_osr: 0x10,
};

/// One row of the codec's clock table, as the fields the reference driver names,
/// so a rate retune is one row rather than a hand-computed set of masked writes
/// spread across six registers.
struct Coeff {
    pre_div: u8,
    pre_multi: u8,
    adc_div: u8,
    dac_div: u8,
    fs_mode: u8,
    lrck_h: u8,
    lrck_l: u8,
    bclk_div: u8,
    adc_osr: u8,
    dac_osr: u8,
}

impl Coeff {
    /// The byte `0x02` carries: the pre-divider in bits 7-5 and the multiplier in
    /// bits 4-3, with the multiplier encoded as its log2.
    const fn div_mult(self) -> u8 {
        let multiplier = match self.pre_multi {
            1 => 0,
            2 => 1,
            4 => 2,
            _ => 3,
        };
        ((self.pre_div - 1) << 5) | (multiplier << 3)
    }

    /// The byte `0x05` carries: the ADC divider in bits 4-1 and the DAC divider
    /// in bits 0, each biased by one because zero means "no division".
    const fn adc_dac_div(self) -> u8 {
        ((self.adc_div - 1) << 4) | (self.dac_div - 1)
    }

    /// The byte `0x03` carries: the fractional part in bits 7-6 and the ADC OSR
    /// below it. The reference driver keeps bit 7 of the register it is about to
    /// overwrite, but its next line is `regv |= fs_mode << 6`, which decides that
    /// same bit — so the preserved copy is a value the row overwrites, and bit 7
    /// belongs to the fractional part with the rest of the field.
    const fn adc(self) -> u8 {
        (self.fs_mode << 6) | self.adc_osr
    }

    /// The byte `0x04` carries: the DAC OSR on its own.
    const fn dac_osr(self) -> u8 {
        self.dac_osr
    }

    /// The byte `0x06` carries, with the divider's encoding split the way the
    /// reference driver splits it: below 19 the field is `div - 1`, from 19 up it
    /// is `div`. The ES8311 datasheet is not distributed, so this encoding is
    /// taken from the reference driver rather than derived.
    const fn bclk(self) -> u8 {
        if self.bclk_div < 19 {
            self.bclk_div - 1
        } else {
            self.bclk_div
        }
    }
}

/// The serial ports. Bit 6 is the run bit the reference driver clears to let a
/// module out of reset, bits 3-2 are the word width — 0b11 is 16 bit, which is
/// what the host port emits — and bits 1-0 are the frame — 0b00 is standard I2S,
/// which is what the host peripheral emits in Philips mode. A width left at its
/// power-on value is not to be trusted: the reference driver sets it for every
/// sample format it configures, and a DAC told 24 bit treats each 16-bit host
/// frame as a different word and holds its output silent. Both ports are written
/// through masks because the DAC is the only one this board runs, and a
/// whole-byte write would carry the ADC's power-on bits into a port the ADC
/// never uses.
const SDP_RUN_MASK: u8 = 0x40;
const SDP_WIDTH_MASK: u8 = 0x0C;
const SDP_16_BIT: u8 = 0x0C;
const SDP_FRAME_MASK: u8 = 0x03;
const SDP_I2S: u8 = 0x00;

/// DAC volume: 0x00 is −95.5 dB, 0x5B is −50 dB, 0xBF is 0 dB and 0xFF is
/// +32 dB, in 0.5 dB steps. Sixty per cent means −20 dB following the
/// esp_codec_dev convention the xiaozhi reference uses (percent maps linearly
/// onto dB from −50 at 0 to 0 at 100, not onto amplitude): unity drives the
/// small power amp into clipping at full-scale tones, and the mute latch is
/// still what a listener reaches for.
pub const DAC_VOLUME_60: u8 = 0x97;

/// DAC mute: bits 5-6 are the soft mute. Named as the field this driver owns, so
/// a read-modify-write leaves every other bit of the register exactly as the part
/// holds it — the reference driver states the same thing with the polarity
/// flipped, keeping `read & 0x9F` and OR-ing `0x60` in, which is the same mask
/// written as "the bits to preserve" rather than "the bits to change".
const DAC_MUTE_FIELD: u8 = 0x60;
const DAC_MUTED: u8 = 0x60;

/// The chip ID, as the part reports it. Checked rather than trusted: the address
/// is the one thing on this bus that a board can get wrong silently, and an
/// unverified part is a part that has to be assumed working.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChipId {
    pub first: u8,
    pub second: u8,
}

impl ChipId {
    pub const EXPECTED: Self = Self {
        first: 0x83,
        second: 0x11,
    };

    pub fn is_expected(self) -> bool {
        self == Self::EXPECTED
    }
}

/// One entry in a bring-up sequence: write `value` across `reg`, or merge it into
/// just the bits `mask` selects. Same shape and the same reason as the capture
/// codec's: a masked write is how the shared clock manager keeps the source and
/// inversion bits the earlier stage set.
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

/// I2C noise immunity, written twice. The reference driver does this because the
/// part intermittently drops the first write after power-up; the second write is
/// not redundant, it is the one that has to land.
const BRING_UP_NOISE_IMMUNITY: &[Step] = &[write(REG_GPIO, 0x08), write(REG_GPIO, 0x08)];

/// The power-on byte set: clock gates off, then the base system registers. These
/// are whole-register writes because the part has just been reset and the
/// reference driver's values are its power-on state with the ports held idle.
const BRING_UP_POWER_ON: &[Step] = &[
    write(REG_CLK_ENABLE, 0x30),
    write(REG_CLK_DIV_MULT, 0x00),
    write(REG_CLK_ADC, 0x10),
    write(REG_ADC_16, 0x24),
    write(REG_CLK_DAC_OSR, 0x10),
    write(REG_CLK_ADC_DAC_DIV, 0x00),
    write(REG_SYSTEM_0B, 0x00),
    write(REG_SYSTEM_0C, 0x00),
    write(REG_SYSTEM_10, 0x1F),
    write(REG_SYSTEM_11, 0x7F),
];

/// Slave mode and every clock gate open. The digital block is released and the
/// master bit cleared in one read-modify-write, because the bit the part holds
/// after the reset write is the one field of `0x00` that is not a power-on
/// value.
const BRING_UP_SLAVE: &[Step] = &[
    write(REG_RESET, RESET_DIGITAL),
    merge(REG_RESET, MASTER_BIT, 0x00),
    write(REG_CLK_ENABLE, CLOCKS_ALL_RUN),
];

/// The clock table row for this board's MCLK and rate, plus the source and
/// inversion bits cleared. Every one of these is a masked write: the bring-up
/// opened the clock gates in the same register the MCLK source lives in, and
/// `0x03`/`0x04`/`0x06` carry bits the reference driver deliberately preserves.
const BRING_UP_CLOCKS: &[Step] = &[
    merge(REG_CLK_ENABLE, MCLK_SOURCE_MASK, 0x00),
    merge(REG_CLK_ENABLE, MCLK_INVERT_MASK, 0x00),
    merge(REG_CLK_DIV_MULT, 0x07, COEFF_48K.div_mult()),
    write(REG_CLK_ADC_DAC_DIV, COEFF_48K.adc_dac_div()),
    merge(REG_CLK_ADC, CLOCK_FIELD_MASK, COEFF_48K.adc()),
    merge(REG_CLK_DAC_OSR, CLOCK_FIELD_MASK, COEFF_48K.dac_osr()),
    merge(REG_CLK_LRCK_H, 0xC0, COEFF_48K.lrck_h),
    write(REG_CLK_LRCK_L, COEFF_48K.lrck_l),
    merge(REG_CLK_BCLK, BCLK_DIV_MASK, COEFF_48K.bclk()),
    merge(REG_CLK_BCLK, BCLK_INVERT_MASK, 0x00),
];

/// The DAC's serial port in standard I2S, the mixed-signal and gain defaults the
/// reference driver writes, and the analog power-up. The frame is merged rather
/// than written so the run state and word width the bring-up has not committed
/// to yet stay as the part holds them; `START` pins the width to 16 bit the
/// moment the port comes out of reset.
const BRING_UP_DAC: &[Step] = &[
    merge(REG_SDP_DAC, SDP_FRAME_MASK, SDP_I2S),
    write(REG_SYSTEM_13, 0x10),
    write(REG_ADC_1B, 0x0A),
    write(REG_ADC_1C, 0x6A),
];

const BRING_UP: &[&[Step]] = &[
    BRING_UP_NOISE_IMMUNITY,
    BRING_UP_POWER_ON,
    BRING_UP_SLAVE,
    BRING_UP_CLOCKS,
    BRING_UP_DAC,
];

/// Releases the DAC out of reset with its port running, at unity volume and
/// unmuted. Separate from `init` so a part that lost its clocks can be restarted
/// without a full re-init, and separate from the mute latch so a listener's
/// choice survives a restart.
const START: &[Step] = &[
    merge(
        REG_SDP_DAC,
        SDP_RUN_MASK | SDP_FRAME_MASK | SDP_WIDTH_MASK,
        SDP_16_BIT,
    ),
    write(REG_ADC_17, 0xBF),
    write(REG_SYSTEM_0E, 0x02),
    write(REG_SYSTEM_12, 0x00),
    write(REG_SYSTEM_14, 0x1A),
    write(REG_SYSTEM_0D, 0x01),
    write(REG_ADC_15, 0x40),
    write(REG_DAC_RAMP, 0x08),
    write(REG_GP, 0x00),
    // The internal reference signal, on the register the datasheet calls
    // "dac2adc for test" — it is the path the DAC output is compared against, and
    // leaving it off is a DAC that starts but never settles.
    write(REG_GPIO, 0x58),
    write(REG_DAC_VOLUME, DAC_VOLUME_60),
    merge(REG_DAC_MUTE, DAC_MUTE_FIELD, 0x00),
];

/// What playback needs from a codec driver: the mute latch and nothing else. The
/// bring-up, the clock row and the registers stay behind I2C in the driver, so
/// the transport never names a register or a bus.
pub trait Mute {
    /// What a mute write reports, named so a transport can log it without being
    /// able to name a bus error type.
    type MuteError;

    /// Latches the output quiet, or audible again.
    fn set_muted(&mut self, muted: bool) -> Result<(), Self::MuteError>;
}

impl<D: I2c> Mute for Es8311<D> {
    type MuteError = Es8311Error<D::Error>;

    fn set_muted(&mut self, muted: bool) -> Result<(), Es8311Error<D::Error>> {
        // Fully qualified, because the trait method and the inherent one this
        // forwards to share a name and the inherent one wins resolution — a bare
        // `self.set_muted` would be right by accident rather than by statement,
        // and would look like recursion to whoever reads it next.
        Es8311::set_muted(self, muted)
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Es8311Error<E> {
    Bus(E),
}

impl<E> From<E> for Es8311Error<E> {
    fn from(error: E) -> Self {
        Self::Bus(error)
    }
}

pub struct Es8311<D> {
    i2c: D,
    addr: u8,
    running: bool,
}

impl<D: I2c> Es8311<D> {
    pub const fn new(i2c: D, addr: u8) -> Self {
        Self {
            i2c,
            addr,
            running: false,
        }
    }

    /// Reads the part's ID registers.
    ///
    /// A separate step from `init` because it answers a different question — is
    /// there a codec at this address at all — and a board that has to try two
    /// addresses needs to ask that before it commits to a bring-up it cannot
    /// unwrite.
    pub fn chip_id(&mut self) -> Result<ChipId, Es8311Error<D::Error>> {
        Ok(ChipId {
            first: self.read_reg(REG_CHIP_ID1)?,
            second: self.read_reg(REG_CHIP_ID2)?,
        })
    }

    /// Brings the part up and starts its DAC. Idempotent, so a board that wires
    /// the speaker into more than one bring-up path does not re-reset a running
    /// codec underneath a playing sound.
    pub fn init(&mut self) -> Result<(), Es8311Error<D::Error>> {
        if self.running {
            return Ok(());
        }
        self.run(BRING_UP)?;
        self.start()?;
        self.running = true;
        Ok(())
    }

    /// Releases the DAC out of reset, at unity volume and unmuted.
    pub fn start(&mut self) -> Result<(), Es8311Error<D::Error>> {
        self.run(&[START])
    }

    /// Latches the DAC output quiet, or audible again.
    ///
    /// A read-modify-write of the soft-mute bits rather than a literal, because
    /// the same register carries the volume ramp and the power state, and a
    /// literal would clear both — which is audible as a click on every unmute.
    pub fn set_muted(&mut self, muted: bool) -> Result<(), Es8311Error<D::Error>> {
        let value = if muted { DAC_MUTED } else { 0x00 };
        self.update(merge(REG_DAC_MUTE, DAC_MUTE_FIELD, value))
    }

    fn run(&mut self, stages: &[&[Step]]) -> Result<(), Es8311Error<D::Error>> {
        for stage in stages {
            for step in *stage {
                if step.mask == 0 {
                    self.i2c
                        .write(self.addr, &[step.reg, step.value])
                        .map_err(Es8311Error::Bus)?;
                } else {
                    self.update(*step)?;
                }
            }
        }
        Ok(())
    }

    fn update(&mut self, step: Step) -> Result<(), Es8311Error<D::Error>> {
        let current = self.read_reg(step.reg)?;
        let merged = (current & !step.mask) | (step.value & step.mask);
        self.i2c
            .write(self.addr, &[step.reg, merged])
            .map_err(Es8311Error::Bus)
    }

    fn read_reg(&mut self, reg: u8) -> Result<u8, Es8311Error<D::Error>> {
        let mut value = [0u8; 1];
        self.i2c.write(self.addr, &[reg])?;
        self.i2c.read(self.addr, &mut value)?;
        Ok(value[0])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec::Vec;
    use embedded_hal::i2c::{ErrorKind, ErrorType, NoAcknowledgeSource, Operation};

    /// The fault the mock bus injects, in the vocabulary the embedded-hal trait
    /// defines rather than a bespoke type, so a test that asserts on it reads the
    /// same as a real peripheral failure.
    const NACK: ErrorKind = ErrorKind::NoAcknowledge(NoAcknowledgeSource::Data);

    const REGISTERS: usize = 0xFF;

    struct MockI2c {
        registers: [u8; REGISTERS],
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
            let mut registers = [0u8; REGISTERS];
            // Seed the power-on values the sequence has to correct, and the ID
            // registers so `chip_id` reports the part rather than a zeroed bus.
            registers[REG_RESET as usize] = 0x00;
            registers[REG_CLK_ENABLE as usize] = 0x00;
            registers[REG_SDP_DAC as usize] = 0x83;
            registers[REG_DAC_MUTE as usize] = 0x01;
            registers[REG_CLK_ADC as usize] = 0x80;
            registers[REG_CLK_DAC_OSR as usize] = 0x80;
            registers[REG_CLK_BCLK as usize] = 0xE0;
            registers[REG_CLK_LRCK_H as usize] = 0xC0;
            registers[REG_CHIP_ID1 as usize] = 0x83;
            registers[REG_CHIP_ID2 as usize] = 0x11;
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
                *byte = self.registers[(self.pointer as usize + offset).min(REGISTERS - 1)];
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

    fn codec() -> Es8311<MockI2c> {
        Es8311::new(MockI2c::new(), ES8311_I2C_ADDR)
    }

    fn settle() -> Es8311<MockI2c> {
        let mut codec = codec();
        codec.init().expect("init");
        codec
    }

    #[test]
    fn the_noise_immunity_write_is_issued_twice() {
        let codec = settle();
        assert_eq!(codec.i2c.trace[0], (REG_GPIO, 0x08));
        assert_eq!(codec.i2c.trace[1], (REG_GPIO, 0x08));
    }

    #[test]
    fn the_part_runs_as_a_slave_of_the_host_clocks() {
        let codec = settle();
        // Every value `0x00` ever holds: the digital block released and the
        // master bit never set, whatever the power-on value was.
        for value in codec.i2c.writes_of(REG_RESET) {
            assert_eq!(value & MASTER_BIT, 0x00, "the part was left a clock master");
            assert_eq!(value & RESET_DIGITAL, RESET_DIGITAL);
        }
    }

    #[test]
    fn the_clock_manager_keeps_every_gate_open_through_the_row_write() {
        let codec = settle();
        // The row writes are masked, so the gate-open byte the slave stage wrote
        // has to survive them.
        assert_eq!(
            codec.i2c.writes_of(REG_CLK_ENABLE).last().copied(),
            Some(CLOCKS_ALL_RUN)
        );
    }

    #[test]
    fn the_row_produces_12_288_mhz_over_256_at_48_khz() {
        // The row's LRCK divider and pre-division are the arithmetic the sample
        // rate actually comes from, so they are pinned here rather than trusted
        // to have been transcribed right.
        assert_eq!(
            MCLK_HZ / (u32::from(COEFF_48K.lrck_l) + 1),
            SAMPLE_RATE_HZ,
            "the LRCK divider does not turn this MCLK into the board's sample rate"
        );
        assert_eq!(COEFF_48K.div_mult() & 0x07, 0x00);
        assert_eq!(COEFF_48K.adc_dac_div(), 0x00);
        assert_eq!(COEFF_48K.bclk(), 3);
    }

    #[test]
    fn the_row_lands_the_bytes_the_reference_driver_writes() {
        let codec = settle();
        // The reference driver reaches the same three registers through
        // read-modify-writes seeded with the power-on values the mock holds, so
        // these are its bytes: the OSRs at `0x10`, and BCLK holding the divider
        // `3` under the three high bits the bring-up's whole-register write left.
        assert_eq!(codec.i2c.writes_of(REG_CLK_ADC).last().copied(), Some(0x10));
        assert_eq!(
            codec.i2c.writes_of(REG_CLK_DAC_OSR).last().copied(),
            Some(0x10)
        );
        assert_eq!(
            codec.i2c.writes_of(REG_CLK_BCLK).last().copied(),
            Some(0xC3),
            "the BCLK write either lost a bit the divider sits under or inverted BCLK"
        );
    }

    #[test]
    fn the_dac_port_starts_in_standard_i2s_16_bit_with_its_run_bit_clear() {
        let codec = settle();
        let port = *codec.i2c.writes_of(REG_SDP_DAC).last().expect("port write");
        assert_eq!(port & SDP_FRAME_MASK, SDP_I2S);
        assert_eq!(
            port & SDP_WIDTH_MASK,
            SDP_16_BIT,
            "the DAC port was left at a word width that swallows the host's 16-bit frames"
        );
        assert_eq!(port & SDP_RUN_MASK, 0x00, "the DAC port was left in reset");
    }

    #[test]
    fn the_start_sequence_runs_the_dac_quiet_and_unmuted() {
        let codec = settle();
        assert_eq!(
            codec.i2c.writes_of(REG_DAC_VOLUME).last().copied(),
            Some(DAC_VOLUME_60)
        );
        let mute = *codec
            .i2c
            .writes_of(REG_DAC_MUTE)
            .last()
            .expect("mute write");
        assert_eq!(
            mute & DAC_MUTE_FIELD,
            0x00,
            "the bring-up left the DAC muted"
        );
        assert_eq!(
            mute & 0x01,
            0x01,
            "the bring-up cleared a bit it did not own"
        );
    }

    #[test]
    fn init_is_idempotent() {
        let mut codec = codec();
        codec.init().expect("init");
        let after_init = codec.i2c.trace.len();
        codec.init().expect("second init");
        assert_eq!(
            codec.i2c.trace.len(),
            after_init,
            "init reset a running part"
        );
    }

    #[test]
    fn muting_latches_the_soft_mute_and_unmuting_releases_only_it() {
        let mut codec = settle();
        codec.set_muted(true).expect("mute");
        assert_eq!(
            codec.i2c.writes_of(REG_DAC_MUTE).last().copied(),
            Some(0x01 | DAC_MUTED)
        );
        codec.set_muted(false).expect("unmute");
        assert_eq!(
            codec.i2c.writes_of(REG_DAC_MUTE).last().copied(),
            Some(0x01),
            "unmuting cleared a bit the latch does not own"
        );
    }

    #[test]
    fn a_mute_does_not_disturb_a_running_sound() {
        let mut codec = settle();
        let volume_before = codec.i2c.writes_of(REG_DAC_VOLUME).last().copied();
        codec.set_muted(true).expect("mute");
        assert_eq!(
            codec.i2c.writes_of(REG_DAC_VOLUME).last().copied(),
            volume_before
        );
    }

    #[test]
    fn a_wrong_chip_id_is_reported_rather_than_accepted() {
        let mut codec = codec();
        assert!(codec.chip_id().expect("id").is_expected());
        codec.i2c.registers[REG_CHIP_ID2 as usize] = 0x12;
        assert!(!codec.chip_id().expect("id").is_expected());
    }

    #[test]
    fn a_bus_error_during_the_sequence_is_reported() {
        let mut codec = codec();
        codec.i2c.fail_at = Some(0);
        assert_eq!(codec.init(), Err(Es8311Error::Bus(NACK)));
    }

    #[test]
    fn a_bus_error_while_latching_the_mute_is_reported() {
        let mut codec = settle();
        let writes = codec.i2c.trace.len();
        codec.i2c.fail_at = Some(writes);
        assert_eq!(codec.set_muted(true), Err(Es8311Error::Bus(NACK)));
    }
}
