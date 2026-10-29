use embedded_hal::delay::DelayNs;
use embedded_hal::i2c::I2c;
use iot_core::config::MotionConfig;
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
/// for the knock's broadband transient, and the core recognizer's bars (peak
/// 250 mG / quiet 150 mG) were calibrated on this unfiltered stream. The gyro
/// filter stays off because nothing reads the gyro path for peaks.
const CTRL5_VALUE: u8 = 0x00;
const CTRL7_VALUE: u8 = 0x03;

const CTRL8_NO_MOTION_EN: u8 = 1 << 2;
/// This board wires no INT line, so the CTRL9 handshake has to be polled
/// through `STATUSINT.bit7` instead of waiting on the INT1 pin.
const CTRL8_STATUSINT_HANDSHAKE: u8 = 1 << 7;
/// The tap engine is left disarmed on purpose; the knock contract lives in the
/// core recognizer. Only No-Motion is armed here.
const CTRL8_VALUE: u8 = CTRL8_NO_MOTION_EN | CTRL8_STATUSINT_HANDSHAKE;

const STATUS1_NO_MOTION: u8 = 1 << 6;
const STATUSINT_CMD_DONE: u8 = 1 << 7;

const CTRL_CMD_ACK: u8 = 0x00;
const CTRL_CMD_CONFIGURE_MOTION: u8 = 0x0E;

const CMD_DONE_ATTEMPTS: u16 = 1_000;
const CMD_DONE_POLL_US: u32 = 1_000;
/// Datasheet 7.4 puts 15ms as the worst case for the reset process, so the
/// budget is that plus margin. A blind fixed wait would be a guess in both
/// directions: too short leaves the part mid-reset when the first configuration
/// write lands, too long just delays boot.
const RESET_POLL_ATTEMPTS: u8 = 100;
const RESET_POLL_US: u32 = 200;

/// This firmware enables both sensors, where note 13 makes the gyro set the
/// rate for both, so `0b0011` is the 6DOF ODR of 896.8 Hz. The No-Motion
/// engine counts samples while the product reasons in milliseconds, so the
/// index is pinned here to stop a retuned ODR from silently rescaling every
/// window.
const ODR_INDEX: u8 = 0b0011;
const ODR_HZ_X10: u32 = 8_968;
const _: () = assert!(CTRL2_VALUE & 0x0F == ODR_INDEX && CTRL3_VALUE & 0x0F == ODR_INDEX);

/// Converts a window from milliseconds into the sample count the No-Motion engine expects.
/// The rate is a parameter because the part's two ODR tables do not agree: with both sensors
/// on the gyro sets the rate for both, alone the accelerometer follows its own oscillator. A
/// window converted through the wrong table is a window of the wrong length.
const fn odr_samples(ms: u16, odr_hz_x10: u32) -> u16 {
    ((ms as u32 * odr_hz_x10 + 5_000) / 10_000) as u16
}

// Motion parameter values, datasheet Table 32; byte positions from Table 34.
// NoMotionX/Y/ZThr carry five fractional bits of 0.03125 g, so 0x08 is 0.25 g
// of slope; the windows count accel samples.
const NO_MOTION_THR: u8 = 0x08;
const NO_MOTION_WINDOW_MS: u16 = 80;
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
const ANY_MOTION_WINDOW_MS: u16 = 80;
const SIG_MOTION_WAIT_WINDOW: u16 = 0;
const SIG_MOTION_CONFIRM_WINDOW: u16 = 0;

const ACCEL_LSB_PER_G: i32 = 16_384 >> ((CTRL2_VALUE >> 4) & 0x07);
const GYRO_LSB_PER_DPS: i32 = 2_048 >> ((CTRL3_VALUE >> 4) & 0x07);
const _: () = assert!(ACCEL_LSB_PER_G == 8_192 && GYRO_LSB_PER_DPS == 64);
const _: () = assert!(CTRL2_VALUE & 0x80 == 0 && CTRL3_VALUE & 0x80 == 0);
const SCALE: MotionScale = MotionScale::from_ranges(ACCEL_LSB_PER_G, GYRO_LSB_PER_DPS);
const DATA_BYTES: usize = 12;
const DATA_READY_MASK: u8 = 0x03;

