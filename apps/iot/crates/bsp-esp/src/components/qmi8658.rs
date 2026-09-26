use embedded_hal::delay::DelayNs;
use embedded_hal::i2c::I2c;
use iot_core::drivers::motion::{
    MotionCapabilities, MotionError, MotionEvent, MotionEvents, MotionReading, MotionSample,
    MotionScale, MotionSource, RECOGNIZER_CAPABILITIES,
};

pub const QMI8658_I2C_ADDR: u8 = 0x6A;

const REG_WHO_AM_I: u8 = 0x00;
const REG_CTRL1: u8 = 0x02;
const REG_CTRL2: u8 = 0x03;
const REG_CTRL3: u8 = 0x04;
const REG_CTRL5: u8 = 0x06;
const REG_CTRL7: u8 = 0x08;
const REG_CTRL8: u8 = 0x09;
const REG_CTRL9: u8 = 0x0A;
const REG_CAL1_L: u8 = 0x0B;
const REG_STATUSINT: u8 = 0x2D;
const REG_STATUS0: u8 = 0x2E;
const REG_STATUS1: u8 = 0x2F;
const REG_AX_L: u8 = 0x35;
const REG_RESET: u8 = 0x60;
/// Section 5.9 and section 7.4 disagree on the reset byte — the register table
/// says 0xB0 while the prose says 0x0B, which are bit reversals of each other.
/// Table 27 describes the register itself, so 0xB0 is used, and the 0x4D flag
/// below turns the ambiguity into a checked outcome instead of a silent one: if
/// the byte is wrong the part never reports a completed reset.
const RESET_VALUE: u8 = 0xB0;
/// Datasheet 5.9: reads 0x80 after a successful power-on or soft reset. It is
/// overwritten by later operations, so it has to be read straight afterwards.
const REG_RESET_FLAG: u8 = 0x4D;
const RESET_FLAG_READY: u8 = 0x80;

const WHO_AM_I_VALUE: u8 = 0x05;
const CTRL1_VALUE: u8 = 0x40;
const CTRL2_VALUE: u8 = 0x13;
const CTRL3_VALUE: u8 = 0x53;
/// Datasheet Table 22: bit4 `gLPF_EN` clear, bit0 `aLPF_EN` clear. The
/// accelerometer low-pass filter is off, keeping the full 896.8 Hz bandwidth
/// for the knock's broadband transient. An on-device A/B pass (same gestures,
/// aLPF off/on) showed identical resting noise and knock readings either way —
/// the attenuation comes from the structure and coupling, not this filter — so
/// the radio is left clear and the recognizer's bars (peak 250 mG / quiet
/// 150 mG) were calibrated on this unfiltered stream. The gyro filter stays
/// off because nothing reads the gyro path for peaks.
const CTRL5_VALUE: u8 = 0x00;
const CTRL7_VALUE: u8 = 0x03;

const CTRL8_NO_MOTION_EN: u8 = 1 << 2;
/// This board wires no INT line, so the CTRL9 handshake has to be polled
/// through `STATUSINT.bit7` instead of waiting on the INT1 pin.
const CTRL8_STATUSINT_HANDSHAKE: u8 = 1 << 7;
/// The tap engine is left disarmed: it latched a stuck tap bit and a frozen
/// `TAP_NUM` at its enable transient and never resolved a real blow, so that
/// contract moved to the core recognizer. Only No-Motion is armed here.
const CTRL8_VALUE: u8 = CTRL8_NO_MOTION_EN | CTRL8_STATUSINT_HANDSHAKE;

const STATUS1_NO_MOTION: u8 = 1 << 6;
const STATUSINT_CMD_DONE: u8 = 1 << 7;

const CTRL_CMD_ACK: u8 = 0x00;
const CTRL_CMD_CONFIGURE_MOTION: u8 = 0x0E;

/// The datasheet gives no worst case for `CmdDone`, so the budget is taken from
/// the one independent implementation that publishes one (RIOT uses a 1000ms
/// command timeout). The part raises `CmdDone` as a sticky flag, so a tight poll
/// would give up long before a slow part finished, and a false timeout costs the
/// whole motion plane permanently. Being loose is free here because this runs
/// once per boot inside `init`, and on the happy path the first poll already
/// answers.
const CMD_DONE_ATTEMPTS: u16 = 1_000;
const CMD_DONE_POLL_US: u32 = 1_000;
/// Datasheet 7.4 puts 15ms as the worst case for the reset process, so the
/// budget is that plus margin. A blind fixed wait would be a guess in both
/// directions: too short leaves the part mid-reset when the first configuration
/// write lands, too long just delays boot.
const RESET_POLL_ATTEMPTS: u8 = 100;
const RESET_POLL_US: u32 = 200;

/// This firmware enables both sensors, where note 13 makes the gyro set the
/// rate for both, so `0b0011` is the 6DOF ODR of 896.8 Hz. The rate no longer
/// gates the tap contract — the knock detector lives in core and sees the
/// filtered 20 ms plane, not the raw ODR — but it still sits well above the
/// No-Motion engine's needs and keeps the gyro path (yaw diagnostics) full
/// rate. The No-Motion engine counts samples while the product reasons in
/// milliseconds, so the index is pinned here to stop a retuned ODR from
/// silently rescaling every window.
const ODR_INDEX: u8 = 0b0011;
const ODR_HZ_X10: u32 = 8_968;
const _: () = assert!(CTRL2_VALUE & 0x0F == ODR_INDEX && CTRL3_VALUE & 0x0F == ODR_INDEX);

