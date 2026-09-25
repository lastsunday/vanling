use embedded_hal::i2c::I2c;

pub const PCA9557_REG_OUTPUT: u8 = 0x01;
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
}
