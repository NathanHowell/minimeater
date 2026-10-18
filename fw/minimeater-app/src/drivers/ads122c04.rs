//! TI ADS122C04 24-bit ADC as the [`ProbeAdc`] (ADR 0002, amended): ratiometric against REFP/REFN,
//! PGA bypassed, 4 single-ended inputs, single-shot 20 SPS. At 20 SPS the filter rejects 50 and 60 Hz
//! together, so there is no setting for it. I2C only: the conversion's end is read as register 2's DRDY bit (the
//! chip's DRDY pin is not wired: its GPIO went to a probe jack's ring, minimeat-098). Generic over `embedded-hal-async`,
//! so it is host-tested against a scripted bus.

use embedded_hal_async::delay::DelayNs;
use embedded_hal_async::i2c::I2c;

use crate::hal::{HalError, PROBE_JACKS, ProbeAdc};

/// A1 = A0 = DGND.
pub const ADDRESS: u8 = 0x40;

const RESET: u8 = 0x06;
const START: u8 = 0x08;
const RDATA: u8 = 0x10;
/// RREG for register 2: the byte whose top bit is DRDY, set while a conversion's result waits.
const RREG_CONFIG2: u8 = 0x20 | (2 << 2);
const DRDY: u8 = 0x80;
/// WREG for register `r`: one register per command byte, followed by its data byte.
const fn wreg(r: u8) -> u8 {
    0x40 | (r << 2)
}

/// Register 0: MUX[7:4], GAIN[3:1] = 1, PGA_BYPASS = 1.
const fn reg0(mux: u8) -> u8 {
    (mux << 4) | 0x01
}
/// MUX: AINx / AVSS for jack x.
const MUX_AIN0_AVSS: u8 = 0b1000;
/// MUX: inputs shorted to (AVDD + AVSS) / 2, the offset check.
const MUX_SHORTED: u8 = 0b1110;
/// Register 1: DR = 20 SPS, normal mode, single shot, VREF = external REFP/REFN; TS (bit 0) per conversion.
/// In temperature-sensor mode the chip uses its internal reference whatever VREF says.
const REG1: u8 = 0x02;
const REG1_TS: u8 = 0x03;
/// Register 2: no CRC, no counter, burn-out sources and IDACs off.
const REG2: u8 = 0x00;
/// Register 3: IDACs not routed.
const REG3: u8 = 0x00;

/// A 20 SPS conversion takes about 51 ms; anything past this is a fault. DRDY is read every `POLL_MS`.
const CONVERSION_TIMEOUT_MS: u32 = 200;
const POLL_MS: u32 = 5;

/// The ADC is reset and configured on first use and again after any failed conversion, so a chip
/// that is missing at boot reports [`HalError::Timeout`] or [`HalError::Bus`] instead of stopping the firmware.
pub struct Ads122c04<I, T> {
    i2c: I,
    address: u8,
    delay: T,
    configured: bool,
}

impl<I: I2c, T: DelayNs> Ads122c04<I, T> {
    pub fn new(i2c: I, address: u8, delay: T) -> Self {
        Self {
            i2c,
            address,
            delay,
            configured: false,
        }
    }

    /// Resets the ADC and writes the fixed configuration (no wait is needed after RESET).
    async fn configure(&mut self) -> Result<(), HalError> {
        self.i2c
            .write(self.address, &[RESET])
            .await
            .map_err(|_| HalError::Bus)?;
        let cfg = [
            wreg(0),
            reg0(MUX_AIN0_AVSS),
            wreg(1),
            REG1,
            wreg(2),
            REG2,
            wreg(3),
            REG3,
        ];
        self.i2c.write(self.address, &cfg).await.map_err(|_| HalError::Bus)?;
        self.configured = true;
        Ok(())
    }

    async fn convert(&mut self, mux: u8, temp_sensor: bool) -> Result<i32, HalError> {
        if !self.configured {
            self.configure().await?;
        }
        let r = self.convert_once(mux, temp_sensor).await;
        self.configured = r.is_ok();
        r
    }

    /// One single-shot conversion; returns the signed 24-bit code.
    async fn convert_once(&mut self, mux: u8, temp_sensor: bool) -> Result<i32, HalError> {
        let r1 = if temp_sensor { REG1_TS } else { REG1 };
        self.i2c
            .write(self.address, &[wreg(0), reg0(mux), wreg(1), r1])
            .await
            .map_err(|_| HalError::Bus)?;
        self.i2c
            .write(self.address, &[START])
            .await
            .map_err(|_| HalError::Bus)?;
        let mut waited = 0;
        loop {
            self.delay.delay_ms(POLL_MS).await;
            waited += POLL_MS;
            let mut r = [0u8; 1];
            self.i2c
                .write_read(self.address, &[RREG_CONFIG2], &mut r)
                .await
                .map_err(|_| HalError::Bus)?;
            if r[0] & DRDY != 0 {
                break;
            }
            if waited >= CONVERSION_TIMEOUT_MS {
                return Err(HalError::Timeout);
            }
        }
        let mut b = [0u8; 3];
        self.i2c
            .write_read(self.address, &[RDATA], &mut b)
            .await
            .map_err(|_| HalError::Bus)?;
        // sign-extend 24 -> 32 bits
        Ok(i32::from_be_bytes([b[0], b[1], b[2], 0]) >> 8)
    }
}

