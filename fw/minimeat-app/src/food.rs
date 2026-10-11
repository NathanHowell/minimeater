//! The food probes' estimators (`.4.22`), one per jack, run every control period alongside the
//! pit loop: they see each jack's reading, the pit, and whether the lid has the pit disturbed.

use heapless::Vec;
use minimeat_core::food::{Food, FoodEstimator, Stall};

use crate::config::Probe;
use crate::hal::PROBE_JACKS;
use crate::pit::{Event, Phase};

pub struct Foods {
    est: [FoodEstimator; PROBE_JACKS],
    events: Vec<Event, PROBE_JACKS>,
}

impl Foods {
    pub fn new(probes: &[Probe; PROBE_JACKS]) -> Self {
        Self {
            est: core::array::from_fn(|i| FoodEstimator::new(probes[i].cut)),
            events: Vec::new(),
        }
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
            let ev = match (before, now) {
                (Stall::None, Stall::In { .. }) => Some(Event::StallStart { probe, t_f }),
                (Stall::In { .. }, Stall::Done) => Some(Event::StallEnd { probe, t_f }),
                _ => None,
            };
            if let Some(ev) = ev {
                let _ = self.events.push(ev);
            }
        }
        &self.events
    }
}
