//! Actuator drivers (`.4.8`): the logic between "open the flap to 40 degrees, run the fan at 30 %"
//! and the pulse width, power gate and PWM duty that do it. Pure state machines that the
//! [`crate::tasks::actuators`] task steps once a second, so they run unchanged on the host.
//!
//! - [`ServoDrive`]: angle to pulse width through the calibration (either direction), a slew limit
//!   so the damper is not slammed, and the power gate: the servo is powered only while it moves and
//!   for [`HOLD_S`] after, which stops hold current and jitter.
//! - [`FanCurve`] and [`FanDrive`]: speed fraction to PWM duty, and an optional spin-up kick.
//! - [`TachWatch`]: a fan that is driven but does not turn is a fault (`fan_stall`).

/// The servo stays powered this long after its last move, s.
pub const HOLD_S: f64 = 2.0;

/// Pulse widths at the flap's closed stop and its fully open (90 degree) stop, us. `closed_us >
/// open_us` is a servo mounted the other way round.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ServoCal {
    pub closed_us: f64,
    pub open_us: f64,
}

impl From<crate::config::ServoPulses> for ServoCal {
    fn from(p: crate::config::ServoPulses) -> Self {
        Self {
            closed_us: f64::from(p.closed_us),
            open_us: f64::from(p.open_us),
        }
    }
}

impl ServoCal {
    /// A typical hobby servo, 1 to 2 ms over 90 degrees. [est until `.2.9` picks the servo]
    pub const DEFAULT: ServoCal = ServoCal {
        closed_us: 1000.0,
        open_us: 2000.0,
    };

    /// Pulse width for a flap angle (clamped to the stops), us.
    pub fn pulse_us(&self, angle_deg: f64) -> u16 {
        let frac = (angle_deg / 90.0).clamp(0.0, 1.0);
        (self.closed_us + frac * (self.open_us - self.closed_us) + 0.5) as u16
    }
}

/// What to put on the servo pins this second.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ServoOut {
    pub pulse_us: u16,
    pub power: bool,
}

#[derive(Clone, Debug)]
pub struct ServoDrive {
    pub cal: ServoCal,
    /// Slew limit, degrees per second.
    pub slew_dps: f64,
    /// Where the flap is believed to be; `None` until the first command (it could be anywhere
    /// after a power-up, so the first move is a jump).
    angle_deg: Option<f64>,
    still_s: f64,
}

impl ServoDrive {
    /// Full travel in 1.5 s.
    pub const SLEW_DPS: f64 = 60.0;

    pub fn new(cal: ServoCal) -> Self {
        Self {
            cal,
            slew_dps: Self::SLEW_DPS,
            angle_deg: None,
            still_s: HOLD_S,
        }
    }

    /// Forgets where the flap is: something else moved the servo, so the next command is a jump.
    pub fn forget(&mut self) {
        self.angle_deg = None;
    }

    /// Where the servo has been told to be.
    pub fn angle_deg(&self) -> Option<f64> {
        self.angle_deg
    }

    /// One step of `dt` seconds toward `target_deg`. The caller sets the pulse before it applies
    /// power, so a servo that wakes up goes straight to its new position and not first to the old one.
    pub fn tick(&mut self, target_deg: f64, dt: f64) -> ServoOut {
        let target = target_deg.clamp(0.0, 90.0);
        let next = match self.angle_deg {
            None => target,
            Some(a) => a + (target - a).clamp(-self.slew_dps * dt, self.slew_dps * dt),
        };
        if self.angle_deg != Some(next) {
            self.still_s = 0.0;
        } else {
            self.still_s += dt;
        }
        self.angle_deg = Some(next);
        ServoOut {
            pulse_us: self.cal.pulse_us(next),
            power: self.still_s < HOLD_S,
        }
    }
}

/// Fan speed fraction to PWM duty: piecewise linear through `(speed, duty)` points, speed ascending.
/// Speed 0 is always duty 0.
#[derive(Clone, Copy, Debug)]
pub struct FanCurve {
    pub points: &'static [(f64, f64)],
}

impl FanCurve {
    /// Duty equals speed. [est until `.2.2` picks the fan and its datasheet curve is known]
    pub const LINEAR: FanCurve = FanCurve {
        points: &[(0.0, 0.0), (1.0, 1.0)],
    };

