//! The probe pipeline (`.4.7`, ADR 0002): the round robin over the four jacks, averaging and
//! filtering, what each jack holds, and the conversion to temperature.
//!
//! - [`Scanner`] runs continuously (the `probes` task in [`crate::tasks`]). One pass averages
//!   [`AVG`] conversions on each jack (a trimmed mean, so one glitched conversion does not count),
//!   then reads the ADC's offset and die temperature. The result per jack is a [`Sense`]: nothing
//!   plugged in, a short, a filtered divider ratio, or an ADC that is not answering.
//! - [`reading`] turns a [`Sense`] into a temperature and a wire status with the probe's own
//!   coefficients and offset. That happens in the control task, so a changed probe type or offset
//!   takes effect on the next period without the scanner knowing about settings.
//! - [`selfcheck`] compares every probe against the ambient sensor (ADR 0002).

use embassy_futures::yield_now;
use minimeat_core::thermistor::{Jack, R_BIAS, r_from_ratio};
use minimeat_core::units::k_to_f;

use crate::config::Probe;
use crate::hal::{PROBE_JACKS, ProbeAdc};

/// Conversions averaged per jack per pass (ADR 0002: 8 at 20 SPS is 0.4 s per jack).
pub const AVG: usize = 8;
/// Weight of a new pass in the filtered ratio. A pass comes about every 2.5 s, so this is a time
/// constant of about 2.5 s, well inside a probe's own lag.
const ALPHA: f64 = 0.5;
/// A jack whose conversions all fail keeps its last reading for this many passes before it reports
/// [`Sense::Fault`].
const MISS_MAX: u8 = 3;
/// An ADC offset (shorted inputs) beyond this ratio is a broken ADC, not an offset: it is not applied.
const OFFSET_MAX: f64 = 5e-4;
/// Readings outside this range are not `ok` (docs/protocol.md), F.
pub const RANGE_F: (f64, f64) = (-20.0, 700.0);
/// A probe further than this from ambient fails the self-check (ADR 0002), F.
pub const SELFCHECK_TOL_F: f64 = 2.0;

/// What a jack holds, from the scanner.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Sense {
    /// Not scanned yet.
    #[default]
    Pending,
    /// Nothing plugged in, or a broken probe.
    Open,
    Short,
    /// A probe: the filtered divider ratio `Rt / (Rt + R_BIAS)`, offset-corrected.
    Probe(f64),
    /// The ADC did not answer.
    Fault,
}

/// The scanner's latest result.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Snapshot {
    /// Completed passes since boot; control uses it to notice a scanner that has stopped.
    pub passes: u32,
    pub sense: [Sense; PROBE_JACKS],
    /// ADC die temperature, C (board health; the ADC must stay below 85C).
    pub die_c: Option<f64>,
    /// ADC offset, as a ratio, `None` if it could not be read or is implausible.
    pub offset: Option<f64>,
}

#[derive(Clone, Copy, Default)]
struct Chan {
    sense: Sense,
    misses: u8,
}

/// Round robin over the jacks. Call [`Scanner::pass`] in a loop.
pub struct Scanner<A> {
    adc: A,
    chans: [Chan; PROBE_JACKS],
    snap: Snapshot,
}

impl<A: ProbeAdc> Scanner<A> {
    pub fn new(adc: A) -> Self {
        Self {
            adc,
            chans: [Chan::default(); PROBE_JACKS],
            snap: Snapshot::default(),
        }
    }

    /// One pass over the four jacks plus the ADC's own offset and die temperature.
    pub async fn pass(&mut self) -> Snapshot {
        let offset = match self.adc.offset().await {
            Ok(o) if o.abs() <= OFFSET_MAX => Some(o),
            _ => None,
        };
        for j in 0..PROBE_JACKS {
            let mean = self.average(j).await;
            self.chans[j].update(mean.map(|m| m - offset.unwrap_or(0.0)));
        }
        self.snap = Snapshot {
            passes: self.snap.passes.wrapping_add(1),
            sense: self.chans.map(|c| c.sense),
            die_c: self.adc.die_temp_c().await.ok(),
            offset,
        };
        self.snap
    }