/// One configuration's register set and the conversion that goes with it. The scale
/// lives here rather than in a `const` because it is derived from the full-scale
/// fields in CTRL2 and CTRL3.
#[derive(Debug, Clone, Copy, PartialEq)]
struct SensorConfig {
    ctrl2: u8,
    ctrl3: u8,
    ctrl5: u8,
    ctrl7: u8,
    odr_hz_x10: u32,
    scale: MotionScale,
}

/// The register set a configuration needs, or `None` when this driver cannot reach it without
/// guessing. Only the configurations that stay on the bring-up arrangement are spelled out:
///
/// - `WakeDetector` needs the accelerometer alone, which changes both the power-mode fields in
///   CTRL7 and which of the part's two ODR tables applies; the datasheet publishes both
///   tables' frequencies but not the index each uses, and the engine windows below are
///   wrong by exactly that much if guessed.
/// - `Fifo` needs the FIFO, whose registers start at the same 0x0B this driver writes
///   motion calibration through — a sequencing problem, not a register write.
///
/// Refusing is the point: each is reachable by adding one encoding with the register table
/// open, none by writing a plausible byte.
const fn config_for(config: MotionConfig) -> Option<SensorConfig> {
    let both_sensors = SensorConfig {
        ctrl2: CTRL2_VALUE,
        ctrl3: CTRL3_VALUE,
        ctrl5: CTRL5_VALUE,
        ctrl7: CTRL7_VALUE,
        odr_hz_x10: ODR_HZ_X10,
        scale: SCALE,
    };

    match config {
        MotionConfig::PolledRegisters => Some(both_sensors),
        // Same axes and rate, only the filters come on — a different stream
        // from the one the core recognizer's bars were calibrated against.
        MotionConfig::FilteredRegisters => Some(SensorConfig {
            ctrl5: 0x11,
            ..both_sensors
        }),
        MotionConfig::WakeDetector | MotionConfig::Fifo => None,
    }
}

/// Every configuration at which this driver can leave the chip, low to high. A probe
/// walks this rather than a ladder, so cells it cannot measure come out unreachable rather
/// than carrying a number nobody measured.
///
/// Off is absent: it is honoured by not polling, which is the consumer's side, and
/// configuring the part *for* off is a separate question.
pub const SUPPORTED_CONFIGS: &[MotionConfig] = &[
    MotionConfig::PolledRegisters,
    MotionConfig::FilteredRegisters,
];

const _: () = {
    assert!(config_for(MotionConfig::PolledRegisters).is_some());
    assert!(config_for(MotionConfig::FilteredRegisters).is_some());
    assert!(config_for(MotionConfig::WakeDetector).is_none());
    assert!(config_for(MotionConfig::Fifo).is_none());
};

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
    /// The demanded configuration needs a register encoding this driver does not carry.
    /// Answering it with a plausible byte would put the part into a mode nobody has read a
    /// datasheet row for.
    UnsupportedConfig(MotionConfig),
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
    /// The configuration the part is running, and its conversion. Runtime because the
    /// scale is derived from the full-scale fields.
    config: MotionConfig,
    scale: MotionScale,
    last_sample_ms: Option<u64>,
    last_sample: Option<MotionSample>,
    yaw_deg_x10: i16,
    /// Previous STATUS1 value, so a level-true engine flag is reported on its
    /// rising edge instead of once per poll it holds.
    last_status: u8,
    error_logged: bool,
    /// Bus transactions this part has issued, so a cost report can state a measured rate
    /// instead of counting what the driver was written to do.
    transactions: u32,
}

