//! The outside-air reading: the TMP117 sits on the board's underside in a vented well, so it reads a little of the board's
//! own heat on top of the air (`docs/sim/dissipation.md`: the whole board puts out under 0.2 W). The board temperature is
//! the ADC's die, so the air is recovered from the two readings.

/// The fraction of the board's excess over the air that the sensor picks up. `[est]`, 0.1 from the well's venting and the
/// sensor's distance from U4; the bench measures it (`docs/icd.md`, ambient) and this is the value to change.
pub const SELF_HEAT: f64 = 0.1;
/// The most the compensation may move a reading, C. A wrong coefficient or a die reading that is off must not make the
/// airflow map plan for another day's weather.
pub const MAX_CORRECTION_C: f64 = 5.0;

/// The air temperature, C, from the sensor's reading `raw_c` and the board temperature `board_c` (None: the ADC is not
/// answering, and the sensor's reading stands). raw = air + `SELF_HEAT` * (board - air), solved for the air.
pub fn compensate(raw_c: f64, board_c: Option<f64>) -> f64 {
    let Some(board) = board_c else { return raw_c };
    let air = (raw_c - SELF_HEAT * board) / (1.0 - SELF_HEAT);
    air.clamp(raw_c - MAX_CORRECTION_C, raw_c + MAX_CORRECTION_C)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recovers_the_air_from_a_warm_board() {
        // 25C air, 45C board: the sensor reads 2C high
        let raw = 25.0 + SELF_HEAT * 20.0;
        assert!((compensate(raw, Some(45.0)) - 25.0).abs() < 1e-9);
    }

    #[test]
    fn a_board_at_air_temperature_changes_nothing() {
        assert!((compensate(22.0, Some(22.0)) - 22.0).abs() < 1e-9);
    }

    #[test]
    fn without_the_board_reading_the_sensor_stands() {
        assert_eq!(compensate(31.5, None), 31.5);
    }

    #[test]
    fn the_correction_is_bounded() {
        // a die reading of 200C is a broken reading, not a hot board
        assert_eq!(compensate(20.0, Some(200.0)), 15.0);
        assert_eq!(compensate(20.0, Some(-100.0)), 25.0);
    }
}
