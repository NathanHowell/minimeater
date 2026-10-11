//! Fallback controller: feedforward + PI on airflow with SIMC gains from the scheduled FOPDT model.

use super::{Feedforward, LidRecovery, Obs, RefShaper, Schedule, Snapshot};

#[derive(Clone, Debug)]
pub struct FfPi {
    pub schedule: Schedule,
    pub shaper: RefShaper,
    pub lid: LidRecovery,
    /// SIMC closed-loop time constant as a multiple of the dead time.
    pub tc_over_theta: f64,
    /// Integral term, CFM.
    integ: f64,
    q_last: f64,
}

impl Default for FfPi {
    fn default() -> Self {
        Self::new(Schedule::default())
    }
}

impl FfPi {
    pub fn new(schedule: Schedule) -> Self {
        Self {
            schedule,
            shaper: RefShaper::default(),
            lid: LidRecovery::default(),
            tc_over_theta: 2.0,
            integ: 0.0,
            q_last: 0.0,
        }
    }

    pub fn step(&mut self, obs: &Obs, ff: &impl Feedforward, dt: f64) -> f64 {
        let prev = Snapshot {
            q: self.q_last,
            state: self.integ,
        };
        if let Some(o) = self.lid.update(obs, dt, prev) {
            if let Some(s) = o.restore {
                self.integ = s.state;
            }
            return o.q.clamp(0.0, obs.q_max);
        }
        let r = self.shaper.step(obs.sp_f, obs.pit_f, dt);
        let (kc, ti) = self.gains(obs.sp_f);
        let e = r - obs.pit_f;
        let q_unsat = ff.q_cfm(r) + kc * e + self.integ;
        let q = q_unsat.clamp(obs.q_min, obs.q_max);
        // conditional integration: stop integrating further into saturation
        if q == q_unsat || (q_unsat > obs.q_max && e < 0.0) || (q_unsat < obs.q_min && e > 0.0) {
            self.integ += kc * e * dt / ti;
        }
        self.q_last = q;
        q
    }

    /// SIMC proportional gain (CFM per F) and integral time (s) at a setpoint.
    fn gains(&self, sp_f: f64) -> (f64, f64) {
        let m = self.schedule.at(sp_f);
        let tc = (self.tc_over_theta * m.theta_s).max(60.0);
        (m.tau_s / (m.k * (tc + m.theta_s)), m.tau_s.min(4.0 * (tc + m.theta_s)))
    }

    /// One control period in which something else commanded `q_applied`: back-calculates the
    /// integral so the next [`Self::step`] continues from that airflow without a bump.
    pub fn track(&mut self, obs: &Obs, ff: &impl Feedforward, q_applied: f64, dt: f64) {
        let r = self.shaper.step(obs.sp_f, obs.pit_f, dt);
        let (kc, _) = self.gains(obs.sp_f);
        self.integ = q_applied - ff.q_cfm(r) - kc * (r - obs.pit_f);
        self.q_last = q_applied;
    }
}
