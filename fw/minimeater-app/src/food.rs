//! The food probes' estimators (`.4.22`), one per jack, run every control period alongside the
//! pit loop: they see each jack's reading, the pit, and whether the lid has the pit disturbed.
//!
//! [`StallResponder`] carries out each probe's stall policy (`.4.23`): the wrap reminder is raised
//! with the stall's start, and a bump goes through [`Pit::bump`], a normal setpoint change.

use heapless::Vec;
use minimeater_core::food::{Food, FoodEstimator, Stall};

use crate::config::{Mode, Probe, StallPolicy};
use crate::hal::PROBE_JACKS;
use crate::pit::{Event, Phase, Pit};

/// A bump with no stall to end it (the estimator starts over after a power cut, and a probe may be
/// pulled) is put back after this long without one, s.
const RESTORE_AFTER_S: f64 = 900.0;

pub struct Foods {
    est: [FoodEstimator; PROBE_JACKS],
    events: Vec<Event, { 2 * PROBE_JACKS }>,
}

impl Foods {
    pub fn new(probes: &[Probe; PROBE_JACKS]) -> Self {
        Self {
            est: core::array::from_fn(|i| FoodEstimator::new(probes[i].cut)),
            events: Vec::new(),
        }
    }

    fn stall(&self, jack: usize) -> Stall {
        self.est[jack].food().stall
    }

    /// `probes[n].food` in the status; `None` for the pit jack and a jack without a reading.
    pub fn food(&self, jack: usize, temps: &[Option<f64>; PROBE_JACKS], pit_jack: usize) -> Option<Food> {
        (jack != pit_jack && temps[jack].is_some()).then(|| self.est[jack].food())
    }

    /// One control period: each jack's reading (`None` unless `ok`), the pit jack, the pit, and the
    /// loop's phase (the lid disturbs the pit). Returns the stall events raised.
    pub fn step(
        &mut self,
        temps: &[Option<f64>; PROBE_JACKS],
        pit_jack: usize,
        pit_f: Option<f64>,
        phase: Phase,
        probes: &[Probe; PROBE_JACKS],
        dt: f64,
    ) -> &[Event] {
        self.events.clear();
        let disturbed = matches!(phase, Phase::Lid | Phase::Recover);
        for (i, e) in self.est.iter_mut().enumerate() {
            if e.cut != probes[i].cut {
                *e = FoodEstimator::new(probes[i].cut);
            }
            if i == pit_jack {
                continue;
            }
            let target = probes[i].hi_f.or(probes[i].cut.target_f());
            let before = e.food().stall;
            let now = e.update(temps[i], pit_f, target, disturbed, dt).stall;
            let (probe, t_f) = (i as u8, temps[i].unwrap_or(f64::NAN));
            let (ev, wrap) = match (before, now) {
                (Stall::None, Stall::In { .. }) => (
                    Some(Event::StallStart { probe, t_f }),
                    (probes[i].stall == StallPolicy::Wrap).then_some(Event::WrapReminder { probe, t_f }),
                ),
                (Stall::In { .. }, Stall::Done) => (Some(Event::StallEnd { probe, t_f }), None),
                _ => (None, None),
            };
            for ev in ev.into_iter().chain(wrap) {
                let _ = self.events.push(ev);
            }
        }
        &self.events
    }
}

/// Raises the setpoint while a probe whose policy is `bump` is in its stall, and puts it back when
/// the stall breaks (`.4.23`).
#[derive(Default)]
pub struct StallResponder {
    /// Time with a bump on and no stall in progress, s.
    quiet_s: f64,
}

impl StallResponder {
    /// One control period, after [`Foods::step`]. A probe with no reading does not count as stalled
    /// (it cannot say when the stall ends). Returns the event if the setpoint moved.
    pub fn step(
        &mut self,
        foods: &Foods,
        temps: &[Option<f64>; PROBE_JACKS],
        pit_jack: usize,
        probes: &[Probe; PROBE_JACKS],
        pit: &mut Pit,
        dt: f64,
    ) -> Option<Event> {
        let bumping = |i: usize| i != pit_jack && probes[i].stall == StallPolicy::Bump;
        let want = (0..PROBE_JACKS)
            .filter(|&i| bumping(i) && temps[i].is_some() && matches!(foods.stall(i), Stall::In { .. }))
            .max_by(|&a, &b| probes[a].bump_f.total_cmp(&probes[b].bump_f));
        let running = pit.cook().mode == Mode::Run;
        if let Some(i) = want.filter(|_| running) {
            self.quiet_s = 0.0;
            return pit
                .bump(probes[i].bump_f)
                .map(|sp_f| Event::StallBump { probe: i as u8, sp_f });
        }
        if pit.cook().base_sp.is_none() {
            self.quiet_s = 0.0;
            return None;
        }
        self.quiet_s += dt;
        let broke = (0..PROBE_JACKS).any(|i| bumping(i) && foods.stall(i) == Stall::Done);
        if running && !broke && self.quiet_s < RESTORE_AFTER_S {
            return None;
        }
        self.quiet_s = 0.0;
        pit.unbump().map(|sp_f| Event::StallRestore { sp_f })
    }
}
