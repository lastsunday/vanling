use embedded_hal::i2c::I2c;

/// One entry in a register bring-up sequence: write `value` across `reg`, or
/// merge it into just the bits `mask` selects.
///
/// A masked write is how a shared register stays intact — a codec's serial-port
/// register carries the frame, the word width and the serial enable at once, and
/// its gain register carries the PGA enable beside its gain. A whole-register
/// write cannot express "change my field, leave the rest", and a codec whose
/// bring-up has already set those other fields would lose them.
///
/// Shared by every register-driven part rather than living per driver: the mask
/// arithmetic and the write/read sequencing are the part's protocol, not its
/// register map, and the tables stay per part because only they are load-bearing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Step {
    pub reg: u8,
    pub mask: u8,
    pub value: u8,
}

pub const fn write(reg: u8, value: u8) -> Step {
    Step {
        reg,
        mask: 0,
        value,
    }
}

pub const fn merge(reg: u8, mask: u8, value: u8) -> Step {
    Step { reg, mask, value }
}

/// Replays `stages` in order, each a run of steps that must all land.
pub fn run<D: I2c>(i2c: &mut D, addr: u8, stages: &[&[Step]]) -> Result<(), D::Error> {
    for stage in stages {
        for step in *stage {
            if step.mask == 0 {
                i2c.write(addr, &[step.reg, step.value])?;
            } else {
                update(i2c, addr, *step)?;
            }
        }
    }
    Ok(())
}

/// Read-modify-writes one masked step, leaving every bit outside `mask` as the
/// part holds it.
pub fn update<D: I2c>(i2c: &mut D, addr: u8, step: Step) -> Result<(), D::Error> {
    let current = read_reg(i2c, addr, step.reg)?;
    let merged = (current & !step.mask) | (step.value & step.mask);
    i2c.write(addr, &[step.reg, merged])
}

pub fn read_reg<D: I2c>(i2c: &mut D, addr: u8, reg: u8) -> Result<u8, D::Error> {
    let mut value = [0u8; 1];
    i2c.write_read(addr, &[reg], &mut value)?;
    Ok(value[0])
}
