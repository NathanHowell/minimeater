//! Probe alarms (`.4.12`, docs/protocol.md): a per-probe low and high limit, latched, with 5F of
//! hysteresis. An alarm is reported for as long as it holds, whatever the user has acknowledged:
//! `alarm_ack` only silences the LED.

use crate::config::Probe;
use crate::hal::PROBE_JACKS;
use crate::pit::Event;

/// An alarm clears when the reading is this far back inside its limit, F.
pub const HYSTERESIS_F: f64 = 5.0;

/// Which limit a probe crossed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Limit {
    Lo,
    Hi,
}

impl Limit {
    pub fn name(self) -> &'static str {
        match self {
            Limit::Lo => "lo",
            Limit::Hi => "hi",
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Alarms {
    active: [Option<Limit>; PROBE_JACKS],
    acked: [bool; PROBE_JACKS],
}

impl Alarms {
    /// The alarm each probe has now.
    pub fn active(&self) -> [Option<Limit>; PROBE_JACKS] {
        self.active
    }

    /// An alarm is on that the user has not acknowledged: the LED shows it.
    pub fn unacked(&self) -> bool {
        self.active.iter().zip(self.acked).any(|(a, ack)| a.is_some() && !ack)
    }

    /// Silences the LED for the alarms active now (a new alarm lights it again).
    pub fn ack(&mut self) {
        self.acked = self.active.map(|a| a.is_some());
    }

    /// Checks every probe against its limits. A probe without a reading keeps the alarm it has.
    pub fn step<'a>(
        &'a mut self,
        temps: &[Option<f64>; PROBE_JACKS],
        probes: &[Probe],
    ) -> impl Iterator<Item = Event> + 'a {
        let mut events: [Option<Event>; PROBE_JACKS] = [None; PROBE_JACKS];
        for (j, ev) in events.iter_mut().enumerate() {
            let Some(t) = temps[j] else { continue };
            let (lo, hi) = (probes[j].lo_f, probes[j].hi_f);
            let now = match self.active[j] {
                // latched: out only once 5F back inside
                Some(Limit::Hi) if hi.is_some_and(|h| t > h - HYSTERESIS_F) => Some(Limit::Hi),
                Some(Limit::Lo) if lo.is_some_and(|l| t < l + HYSTERESIS_F) => Some(Limit::Lo),
                _ if hi.is_some_and(|h| t >= h) => Some(Limit::Hi),
                _ if lo.is_some_and(|l| t <= l) => Some(Limit::Lo),
                _ => None,
            };
            let probe = j as u8;
            *ev = match (self.active[j], now) {
                (None, Some(which)) => Some(Event::Alarm { probe, which, t_f: t }),
                (Some(which), None) => Some(Event::AlarmClear { probe, which, t_f: t }),
                // straight from one limit to the other (only with the limits crossed): the new one
                (Some(a), Some(b)) if a != b => Some(Event::Alarm {
                    probe,
                    which: b,
                    t_f: t,
                }),
                _ => None,
            };
            if now.is_none() || now != self.active[j] {
                self.acked[j] = false;
            }
            self.active[j] = now;
        }
        events.into_iter().flatten()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn probes(lo: Option<f64>, hi: Option<f64>) -> [Probe; PROBE_JACKS] {
        core::array::from_fn(|j| {
            let mut p = Probe::default_for(j);
            (p.lo_f, p.hi_f) = (lo, hi);
            p
        })
    }

    fn feed(a: &mut Alarms, p: &[Probe], t0: Option<f64>) -> Vec<Event> {
        a.step(&[t0, None, None, None], p).collect()
    }

    #[test]
    fn a_high_alarm_fires_latches_and_clears_5f_inside() {
        let (mut a, p) = (Alarms::default(), probes(None, Some(203.0)));
        assert!(feed(&mut a, &p, Some(190.0)).is_empty());
        assert!(feed(&mut a, &p, Some(202.9)).is_empty());
        let fired = feed(&mut a, &p, Some(203.0));
        assert_eq!(
            fired,
            [Event::Alarm {
                probe: 0,
                which: Limit::Hi,
                t_f: 203.0
            }]
        );
        assert_eq!(a.active()[0], Some(Limit::Hi));
        // wobbling around the limit does not re-fire or clear
        for t in [201.0, 204.0, 198.5, 203.5] {
            assert!(feed(&mut a, &p, Some(t)).is_empty(), "{t}");
            assert_eq!(a.active()[0], Some(Limit::Hi));
        }
        assert!(feed(&mut a, &p, None).is_empty(), "no reading: the alarm stays");
        assert_eq!(a.active()[0], Some(Limit::Hi));
        let cleared = feed(&mut a, &p, Some(197.9));
        assert!(matches!(
            cleared[..],
            [Event::AlarmClear {
                probe: 0,
                which: Limit::Hi,
                ..
            }]
        ));
        assert_eq!(a.active()[0], None);
    }

    #[test]
    fn a_low_alarm_mirrors_it() {
        let (mut a, p) = (Alarms::default(), probes(Some(225.0), None));
        assert!(feed(&mut a, &p, Some(260.0)).is_empty());
        assert!(matches!(
            feed(&mut a, &p, Some(224.0))[..],
            [Event::Alarm { which: Limit::Lo, .. }]
        ));
        assert!(feed(&mut a, &p, Some(229.0)).is_empty());
        assert!(matches!(
            feed(&mut a, &p, Some(230.5))[..],
            [Event::AlarmClear { which: Limit::Lo, .. }]
        ));
    }

    #[test]
    fn ack_silences_the_led_until_the_next_alarm() {
        let (mut a, p) = (Alarms::default(), probes(Some(150.0), Some(203.0)));
        assert!(!a.unacked());
        feed(&mut a, &p, Some(210.0));
        assert!(a.unacked());
        a.ack();
        assert!(!a.unacked(), "acknowledged");
        assert_eq!(a.active()[0], Some(Limit::Hi), "but still reported");
        feed(&mut a, &p, Some(211.0));
        assert!(!a.unacked(), "staying in alarm stays acknowledged");
        // cleared and back in alarm: lights again
        feed(&mut a, &p, Some(190.0));
        feed(&mut a, &p, Some(204.0));
        assert!(a.unacked());
    }

    #[test]
    fn limits_off_and_changed_limits() {
        let (mut a, mut p) = (Alarms::default(), probes(None, None));
        assert!(feed(&mut a, &p, Some(900.0)).is_empty() && feed(&mut a, &p, Some(-10.0)).is_empty());
        p[0].hi_f = Some(300.0);
        assert!(matches!(feed(&mut a, &p, Some(310.0))[..], [Event::Alarm { .. }]));
        // the user raises the limit while it is ringing: it clears
        p[0].hi_f = Some(400.0);
        assert!(matches!(feed(&mut a, &p, Some(310.0))[..], [Event::AlarmClear { .. }]));
        // and takes the limit away
        p[0].hi_f = Some(300.0);
        feed(&mut a, &p, Some(310.0));
        p[0].hi_f = None;
        assert!(matches!(feed(&mut a, &p, Some(310.0))[..], [Event::AlarmClear { .. }]));
    }
}
