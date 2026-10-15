//! The damper servo's supply current from the current-sense amplifier's output (ADR 0004): the 0.1 ohm shunt R36 in the
//! servo's supply and the INA181A1 (U13, gain 20) give 2 mV at the ADC pin per mA, 0 V at 0 A (REF is grounded).

/// Shunt resistance times the amplifier's gain: mV at the ADC pin per A.
pub const MV_PER_A: u32 = 2000;

/// Servo current, mA, from the amplifier's output in mV.
pub fn ma_from_mv(mv: u16) -> u16 {
    ((u32::from(mv) * 1000 + MV_PER_A / 2) / MV_PER_A) as u16
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_millivolts_per_milliamp() {
        // running, the switch's limit and the start-up surge (vpg's levels: 0.4, 1.2 and 1.8 V)
        assert_eq!(ma_from_mv(400), 200);
        assert_eq!(ma_from_mv(1200), 600);
        assert_eq!(ma_from_mv(1800), 900);
        assert_eq!(ma_from_mv(0), 0);
        assert_eq!(ma_from_mv(1), 1, "rounds to the nearest mA");
    }
}
