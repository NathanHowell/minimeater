//! Scripted buses for driver tests.

extern crate std;

use std::collections::VecDeque;
use std::vec::Vec;

use embedded_hal_async::delay::DelayNs;
use embedded_hal_async::i2c::{self, I2c};

pub use embassy_futures::block_on;

/// I2C device that records every write (one entry per write operation) and answers reads from a
/// queue; other addresses NAK.
pub struct I2cLog {
    address: u8,
    writes: Vec<Vec<u8>>,
    reads: VecDeque<Vec<u8>>,
}

impl I2cLog {
    pub fn with_reads(address: u8, reads: impl IntoIterator<Item = Vec<u8>>) -> Self {
        Self {
            address,
            writes: Vec::new(),
            reads: reads.into_iter().collect(),
        }
    }

    pub fn writes(&self) -> &[Vec<u8>] {
        &self.writes
    }
}

/// Delays complete immediately.
pub struct NoDelay;

impl DelayNs for NoDelay {
    async fn delay_ns(&mut self, _ns: u32) {}
}

/// I2C device with 16-bit big-endian registers; other addresses NAK.
pub struct I2cRegs {
    address: u8,
    regs: Vec<(u8, u16)>,
}

impl I2cRegs {
    pub fn new(address: u8, regs: &[(u8, u16)]) -> Self {
        Self {
            address,
            regs: regs.to_vec(),
        }
    }

    pub fn set(&mut self, r: u8, v: u16) {
        self.regs.retain(|x| x.0 != r);
        self.regs.push((r, v));
    }
}

#[derive(Debug)]
pub struct Nak;

impl i2c::Error for Nak {
    fn kind(&self) -> i2c::ErrorKind {
        i2c::ErrorKind::NoAcknowledge(i2c::NoAcknowledgeSource::Address)
    }
}

impl i2c::ErrorType for I2cRegs {
    type Error = Nak;
}

impl I2c for I2cRegs {
    async fn transaction(&mut self, address: u8, ops: &mut [i2c::Operation<'_>]) -> Result<(), Nak> {
        if address != self.address {
            return Err(Nak);
        }
        let mut reg = None;
        for op in ops {
            match op {
                i2c::Operation::Write(b) => reg = Some(b[0]),
                i2c::Operation::Read(b) => {
                    let r = reg.expect("read before register pointer");
                    let v = self.regs.iter().find(|x| x.0 == r).map_or(0, |x| x.1);
                    b.copy_from_slice(&v.to_be_bytes());
                }
            }
        }
        Ok(())
    }
}

impl i2c::ErrorType for I2cLog {
    type Error = Nak;
}

impl I2c for I2cLog {
    async fn transaction(&mut self, address: u8, ops: &mut [i2c::Operation<'_>]) -> Result<(), Nak> {
        if address != self.address {
            return Err(Nak);
        }
        for op in ops {
            match op {
                i2c::Operation::Write(b) => self.writes.push(b.to_vec()),
                i2c::Operation::Read(b) => b.copy_from_slice(&self.reads.pop_front().expect("unscripted read")),
            }
        }
        Ok(())
    }
}