    pub fn duty(&self, speed: f64) -> f64 {
        if speed <= 0.0 {
            return 0.0;
        }
        let s = speed.min(1.0);
        let mut prev = self.points[0];
        for &(x, y) in &self.points[1..] {
            if s == x {
                return y;
            }
            if s < x {
                return prev.1 + (y - prev.1) * (s - prev.0) / (x - prev.0);
            }
            prev = (x, y);
        }
        prev.1
    }
}

/// Speed to duty, with an optional kick: the first second of a start runs at least `kick` duty so a
/// fan that needs more than its running minimum to get going does.
#[derive(Clone, Debug)]
pub struct FanDrive {
    pub curve: FanCurve,
    pub kick: Option<f64>,
    running: bool,
}

impl FanDrive {
    pub fn new(curve: FanCurve, kick: Option<f64>) -> Self {
        Self {
            curve,
            kick,
            running: false,
        }
    }

    /// PWM duty (0..1) for the speed to apply this second.
    pub fn duty(&mut self, speed: f64) -> f64 {
        let d = self.curve.duty(speed);
        let starting = d > 0.0 && !self.running;
        self.running = d > 0.0;
        match self.kick {
            Some(k) if starting => d.max(k),
            _ => d,
        }
    }
}

/// Duty above which a silent tach is a stall (docs/protocol.md `fan_stall`).
const STALL_DUTY: f64 = 0.3;
/// A stall is raised after this long of driving a fan that does not turn, s (spin-up included).
const STALL_AFTER_S: f64 = 5.0;
/// ... and cleared after this long of it turning again, s.
const RECOVER_AFTER_S: f64 = 3.0;
/// The tach must read at least this fraction of the speed expected from the duty to count as turning.
const TURNING_FRAC: f64 = 0.25;

/// Raises `fan_stall` when the fan is driven but the tach shows it not turning.
#[derive(Clone, Debug, Default)]
pub struct TachWatch {
    stalled: bool,
    bad_s: f64,
    good_s: f64,
}

impl TachWatch {
    pub fn stalled(&self) -> bool {
        self.stalled
    }

    /// One step of `dt` seconds: `duty` applied over it, `rpm` the tach's mean over it (`None` if it
    /// had nothing to measure) and `rpm_max` the fan's speed at full duty. Returns the new state
    /// when it changed.
    pub fn tick(&mut self, duty: f64, rpm: Option<f64>, rpm_max: f64, dt: f64) -> Option<bool> {
        let rpm = rpm?;
        if duty < STALL_DUTY {
            // not driven hard enough to say; a stalled fan stays so until it turns
            (self.bad_s, self.good_s) = (0.0, 0.0);
            return None;
        }
        if rpm >= TURNING_FRAC * duty * rpm_max {
            self.bad_s = 0.0;
            self.good_s += dt;
            if self.stalled && self.good_s >= RECOVER_AFTER_S {
                self.stalled = false;
                return Some(false);
            }
        } else {
            self.good_s = 0.0;
            self.bad_s += dt;
            if !self.stalled && self.bad_s >= STALL_AFTER_S {
                self.stalled = true;
                return Some(true);
            }
        }
        None
    }
}

/// The advertised source current the fan and servo need together (ADR 0004): worst case is the radio's burst, the
/// fan and a servo at its 0.6 A limit, about 1.1 A. Below this the box does not run them at all.
pub const SUPPLY_MIN_MA: u16 = 1500;
/// Consecutive reads, one a second, before the supply is judged good or bad (a CC reading can glitch).
const SUPPLY_GOOD_READS: u8 = 2;
const SUPPLY_BAD_READS: u8 = 3;

/// Decides whether the USB-C source can run the fan and the servo, from its advertised current. All or nothing:
/// there is no reduced mode. Until the first verdict the actuators stay off and no fault is raised.
#[derive(Clone, Copy, Debug, Default)]
pub struct SupplyWatch {
    verdict: Option<bool>,
    run: u8,
}

impl SupplyWatch {
    /// The actuators may run.
    pub fn ok(&self) -> bool {
        self.verdict == Some(true)
    }

