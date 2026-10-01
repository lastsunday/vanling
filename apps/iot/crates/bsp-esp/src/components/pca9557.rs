use embedded_hal::i2c::I2c;

pub const PCA9557_REG_OUTPUT: u8 = 0x01;
pub const PCA9557_REG_INPUT: u8 = 0x00;
pub const PCA9557_REG_CONFIG: u8 = 0x03;

/// PCA9557 8-bit I2C GPIO expander, parameterized over any embedded-hal I2C
/// instance so the board can hand in a shared-bus device.
pub struct Pca9557<D: I2c> {
    i2c: D,
    addr: u8,
}

impl<D: I2c> Pca9557<D> {
    pub fn new(i2c: D, addr: u8) -> Self {
        Self { i2c, addr }
    }

    /// Bring the expander up: set the output register before releasing the
    /// pins (so the CS line stays high during board bring-up) then mark the
    /// pins as outputs.
    pub fn init(&mut self, output: u8, config: u8) -> Result<(), D::Error> {
        self.i2c.write(self.addr, &[PCA9557_REG_OUTPUT, output])?;
        self.i2c.write(self.addr, &[PCA9557_REG_CONFIG, config])?;
        Ok(())
    }

    /// Rewrite the output register, e.g. to drop the LCD chip-select bit.
    pub fn set_output(&mut self, output: u8) -> Result<(), D::Error> {
        self.i2c.write(self.addr, &[PCA9557_REG_OUTPUT, output])?;
        Ok(())
    }

    /// Sets or clears one output bit and leaves the rest as they are.
    ///
    /// A read-modify-write of the *output* register, which the part allows
    /// because a pin configured as an output reads back the latch rather than the
    /// pad — so the value the register is holding is what comes back. It is the
    /// only way to reach a line another part of the bring-up also owns: writing
    /// the whole register would clear the panel's own bits along with this one.
    pub fn set_output_bit(&mut self, bit: u8, high: bool) -> Result<(), D::Error> {
        let mask = 1 << bit;
        let output = self.output()?;
        let updated = if high { output | mask } else { output & !mask };
        if updated == output {
            return Ok(());
        }
        self.set_output(updated)
    }

    /// The output register as the part holds it.
    pub fn output(&mut self) -> Result<u8, D::Error> {
        let mut value = [0_u8; 1];
        self.i2c.write(self.addr, &[PCA9557_REG_OUTPUT])?;
        self.i2c.read(self.addr, &mut value)?;
        Ok(value[0])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec::Vec;
    use embedded_hal::i2c::{ErrorType, Operation};

    struct MockState {
        registers: [u8; 4],
        pointer: u8,
        /// Register writes in order, so a call that short-circuits on an
        /// unchanged value leaves no trace.
        history: Vec<(u8, u8)>,
    }

    impl MockState {
        fn new() -> Self {
            Self {
                registers: [0; 4],
                pointer: 0,
                history: Vec::new(),
            }
        }

        fn apply(&mut self, write: &[u8]) {
            match write {
                [reg] => self.pointer = *reg,
                [reg, value] => {
                    self.registers[*reg as usize] = *value;
                    self.history.push((*reg, *value));
                }
                _ => {}
            }
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
    }

    impl ErrorType for MockI2c {
        type Error = core::convert::Infallible;
    }

    impl I2c for MockI2c {
        fn read(&mut self, _address: u8, read: &mut [u8]) -> Result<(), Self::Error> {
            for byte in read.iter_mut() {
                *byte = self.state.registers[self.state.pointer as usize];
            }
            Ok(())
        }

        fn write(&mut self, _address: u8, write: &[u8]) -> Result<(), Self::Error> {
            self.state.apply(write);
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
                    Operation::Write(write) => self.write(address, write)?,
                }
            }
            Ok(())
        }
    }

    #[test]
    fn set_output_bit_takes_the_bit_number_not_a_mask() {
        let mut expander = Pca9557::new(MockI2c::new(), 0x19);
        expander.init(0x04, 0xf8).expect("init");
        let init_writes = expander.i2c.state.history.clone();

        // The API wants the bit *number*: pin 1 is the amplifier enable, and
        // `1_u8` alone (not `1 << 1`) must reach the read-modify-write.
        expander.set_output_bit(1, true).expect("set");

        assert_eq!(expander.output().expect("read back"), 0x06);
        let mut history = init_writes;
        history.push((0x01, 0x06));
        assert_eq!(&expander.i2c.state.history, &history);
    }

    #[test]
    fn a_mask_passed_as_a_bit_number_leaves_the_amplifier_unmoved() {
        let mut expander = Pca9557::new(MockI2c::new(), 0x19);
        expander.init(0x04, 0xf8).expect("init");
        let init_writes = expander.i2c.state.history.clone();

        // The historical caller passed `1 << 1` where the API wants the pin
        // number, so only the already-set DVP_PWDN bit was re-asserted and the
        // no-op short circuit skipped the write entirely.
        expander.set_output_bit(1 << 1, true).expect("set");

        assert_eq!(expander.output().expect("read back"), 0x04);
        assert_eq!(
            &expander.i2c.state.history, &init_writes,
            "an unchanged value must not touch the bus"
        );
    }
}
