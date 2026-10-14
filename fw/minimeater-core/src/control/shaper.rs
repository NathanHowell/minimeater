//! Reference shaping (light-off / setpoint increases).

/// Shape the reference only while *approaching* a setpoint (light-off or a setpoint increase).
///
/// While armed: approach along a first-order trajectory (rate-capped), never more than `tether_f`
/// above the pit, so a slow-to-spread fire cannot wind the controller up. Disarms once the pit is
/// within `band_f` of the setpoint; afterwards disturbances (lid, weather) see the true setpoint.
/// Falling setpoints apply immediately - the only way down is less air.
#[derive(Clone, Debug)]
pub struct RefShaper {
    pub tau_s: f64,
    pub rate_max_f_s: f64,
    pub tether_f: f64,
    pub band_f: f64,
    r: Option<f64>,
    armed: bool,
    sp: Option<f64>,
}

impl Default for RefShaper {
    fn default() -> Self {
        Self::DEFAULT
    }
}

impl RefShaper {
    pub const DEFAULT: Self = Self {
        tau_s: 360.0,
        rate_max_f_s: 12.0 / 60.0,
        tether_f: 40.0,
        band_f: 5.0,
        r: None,
        armed: true,
        sp: None,
    };
}

impl RefShaper {
    pub fn armed(&self) -> bool {
        self.armed
    }

    pub fn step(&mut self, sp: f64, y: f64, dt: f64) -> f64 {
        if self.sp.is_some_and(|prev| sp > prev + self.band_f) {
            self.armed = true;
            self.r = Some(y.min(sp));
        }
        self.sp = Some(sp);
        let mut r = self.r.unwrap_or(y.min(sp));
        if !self.armed || sp <= r {
            r = sp;
        } else {
            r += dt * ((sp - r) / self.tau_s).min(self.rate_max_f_s);
            r = r.min(y + self.tether_f).min(sp);
        }
        if self.armed && (y - sp).abs() < self.band_f {
            self.armed = false;
            r = sp;
        }
        self.r = Some(r);
        r
    }

    /// The reference trajectory the shaper would produce if the pit kept up (no tether), starting
    /// after the current step: element k is the reference k+1 periods ahead.
    pub fn preview(&self, sp: f64, dt: f64) -> Preview {
        Preview {
            r: self.r.unwrap_or(sp),
            sp,
            dt,
            armed: self.armed,
            tau_s: self.tau_s,
            rate: self.rate_max_f_s,
        }
    }
}

/// Infinite iterator over [`RefShaper::preview`].
#[derive(Clone, Debug)]
pub struct Preview {
    r: f64,
    sp: f64,
    dt: f64,
    armed: bool,
    tau_s: f64,
    rate: f64,
}

impl Iterator for Preview {
    type Item = f64;

    fn next(&mut self) -> Option<f64> {
        if !self.armed || self.sp <= self.r {
            self.r = self.sp;
        } else {
            self.r = (self.r + self.dt * ((self.sp - self.r) / self.tau_s).min(self.rate)).min(self.sp);
        }
        Some(self.r)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shaper_tethers_on_light_off_then_disarms() {
        let mut sh = RefShaper::default();
        assert!(sh.step(225.0, 70.0, 10.0) <= 70.0 + sh.tether_f);
        sh.step(225.0, 223.0, 10.0);
        assert!(!sh.armed());
        assert_eq!(sh.step(225.0, 110.0, 10.0), 225.0, "a lid dip does not re-tether");
        assert!(
            sh.step(275.0, 225.0, 10.0) < 275.0 && sh.armed(),
            "a setpoint increase re-arms"
        );
    }
}