/// Converts a window from the milliseconds a person reasons about into the
/// sample count the No-Motion engine expects, rounded to nearest.
const fn odr_samples(ms: u16) -> u16 {
    ((ms as u32 * ODR_HZ_X10 + 5_000) / 10_000) as u16
}

// Motion parameter values, datasheet Table 32; byte positions from Table 34.
// NoMotionX/Y/ZThr carry five fractional bits of 0.03125 g, so 0x08 is 0.25 g
// of slope; the windows count accel samples.
const NO_MOTION_THR: u8 = 0x08;
const NO_MOTION_WINDOW: u8 = odr_samples(80) as u8;
/// Bits 0-2 enable the AnyMotion axes, 4-6 the NoMotion axes, and bit 7 picks
/// whether the engine ANDs or ORs the enabled axes. Only NoMotion is armed,
/// and ANDing all three is the stricter reading of a still device: every axis
/// has to stay under its slope threshold for the whole window. Leaving bit 7
/// clear would let one settled axis report still while the device is turning.
const MOTION_MODE_CTRL: u8 = 0b1111_0111;
// AnyMotion and SignificantMotion stay disabled in CTRL8, so their thresholds
// and windows are never consulted; they are still written to keep the two
// command sets shaped the way the datasheet describes. 0x00 is a placeholder
// rather than a chosen threshold: a real AnyMotion setting of zero would fire on
// any deviation at all, which is exactly why the engine is left disarmed and a
// test pins it there.
const ANY_MOTION_THR: u8 = 0x00;
const ANY_MOTION_WINDOW: u8 = odr_samples(80) as u8;
const SIG_MOTION_WAIT_WINDOW: u16 = 0;
const SIG_MOTION_CONFIRM_WINDOW: u16 = 0;
const _: () = assert!(NO_MOTION_WINDOW == 72 && ANY_MOTION_WINDOW == 72);

const ACCEL_LSB_PER_G: i32 = 16_384 >> ((CTRL2_VALUE >> 4) & 0x07);
const GYRO_LSB_PER_DPS: i32 = 2_048 >> ((CTRL3_VALUE >> 4) & 0x07);
const _: () = assert!(ACCEL_LSB_PER_G == 8_192 && GYRO_LSB_PER_DPS == 64);
const _: () = assert!(CTRL2_VALUE & 0x80 == 0 && CTRL3_VALUE & 0x80 == 0);
const SCALE: MotionScale = MotionScale::from_ranges(ACCEL_LSB_PER_G, GYRO_LSB_PER_DPS);
const DATA_BYTES: usize = 12;
const DATA_READY_MASK: u8 = 0x03;

/// What this part contributes on its own: raw telemetry plus the No-Motion
/// engine it arms. Everything else the product reports is derived core-side
/// from the data plane — including the tap contract after that engine proved
/// unreachable — so claiming it here would misattribute it to the hardware.
pub const QMI8658_CAPABILITIES: MotionCapabilities =
    MotionCapabilities::TELEMETRY.union(MotionCapabilities::STILL);

/// What the whole motion stack can report for this board.
pub const MOTION_CAPABILITIES: MotionCapabilities =
    QMI8658_CAPABILITIES.union(RECOGNIZER_CAPABILITIES);

#[derive(Debug)]
pub enum Qmi8658Error<E> {
    Bus(E),
    Identity(u8),
    /// The part never reported a completed soft reset, so nothing downstream can
    /// be trusted: the reset byte was rejected or the chip is not responding.
    ResetIncomplete,
    EngineConfig,
}

impl<E> From<E> for Qmi8658Error<E> {
    fn from(error: E) -> Self {
        Self::Bus(error)
    }
}

pub struct Qmi8658<D> {
    i2c: D,
    addr: u8,
    initialized: bool,
    last_sample_ms: Option<u64>,
    last_sample: Option<MotionSample>,
    yaw_deg_x10: i16,
    /// Previous STATUS1 value, so a level-true engine flag is reported on its
    /// rising edge instead of once per poll it holds.
    last_status: u8,
    error_logged: bool,
}

impl<D: I2c> Qmi8658<D> {
    pub const fn new(i2c: D, addr: u8) -> Self {
        Self {
            i2c,
            addr,
            initialized: false,
            last_sample_ms: None,
            last_sample: None,
            yaw_deg_x10: 0,
            last_status: 0,
            error_logged: false,
        }
    }

    pub fn init(&mut self, delay: &mut impl DelayNs) -> Result<(), Qmi8658Error<D::Error>> {
        if self.initialized {
            return Ok(());
        }
        self.validate_id()?;
        self.i2c
            .write(self.addr, &[REG_RESET, RESET_VALUE])
            .map_err(Qmi8658Error::Bus)?;
        self.await_reset(delay)?;
        self.configure(delay)?;
        self.initialized = true;
        Ok(())
    }

    /// Datasheet 5.9 and 7.4: 0x4D reads 0x80 once the reset process finished,
    /// and the host is expected to read it immediately because enabling the
    /// sensors or issuing a CTRL9 command overwrites it.
    fn await_reset(&mut self, delay: &mut impl DelayNs) -> Result<(), Qmi8658Error<D::Error>> {
        for _ in 0..RESET_POLL_ATTEMPTS {
            if self.read_reg(REG_RESET_FLAG)? & RESET_FLAG_READY != 0 {
                return Ok(());
            }
            delay.delay_us(RESET_POLL_US);
        }
        Err(Qmi8658Error::ResetIncomplete)
    }

