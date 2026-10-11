//! USB-C source current from the CC pin voltage across the sink's 5.1k Rd (USB Type-C spec 4.11.3,
//! vRd ranges). The active CC line is the higher of the two; the other floats near 0 V.

/// Advertised current in mA from the CC1/CC2 voltages in mV; `None` below vRd-Connect (no source).
/// "Default USB power" is reported as 500 mA, the USB 2.0 floor.
pub fn source_current_ma(cc1_mv: u16, cc2_mv: u16) -> Option<u16> {
    match cc1_mv.max(cc2_mv) {
        0..200 => None,
        200..660 => Some(500),
        660..1230 => Some(1500),
        _ => Some(3000),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spec_nominals() {
        // Rp to 5 V of 56k / 22k / 10k against Rd 5.1k: 0.42 V, 0.94 V, 1.68 V
        assert_eq!(source_current_ma(0, 0), None);
        assert_eq!(source_current_ma(420, 0), Some(500));
        assert_eq!(source_current_ma(10, 940), Some(1500));
        assert_eq!(source_current_ma(1680, 20), Some(3000));
    }
}