impl<D: I2c> Qmi8658<D> {
    /// Boots at [`MotionConfig::PolledRegisters`], the configuration the board's bring-up
    /// validated. A board that wants a different one says so with [`Self::init_at`] before
    /// its first poll.
    pub const fn new(i2c: D, addr: u8) -> Self {
        let mut scale = SCALE;
        if let Some(config) = config_for(MotionConfig::PolledRegisters) {
            scale = config.scale;
        }
        Self {
            i2c,
            addr,
            initialized: false,
            config: MotionConfig::PolledRegisters,
            scale,
            last_sample_ms: None,
            last_sample: None,
            yaw_deg_x10: 0,
            last_status: 0,
            error_logged: false,
            transactions: 0,
        }
    }

    /// How many bus transactions this part has issued since boot.
    pub const fn transactions(&self) -> u32 {
        self.transactions
    }

    /// The configuration the part is running right now.
    pub const fn config(&self) -> MotionConfig {
        self.config
    }

    pub fn init(&mut self, delay: &mut impl DelayNs) -> Result<(), Qmi8658Error<D::Error>> {
        let config = self.config;
        self.init_at(config, delay)
    }

    pub fn init_at(
        &mut self,
        config: MotionConfig,
        delay: &mut impl DelayNs,
    ) -> Result<(), Qmi8658Error<D::Error>> {
        let Some(register_set) = config_for(config) else {
            return Err(Qmi8658Error::UnsupportedConfig(config));
        };
        if self.initialized && self.config == config {
            return Ok(());
        }
        self.validate_id()?;
        self.write(&[REG_RESET, RESET_VALUE])
            .map_err(Qmi8658Error::Bus)?;
        self.await_reset(delay)?;
        self.configure(&register_set, delay)?;
        self.config = config;
        self.scale = register_set.scale;
        self.initialized = true;
        // The recognizer's peaks, yaw integral and stillness edge all describe the stream
        // that was running before the switch, so carrying them across makes the core read
        // the gap as motion.
        self.last_sample = None;
        self.last_sample_ms = None;
        self.yaw_deg_x10 = 0;
        self.last_status = 0;
        Ok(())
    }