    pub fn read_sample(&mut self, now_ms: u64) -> Result<Option<MotionReading>, MotionError> {
        if !self.initialized {
            return Err(MotionError::NotReady);
        }
        let bus = |_| MotionError::Bus;

        let mut events = MotionEvents::new();
        let status = self.decode_events(&mut events).map_err(bus)?;

        if self.read_reg(REG_STATUS0).map_err(bus)? & DATA_READY_MASK != DATA_READY_MASK {
            // An engine can fire between accel frames, so a hardware event is
            // still worth publishing even when the data plane has not moved.
            return match (events.is_empty(), self.last_sample) {
                (true, _) => Ok(None),
                (false, Some(sample)) => Ok(Some(MotionReading {
                    sample: MotionSample { status, ..sample },
                    hardware: events,
                })),
                (false, None) => Ok(Some(MotionReading {
                    sample: MotionSample {
                        status,
                        ..MotionSample::error()
                    },
                    hardware: events,
                })),
            };
        }

        let mut data = [0u8; DATA_BYTES];
        self.read_bytes(REG_AX_L, &mut data).map_err(bus)?;
        let raw_accel = [
            i16::from_le_bytes([data[0], data[1]]),
            i16::from_le_bytes([data[2], data[3]]),
            i16::from_le_bytes([data[4], data[5]]),
        ];
        let raw_gyro = [
            i16::from_le_bytes([data[6], data[7]]),
            i16::from_le_bytes([data[8], data[9]]),
            i16::from_le_bytes([data[10], data[11]]),
        ];
        let elapsed_ms = self
            .last_sample_ms
            .map(|previous| now_ms.saturating_sub(previous))
            .unwrap_or_default();
        let sample = MotionSample::from_raw(raw_accel, raw_gyro, SCALE, status, self.yaw_deg_x10);
        self.yaw_deg_x10 = integrate_yaw(self.yaw_deg_x10, sample.gyro_dps_x10[2], elapsed_ms);
        self.last_sample_ms = Some(now_ms);
        self.last_sample = Some(sample);
        Ok(Some(MotionReading {
            sample,
            hardware: events,
        }))
    }

    fn validate_id(&mut self) -> Result<(), Qmi8658Error<D::Error>> {
        match self.read_reg(REG_WHO_AM_I) {
            Ok(id) if id == WHO_AM_I_VALUE => Ok(()),
            Ok(id) => Err(Qmi8658Error::Identity(id)),
            Err(error) => Err(Qmi8658Error::Bus(error)),
        }
    }

    fn configure(&mut self, delay: &mut impl DelayNs) -> Result<(), Qmi8658Error<D::Error>> {
        let bus = |error: D::Error| Qmi8658Error::Bus(error);
        self.i2c
            .write(self.addr, &[REG_CTRL1, CTRL1_VALUE])
            .map_err(bus)?;
        self.i2c
            .write(self.addr, &[REG_CTRL2, CTRL2_VALUE])
            .map_err(bus)?;
        self.i2c
            .write(self.addr, &[REG_CTRL3, CTRL3_VALUE])
            .map_err(bus)?;
        self.i2c
            .write(self.addr, &[REG_CTRL5, CTRL5_VALUE])
            .map_err(bus)?;
        // The CTRL9 protocol requires both sensors quiet, and the engines only
        // arm once their parameters have landed.
        self.i2c.write(self.addr, &[REG_CTRL7, 0x00]).map_err(bus)?;
        self.i2c
            .write(self.addr, &[REG_CTRL8, CTRL8_STATUSINT_HANDSHAKE])
            .map_err(bus)?;

        // Motion, datasheet Table 34. The three AnyMotion thresholds and the
        // three NoMotion thresholds fill the first set, the windows and the
        // SignificantMotion windows fill the second. NoMotion is the engine this
        // product arms, so its thresholds and window have to land where the
        // engine reads them.
        let motion_first = [
            ANY_MOTION_THR,
            ANY_MOTION_THR,
            ANY_MOTION_THR,
            NO_MOTION_THR,
            NO_MOTION_THR,
            NO_MOTION_THR,
            MOTION_MODE_CTRL,
            CAL_SET_FIRST,
        ];
        let motion_second = [
            ANY_MOTION_WINDOW,
            NO_MOTION_WINDOW,
            SIG_MOTION_WAIT_WINDOW as u8,
            (SIG_MOTION_WAIT_WINDOW >> 8) as u8,
            SIG_MOTION_CONFIRM_WINDOW as u8,
            (SIG_MOTION_CONFIRM_WINDOW >> 8) as u8,
            0x00,
            CAL_SET_SECOND,
        ];
        self.submit_calibration(&motion_first, CTRL_CMD_CONFIGURE_MOTION, delay)?;
        self.submit_calibration(&motion_second, CTRL_CMD_CONFIGURE_MOTION, delay)?;

        self.i2c
            .write(self.addr, &[REG_CTRL8, CTRL8_VALUE])
            .map_err(bus)?;
        self.i2c
            .write(self.addr, &[REG_CTRL7, CTRL7_VALUE])
            .map_err(bus)?;
        Ok(())
    }