    /// One read of the advertised current. Returns the new fault state (`true` = `supply` raised) when it changed.
    pub fn tick(&mut self, advertised_ma: Option<u16>) -> Option<bool> {
        let good = advertised_ma.is_some_and(|ma| ma >= SUPPLY_MIN_MA);
        // the run counts reads that disagree with the current verdict (or with nothing yet)
        let agrees = self.verdict == Some(good);
        if agrees {
            self.run = 0;
            return None;
        }
        self.run += 1;
        let need = if good { SUPPLY_GOOD_READS } else { SUPPLY_BAD_READS };
        if self.run < need {
            return None;
        }
        let first = self.verdict.is_none();
        self.verdict = Some(good);
        self.run = 0;
        // the first verdict being good is not a change; being bad is a fault
        (!(first && good)).then_some(!good)
    }
}

/// Consecutive one-second samples of the fault line held low before it counts: the TPS2553 only asserts it after a
/// deglitch of several ms, and a start-up into the output capacitors never lasts that long, so a real trip is steady.
const LOAD_TRIP_READS: u8 = 3;

/// Latches when a load switch holds the shared fault line low (ADR 0004): the servo stalled or the fan jammed or a
/// short. Both loads then stay off until the next reboot; the switches thermal-cycle on their own, and retrying
/// from here would only keep cycling them.
#[derive(Clone, Copy, Debug, Default)]
pub struct LoadWatch {
    low: u8,
    tripped: bool,
}

impl LoadWatch {
    pub fn tripped(&self) -> bool {
        self.tripped
    }

