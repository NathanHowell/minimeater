//! TI ADS1220 24-bit ADC as the [`ProbeAdc`] (ADR 0002): ratiometric against REFP0/REFN0, PGA
//! bypassed, 4 single-ended inputs, single-shot 20 SPS with 50/60 Hz rejection. SPI mode 1; DRDY on a
//! GPIO. Generic over `embedded-hal-async`, so it is host-tested against a scripted bus.

use embassy_futures::select::{Either, select};
use embedded_hal_async::delay::DelayNs;
use embedded_hal_async::digital::Wait;
use embedded_hal_async::spi::{Operation, SpiDevice};

use crate::hal::{HalError, PROBE_JACKS, ProbeAdc};

const RESET: u8 = 0x06;
const START: u8 = 0x08;
const RDATA: u8 = 0x10;
/// WREG starting at register `r`, `n` registers.
const fn wreg(r: u8, n: u8) -> u8 {
    0x40 | (r << 2) | (n - 1)
}

/// Register 0: MUX[7:4], GAIN[3:1] = 1, PGA_BYPASS = 1.
const fn reg0(mux: u8) -> u8 {
    (mux << 4) | 0x01
}
/// MUX: AINx / AVSS for jack x.
const MUX_AIN0_AVSS: u8 = 0b1000;
/// MUX: inputs shorted to (AVDD + AVSS) / 2, the offset check.
const MUX_SHORTED: u8 = 0b1110;
/// Register 1: DR = 20 SPS, normal mode, single shot, burn-out sources off; TS (bit 1) per conversion.
const REG1: u8 = 0x00;
const REG1_TS: u8 = 0x02;
/// Register 2: VREF = REFP0/REFN0, 50/60 = simultaneous rejection, low-side switch open, IDACs off.
const REG2: u8 = 0x50;
/// Register 3: IDACs disconnected, DRDY only on its pin.
const REG3: u8 = 0x00;

/// A 20 SPS conversion takes about 51 ms; anything past this is a fault.
const CONVERSION_TIMEOUT_MS: u32 = 200;

/// The ADC is reset and configured on first use and again after any failed conversion, so a chip
/// that is missing at boot reports [`HalError::Timeout`] instead of stopping the firmware.
pub struct Ads1220<S, D, T> {
    spi: S,
    drdy: D,
    delay: T,
    configured: bool,
}

impl<S: SpiDevice, D: Wait, T: DelayNs> Ads1220<S, D, T> {
    pub fn new(spi: S, drdy: D, delay: T) -> Self {
        Self {
            spi,
            drdy,
            delay,
            configured: false,
        }
    }

    /// Resets the ADC and writes the fixed configuration.
    async fn configure(&mut self) -> Result<(), HalError> {
        self.spi.write(&[RESET]).await.map_err(|_| HalError::Bus)?;
        self.delay.delay_us(100).await;
        let cfg = [wreg(0, 4), reg0(MUX_AIN0_AVSS), REG1, REG2, REG3];
        self.spi.write(&cfg).await.map_err(|_| HalError::Bus)?;
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
        self.spi
            .write(&[wreg(0, 2), reg0(mux), r1])
            .await
            .map_err(|_| HalError::Bus)?;
        self.spi.write(&[START]).await.map_err(|_| HalError::Bus)?;
        match select(self.drdy.wait_for_low(), self.delay.delay_ms(CONVERSION_TIMEOUT_MS)).await {
            Either::First(Ok(())) => {}
            Either::First(Err(_)) => return Err(HalError::Bus),
            Either::Second(()) => return Err(HalError::Timeout),
        }
        let mut b = [0u8; 3];
        self.spi
            .transaction(&mut [Operation::Write(&[RDATA]), Operation::Read(&mut b)])
            .await
            .map_err(|_| HalError::Bus)?;
        // sign-extend 24 -> 32 bits
        Ok(i32::from_be_bytes([b[0], b[1], b[2], 0]) >> 8)
    }
}

/// Full-scale code with gain 1: the input equals VREF at 2^23.
const FULL_SCALE: f64 = 8_388_608.0;

impl<S: SpiDevice, D: Wait, T: DelayNs> ProbeAdc for Ads1220<S, D, T> {
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
    use crate::drivers::mock::{Bus, NoDelay, Pin, block_on};

    fn adc(reads: &[[u8; 3]]) -> Ads1220<Bus, Pin, NoDelay> {
        let bus = Bus::with_reads(reads.iter().map(|r| r.to_vec()));
        Ads1220::new(bus, Pin::low(), NoDelay)
    }

    #[test]
    fn first_use_resets_then_configures() {
        let mut a = adc(&[[0, 0, 0], [0, 0, 0]]);
        block_on(a.ratio(0)).unwrap();
        assert_eq!(a.spi.writes()[..2], [vec![RESET], vec![0x43, 0x81, 0x00, 0x50, 0x00]]);
        block_on(a.ratio(0)).unwrap();
        assert_eq!(a.spi.writes().len(), 2 + 3 + 3, "configured once");
    }

    #[test]
    fn ratio_selects_jack_and_scales() {
        let mut a = adc(&[[0x40, 0x00, 0x00]]);
        assert_eq!(block_on(a.ratio(2)), Ok(0.5));
        // after init: WREG reg0..1 (AIN2/AVSS, TS off), START, RDATA
        assert_eq!(a.spi.writes()[2..], [vec![0x41, 0xA1, 0x00], vec![START], vec![RDATA]]);
    }

    #[test]
    fn offset_and_negative_codes() {
        let mut a = adc(&[[0xFF, 0xFF, 0xFF]]);
        assert_eq!(block_on(a.offset()), Ok(-1.0 / FULL_SCALE));
        assert_eq!(a.spi.writes()[2], [0x41, 0xE1, 0x00]);
    }

    #[test]
    fn die_temperature_datasheet_codes() {
        // ADS1220 datasheet table 12: 0x0320 = 25C, 0x3CE0 = -25C (14-bit codes, left-justified)
        let mut a = adc(&[[0x0C, 0x80, 0x00], [0xF3, 0x80, 0x00]]);
        assert_eq!(block_on(a.die_temp_c()), Ok(25.0));
        assert_eq!(block_on(a.die_temp_c()), Ok(-25.0));
        assert_eq!(a.spi.writes()[2], [0x41, 0x81, REG1_TS]);
    }

    #[test]
    fn missing_chip_times_out_and_reconfigures() {
        let mut a = Ads1220::new(Bus::with_reads([]), Pin::high(), NoDelay);
        assert_eq!(block_on(a.ratio(0)), Err(HalError::Timeout));
        assert_eq!(block_on(a.ratio(0)), Err(HalError::Timeout));
        let resets = a.spi.writes().iter().filter(|w| **w == [RESET]).count();
        assert_eq!(resets, 2, "a failed conversion forces a fresh reset");
    }
}
