//! TI TMP117 as the [`AmbientSensor`] (ADR 0002). Default configuration: continuous conversion, 8x
//! averaging, one result per second; the driver only reads it.

use embedded_hal_async::i2c::I2c;

use crate::hal::{AmbientSensor, HalError};

/// ADD0 tied to GND.
pub const ADDRESS: u8 = 0x48;
const REG_TEMP: u8 = 0x00;
const REG_DEVICE_ID: u8 = 0x0F;
/// Power-on value of the temperature register, before the first conversion completes.
const TEMP_RESET: i16 = i16::MIN;

/// The device ID is checked before the first reading (and again after a bus error), so a missing or
/// wrong part reads as an error rather than as a plausible temperature.
pub struct Tmp117<I> {
    i2c: I,
    address: u8,
    checked: bool,
}

impl<I: I2c> Tmp117<I> {
    pub fn new(i2c: I, address: u8) -> Self {
        Self {
            i2c,
            address,
            checked: false,
        }
    }

    async fn reg(&mut self, r: u8) -> Result<u16, HalError> {
        let mut b = [0u8; 2];
        self.i2c
            .write_read(self.address, &[r], &mut b)
            .await
            .map_err(|_| HalError::Bus)?;
        Ok(u16::from_be_bytes(b))
    }
}

impl<I: I2c> AmbientSensor for Tmp117<I> {
    async fn read_c(&mut self) -> Result<f64, HalError> {
        if !self.checked {
            if self.reg(REG_DEVICE_ID).await? & 0x0FFF != 0x0117 {
                return Err(HalError::Absent);
            }
            self.checked = true;
        }
        let raw = self.reg(REG_TEMP).await.inspect_err(|_| self.checked = false)? as i16;
        if raw == TEMP_RESET {
            return Err(HalError::NotReady);
        }
        Ok(f64::from(raw) * 7.8125e-3)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drivers::mock::{I2cRegs, block_on};

    #[test]
    fn reads_datasheet_codes() {
        // datasheet table 7-1: 0x0C80 = 25C, 0xFF80 = -1C
        let i2c = I2cRegs::new(ADDRESS, &[(REG_DEVICE_ID, 0x1117), (REG_TEMP, 0x0C80)]);
        let mut t = Tmp117::new(i2c, ADDRESS);
        assert_eq!(block_on(t.read_c()), Ok(25.0));
        t.i2c.set(REG_TEMP, 0xFF80);
        assert_eq!(block_on(t.read_c()), Ok(-1.0));
        t.i2c.set(REG_TEMP, 0x8000);
        assert_eq!(block_on(t.read_c()), Err(HalError::NotReady));
    }

    #[test]
    fn wrong_part_or_no_ack() {
        let wrong = I2cRegs::new(ADDRESS, &[(REG_DEVICE_ID, 0x0190)]);
        assert_eq!(block_on(Tmp117::new(wrong, ADDRESS).read_c()), Err(HalError::Absent));
        let elsewhere = I2cRegs::new(0x49, &[(REG_DEVICE_ID, 0x0117)]);
        assert_eq!(block_on(Tmp117::new(elsewhere, ADDRESS).read_c()), Err(HalError::Bus));
    }
}
