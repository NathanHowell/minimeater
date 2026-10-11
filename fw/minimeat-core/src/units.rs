//! Unit conversions. Temperatures are F at the edges (protocol, config) and K inside the models.

pub fn f_to_k(f: f64) -> f64 {
    (f - 32.0) * 5.0 / 9.0 + 273.15
}

pub fn k_to_f(k: f64) -> f64 {
    (k - 273.15) * 9.0 / 5.0 + 32.0
}

pub fn c_to_k(c: f64) -> f64 {
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
