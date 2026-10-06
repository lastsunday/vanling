//! A register-backed I²C bus for driver tests: `N` bytes of registers, a write
//! pointer, and a trace of every write in order.
//!
//! Shared because it is the same bus three times over and the driver tests all
//! read it the same way. A driver supplies its own `seed` and, where the test
//! needs to place a fault at one specific write, `fail_at`.

use alloc::vec::Vec;
use embedded_hal::i2c::{ErrorKind, ErrorType, I2c, NoAcknowledgeSource, Operation};

/// The fault the mock bus injects, in the vocabulary the embedded-hal trait
/// defines rather than a bespoke type, so a test asserting on it reads the same
/// as a real peripheral failure.
pub const NACK: ErrorKind = ErrorKind::NoAcknowledge(NoAcknowledgeSource::Data);

pub struct MockI2c<const N: usize> {
    pub registers: [u8; N],
    pointer: u8,
    /// Every write in order, as (register, value). Masked writes record the
    /// merged byte the bus actually saw, so a wrong mask shows up as a wrong
    /// trace rather than as nothing at all.
    pub trace: Vec<(u8, u8)>,
    /// Index into `trace` at which the bus starts failing.
    pub fail_at: Option<usize>,
}

impl<const N: usize> MockI2c<N> {
    pub fn new(seed: impl FnOnce(&mut [u8; N])) -> Self {
        let mut registers = [0u8; N];
        seed(&mut registers);
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

    pub fn writes_of(&self, reg: u8) -> Vec<u8> {
        self.trace
            .iter()
            .filter(|&&(written, _)| written == reg)
            .map(|&(_, value)| value)
            .collect()
    }
}

impl<const N: usize> ErrorType for MockI2c<N> {
    type Error = ErrorKind;
}

impl<const N: usize> I2c for MockI2c<N> {
    fn read(&mut self, _address: u8, read: &mut [u8]) -> Result<(), Self::Error> {
        for (offset, byte) in read.iter_mut().enumerate() {
            *byte = self.registers[(self.pointer as usize + offset).min(N - 1)];
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
