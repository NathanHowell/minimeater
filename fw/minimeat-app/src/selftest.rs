//! Power-on survey of the board: what every peripheral reads before the controller starts. Logged at
//! boot, and the basis of the probe self-check against ambient (ADR 0002, `.4.7`).

use log::info;
use minimeat_core::thermistor::Jack;

use crate::hal::{AmbientSensor, Board, HalError, Io, PROBE_JACKS, ProbeAdc, Tach, UsbPower};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BootReport {
    pub ambient_c: Result<f64, HalError>,
    pub jacks: [Result<Jack, HalError>; PROBE_JACKS],
    /// ADC offset check, as a ratio.
    pub adc_offset: Result<f64, HalError>,
    pub adc_die_c: Result<f64, HalError>,
    pub usb_ma: Option<u16>,
    pub fan_rpm: Option<f64>,
}

pub async fn survey<B: Board>(io: &mut Io<B>) -> BootReport {
    let mut jacks = [Err(HalError::NotReady); PROBE_JACKS];
    for (i, j) in jacks.iter_mut().enumerate() {
        *j = io.probes.ratio(i).await.map(Jack::classify);
    }
    let r = BootReport {
        ambient_c: io.ambient.read_c().await,
        jacks,
        adc_offset: io.probes.offset().await,
        adc_die_c: io.probes.die_temp_c().await,
        usb_ma: io.usb.source_current_ma(),
        fan_rpm: io.tach.rpm(),
    };
    info!(
        "survey: ambient {:?} C, adc die {:?} C offset {:?}, usb {:?} mA, fan {:?} rpm",
        r.ambient_c, r.adc_die_c, r.adc_offset, r.usb_ma, r.fan_rpm
    );
    for (i, j) in r.jacks.iter().enumerate() {
        info!("survey: jack {i}: {j:?}");
    }
    r
}