    fn submit_calibration(
        &mut self,
        parameters: &[u8; 8],
        command: u8,
        delay: &mut impl DelayNs,
    ) -> Result<(), Qmi8658Error<D::Error>> {
        let bus = |error: D::Error| Qmi8658Error::Bus(error);
        let mut burst = [REG_CAL1_L; 9];
        burst[1..].copy_from_slice(parameters);
        self.i2c.write(self.addr, &burst).map_err(bus)?;
        self.i2c
            .write(self.addr, &[REG_CTRL9, command])
            .map_err(bus)?;
        self.await_cmd_done(delay)?;
        self.i2c
            .write(self.addr, &[REG_CTRL9, CTRL_CMD_ACK])
            .map_err(bus)?;
        Ok(())
    }

    fn await_cmd_done(&mut self, delay: &mut impl DelayNs) -> Result<(), Qmi8658Error<D::Error>> {
        for _ in 0..CMD_DONE_ATTEMPTS {
            if self.read_reg(REG_STATUSINT)? & STATUSINT_CMD_DONE != 0 {
                return Ok(());
            }
            delay.delay_us(CMD_DONE_POLL_US);
        }
        Err(Qmi8658Error::EngineConfig)
    }

    fn decode_events(&mut self, events: &mut MotionEvents) -> Result<u8, D::Error> {
        let status = self.read_reg(REG_STATUS1)?;
        // STATUS1 flags are levels, not events: the NoMotion engine asserts
        // every poll it still considers the device still, so publishing the bit
        // itself re-reported stillness at the frame rate on the bench. Only the
        // 0→1 rise is a transition into the state and worth an event; a bit
        // held high across polls stays out, and a later rise is a new one.
        if status & STATUS1_NO_MOTION != 0 && self.last_status & STATUS1_NO_MOTION == 0 {
            events.push(MotionEvent::Still);
        }
        self.last_status = status;
        Ok(status)
    }

    fn read_reg(&mut self, reg: u8) -> Result<u8, D::Error> {
        let mut value = [0u8; 1];
        self.read_bytes(reg, &mut value)?;
        Ok(value[0])
    }

    fn read_bytes(&mut self, reg: u8, out: &mut [u8]) -> Result<(), D::Error> {
        self.i2c.write(self.addr, &[reg])?;
        self.i2c.read(self.addr, out)?;
        Ok(())
    }
}

/// A parameter set is selected by the high nibble of CAL4_H; the engine needs
/// both halves written before it will accept new parameters.
const CAL_SET_FIRST: u8 = 0x01;
const CAL_SET_SECOND: u8 = 0x02;

impl<D: I2c> MotionSource for Qmi8658<D> {
    fn sample(&mut self, now_ms: u64) -> Result<Option<MotionReading>, MotionError> {
        let result = self.read_sample(now_ms);
        if let Err(error) = result {
            if !self.error_logged {
                log::warn!("[MOTION] QMI8658 poll failed: {error:?}");
                self.error_logged = true;
            }
            return Err(error);
        }
        self.error_logged = false;
        result
    }
}

/// Minimum quiet time between poll-side re-init attempts, so a dead bus is not
/// hammered at the plane cadence once polling is live.
pub const MOTION_REINIT_MS: u64 = 1_000;

/// Poll-side recovery when boot init did not complete: re-attempts `init` at
/// [`MOTION_REINIT_MS`] cadence so a cold-boot NACK heals within the first
/// seconds. Embedded-hal only, so host tests cover it with a mock bus/delay.
pub struct RecoverableQmi<D, Del> {
    inner: Qmi8658<D>,
    delay: Del,
    last_init_ms: u64,
}

impl<D, Del> RecoverableQmi<D, Del> {
    pub const fn new(inner: Qmi8658<D>, delay: Del) -> Self {
        Self {
            inner,
            delay,
            last_init_ms: 0,
        }
    }
}

impl<D: I2c, Del: DelayNs> MotionSource for RecoverableQmi<D, Del> {
    fn sample(&mut self, now_ms: u64) -> Result<Option<MotionReading>, MotionError> {
        let first = self.inner.sample(now_ms);
        if !matches!(first, Err(MotionError::NotReady)) {
            return first;
        }
        if now_ms.wrapping_sub(self.last_init_ms) < MOTION_REINIT_MS {
            return first;
        }
        self.last_init_ms = now_ms;
        if self.inner.init(&mut self.delay).is_ok() {
            self.inner.sample(now_ms)
        } else {
            first
        }
    }
}