    /// One read of the line (`low` = asserted). Returns `true` once, when it latches.
    pub fn tick(&mut self, low: bool) -> bool {
        if self.tripped {
            return false;
        }
        self.low = if low { self.low + 1 } else { 0 };
        self.tripped = self.low >= LOAD_TRIP_READS;
        self.tripped
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_steady_fault_latches_and_a_blip_does_not() {
        let mut w = LoadWatch::default();
        assert!(!w.tick(true) && !w.tick(true));
        assert!(!w.tick(false), "a blip resets the count");
        assert!(!w.tick(true) && !w.tick(true));
        assert!(w.tick(true), "the third in a row latches");
        assert!(w.tripped());
        assert!(!w.tick(false), "stays latched, and says so once");
        assert!(w.tripped());
    }

    #[test]
    fn supply_needs_1500_ma_and_debounces_both_ways() {
        let mut w = SupplyWatch::default();
        assert!(!w.ok(), "off until the first verdict");
        assert_eq!(w.tick(Some(3000)), None);
        assert!(!w.ok());
        assert_eq!(w.tick(Some(3000)), None, "a good source from the start raises nothing");
        assert!(w.ok());
        // a glitch does not drop it, a lasting loss does
        assert_eq!(w.tick(Some(500)), None);
        assert_eq!(w.tick(Some(500)), None);
        assert_eq!(w.tick(Some(3000)), None);
        assert!(w.ok());
        for _ in 0..2 {
            assert_eq!(w.tick(None), None);
        }
        assert_eq!(w.tick(None), Some(true));
        assert!(!w.ok());
        assert_eq!(w.tick(Some(1500)), None);
        assert_eq!(w.tick(Some(1500)), Some(false), "1.5 A is enough");
        assert!(w.ok());
    }

    #[test]
    fn a_default_source_is_a_fault_from_the_start() {
        let mut w = SupplyWatch::default();
        assert_eq!(w.tick(Some(500)), None);
        assert_eq!(w.tick(Some(500)), None);
        assert_eq!(w.tick(Some(500)), Some(true));
        assert!(!w.ok());
    }

    #[test]
    fn servo_pulse_follows_the_calibration_either_way_round() {
        let c = ServoCal::DEFAULT;
        assert_eq!(
            [c.pulse_us(0.0), c.pulse_us(45.0), c.pulse_us(90.0)],
            [1000, 1500, 2000]
        );
        assert_eq!(c.pulse_us(-10.0), 1000, "clamped to the stops");
        assert_eq!(c.pulse_us(120.0), 2000);
        let flipped = ServoCal {
            closed_us: 2100.0,
            open_us: 900.0,
        };
        assert_eq!(
            [flipped.pulse_us(0.0), flipped.pulse_us(45.0), flipped.pulse_us(90.0)],
            [2100, 1500, 900]
        );
    }

    #[test]
    fn servo_slews_gates_power_and_jumps_the_first_time() {
        let mut s = ServoDrive::new(ServoCal::DEFAULT);
        // first command: straight there, powered for the hold time
        let o = s.tick(30.0, 1.0);
        assert_eq!(
            o,
            ServoOut {
                pulse_us: 1333,
                power: true
            }
        );
        assert!(s.tick(30.0, 1.0).power);
        assert!(!s.tick(30.0, 1.0).power, "off after {HOLD_S} s still");
        // a big move is slewed, powered all the way and for the hold after
        let mut angles = vec![];
        let mut powered = vec![];
        for _ in 0..8 {
            let o = s.tick(90.0, 1.0);
            angles.push(s.angle_deg().unwrap());
            powered.push(o.power);
        }
        assert_eq!(angles[..2], [90.0, 90.0], "30 -> 90 at 60 deg/s takes one second");
        assert_eq!(powered, [true, true, false, false, false, false, false, false]);
        // slower servo, smaller steps, both directions
        s.slew_dps = 20.0;
        s.tick(0.0, 1.0);
        assert_eq!(s.angle_deg(), Some(70.0));
        s.tick(80.0, 1.0);
        assert_eq!(s.angle_deg(), Some(80.0));
    }

    #[test]
    fn a_target_outside_the_travel_is_clamped() {
        let mut s = ServoDrive::new(ServoCal::DEFAULT);
        s.tick(-5.0, 1.0);
        assert_eq!(s.angle_deg(), Some(0.0));
        s.tick(500.0, 1.0);
        assert_eq!(s.angle_deg(), Some(60.0));
    }

    #[test]
    fn fan_curve_interpolates() {
        const C: FanCurve = FanCurve {
            points: &[(0.0, 0.0), (0.2, 0.3), (1.0, 1.0)],
        };
        assert_eq!(C.duty(0.0), 0.0);
        assert!((C.duty(0.1) - 0.15).abs() < 1e-12);
        assert!((C.duty(0.2) - 0.3).abs() < 1e-12);
        assert!((C.duty(0.6) - 0.65).abs() < 1e-12);
        assert_eq!(C.duty(1.5), 1.0);
        assert_eq!(C.duty(-1.0), 0.0);
        assert_eq!(FanCurve::LINEAR.duty(0.37), 0.37);
    }

    #[test]
    fn fan_kick_applies_to_starts_only() {
        let mut f = FanDrive::new(FanCurve::LINEAR, Some(0.6));
        assert_eq!(f.duty(0.2), 0.6, "start");
        assert_eq!(f.duty(0.2), 0.2, "running");
        assert_eq!(f.duty(0.0), 0.0);
        assert_eq!(f.duty(0.9), 0.9, "a start already above the kick");
        assert_eq!(f.duty(0.0), 0.0);
        assert_eq!(f.duty(0.2), 0.6);
        let mut plain = FanDrive::new(FanCurve::LINEAR, None);
        assert_eq!(plain.duty(0.2), 0.2);
    }

    #[test]
    fn tach_watch_raises_and_clears_a_stall_with_hysteresis() {
        let mut t = TachWatch::default();
        let events: Vec<_> = (0..12).map(|_| t.tick(0.5, Some(0.0), 3000.0, 1.0)).collect();
        assert_eq!(events.iter().position(|e| *e == Some(true)), Some(4), "after 5 s");
        assert_eq!(events.iter().flatten().count(), 1, "once");
        // a blip of turning does not clear it, and nor does running below the stall duty
        assert_eq!(t.tick(0.5, Some(1500.0), 3000.0, 1.0), None);
        assert_eq!(t.tick(0.5, Some(0.0), 3000.0, 1.0), None);
        assert_eq!(t.tick(0.1, Some(0.0), 3000.0, 1.0), None);
        assert!(t.stalled());
        let cleared: Vec<_> = (0..4).map(|_| t.tick(0.5, Some(1500.0), 3000.0, 1.0)).collect();
        assert_eq!(cleared, [None, None, Some(false), None]);
    }

    #[test]
    fn tach_watch_ignores_a_healthy_fan_and_a_missing_reading() {
        let mut t = TachWatch::default();
        for _ in 0..100 {
            // rpm tracks duty, with the slack of a real fan
            assert_eq!(t.tick(0.4, Some(0.4 * 3000.0 * 0.9), 3000.0, 1.0), None);
            assert_eq!(t.tick(0.0, Some(0.0), 3000.0, 1.0), None);
            assert_eq!(t.tick(0.8, None, 3000.0, 1.0), None);
        }
        assert!(!t.stalled());
    }
}