    /// Trimmed mean of [`AVG`] conversions; `None` if none succeeded.
    async fn average(&mut self, jack: usize) -> Option<f64> {
        let mut xs = [0.0; AVG];
        let mut n = 0;
        for _ in 0..AVG {
            if let Ok(x) = self.adc.ratio(jack).await {
                xs[n] = x;
                n += 1;
            }
            // a host ADC answers at once: let the other tasks run between conversions
            yield_now().await;
        }
        trimmed_mean(&mut xs[..n])
    }
}

/// Mean without the lowest and highest value once there are enough to spare them.
fn trimmed_mean(xs: &mut [f64]) -> Option<f64> {
    if xs.is_empty() {
        return None;
    }
    xs.sort_unstable_by(f64::total_cmp);
    let keep = if xs.len() >= 5 { &xs[1..xs.len() - 1] } else { &xs[..] };
    Some(keep.iter().sum::<f64>() / keep.len() as f64)
}

impl Chan {
    fn update(&mut self, mean: Option<f64>) {
        let Some(x) = mean else {
            self.misses = self.misses.saturating_add(1);
            if self.misses >= MISS_MAX {
                self.sense = Sense::Fault;
            }
            return;
        };
        self.misses = 0;
        self.sense = match Jack::classify(x) {
            Jack::Short => Sense::Short,
            Jack::Open => Sense::Open,
            Jack::Probe(_) => match self.sense {
                Sense::Probe(prev) => Sense::Probe(prev + ALPHA * (x - prev)),
                _ => Sense::Probe(x),
            },
        };
    }
}

/// A probe's state as published (`probes[n].st`, docs/protocol.md).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProbeSt {
    Ok,
    Open,
    Short,
    Range,
    /// Failed the power-up self-check against ambient.
    Disagree,
    /// The ADC is not answering.
    Fault,
}

impl ProbeSt {
    pub fn name(self) -> &'static str {
        match self {
            ProbeSt::Ok => "ok",
            ProbeSt::Open => "open",
            ProbeSt::Short => "short",
            ProbeSt::Range => "range",
            ProbeSt::Disagree => "disagree",
            ProbeSt::Fault => "fault",
        }
    }
}

/// One jack as the application sees it: temperature with the probe's offset applied, and status.
/// `temp_f` is present for `Ok` and `Disagree` (a probe of the wrong type may still be usable once
/// the user has told the controller what it is).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Reading {
    pub temp_f: Option<f64>,
    pub st: ProbeSt,
}

impl Reading {
    pub const NONE: Reading = Reading {
        temp_f: None,
        st: ProbeSt::Fault,
    };
}

/// Converts a scanner result with the probe's coefficients and offset; `disagree` is its sticky
/// self-check flag.
pub fn reading(sense: Sense, probe: &Probe, disagree: bool) -> Reading {
    let st = |st| Reading { temp_f: None, st };
    match sense {
        Sense::Pending | Sense::Fault => st(ProbeSt::Fault),
        Sense::Open => st(ProbeSt::Open),
        Sense::Short => st(ProbeSt::Short),
        Sense::Probe(x) => {
            let r = r_from_ratio(x, R_BIAS);
            let t = k_to_f(probe.kind.thermistor().t_k(r)) + probe.offset_f;
            if t.is_finite() && (RANGE_F.0..=RANGE_F.1).contains(&t) {
                Reading {
                    temp_f: Some(t),
                    st: if disagree { ProbeSt::Disagree } else { ProbeSt::Ok },
                }
            } else {
                st(ProbeSt::Range)
            }
        }
    }
}

/// One jack's self-check result (`selfcheck` event, docs/protocol.md).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Check {
    /// Probe minus ambient, F.
    Delta(f64),
    /// Nothing to compare: the jack's status (or `Fault` if there is no ambient reading).
    St(ProbeSt),
}

/// Compares each probe with ambient (`None` if the sensor does not answer, which checks nothing).
pub fn selfcheck(readings: &[Reading; PROBE_JACKS], ambient_f: Option<f64>) -> [Check; PROBE_JACKS] {
    readings.map(|r| match (r.temp_f, ambient_f) {
        (Some(t), Some(a)) => Check::Delta(t - a),
        (Some(_), None) => Check::St(ProbeSt::Fault),
        (None, _) => Check::St(r.st),
    })
}

/// Which probes failed a self-check, to be shown as [`ProbeSt::Disagree`] until unplugged or checked again.
pub fn disagreeing(results: &[Check; PROBE_JACKS]) -> [bool; PROBE_JACKS] {
    results.map(|c| matches!(c, Check::Delta(d) if d.abs() > SELFCHECK_TOL_F))
}

