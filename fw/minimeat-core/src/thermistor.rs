//! NTC probe conversion (ADR 0002). Port of `src/minimeat/model/probes.py`.
//!
//! The ADS1220 reads the ratiometric divider `x = R_t / (R_t + R_bias)`; [`Thermistor::t_k`] turns
//! the recovered resistance into temperature with Steinhart-Hart `1/T = a + b ln R + c (ln R)^3`.

use libm::{cbrt, exp, log, sqrt};

/// Bias resistor from VREF_PROBE to each jack tip (ADR 0002: 47.0k 0.1%).
pub const R_BIAS: f64 = 47e3;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Thermistor {
    pub a: f64,
    pub b: f64,
    pub c: f64,
}

impl Thermistor {
    /// Beta model expressed as Steinhart-Hart with `c = 0`.
    pub fn from_beta(r25: f64, beta: f64) -> Self {
        let b = 1.0 / beta;
        Self {
            a: 1.0 / crate::units::c_to_k(25.0) - b * log(r25),
            b,
            c: 0.0,
        }
    }

    pub fn t_k(&self, r: f64) -> f64 {
        let ln = log(r);
        1.0 / (self.a + self.b * ln + self.c * ln * ln * ln)
    }

    /// Inverse of [`Self::t_k`]: the depressed cubic in `ln R` has one real root.
    pub fn r(&self, t_k: f64) -> f64 {
        if self.c == 0.0 {
            return exp((1.0 / t_k - self.a) / self.b);
        }
        let p = self.b / self.c;
        let q = (self.a - 1.0 / t_k) / self.c;
        let disc = sqrt((q / 2.0) * (q / 2.0) + (p / 3.0) * (p / 3.0) * (p / 3.0));
        exp(cbrt(-q / 2.0 + disc) + cbrt(-q / 2.0 - disc))
    }

    /// True if the curve is a usable NTC over -20..700F with 20 ohm..50 Mohm, the `set/probe/<n>/coeffs`
    /// acceptance rule in docs/protocol.md.
    pub fn plausible(&self) -> bool {
        let (r_cold, r_hot) = (self.r(crate::units::f_to_k(-20.0)), self.r(crate::units::f_to_k(700.0)));
        let finite = r_cold.is_finite() && r_hot.is_finite();
        // 1/T is monotonic in ln R iff b + 3c(ln R)^2 > 0 throughout, i.e. at the extremes when b, c >= 0.
        finite && self.b > 0.0 && self.c >= 0.0 && r_hot >= 20.0 && r_cold <= 50e6 && r_cold > r_hot
    }
}

/// Probe presets selectable by `set/probe/<n>/type`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Preset {
    ThermoworksPro,
    MaverickEt732,
    GenericB3950,
}

impl Preset {
    pub const ALL: [Preset; 3] = [Preset::ThermoworksPro, Preset::MaverickEt732, Preset::GenericB3950];

    pub fn thermistor(self) -> Thermistor {
        match self {
            Preset::ThermoworksPro => Thermistor {
                a: 7.343140544e-4,
                b: 2.157437229e-4,
                c: 9.51568577e-8,
            },
            Preset::MaverickEt732 => Thermistor {
                a: 5.3612e-4,
                b: 1.91532e-4,
                c: 6.557e-8,
            },
            Preset::GenericB3950 => Thermistor::from_beta(100e3, 3950.0),
        }
    }

    /// Wire name used in docs/protocol.md.
    pub fn name(self) -> &'static str {
        match self {
            Preset::ThermoworksPro => "thermoworks_pro",
            Preset::MaverickEt732 => "maverick_et732",
            Preset::GenericB3950 => "generic_b3950",
        }
    }
}

/// Divider resistance from the ADC ratio `x = R_t / (R_t + r_bias)`.
pub fn r_from_ratio(x: f64, r_bias: f64) -> f64 {
    r_bias * x / (1.0 - x)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::units::{f_to_k, k_to_f};

    // (preset, F, R ohm, ratio at 47k) from `probes.py` (uv run python; see fw/README.md).
    const REFERENCE: [(Preset, f64, f64, f64); 9] = [
        (Preset::ThermoworksPro, 32.0, 317654.9516280019, 0.8711110330734068),
        (Preset::ThermoworksPro, 225.0, 4965.547363214056, 0.09555460521770857),
        (Preset::ThermoworksPro, 500.0, 186.27404356550826, 0.00394763196164901),
        (Preset::MaverickEt732, 32.0, 3721126.0013272776, 0.9875269563747484),
        (Preset::MaverickEt732, 225.0, 37332.79522681363, 0.4426841909651734),
        (Preset::MaverickEt732, 500.0, 974.7158911010443, 0.020317283239645968),
        (Preset::GenericB3950, 32.0, 336206.0372143574, 0.8773505753154166),
        (Preset::GenericB3950, 225.0, 5705.154679119584, 0.10824661674657447),
        (Preset::GenericB3950, 500.0, 291.00991008692637, 0.006153598974524235),
    ];

    #[test]
    fn matches_python_reference() {
        for (p, t_f, r, x) in REFERENCE {
            let th = p.thermistor();
            assert!((th.r(f_to_k(t_f)) / r - 1.0).abs() < 1e-9, "{p:?} {t_f}F");
            assert!(
                (k_to_f(th.t_k(r_from_ratio(x, R_BIAS))) - t_f).abs() < 1e-6,
                "{p:?} {t_f}F"
            );
        }
    }

    #[test]
    fn presets_plausible_and_garbage_rejected() {
        assert!(Preset::ALL.iter().all(|p| p.thermistor().plausible()));
        assert!(
            !Thermistor {
                a: 1e-3,
                b: -2e-4,
                c: 0.0
            }
            .plausible()
        );
        assert!(!Thermistor { a: 0.0, b: 0.0, c: 0.0 }.plausible());
    }
}