    /// Reconfigure a part that is already running, under the same rules as `init_at`: a
    /// sample straddling the switch is discarded rather than converted. Returns the
    /// configuration unchanged when the part was never initialized, so a board that failed
    /// at boot and recovers through the poll side lands on the demanded configuration.
    pub fn set_config(
        &mut self,
        config: MotionConfig,
        delay: &mut impl DelayNs,
    ) -> Result<(), Qmi8658Error<D::Error>> {
        if self.initialized && self.config == config {
            return Ok(());
        }
        self.initialized = false;
        self.init_at(config, delay)
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
        let sample =
            MotionSample::from_raw(raw_accel, raw_gyro, self.scale, status, self.yaw_deg_x10);
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

    fn configure(
        &mut self,
        config: &SensorConfig,
        delay: &mut impl DelayNs,
    ) -> Result<(), Qmi8658Error<D::Error>> {
        let bus = |error: D::Error| Qmi8658Error::Bus(error);
        for (reg, value) in [
            (REG_CTRL1, CTRL1_VALUE),
            (REG_CTRL2, config.ctrl2),
            (REG_CTRL3, config.ctrl3),
            (REG_CTRL5, config.ctrl5),
        ] {
            self.write(&[reg, value]).map_err(bus)?;
        }
        // The CTRL9 protocol requires both sensors quiet, and the engines only
        // arm once their parameters have landed.
        self.write(&[REG_CTRL7, 0x00]).map_err(bus)?;
        self.write(&[REG_CTRL8, CTRL8_STATUSINT_HANDSHAKE])
            .map_err(bus)?;

        // The windows count samples the part actually produces, so they follow the selected
        // rate.
        let no_motion_window = odr_samples(NO_MOTION_WINDOW_MS, config.odr_hz_x10) as u8;
        let any_motion_window = odr_samples(ANY_MOTION_WINDOW_MS, config.odr_hz_x10) as u8;

        // Motion, datasheet Table 34.
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
            any_motion_window,
            no_motion_window,
            SIG_MOTION_WAIT_WINDOW as u8,
            (SIG_MOTION_WAIT_WINDOW >> 8) as u8,
            SIG_MOTION_CONFIRM_WINDOW as u8,
            (SIG_MOTION_CONFIRM_WINDOW >> 8) as u8,
            0x00,
            CAL_SET_SECOND,
        ];
        self.submit_calibration(&motion_first, CTRL_CMD_CONFIGURE_MOTION, delay)?;
        self.submit_calibration(&motion_second, CTRL_CMD_CONFIGURE_MOTION, delay)?;

        self.write(&[REG_CTRL8, CTRL8_VALUE]).map_err(bus)?;
        self.write(&[REG_CTRL7, config.ctrl7]).map_err(bus)?;
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
        self.write(&burst).map_err(bus)?;
        self.write(&[REG_CTRL9, command]).map_err(bus)?;
        self.await_cmd_done(delay)?;
        self.write(&[REG_CTRL9, CTRL_CMD_ACK]).map_err(bus)?;
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

    /// One register write, counted.
    fn write(&mut self, bytes: &[u8]) -> Result<(), D::Error> {
        self.transactions = self.transactions.saturating_add(1);
        self.i2c.write(self.addr, bytes)
    }

    fn read_reg(&mut self, reg: u8) -> Result<u8, D::Error> {
        let mut value = [0u8; 1];
        self.read_bytes(reg, &mut value)?;
        Ok(value[0])
    }

    fn read_bytes(&mut self, reg: u8, out: &mut [u8]) -> Result<(), D::Error> {
        self.transactions = self.transactions.saturating_add(1);
        self.i2c.write_read(self.addr, &[reg], out)
    }
}

/// A parameter set is selected by the high nibble of CAL4_H; the engine needs
/// both halves written before it will accept new parameters.
const CAL_SET_FIRST: u8 = 0x01;
const CAL_SET_SECOND: u8 = 0x02;

impl<D: I2c> MotionSource for Qmi8658<D> {
    /// No delay, so no switch. Already being in `wanted` still answers, because that part is
    /// true of a part that was brought up here.
    fn configure_for(
        &mut self,
        wanted: Option<MotionConfig>,
    ) -> Result<Option<MotionConfig>, MotionConfig> {
        match wanted {
            None => Ok(None),
            Some(wanted) if self.config == wanted && self.initialized => Ok(Some(wanted)),
            Some(wanted) => Err(wanted),
        }
    }

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

    // `configure_for` is not overridden here. This driver holds no delay, so it cannot
    // complete a switch, and the trait has no default to fall back on precisely so that a
    // driver asked to switch and unable to would have to say so. `RecoverableQmi` is what
    // production configures through, because it carries the delay this one lacks.
}

impl<D: I2c> Qmi8658<D> {
    /// Enters `wanted` without the soft reset a full bring-up needs.
    ///
    /// The reset gives a part that has never run a clean start; it is not part of changing
    /// configuration. `Self::configure` already quiets the part before writing and restores
    /// it afterwards, so resetting on top of that only adds a window in which the part is
    /// deaf. Reconfiguring a running part stays destructive either way — the recognizer's
    /// baseline does not survive a change of filtering or scale — but the deafness does not
    /// have to be part of the price.
    pub fn enter(
        &mut self,
        wanted: MotionConfig,
        delay: &mut impl DelayNs,
    ) -> Result<MotionConfig, MotionConfig> {
        // Being in `wanted` is only the answer if the part is actually running in it. `config`
        // is the target the driver last wrote, so before the first bring-up it already reads as
        // `PolledRegisters` while nothing has been brought up at all — short-circuiting there
        // leaves a silent part that every later poll reads as "no new data".
        if self.config == wanted && self.initialized {
            return Ok(wanted);
        }
        let Some(register_set) = config_for(wanted) else {
            return Err(wanted);
        };
        if !self.initialized {
            // Never came up, so there is no baseline to preserve and no state to lose: a full
            // bring-up is the honest way to reach a configuration the part never held.
            return self
                .set_config(wanted, delay)
                .map_err(|_| wanted)
                .map(|()| wanted);
        }
        self.configure(&register_set, delay).map_err(|_| wanted)?;
        self.config = wanted;
        self.scale = register_set.scale;
        // The recognizer's peaks, yaw integral and stillness edge all describe the stream
        // that was running before the switch, so carrying them across makes the core read the
        // gap as motion.
        self.last_sample = None;
        self.last_sample_ms = None;
        self.yaw_deg_x10 = 0;
        self.last_status = 0;
        Ok(wanted)
    }
}

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
    /// This is the wrapper production configures through: it already holds the delay the
    /// inner driver refuses to switch without.
    fn configure_for(
        &mut self,
        wanted: Option<MotionConfig>,
    ) -> Result<Option<MotionConfig>, MotionConfig> {
        match wanted {
            None => Ok(None),
            Some(wanted) => self.inner.enter(wanted, &mut self.delay).map(Some),
        }
    }

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

impl<D: I2c> crate::hardware::SelfCheck for Qmi8658<D> {
    fn name(&self) -> &'static str {
        "motion"
    }