fn integrate_yaw(current: i16, gyro_z_dps_x10: i32, elapsed_ms: u64) -> i16 {
    let delta = (i64::from(gyro_z_dps_x10) * elapsed_ms.min(1_000) as i64 / 1_000) as i32;
    let mut yaw = i32::from(current) + delta;
    yaw = yaw.rem_euclid(3_600);
    if yaw > 1_800 {
        yaw -= 3_600;
    }
    yaw as i16
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;
    use alloc::vec::Vec;
    use embedded_hal::i2c::{ErrorType, Operation};

    const fn register(reg: u8) -> usize {
        reg as usize
    }

    struct MockState {
        registers: [u8; 0x61],
        pointer: u8,
        completes_reset: bool,
        completes_commands: bool,
        /// Every bus access in order. Reads carry `READ` as their value so that
        /// ordering between reads and writes stays comparable in one timeline.
        history: Vec<(u8, u8)>,
        calibrations: Vec<[u8; 8]>,
    }

    impl MockState {
        fn new() -> Self {
            let mut state = Self {
                registers: [0u8; 0x61],
                pointer: 0,
                completes_reset: true,
                completes_commands: true,
                history: Vec::new(),
                calibrations: Vec::new(),
            };
            state.registers[register(REG_WHO_AM_I)] = WHO_AM_I_VALUE;
            state.registers[register(REG_STATUS0)] = DATA_READY_MASK;
            state.registers[register(REG_CTRL7)] = CTRL7_VALUE;
            state.registers[register(REG_CTRL8)] = CTRL8_VALUE;
            state.set_raw(REG_AX_L, [0, 0, 8192, 0, 0, 0]);
            state
        }

        fn set_raw(&mut self, reg: u8, values: [i16; 6]) {
            for (offset, value) in values.iter().enumerate() {
                let bytes = value.to_le_bytes();
                self.registers[reg as usize + offset * 2] = bytes[0];
                self.registers[reg as usize + offset * 2 + 1] = bytes[1];
            }
        }

        fn control_writes(&self, reg: u8) -> Vec<u8> {
            self.history
                .iter()
                .filter(|&&(written, _)| written == reg)
                .map(|&(_, value)| value)
                .collect()
        }
    }

    struct MockI2c {
        state: MockState,
    }

    impl MockI2c {
        fn new() -> Self {
            Self {
                state: MockState::new(),
            }
        }

        fn apply(&mut self, write: &[u8]) {
            match write {
                [reg] => self.state.pointer = *reg,
                [reg, value] => {
                    self.state.registers[register(*reg)] = *value;
                    self.state.history.push((*reg, *value));
                    if *reg == REG_RESET && self.state.completes_reset {
                        // A real part raises 0x4D.bit7 once the reset process
                        // finishes, and later operations overwrite it again.
                        self.state.registers[register(REG_RESET_FLAG)] = RESET_FLAG_READY;
                    }
                    if *reg == REG_CTRL9 && *value != CTRL_CMD_ACK && self.state.completes_commands
                    {
                        // A real part raises CmdDone for a command and drops it
                        // on the acknowledge that follows.
                        self.state.registers[register(REG_STATUSINT)] |= STATUSINT_CMD_DONE;
                    } else if *reg == REG_CTRL9 {
                        self.state.registers[register(REG_STATUSINT)] &= !STATUSINT_CMD_DONE;
                    }
                }
                [reg, values @ ..] => {
                    self.state.history.push((*reg, values[0]));
                    if values.len() == 8 {
                        let mut parameters = [0u8; 8];
                        parameters.copy_from_slice(values);
                        self.state.calibrations.push(parameters);
                    }
                    for (offset, value) in values.iter().enumerate() {
                        self.state.registers[(*reg as usize + offset).min(0x60)] = *value;
                    }
                }
                [] => {}
            }
        }
    }

    impl ErrorType for MockI2c {
        type Error = core::convert::Infallible;
    }

    impl I2c for MockI2c {
        fn read(&mut self, _address: u8, read: &mut [u8]) -> Result<(), Self::Error> {
            self.state.history.push((self.state.pointer, READ));
            for (offset, byte) in read.iter_mut().enumerate() {
                *byte = self.state.registers[(self.state.pointer as usize + offset).min(0x60)];
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
                    Operation::Write(write) => self.apply(write),
                }
            }
            Ok(())
        }
    }

    fn driver(i2c: MockI2c) -> Qmi8658<MockI2c> {
        Qmi8658::new(i2c, QMI8658_I2C_ADDR)
    }

    /// Sentinel value marking a read in the shared access trace. Register
    /// writes are data, so no configuration value can collide with it.
    const READ: u8 = u8::MAX;

    fn writes(history: &[(u8, u8)]) -> Vec<(u8, u8)> {
        history
            .iter()
            .copied()
            .filter(|&(_, value)| value != READ)
            .collect()
    }

    fn settle(driver: &mut Qmi8658<MockI2c>) {
        let mut delay = NoDelay;
        driver.init(&mut delay).expect("init");
    }

    struct NoDelay;

    impl DelayNs for NoDelay {
        fn delay_ns(&mut self, _ns: u32) {}
        fn delay_us(&mut self, _us: u32) {}
        fn delay_ms(&mut self, _ms: u32) {}
    }

    #[test]
    fn init_enables_the_sensors_at_the_documented_ranges() {
        let mut driver = driver(MockI2c::new());
        settle(&mut driver);
        let state = &driver.i2c.state;
        assert_eq!(state.registers[register(REG_CTRL1)], CTRL1_VALUE);
        assert_eq!(state.registers[register(REG_CTRL2)], CTRL2_VALUE);
        assert_eq!(state.registers[register(REG_CTRL3)], CTRL3_VALUE);
        assert_eq!(state.registers[register(REG_CTRL7)], CTRL7_VALUE);
    }

    #[test]
    fn the_accelerometer_low_pass_is_disabled_and_gyro_filter_is_not() {
        let mut driver = driver(MockI2c::new());
        settle(&mut driver);
        let ctrl5 = driver.i2c.state.registers[register(REG_CTRL5)];
        assert_eq!(ctrl5, CTRL5_VALUE);
        // Datasheet Table 22: bit4 gLPF_EN clear, bit0 aLPF_EN clear. The accel
        // filter is off so the knock's broadband transient reaches the squared
        // residual intact; an on-device filter on/off A/B read identically, so
        // the low-pass was never the attenuator and the bars are set on the
        // unfiltered stream. The gyro filter stays off since nothing reads the
        // gyro path for peaks.
        assert_eq!(ctrl5 & (1 << 4), 0);
        assert_eq!(ctrl5 & 1, 0);
    }

    #[test]
    fn init_arms_only_no_motion_over_the_statusint_handshake() {
        let mut driver = driver(MockI2c::new());
        settle(&mut driver);
        let ctrl8 = driver.i2c.state.registers[register(REG_CTRL8)];
        assert_eq!(ctrl8 & CTRL8_NO_MOTION_EN, CTRL8_NO_MOTION_EN);
        assert_eq!(ctrl8 & CTRL8_STATUSINT_HANDSHAKE, CTRL8_STATUSINT_HANDSHAKE);
        // Engines this product leaves to the core-side classifier stay off.
        // Tap is one of them: the hardware engine latched a stuck bit at its
        // enable transient and never resolved a blow, so the contract moved to
        // the core recognizer and the silicon is left disarmed.
        assert_eq!(ctrl8 & 1 << 0, 0);
        assert_eq!(ctrl8 & (1 << 1 | 1 << 3 | 1 << 4), 0);
    }

    #[test]
    fn init_pushes_both_parameter_sets_of_the_no_motion_engine() {
        let mut driver = driver(MockI2c::new());
        settle(&mut driver);
        let state = &driver.i2c.state;
        assert_eq!(
            state.control_writes(REG_CTRL9),
            vec![
                CTRL_CMD_CONFIGURE_MOTION,
                CTRL_CMD_ACK,
                CTRL_CMD_CONFIGURE_MOTION,
                CTRL_CMD_ACK,
            ]
        );
        // Every command is acknowledged, so the handshake flag is left clear.
        assert_eq!(
            state.registers[register(REG_STATUSINT)] & STATUSINT_CMD_DONE,
            0
        );
        // CAL4_H selects which half of a parameter set is being written.
        assert_eq!(state.registers[register(REG_CAL1_L + 7)], CAL_SET_SECOND);
        // Each burst is CAL1_L..CAL4_H. Pinning the whole layout rather than a
        // few interesting bytes is the point: the tables interleave unrelated
        // fields, so a plausible-looking subset hides a misfiled threshold. The
        // windows are derived from the production constants so a retuned ODR
        // cannot silently desync the snapshot from what the part is sent.
        assert_eq!(
            state.calibrations,
            vec![
                // Table 34, motion first set: AnyMotionX/Y/ZThr, NoMotionX/Y/ZThr,
                // MOTION_MODE_CTRL, 1st command.
                [
                    ANY_MOTION_THR,
                    ANY_MOTION_THR,
                    ANY_MOTION_THR,
                    NO_MOTION_THR,
                    NO_MOTION_THR,
                    NO_MOTION_THR,
                    MOTION_MODE_CTRL,
                    CAL_SET_FIRST,
                ],
                // Table 34, motion second set: AnyMotionWindow, NoMotionWindow
                // (80 ms = 72 samples each), SigMotionWait 0x0000, SigMotionConfirm
                // 0x0000, NA, 2nd command.
                [
                    ANY_MOTION_WINDOW,
                    NO_MOTION_WINDOW,
                    SIG_MOTION_WAIT_WINDOW as u8,
                    (SIG_MOTION_WAIT_WINDOW >> 8) as u8,
                    SIG_MOTION_CONFIRM_WINDOW as u8,
                    (SIG_MOTION_CONFIRM_WINDOW >> 8) as u8,
                    0,
                    CAL_SET_SECOND,
                ],
            ]
        );
    }

    #[test]
    fn an_unreported_reset_stops_configuration() {
        let mut i2c = MockI2c::new();
        i2c.state.completes_reset = false;
        let mut driver = driver(i2c);
        let mut delay = NoDelay;
        assert!(matches!(
            driver.init(&mut delay),
            Err(Qmi8658Error::ResetIncomplete)
        ));
        // The part is not in a known state, so nothing may be written past the
        // reset itself. Configuring it anyway is how a rejected reset byte turns
        // into mysterious motion data much later.
        assert_eq!(
            writes(&driver.i2c.state.history),
            vec![(REG_RESET, RESET_VALUE)],
            "configuration must not run on a part that never reported a reset"
        );
    }

    #[test]
    fn the_reset_flag_is_read_before_anything_overwrites_it() {
        let mut driver = driver(MockI2c::new());
        settle(&mut driver);
        let trace = &driver.i2c.state.history;
        let reset_flag_read = trace
            .iter()
            .position(|&(reg, value)| reg == REG_RESET_FLAG && value == READ)
            .expect("0x4D is read back to confirm the reset");
        let first_configure = trace
            .iter()
            .position(|&(reg, value)| reg == REG_CTRL1 && value != READ)
            .expect("CTRL1 is configured");
        assert!(
            reset_flag_read < first_configure,
            "0x4D must be read straight after the reset, before any later \
             operation can overwrite it"
        );
    }

    #[test]
    fn init_arms_the_engines_only_after_every_command_lands() {
        let mut driver = driver(MockI2c::new());
        settle(&mut driver);
        let history = &driver.i2c.state.history;
        let armed = history
            .iter()
            .position(|&(reg, value)| reg == REG_CTRL8 && value == CTRL8_VALUE)
            .expect("engines armed");
        let last_command = history
            .iter()
            .rposition(|&(reg, _)| reg == REG_CTRL9)
            .expect("commands issued");
        assert!(armed > last_command);
    }

    #[test]
    fn a_mismatched_identity_stops_configuration() {
        let mut i2c = MockI2c::new();
        i2c.state.registers[register(REG_WHO_AM_I)] = 0x00;
        let mut driver = driver(i2c);
        let mut delay = NoDelay;
        assert!(matches!(
            driver.init(&mut delay),
            Err(Qmi8658Error::Identity(0x00))
        ));
        assert!(
            writes(&driver.i2c.state.history).is_empty(),
            "nothing is written to a part that is not there"
        );
    }

    #[test]
    fn a_silent_handshake_fails_configuration_instead_of_hanging() {
        let mut i2c = MockI2c::new();
        i2c.state.completes_commands = false;
        let mut driver = driver(i2c);
        let mut delay = NoDelay;
        assert!(matches!(
            driver.init(&mut delay),
            Err(Qmi8658Error::EngineConfig)
        ));
    }

    #[test]
    fn polling_before_initialization_reports_not_ready() {
        let mut driver = driver(MockI2c::new());
        assert!(matches!(
            MotionSource::sample(&mut driver, 0),
            Err(MotionError::NotReady)
        ));
    }

    #[test]
    fn a_not_ready_poll_touches_nothing_until_the_reinit_window_elapses() {
        let mut wrapped = RecoverableQmi::new(driver(MockI2c::new()), NoDelay);
        for now_ms in [0, MOTION_REINIT_MS / 2] {
            assert!(matches!(
                MotionSource::sample(&mut wrapped, now_ms),
                Err(MotionError::NotReady)
            ));
        }
        assert!(
            writes(&wrapped.inner.i2c.state.history).is_empty(),
            "a poll before the reinit window must not touch the bus"
        );
    }

    #[test]
    fn a_not_ready_poll_reinit_succeeds_once_the_window_elapses() {
        let mut wrapped = RecoverableQmi::new(driver(MockI2c::new()), NoDelay);
        assert!(matches!(
            MotionSource::sample(&mut wrapped, 0),
            Err(MotionError::NotReady)
        ));
        let reading = MotionSource::sample(&mut wrapped, MOTION_REINIT_MS)
            .expect("reinit read")
            .expect("data ready");
        assert!(
            reading.sample.valid,
            "a recovered part publishes a real frame, not a fault"
        );
        assert!(wrapped.inner.initialized);
    }

    #[test]
    fn a_wedged_part_retries_only_once_per_window_and_heals() {
        let init_attempts = |i2c: &MockI2c| {
            i2c.state
                .history
                .iter()
                .filter(|&&(reg, _)| reg == REG_RESET)
                .count()
        };
        let mut i2c = MockI2c::new();
        i2c.state.completes_reset = false;
        let mut wrapped = RecoverableQmi::new(driver(i2c), NoDelay);

        assert!(matches!(
            MotionSource::sample(&mut wrapped, 0),
            Err(MotionError::NotReady)
        ));
        assert_eq!(init_attempts(&wrapped.inner.i2c), 0);

        assert!(matches!(
            MotionSource::sample(&mut wrapped, MOTION_REINIT_MS),
            Err(MotionError::NotReady)
        ));
        assert_eq!(
            init_attempts(&wrapped.inner.i2c),
            1,
            "the first window boundary attempts a reinit"
        );

        assert!(matches!(
            MotionSource::sample(&mut wrapped, MOTION_REINIT_MS + 500),
            Err(MotionError::NotReady)
        ));
        assert_eq!(
            init_attempts(&wrapped.inner.i2c),
            1,
            "a poll inside the window must not re-attempt"
        );

        assert!(matches!(
            MotionSource::sample(&mut wrapped, MOTION_REINIT_MS * 2),
            Err(MotionError::NotReady)
        ));
        assert_eq!(init_attempts(&wrapped.inner.i2c), 2);

        // The part starts answering; the next window boundary recovers.
        wrapped.inner.i2c.state.completes_reset = true;
        wrapped.inner.i2c.state.registers[register(REG_RESET_FLAG)] = RESET_FLAG_READY;
        let reading = MotionSource::sample(&mut wrapped, MOTION_REINIT_MS * 3)
            .expect("recovered read")
            .expect("data ready");
        assert!(reading.sample.valid);
    }

    #[test]
    fn the_engine_flags_survive_the_no_motion_detector() {
        let mut driver = driver(MockI2c::new());
        settle(&mut driver);
        driver.i2c.state.registers[register(REG_STATUS1)] = STATUS1_NO_MOTION;
        let reading = driver.read_sample(20).expect("read").expect("data ready");
        assert_eq!(reading.hardware.latest(), Some(MotionEvent::Still));
        assert!(reading.sample.valid);
    }

    #[test]
    fn a_tap_bit_is_no_longer_decoded() {
        // The knock contract moved to the core recognizer, so the driver
        // ignores whatever the silicon's tap engine still asserts on STATUS1
        // — the bit neither becomes a Tap event nor disturbs the Still the
        // NoMotion engine carries, and only rides along as a raw flag.
        let mut driver = driver(MockI2c::new());
        settle(&mut driver);
        driver.i2c.state.registers[register(REG_STATUS1)] = STATUS1_NO_MOTION | 1 << 1;
        let reading = driver.read_sample(20).expect("read").expect("data ready");
        assert_eq!(reading.hardware.latest(), Some(MotionEvent::Still));
        assert_eq!(
            reading.sample.status,
            STATUS1_NO_MOTION | 1 << 1,
            "the raw flag still rides the frame for `ST`"
        );
    }

    #[test]
    fn a_still_bit_held_across_polls_reports_the_transition_once() {
        // The NoMotion flag is a level: the engine asserts it on every poll the
        // device is still, so a driver that published the bit would fire Still
        // at the frame rate for as long as the device rests. Only the rising
        // edge is the transition, and it has to happen once.
        let mut driver = driver(MockI2c::new());
        settle(&mut driver);
        driver.i2c.state.registers[register(REG_STATUS1)] = STATUS1_NO_MOTION;
        let first = driver.read_sample(20).expect("read").expect("first");
        let second = driver.read_sample(40).expect("read").expect("second");
        assert_eq!(first.hardware.latest(), Some(MotionEvent::Still));
        assert_eq!(second.hardware.latest(), None);
    }

    #[test]
    fn a_still_bit_clearing_and_re_asserting_reports_each_transition() {
        let mut driver = driver(MockI2c::new());
        settle(&mut driver);
        driver.i2c.state.registers[register(REG_STATUS1)] = STATUS1_NO_MOTION;
        let _ = driver.read_sample(20).expect("set");
        driver.i2c.state.registers[register(REG_STATUS1)] = 0;
        assert_eq!(
            driver
                .read_sample(40)
                .expect("cleared")
                .expect("frame")
                .hardware
                .latest(),
            None
        );
        driver.i2c.state.registers[register(REG_STATUS1)] = STATUS1_NO_MOTION;
        let reasserted = driver.read_sample(60).expect("reassert").expect("frame");
        assert_eq!(reasserted.hardware.latest(), Some(MotionEvent::Still));
    }

    #[test]
    fn the_status_register_travels_with_the_frame_it_was_decoded_from() {
        // `ST` on the attitude page is the only way to tell a latched event bit
        // from one the engine is genuinely re-raising, so the register the events
        // were decoded from has to ride along with the frame instead of being
        // dropped once the events are known. Bit 0 is reserved and powers up
        // zero, so setting it here is how a readout that masks the register down
        // to just the bits it cares about gets caught.
        let mut driver = driver(MockI2c::new());
        settle(&mut driver);
        let status = STATUS1_NO_MOTION | 0x01;
        driver.i2c.state.registers[register(REG_STATUS1)] = status;
        let reading = driver.read_sample(20).expect("read").expect("data ready");
        assert_eq!(reading.sample.status, status);
    }

    #[test]
    fn an_engine_flag_still_publishes_when_the_data_plane_has_not_moved() {
        let mut driver = driver(MockI2c::new());
        settle(&mut driver);
        let _ = driver.read_sample(20).expect("first frame");
        driver.i2c.state.registers[register(REG_STATUS0)] = 0;
        driver.i2c.state.registers[register(REG_STATUS1)] = STATUS1_NO_MOTION;
        let reading = driver.read_sample(40).expect("read").expect("engine event");
        assert_eq!(reading.hardware.latest(), Some(MotionEvent::Still));
        assert_eq!(
            reading.sample.status, STATUS1_NO_MOTION,
            "an event published without a fresh data plane still has to report the status it was decoded from"
        );
    }

    #[test]
    fn a_quiet_poll_with_nothing_to_report_yields_no_frame() {
        let mut driver = driver(MockI2c::new());
        settle(&mut driver);
        let _ = driver.read_sample(20).expect("first frame");
        driver.i2c.state.registers[register(REG_STATUS0)] = 0;
        assert!(driver.read_sample(40).expect("read").is_none());
    }

    #[test]
    fn a_frame_reports_one_g_along_z_within_range() {
        let mut driver = driver(MockI2c::new());
        settle(&mut driver);
        let reading = driver.read_sample(20).expect("read").expect("data ready");
        assert_eq!(reading.sample.accel_mg, [0, 0, 1000]);
        assert!(reading.sample.valid);
    }

    #[test]
    fn the_driver_claims_only_what_its_own_engines_compute() {
        assert!(QMI8658_CAPABILITIES.contains(MotionCapabilities::TELEMETRY));
        assert!(QMI8658_CAPABILITIES.contains(MotionCapabilities::STILL));
        // Tap now belongs to the core recognizer: the silicon engine proved
        // unreachable and never advertised a gesture, so claiming it here would
        // attribute the core's work to the hardware.
        assert!(!QMI8658_CAPABILITIES.contains(MotionCapabilities::TAP));
        assert!(!QMI8658_CAPABILITIES.contains(MotionCapabilities::SHAKE));
        assert!(!QMI8658_CAPABILITIES.contains(MotionCapabilities::STEP));
    }

    #[test]
    fn the_stack_capabilities_add_the_core_side_classifier() {
        assert!(MOTION_CAPABILITIES.contains(MotionCapabilities::TELEMETRY));
        assert!(MOTION_CAPABILITIES.contains(MotionCapabilities::TAP));
        assert!(MOTION_CAPABILITIES.contains(MotionCapabilities::SHAKE));
        assert!(MOTION_CAPABILITIES.contains(MotionCapabilities::POSTURE));
        assert!(!MOTION_CAPABILITIES.contains(MotionCapabilities::STEP));
    }

    #[test]
    fn yaw_integration_follows_the_elapsed_time_and_wraps_at_a_half_turn() {
        assert_eq!(integrate_yaw(0, 1_000, 1_000), 1_000);
        assert_eq!(integrate_yaw(0, -1_000, 1_000), -1_000);
        assert_eq!(integrate_yaw(1_700, 1_000, 1_000), -900);
        assert_eq!(integrate_yaw(-1_700, -1_000, 1_000), 900);
        assert_eq!(integrate_yaw(1_790, 1_000, 100), -1_710);
        // A stall cannot spin the heading away: the step is bounded.
        assert_eq!(integrate_yaw(0, 1_000, 5_000), 1_000);
    }
}
