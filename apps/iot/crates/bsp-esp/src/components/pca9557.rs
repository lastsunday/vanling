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
        self.i2c
            .write_read(self.addr, &[PCA9557_REG_OUTPUT], &mut value)?;
        Ok(value[0])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::components::mock_i2c::MockI2c;

    type Mock = MockI2c<4>;

    #[test]
    fn set_output_bit_takes_the_bit_number_not_a_mask() {
        let mut expander = Pca9557::new(Mock::new(|_| {}), 0x19);
        expander.init(0x04, 0xf8).expect("init");
        let init_writes = expander.i2c.trace.clone();

        // Pin 1 is the amplifier enable, and the bit *number* must reach the
        // read-modify-write.
        expander.set_output_bit(1, true).expect("set");

        assert_eq!(expander.output().expect("read back"), 0x06);
        let mut history = init_writes;
        history.push((0x01, 0x06));
        assert_eq!(&expander.i2c.trace, &history);
    }

    #[test]
    fn a_mask_passed_as_a_bit_number_leaves_the_amplifier_unmoved() {
        let mut expander = Pca9557::new(Mock::new(|_| {}), 0x19);
        expander.init(0x04, 0xf8).expect("init");
        let init_writes = expander.i2c.trace.clone();

        // A mask read as a bit number is a silent no-op: the short circuit sees
        // an already-set bit and skips the write.
        expander.set_output_bit(1 << 1, true).expect("set");

        assert_eq!(expander.output().expect("read back"), 0x04);
        assert_eq!(
            &expander.i2c.trace, &init_writes,
            "an unchanged value must not touch the bus"
        );
    }
}