    fn id(&self) -> Option<crate::hardware::Address> {
        Some(crate::hardware::Address::new(self.addr))
    }

    fn check(&mut self) -> crate::hardware::Status {
        use crate::hardware::Status;
        match self.validate_id() {
            Err(Qmi8658Error::Bus(_)) => Status::silent(),
            // A part that answers with the wrong ID is present and broken: degraded, not absent.
            Err(Qmi8658Error::Identity(_)) => Status::refused("not a QMI8658"),
            Err(_) => Status::refused("bring-up failed"),
            Ok(()) if self.initialized => Status::answered(),
            Ok(()) => Status::refused("not configured"),
        }
    }
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
                        self.state.registers[register(REG_RESET_FLAG)] = RESET_FLAG_READY;
                    }
                    if *reg == REG_CTRL9 && *value != CTRL_CMD_ACK && self.state.completes_commands
                    {
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
    fn the_supported_list_is_exactly_what_can_be_configured() {
        for config in SUPPORTED_CONFIGS {
            assert!(
                config_for(*config).is_some(),
                "{config:?} is offered as configurable and has no encoding"
            );
        }
        // Everything else on the ladder is refused, so the two lists cannot drift apart
        // silently.
        for config in [MotionConfig::WakeDetector, MotionConfig::Fifo] {
            assert!(
                !SUPPORTED_CONFIGS.contains(&config),
                "{config:?} is not configurable and must not be walked"
            );
        }
    }

    #[test]
    fn a_poll_reports_the_bus_traffic_it_actually_caused() {
        let mut driver = driver(MockI2c::new());
        settle(&mut driver);

        let before = driver.transactions();
        let _ = driver.read_sample(1_000);
        let first_poll = driver.transactions() - before;

        let _ = driver.read_sample(1_020);
        let second_poll = driver.transactions() - before - first_poll;

        assert!(
            first_poll > 0,
            "a poll that reaches the bus must be counted"
        );
        assert_eq!(
            first_poll, second_poll,
            "the same poll costs the same twice over, so a cost report can divide by the cadence"
        );
    }

    #[test]
    fn bring_up_traffic_is_counted_separately_from_polling() {
        // Per-read only: a cost report divides the delta over a window by the window, so a
        // counter shared with the polls would show up as a steady rate.
        let mut driver = driver(MockI2c::new());
        let before = driver.transactions();
        settle(&mut driver);
        let after_bring_up = driver.transactions() - before;
        assert!(after_bring_up > 0, "bringing the part up talks to it");

        let polls = 4u32;
        for index in 0..polls {
            let _ = driver.read_sample(u64::from(index) * 20);
        }
        let polling = driver.transactions() - before - after_bring_up;
        assert_eq!(polling % polls, 0, "four identical polls");
    }

    #[test]
    fn a_configuration_this_driver_cannot_reach_is_refused_before_anything_is_written() {
        for config in [MotionConfig::WakeDetector, MotionConfig::Fifo] {
            let mut driver = driver(MockI2c::new());
            let mut delay = NoDelay;
            assert!(
                matches!(
                    driver.init_at(config, &mut delay),
                    Err(Qmi8658Error::UnsupportedConfig(wanted)) if wanted == config
                ),
                "{config:?} must be refused, not approximated"
            );
            assert!(
                driver.i2c.state.history.is_empty(),
                "{config:?} must not reach the bus"
            );
            assert_eq!(driver.config(), MotionConfig::PolledRegisters);
        }
    }

    #[test]
    fn the_registers_configuration_reproduces_the_bring_up_configuration_byte_for_byte() {
        let Some(config) = config_for(MotionConfig::PolledRegisters) else {
            panic!("PolledRegisters is the configuration the board validated");
        };
        assert_eq!(config.ctrl2, CTRL2_VALUE);
        assert_eq!(config.ctrl3, CTRL3_VALUE);
        assert_eq!(config.ctrl5, CTRL5_VALUE);
        assert_eq!(config.ctrl7, CTRL7_VALUE);
        assert_eq!(config.scale, SCALE);
    }

    #[test]
    fn the_filters_configuration_keeps_the_axes_and_the_rate_and_adds_the_filters() {
        let plain = config_for(MotionConfig::PolledRegisters).expect("PolledRegisters");
        let filtered = config_for(MotionConfig::FilteredRegisters).expect("FilteredRegisters");

        assert_eq!(filtered.ctrl2, plain.ctrl2);
        assert_eq!(filtered.ctrl3, plain.ctrl3);
        assert_eq!(filtered.odr_hz_x10, plain.odr_hz_x10);
        assert_eq!(filtered.scale, plain.scale);
        assert_eq!(plain.ctrl5 & 1, 0, "accelerometer filter off below");
        assert_eq!(
            filtered.ctrl5 & 1,
            1,
            "accelerometer filter on at FilteredRegisters"
        );
        assert_eq!(
            filtered.ctrl5 & 1 << 4,
            1 << 4,
            "gyro filter on at FilteredRegisters"
        );
    }

    #[test]
    fn entering_a_configuration_leaves_a_part_that_answers_polls() {
        // The assertion that matters is not which registers were written but whether the part
        // still produces afterwards. A switch that writes the table and returns before the
        // part starts is silent, and every poll after it reads "no new data" — which looks
        // like a configuration that costs almost nothing rather than one that had not landed.
        let mut driver = driver(MockI2c::new());
        settle(&mut driver);
        let mut delay = NoDelay;

        driver
            .enter(MotionConfig::FilteredRegisters, &mut delay)
            .expect("FilteredRegisters is reachable");

        // The mock reports data ready continuously, so reaching the read at all is the
        // assertion: `enter` returning means the part was polled, not merely written to.
        assert!(
            driver.read_sample(1_000).is_ok(),
            "a part left quiet by a configuration switch reads as free, not as misconfigured"
        );
    }

    #[test]
    fn entering_a_configuration_does_not_soft_reset_the_part() {
        // The distinction `enter` exists for. `set_config` resets because it must also bring
        // up a part that never ran; a change of configuration does not need that, and the
        // reset is a window in which the part is deaf and its identity is re-established.
        let mut driver = driver(MockI2c::new());
        settle(&mut driver);
        let resets_before = driver.i2c.state.control_writes(REG_RESET).len();

        let mut delay = NoDelay;
        driver
            .enter(MotionConfig::FilteredRegisters, &mut delay)
            .expect("FilteredRegisters is reachable");

        assert_eq!(
            driver.i2c.state.control_writes(REG_RESET).len(),
            resets_before,
            "a configuration change paid for a soft reset the part did not need"
        );
        assert_eq!(
            driver.i2c.state.registers[register(REG_CTRL5)],
            0x11,
            "and the configuration it did apply still has to land"
        );
        assert_eq!(driver.config(), MotionConfig::FilteredRegisters);
    }

    #[test]
    fn entering_a_configuration_drops_the_state_that_described_the_stream_before_it() {
        // Same reason `init_at` clears it, reached the other way: the recognizer's baseline
        // describes the stream that was running, so carrying it across makes the core read the
        // gap as motion.
        let mut driver = driver(MockI2c::new());
        settle(&mut driver);
        driver.last_sample = Some(MotionSample::from_raw([1, 2, 3], [4, 5, 6], SCALE, 0, 0));
        driver.last_sample_ms = Some(1_000);
        driver.yaw_deg_x10 = 900;
        driver.last_status = STATUS1_NO_MOTION;

        let mut delay = NoDelay;
        driver
            .enter(MotionConfig::FilteredRegisters, &mut delay)
            .expect("FilteredRegisters is reachable");

        assert!(driver.last_sample.is_none());
        assert_eq!(driver.last_sample_ms, None);
        assert_eq!(driver.yaw_deg_x10, 0);
        assert_eq!(driver.last_status, 0);
    }

    #[test]
    fn entering_the_configuration_the_part_is_already_in_writes_nothing() {
        let mut driver = driver(MockI2c::new());
        settle(&mut driver);
        let writes_before = writes(&driver.i2c.state.history).len();

        let mut delay = NoDelay;
        driver
            .enter(MotionConfig::PolledRegisters, &mut delay)
            .expect("already in it");

        assert_eq!(
            writes(&driver.i2c.state.history).len(),
            writes_before,
            "a demand that resolves back to the boot configuration must not cost a switch"
        );
    }

    #[test]
    fn entering_the_boot_configuration_of_a_part_that_never_ran_still_brings_it_up() {
        // The target the driver was constructed with is already `PolledRegisters` before
        // anything has been written. Reading that as "already there" returns success without
        // bringing the part up, and the first grid of a measurement then reads as a
        // configuration that costs almost nothing.
        let mut driver = driver(MockI2c::new());
        assert!(
            !driver.initialized,
            "the driver starts before the part has been brought up"
        );

        let mut delay = NoDelay;
        driver
            .enter(MotionConfig::PolledRegisters, &mut delay)
            .expect("the configuration it was constructed for is reachable");

        assert!(
            driver.initialized,
            "matching the target configuration is not the same as running in it"
        );
        assert!(driver.read_sample(1_000).is_ok());
    }

    #[test]
    fn switching_configuration_takes_effect_on_the_part_and_in_what_the_driver_reports() {
        let mut driver = driver(MockI2c::new());
        settle(&mut driver);
        let mut delay = NoDelay;
        driver
            .set_config(MotionConfig::FilteredRegisters, &mut delay)
            .expect("FilteredRegisters is reachable");

        assert_eq!(driver.config(), MotionConfig::FilteredRegisters);
        assert_eq!(driver.i2c.state.registers[register(REG_CTRL2)], CTRL2_VALUE);
        assert_eq!(driver.i2c.state.registers[register(REG_CTRL5)], 0x11);
    }

    #[test]
    fn a_switch_drops_the_state_that_described_the_stream_before_it() {
        let mut driver = driver(MockI2c::new());
        settle(&mut driver);
        driver.last_sample = Some(MotionSample::from_raw([1, 2, 3], [4, 5, 6], SCALE, 0, 0));
        driver.last_sample_ms = Some(1_000);
        driver.yaw_deg_x10 = 900;
        driver.last_status = STATUS1_NO_MOTION;

        let mut delay = NoDelay;
        driver
            .set_config(MotionConfig::FilteredRegisters, &mut delay)
            .expect("FilteredRegisters is reachable");

        // The core's recognizer compares each sample against the last one and reads the
        // gap as a peak, so carrying any of this across the switch turns a configuration
        // change into a knock.
        assert_eq!(driver.last_sample, None);
        assert_eq!(driver.last_sample_ms, None);
        assert_eq!(driver.yaw_deg_x10, 0);
        assert_eq!(driver.last_status, 0);
    }

    #[test]
    fn an_engine_window_follows_the_rate_the_part_was_told_to_run_at() {
        // The window counts samples the part actually produces, so a part running four
        // times slower needs a quarter as many: the engine reports stillness four times as
        // often as it was asked to otherwise.
        assert_eq!(odr_samples(NO_MOTION_WINDOW_MS, ODR_HZ_X10), 72);
        assert_eq!(odr_samples(NO_MOTION_WINDOW_MS, ODR_HZ_X10 / 4), 18);
        assert_eq!(odr_samples(NO_MOTION_WINDOW_MS, ODR_HZ_X10 * 2), 143);
    }

    #[test]
    fn a_recovery_after_a_failed_boot_lands_on_the_demanded_configuration() {
        let mut i2c = MockI2c::new();
        i2c.state.completes_reset = false;
        let mut driver = driver(i2c);
        let mut delay = NoDelay;
        assert!(
            driver
                .init_at(MotionConfig::FilteredRegisters, &mut delay)
                .is_err()
        );

        driver.i2c.state.completes_reset = true;
        driver
            .init_at(MotionConfig::FilteredRegisters, &mut delay)
            .expect("the part answers this time");

        assert_eq!(driver.config(), MotionConfig::FilteredRegisters);
    }

    #[test]
    fn the_accelerometer_low_pass_is_disabled_and_gyro_filter_is_not() {
        let mut driver = driver(MockI2c::new());
        settle(&mut driver);
        let ctrl5 = driver.i2c.state.registers[register(REG_CTRL5)];
        assert_eq!(ctrl5, CTRL5_VALUE);
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
        assert_eq!(
            state.registers[register(REG_STATUSINT)] & STATUSINT_CMD_DONE,
            0
        );
        assert_eq!(state.registers[register(REG_CAL1_L + 7)], CAL_SET_SECOND);
        // Pinning the whole layout rather than a few interesting bytes is the
        // point: the tables interleave unrelated fields, so a plausible-looking
        // subset hides a misfiled threshold.
        assert_eq!(
            state.calibrations,
            vec![
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
                    odr_samples(ANY_MOTION_WINDOW_MS, ODR_HZ_X10) as u8,
                    odr_samples(NO_MOTION_WINDOW_MS, ODR_HZ_X10) as u8,
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
    fn an_engine_flag_still_publishes_when_the_data_path_has_not_moved() {
        let mut driver = driver(MockI2c::new());
        settle(&mut driver);
        let _ = driver.read_sample(20).expect("first frame");
        driver.i2c.state.registers[register(REG_STATUS0)] = 0;
        driver.i2c.state.registers[register(REG_STATUS1)] = STATUS1_NO_MOTION;
        let reading = driver.read_sample(40).expect("read").expect("engine event");
        assert_eq!(reading.hardware.latest(), Some(MotionEvent::Still));
        assert_eq!(
            reading.sample.status, STATUS1_NO_MOTION,
            "an event published without a fresh data path still has to report the status it was decoded from"
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

    #[test]
    fn a_self_check_names_the_part_and_the_address_it_answers_on() {
        use crate::hardware::{Address, SelfCheck};

        let motion = Qmi8658::new(MockI2c::new(), QMI8658_I2C_ADDR);
        assert_eq!(motion.name(), "motion");
        assert_eq!(motion.id(), Some(Address::new(QMI8658_I2C_ADDR)));
    }

    #[test]
    fn a_self_check_asks_the_identity_then_the_drivers_own_state() {
        use crate::hardware::{SelfCheck, Status};

        let mut motion = Qmi8658::new(MockI2c::new(), QMI8658_I2C_ADDR);
        assert_eq!(motion.check(), Status::refused("not configured"));

        motion.initialized = true;
        assert_eq!(motion.check(), Status::answered());

        motion.i2c.state.registers[register(REG_WHO_AM_I)] = 0x00;
        assert_eq!(motion.check(), Status::refused("not a QMI8658"));
    }
}