#[cfg(test)]
mod tests {
    use core::cell::RefCell;
    use std::collections::VecDeque;

    use minimeat_core::thermistor::Preset;
    use minimeat_core::units::f_to_k;

    use super::*;
    use crate::config::ProbeKind;
    use crate::drivers::mock::block_on;
    use crate::hal::HalError;

    /// Scripted ADC: per jack a queue of results (the last repeats), plus fixed offset and die.
    struct Script {
        q: RefCell<[VecDeque<Result<f64, HalError>>; PROBE_JACKS]>,
        offset: Result<f64, HalError>,
    }

    impl Script {
        fn new(x: [f64; PROBE_JACKS]) -> Self {
            Self {
                q: RefCell::new(x.map(|x| VecDeque::from([Ok(x)]))),
                offset: Ok(0.0),
            }
        }
        fn push(&self, jack: usize, r: Result<f64, HalError>) {
            self.q.borrow_mut()[jack].push_back(r);
        }
    }

    impl ProbeAdc for &Script {
        async fn ratio(&mut self, jack: usize) -> Result<f64, HalError> {
            let mut q = self.q.borrow_mut();
            if q[jack].len() > 1 {
                q[jack].pop_front().unwrap()
            } else {
                *q[jack].front().unwrap()
            }
        }
        async fn offset(&mut self) -> Result<f64, HalError> {
            self.offset
        }
        async fn die_temp_c(&mut self) -> Result<f64, HalError> {
            Ok(41.0)
        }
    }

    fn ratio_at(p: Preset, f: f64) -> f64 {
        let r = p.thermistor().r(f_to_k(f));
        r / (r + R_BIAS)
    }

    fn probe(p: Preset) -> Probe {
        let mut pr = Probe::default_for(0);
        pr.kind = ProbeKind::Preset(p);
        pr
    }

    #[test]
    fn reads_every_family_and_tells_open_from_short() {
        let s = Script::new([
            ratio_at(Preset::ThermoworksPro, 225.0),
            ratio_at(Preset::MaverickEt732, 150.0),
            1.0,
            0.0,
        ]);
        let snap = block_on(Scanner::new(&s).pass());
        let a = reading(snap.sense[0], &probe(Preset::ThermoworksPro), false);
        let b = reading(snap.sense[1], &probe(Preset::MaverickEt732), false);
        assert!((a.temp_f.unwrap() - 225.0).abs() < 0.01 && a.st == ProbeSt::Ok);
        assert!((b.temp_f.unwrap() - 150.0).abs() < 0.01);
        assert_eq!(snap.sense[2], Sense::Open);
        assert_eq!(snap.sense[3], Sense::Short);
        assert_eq!(snap.die_c, Some(41.0));
        assert_eq!(snap.passes, 1);
    }

    #[test]
    fn offset_trim_and_range() {
        let s = Script::new([ratio_at(Preset::ThermoworksPro, 225.0); 4]);
        let sense = block_on(Scanner::new(&s).pass()).sense[0];
        let mut p = probe(Preset::ThermoworksPro);
        p.offset_f = -1.5;
        assert!((reading(sense, &p, false).temp_f.unwrap() - 223.5).abs() < 0.01);
        // a probe that decodes to 900F is out of range, not a temperature
        let hot = Sense::Probe(ratio_at(Preset::ThermoworksPro, 900.0));
        assert_eq!(
            reading(hot, &probe(Preset::ThermoworksPro), false),
            Reading {
                temp_f: None,
                st: ProbeSt::Range
            }
        );
    }

    #[test]
    fn a_glitched_conversion_is_trimmed() {
        let x = ratio_at(Preset::ThermoworksPro, 225.0);
        let s = Script::new([x; 4]);
        for v in [x, x, 0.9, x, x, x, x] {
            s.push(0, Ok(v));
        }
        let sense = block_on(Scanner::new(&s).pass()).sense[0];
        let t = reading(sense, &probe(Preset::ThermoworksPro), false).temp_f.unwrap();
        assert!((t - 225.0).abs() < 0.01, "{t}");
    }