/// Full-scale code with gain 1: the input equals VREF at 2^23.
const FULL_SCALE: f64 = 8_388_608.0;

impl<I: I2c, T: DelayNs> ProbeAdc for Ads122c04<I, T> {
    async fn ratio(&mut self, jack: usize) -> Result<f64, HalError> {
        assert!(jack < PROBE_JACKS);
        Ok(f64::from(self.convert(MUX_AIN0_AVSS + jack as u8, false).await?) / FULL_SCALE)
    }

    async fn offset(&mut self) -> Result<f64, HalError> {
        Ok(f64::from(self.convert(MUX_SHORTED, false).await?) / FULL_SCALE)
    }

    async fn die_temp_c(&mut self) -> Result<f64, HalError> {
        // 14-bit two's complement, left-justified in the 24-bit result, 0.03125 C/LSB
        let code = self.convert(MUX_AIN0_AVSS, true).await?;
        Ok(f64::from(code >> 10) * 0.03125)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drivers::mock::{I2cLog, NoDelay, block_on};

    /// Each conversion's script: DRDY already set when first read, then the three result bytes.
    fn adc(reads: &[[u8; 3]]) -> Ads122c04<I2cLog, NoDelay> {
        let bus = I2cLog::with_reads(ADDRESS, reads.iter().flat_map(|r| [vec![DRDY], r.to_vec()]));
        Ads122c04::new(bus, ADDRESS, NoDelay)
    }

    #[test]
    fn first_use_resets_then_configures() {
        let mut a = adc(&[[0, 0, 0], [0, 0, 0]]);
        block_on(a.ratio(0)).unwrap();
        assert_eq!(
            a.i2c.writes()[..2],
            [vec![RESET], vec![0x40, 0x81, 0x44, 0x02, 0x48, 0x00, 0x4C, 0x00]]
        );
        block_on(a.ratio(0)).unwrap();
        assert_eq!(a.i2c.writes().len(), 2 + 4 + 4, "configured once");
    }

    #[test]
    fn ratio_selects_jack_and_scales() {
        let mut a = adc(&[[0x40, 0x00, 0x00]]);
        assert_eq!(block_on(a.ratio(2)), Ok(0.5));
        // after init: WREG reg0 and reg1 (AIN2/AVSS, external reference, TS off), START, RREG reg 2 (DRDY), RDATA
        assert_eq!(
            a.i2c.writes()[2..],
            [
                vec![0x40, 0xA1, 0x44, 0x02],
                vec![START],
                vec![RREG_CONFIG2],
                vec![RDATA]
            ]
        );
    }

    #[test]
    fn offset_and_negative_codes() {
        let mut a = adc(&[[0xFF, 0xFF, 0xFF]]);
        assert_eq!(block_on(a.offset()), Ok(-1.0 / FULL_SCALE));
        assert_eq!(a.i2c.writes()[2], [0x40, 0xE1, 0x44, 0x02]);
    }

    #[test]
    fn die_temperature_datasheet_codes() {
        // datasheet: 0x0320 = 25C, 0x3CE0 = -25C... as 14-bit codes, left-justified in the 24-bit result
        let mut a = adc(&[[0x0C, 0x80, 0x00], [0xF3, 0x80, 0x00]]);
        assert_eq!(block_on(a.die_temp_c()), Ok(25.0));
        assert_eq!(block_on(a.die_temp_c()), Ok(-25.0));
        assert_eq!(a.i2c.writes()[2], [0x40, 0x81, 0x44, REG1_TS]);
    }

    #[test]
    fn waits_for_drdy_polling_register_2() {
        // not ready for two reads, then ready
        let bus = I2cLog::with_reads(
            ADDRESS,
            [vec![0x00], vec![0x00], vec![DRDY | 0x01], vec![0x40, 0x00, 0x00]],
        );
        let mut a = Ads122c04::new(bus, ADDRESS, NoDelay);
        assert_eq!(block_on(a.ratio(0)), Ok(0.5));
        let polls = a.i2c.writes().iter().filter(|w| **w == [RREG_CONFIG2]).count();
        assert_eq!(polls, 3);
    }

    #[test]
    fn a_conversion_that_never_finishes_times_out_and_reconfigures() {
        let never = (0..2 * CONVERSION_TIMEOUT_MS / POLL_MS).map(|_| vec![0x00]);
        let mut a = Ads122c04::new(I2cLog::with_reads(ADDRESS, never), ADDRESS, NoDelay);
        assert_eq!(block_on(a.ratio(0)), Err(HalError::Timeout));
        assert_eq!(block_on(a.ratio(0)), Err(HalError::Timeout));
        let resets = a.i2c.writes().iter().filter(|w| **w == [RESET]).count();
        assert_eq!(resets, 2, "a failed conversion forces a fresh reset");
    }

    #[test]
    fn wrong_address_is_a_bus_error() {
        let mut a = Ads122c04::new(I2cLog::with_reads(0x41, []), ADDRESS, NoDelay);
        assert_eq!(block_on(a.ratio(0)), Err(HalError::Bus));
    }
}
