//! Unit conversions and physical constants (SI inside the models). Temperatures are F at the edges
//! (protocol, config) and K inside the models.

use libm::pow;

/// m^3/s per cubic foot per minute.
pub const CFM: f64 = 4.719474e-4;
/// Pa per mm of water column.
pub const MMH2O: f64 = 9.80665;
pub const INCH: f64 = 0.0254;
pub const G: f64 = 9.80665;
/// Stefan-Boltzmann, W/m^2/K^4.
pub const SIGMA: f64 = 5.670374e-8;
/// J/kg/K.
pub const R_AIR: f64 = 287.05;
pub const P_ATM: f64 = 101325.0;

/// Ideal-gas density of air (flue gas is close enough) at 1 atm, kg/m^3.
pub const fn air_density(t_k: f64) -> f64 {
    P_ATM / (R_AIR * t_k)
}

/// Sutherland's law for air, Pa*s.
pub fn air_viscosity(t_k: f64) -> f64 {
    1.716e-5 * pow(t_k / 273.15, 1.5) * (273.15 + 110.4) / (t_k + 110.4)
}

pub const fn f_to_k(f: f64) -> f64 {
    (f - 32.0) * 5.0 / 9.0 + 273.15
}

pub const fn k_to_f(k: f64) -> f64 {
    (k - 273.15) * 9.0 / 5.0 + 32.0
}

pub const fn c_to_k(c: f64) -> f64 {
    c + 273.15
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_points() {
        assert!((f_to_k(32.0) - 273.15).abs() < 1e-12);
        assert!((k_to_f(373.15) - 212.0).abs() < 1e-12);
        assert!((k_to_f(f_to_k(225.0)) - 225.0).abs() < 1e-12);
    }
}