    #[test]
    fn filter_smooths_steps_and_resets_on_replug() {
        let lo = ratio_at(Preset::ThermoworksPro, 100.0);
        let hi = ratio_at(Preset::ThermoworksPro, 200.0);
        let s = Script::new([lo; 4]);
        let mut sc = Scanner::new(&s);
        block_on(sc.pass());
        s.q.borrow_mut()[0] = VecDeque::from([Ok(hi)]);
        let t = |sense| reading(sense, &probe(Preset::ThermoworksPro), false).temp_f.unwrap();
        let one = t(block_on(sc.pass()).sense[0]);
        assert!(one > 120.0 && one < 190.0, "part way after a pass: {one}");
        let mut last = one;
        for _ in 0..8 {
            last = t(block_on(sc.pass()).sense[0]);
        }
        assert!((last - 200.0).abs() < 1.0, "{last}");
        // unplugged, then a probe back: no memory of the old value
        s.q.borrow_mut()[0] = VecDeque::from([Ok(1.0)]);
        assert_eq!(block_on(sc.pass()).sense[0], Sense::Open);
        s.q.borrow_mut()[0] = VecDeque::from([Ok(lo)]);
        assert!((t(block_on(sc.pass()).sense[0]) - 100.0).abs() < 0.01);
    }

    #[test]
    fn adc_errors_are_ridden_out_then_reported() {
        let x = ratio_at(Preset::ThermoworksPro, 225.0);
        let s = Script::new([x; 4]);
        let mut sc = Scanner::new(&s);
        block_on(sc.pass());
        s.q.borrow_mut()[0] = VecDeque::from([Err(HalError::Timeout)]);
        for pass in 1..=4 {
            let snap = block_on(sc.pass());
            let want = if pass < MISS_MAX { Sense::Probe(x) } else { Sense::Fault };
            assert!(
                matches!((snap.sense[0], want), (Sense::Probe(a), Sense::Probe(b)) if (a - b).abs() < 1e-9)
                    || snap.sense[0] == want,
                "pass {pass}: {:?}",
                snap.sense[0]
            );
        }
        // the other jacks never noticed
        assert!(matches!(sc.snap.sense[1], Sense::Probe(_)));
    }

    #[test]
    fn partial_failures_still_average_and_a_bad_offset_is_ignored() {
        let x = ratio_at(Preset::ThermoworksPro, 225.0);
        let mut s = Script::new([x; 4]);
        for _ in 0..5 {
            s.push(0, Err(HalError::Bus));
        }
        s.push(0, Ok(x));
        s.offset = Ok(0.01);
        let snap = block_on(Scanner::new(&s).pass());
        assert!(matches!(snap.sense[0], Sense::Probe(_)));
        assert_eq!(snap.offset, None);
        // a small, believable offset is subtracted
        s.offset = Ok(2e-4);
        let snap = block_on(Scanner::new(&s).pass());
        let Sense::Probe(got) = snap.sense[1] else { panic!() };
        assert!((got - (x - 2e-4)).abs() < 1e-12);
    }

    #[test]
    fn selfcheck_flags_a_wrong_type_not_a_good_probe() {
        let amb = 72.0;
        // a ThermoWorks probe decoded with the Maverick curve reads tens of degrees off
        let x = ratio_at(Preset::ThermoworksPro, amb + 0.5);
        let rd = [
            reading(Sense::Probe(x), &probe(Preset::ThermoworksPro), false),
            reading(Sense::Probe(x), &probe(Preset::MaverickEt732), false),
            reading(Sense::Open, &probe(Preset::ThermoworksPro), false),
            reading(Sense::Short, &probe(Preset::ThermoworksPro), false),
        ];
        let res = selfcheck(&rd, Some(amb));
        assert!(matches!(res[0], Check::Delta(d) if (d - 0.5).abs() < 0.01));
        assert!(matches!(res[1], Check::Delta(d) if d.abs() > 10.0));
        assert_eq!(res[2], Check::St(ProbeSt::Open));
        assert_eq!(res[3], Check::St(ProbeSt::Short));
        assert_eq!(disagreeing(&res), [false, true, false, false]);
        // no ambient reading: nothing is flagged
        assert_eq!(disagreeing(&selfcheck(&rd, None)), [false; 4]);
        // and a flagged probe still shows its temperature
        let r = reading(Sense::Probe(x), &probe(Preset::MaverickEt732), true);
        assert!(r.temp_f.is_some() && r.st == ProbeSt::Disagree);
    }
}
