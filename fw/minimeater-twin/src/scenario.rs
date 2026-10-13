//! Twin scenarios: the board and egg at power-on, the settings in flash, and what happens when.

use minimeater_app::config::{Mode, Settings, StallPolicy};
use minimeater_app::pit::Command;
use minimeater_app::proto;

use crate::net;
use minimeater_app::tasks::{Bus, Reply};
use minimeater_core::food::Cut;
use minimeater_core::model::airflow::AirPath;
use minimeater_core::model::{Ambient, top_vent};
use minimeater_core::thermistor::Preset;
use minimeater_core::units::c_to_k;
use minimeater_sim::hal::{Faults, Wiring, World};
use minimeater_sim::{Dynamics, Environment, Fuel, Meat, Plant};

/// Something that happens to the egg or the board, or something the user does.
#[derive(Clone, Copy, Debug)]
pub enum Action {
    /// What arrives over MQTT.
    Command(Command),
    Lid(bool),
    Weather(Ambient),
    /// Food on (or off) the grate, in meat slot `slot`.
    Meat {
        slot: usize,
        meat: Option<Meat>,
    },
    Wire {
        jack: usize,
        wiring: Wiring,
    },
    /// The hardware faults in force from now on (all of them: set what should stay).
    Faults(Faults),
    /// The broker delivers a message to the device (it must have an `mqtt` setting to be connected).
    Publish(net::Inject),
    /// The USB-C source's advertised current from now on (`None`: nothing detected).
    Usb(Option<u16>),
    /// The broker goes silent and refuses connections (`false`), or comes back.
    Broker(bool),
    /// A `~/set/...` or `~/cmd/...` message, as the MQTT client would receive it: `topic` is the
    /// part after the base topic. It is parsed, then sent as request number `id`.
    Mqtt {
        id: u16,
        topic: &'static str,
        payload: &'static str,
    },
}

impl Action {
    pub(crate) fn apply(self, w: &mut World, bus: &Bus, link: &net::Link) {
        match self {
            Action::Command(c) => {
                if !bus.send(0, c.into()) {
                    log::warn!("twin: command queue full, dropped {c:?}");
                }
            }
            Action::Lid(open) => w.env.lid_open = open,
            Action::Weather(a) => w.env.ambient = a,
            Action::Meat { slot, meat } => w.plant.meats[slot] = meat,
            Action::Wire { jack, wiring } => w.jacks[jack] = wiring,
            Action::Faults(f) => w.faults = f,
            Action::Usb(ma) => w.usb_ma = ma,
            Action::Publish(m) => link.inject(m),
            Action::Broker(up) => link.set_up(up),
            Action::Mqtt { id, topic, payload } => {
                // a message that does not parse is answered at once, as the client would
                match proto::parse(topic, payload.as_bytes(), false) {
                    Ok(req) => {
                        if !bus.send(id, req) {
                            log::warn!("twin: request queue full, dropped {topic}");
                        }
                    }
                    Err(e) => {
                        bus.replies
                            .immediate_publisher()
                            .publish_immediate(Reply { id, result: Err(e) });
                    }
                }
            }
        }
    }
}

/// How the egg is when the controller powers on.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Start {
    /// Cold: ambient, fuel unlit.
    Cold,
    /// Already at steady state at this pit temperature, F.
    Steady(f64),
}

#[derive(Clone)]
pub struct Scenario {
    pub name: &'static str,
    pub path: AirPath,
    pub ambient: Ambient,
    /// Daisy-wheel open area, m^2.
    pub top_area_m2: f64,
    pub start: Start,
    pub fuel: Fuel,
    /// In flash at power-on.
    pub settings: Settings,
    pub jacks: [Wiring; 4],
    /// (seconds from power-on, action).
    pub actions: Vec<(f64, Action)>,
    pub hours: f64,
    /// Where the metrics look (seconds): the window of the first approach to the setpoint, and the
    /// windows in which the pit should be holding it. The setpoint is whatever the controller had.
    pub approach: (f64, f64),
    pub holds: Vec<(f64, f64)>,
}

const H: f64 = 3600.0;

impl Scenario {
    /// Every scenario of the suite (`.4.16.3`), by name.
    pub const NAMES: [&'static str; 20] = [
        "kamado-brisket",
        "kamado-hold",
        "chili-150",
        "hot-fast-325",
        "step-up",
        "step-down",
        "lid-30s",
        "lid-2min",
        "cold-front",
        "hot-day",
        "fuel-burnout",
        "probe-unplug",
        "sensor-noise",
        "adc-outage",
        "fan-jam",
        "servo-stuck",
        "ambient-nack",
        "stall-notify",
        "stall-wrap",
        "stall-bump",
    ];

    pub fn named(name: &str) -> Option<Self> {
        Some(match name {
            "kamado-brisket" => Self::kamado_brisket(),
            "kamado-hold" => Self::kamado_hold(6.0),
            "chili-150" => Self::chili_150(),
            "hot-fast-325" => Self::hot_fast_325(),
            "step-up" => Self::steps("step-up", 225.0, 275.0),
            "step-down" => Self::steps("step-down", 275.0, 225.0),
            "lid-30s" => Self::lid("lid-30s", 30.0),
            "lid-2min" => Self::lid("lid-2min", 120.0),
            "cold-front" => Self::cold_front(),
            "hot-day" => Self::hot_day(),
            "fuel-burnout" => Self::fuel_burnout(),
            "probe-unplug" => Self::probe_unplug(),
            "sensor-noise" => Self::sensor_noise(),
            "adc-outage" => Self::adc_outage(),
            "fan-jam" => Self::fan_jam(),
            "servo-stuck" => Self::servo_stuck(),
            "ambient-nack" => Self::ambient_nack(),
            "stall-notify" => Self::stall("stall-notify", StallPolicy::Notify),
            "stall-wrap" => Self::stall("stall-wrap", StallPolicy::Wrap),
            "stall-bump" => Self::stall("stall-bump", StallPolicy::Bump),
            _ => return None,
        })
    }

    /// A cold egg lit off to `sp` and held: the base of most scenarios. The pit holds from 1.5 h.
    fn egg(name: &'static str, sp: f64, ambient: Ambient, top_area_m2: f64, hours: f64) -> Self {
        let pit_probe = Preset::ThermoworksPro.thermistor();
        let mut settings = Settings::default();
        settings.cook.mode = Mode::Off;
        settings.probes[0].name = "Pit".try_into().unwrap();
        Self {
            name,
            path: AirPath::default(),
            ambient,
            top_area_m2,
            start: Start::Cold,
            fuel: Fuel::default(),
            settings,
            jacks: [Wiring::Pit(pit_probe), Wiring::Empty, Wiring::Empty, Wiring::Empty],
            actions: vec![
                (0.0, Action::Command(Command::Sp(sp))),
                (0.0, Action::Command(Command::Mode(Mode::Run))),
            ],
            hours,
            approach: (0.0, 1.5 * H),
            holds: vec![(1.5 * H, hours * H)],
        }
    }

    fn lid_open(&mut self, at_s: f64, for_s: f64) {
        self.actions.push((at_s, Action::Lid(true)));
        self.actions.push((at_s + for_s, Action::Lid(false)));
    }

    fn faults(&mut self, at_s: f64, f: Faults) {
        self.actions.push((at_s, Action::Faults(f)));
    }

    /// A packer brisket on a cold egg: light-off to 250F, the brisket on after an hour, the lid up
    /// for a minute every couple of hours to spritz, until it is done.
    pub fn kamado_brisket() -> Self {
        let pit_probe = Preset::ThermoworksPro.thermistor();
        let mut sc = Self::egg(
            "kamado-brisket",
            250.0,
            Ambient::default(),
            top_vent::SLIDER_QUARTER,
            18.0,
        );
        sc.settings.probes[1].name = "Brisket".try_into().unwrap();
        sc.settings.probes[1].cut = Cut::Brisket;
        sc.settings.probes[1].hi_f = Some(203.0);
        for (t, a) in [
            (
                1.0 * H,
                Action::Meat {
                    slot: 0,
                    meat: Some(Meat::brisket()),
                },
            ),
            (
                1.0 * H,
                Action::Wire {
                    jack: 1,
                    wiring: Wiring::Food(0, pit_probe),
                },
            ),
        ] {
            sc.actions.push((t, a));
        }
        for at in [1.0, 4.0, 6.0, 8.0, 10.0] {
            sc.lid_open(at * H - 30.0, 60.0);
        }
        // the first lid also puts a cold 6 kg brisket on: hold is judged from 2.5 h, and between lids
        sc.holds = vec![
            (2.5 * H, 3.9 * H),
            (4.5 * H, 5.9 * H),
            (6.5 * H, 7.9 * H),
            (8.5 * H, 9.9 * H),
            (10.5 * H, 15.0 * H),
        ];
        sc
    }

    /// The brisket cook with its probe's stall policy set (`.4.23`). Its stall runs from about 6.5 h
    /// to 11.3 h at 250F; the holds judged are the ones before it.
    fn stall(name: &'static str, policy: StallPolicy) -> Self {
        let mut sc = Self::kamado_brisket();
        sc.name = name;
        sc.settings.probes[1].stall = policy;
        sc.holds.truncate(2);
        sc
    }

    /// A cold egg lit off to 250F and left to hold, with nothing on the grate.
    pub fn kamado_hold(hours: f64) -> Self {
        Self::egg(
            "kamado-hold",
            250.0,
            Ambient::default(),
            top_vent::SLIDER_QUARTER,
            hours,
        )
    }

    /// Chili drying at 150F, the daisy wheel cracked, a minute with the lid up at 4 h.
    fn chili_150() -> Self {
        let mut sc = Self::egg("chili-150", 150.0, Ambient::default(), top_vent::PETALS_OPEN, 6.0);
        sc.lid_open(4.0 * H, 60.0);
        sc.holds = vec![(2.0 * H, 4.0 * H), (4.5 * H, 6.0 * H)];
        sc.approach = (0.0, 2.0 * H);
        sc
    }

    /// Hot and fast: 325F on a cold, windy day, the lid up for two minutes at 2 h.
    fn hot_fast_325() -> Self {
        let cold = Ambient {
            t_k: c_to_k(0.0),
            wind_m_s: 3.0,
        };
        let mut sc = Self::egg("hot-fast-325", 325.0, cold, top_vent::SLIDER_HALF, 4.0);
        sc.lid_open(2.0 * H, 120.0);
        sc.holds = vec![(1.25 * H, 2.0 * H), (2.5 * H, 4.0 * H)];
        sc.approach = (0.0, 1.25 * H);
        sc
    }

    /// Setpoint step from a steady egg at `from` to `to` at 0.5 h.
    fn steps(name: &'static str, from: f64, to: f64) -> Self {
        let mut sc = Self::egg(name, from, Ambient::default(), top_vent::SLIDER_QUARTER, 3.5);
        sc.start = Start::Steady(from);
        sc.actions.push((0.5 * H, Action::Command(Command::Sp(to))));
        sc.approach = (0.5 * H, 1.5 * H);
        sc.holds = vec![(1.5 * H, 3.5 * H)];
        sc
    }

    /// A steady 250F hold with the lid up for `secs` at 2 h.
    fn lid(name: &'static str, secs: f64) -> Self {
        let mut sc = Self::egg(name, 250.0, Ambient::default(), top_vent::SLIDER_QUARTER, 4.0);
        sc.start = Start::Steady(250.0);
        sc.lid_open(2.0 * H, secs);
        sc.approach = (0.0, 0.5 * H);
        sc.holds = vec![(0.5 * H, 2.0 * H), (2.75 * H, 4.0 * H)];
        sc
    }

    /// A cold front at 2 h: 20C and calm to 5C and 4 m/s.
    fn cold_front() -> Self {
        let mut sc = Self::kamado_hold(5.0);
        sc.name = "cold-front";
        sc.actions.push((
            2.0 * H,
            Action::Weather(Ambient {
                t_k: c_to_k(5.0),
                wind_m_s: 4.0,
            }),
        ));
        sc
    }

    /// A hot afternoon: 35C and calm.
    fn hot_day() -> Self {
        let hot = Ambient {
            t_k: c_to_k(35.0),
            wind_m_s: 0.0,
        };
        Self::egg("hot-day", 250.0, hot, top_vent::SLIDER_QUARTER, 5.0)
    }

    /// Half a kilo of charcoal in the firebox: the fire fades hours in, and the controller must say so.
    fn fuel_burnout() -> Self {
        let mut sc = Self::kamado_hold(14.0);
        sc.name = "fuel-burnout";
        sc.fuel = Fuel {
            kg: 0.5,
            ..Fuel::default()
        };
        // judged while there is fuel; what follows is the fire's doing, not the controller's
        sc.holds = vec![(1.5 * H, 3.0 * H)];
        sc
    }

    /// The pit probe pulled out for five minutes at 3 h: the controller fails safe, then resumes.
    fn probe_unplug() -> Self {
        let mut sc = Self::kamado_hold(5.0);
        sc.name = "probe-unplug";
        let probe = Preset::ThermoworksPro.thermistor();
        sc.actions.push((
            3.0 * H,
            Action::Wire {
                jack: 0,
                wiring: Wiring::Empty,
            },
        ));
        sc.actions.push((
            3.0 * H + 300.0,
            Action::Wire {
                jack: 0,
                wiring: Wiring::Pit(probe),
            },
        ));
        sc.holds = vec![(1.5 * H, 3.0 * H), (3.5 * H, 5.0 * H)];
        sc
    }

    /// Noisy conversions and EMI spikes from 2 h to 4 h.
    fn sensor_noise() -> Self {
        let mut sc = Self::kamado_hold(5.0);
        sc.name = "sensor-noise";
        sc.faults(
            2.0 * H,
            Faults {
                adc_noise: 3e-3,
                adc_glitch: 0.05,
                ..Faults::default()
            },
        );
        sc.faults(4.0 * H, Faults::default());
        sc
    }

    /// The probe ADC dead for three minutes at 2 h.
    fn adc_outage() -> Self {
        let mut sc = Self::kamado_hold(4.0);
        sc.name = "adc-outage";
        sc.faults(
            2.0 * H,
            Faults {
                adc_down: true,
                ..Faults::default()
            },
        );
        sc.faults(2.0 * H + 180.0, Faults::default());
        sc.holds = vec![(1.5 * H, 2.0 * H), (2.5 * H, 4.0 * H)];
        sc
    }

    /// The fan rotor jammed for ten minutes at 5 min, in the middle of light-off.
    fn fan_jam() -> Self {
        let mut sc = Self::kamado_hold(3.0);
        sc.name = "fan-jam";
        sc.faults(
            300.0,
            Faults {
                fan_jam: true,
                ..Faults::default()
            },
        );
        sc.faults(900.0, Faults::default());
        sc.approach = (0.0, 2.0 * H);
        sc.holds = vec![(2.0 * H, 3.0 * H)];
        sc
    }

    /// The flap stuck for twenty minutes at 2 h, with a setpoint step up to 300F in the middle of it.
    fn servo_stuck() -> Self {
        let mut sc = Self::kamado_hold(5.0);
        sc.name = "servo-stuck";
        sc.faults(
            2.0 * H,
            Faults {
                servo_stuck: true,
                ..Faults::default()
            },
        );
        sc.actions.push((2.0 * H + 60.0, Action::Command(Command::Sp(300.0))));
        sc.faults(2.0 * H + 1200.0, Faults::default());
        sc.holds = vec![(1.5 * H, 2.0 * H), (4.0 * H, 5.0 * H)];
        sc
    }

    /// The ambient sensor silent for an hour at 2 h.
    fn ambient_nack() -> Self {
        let mut sc = Self::kamado_hold(5.0);
        sc.name = "ambient-nack";
        sc.faults(
            2.0 * H,
            Faults {
                ambient_nack: true,
                ..Faults::default()
            },
        );
        sc.faults(3.0 * H, Faults::default());
        sc
    }

    /// Seconds at which the lid closes again, for the lid-recovery metrics.
    pub fn lid_closes(&self) -> Vec<f64> {
        let mut v: Vec<_> = self
            .actions
            .iter()
            .filter_map(|(t, a)| matches!(a, Action::Lid(false)).then_some(*t))
            .collect();
        v.sort_by(f64::total_cmp);
        v
    }

    pub(crate) fn world(&self) -> World {
        let plant = match self.start {
            Start::Cold => Plant::cold(self.path, Dynamics::default(), &self.ambient),
            Start::Steady(f) => Plant::steady(self.path, Dynamics::default(), &self.ambient, f, 0.0)
                .expect("the egg can hold that temperature"),
        };
        let mut plant = plant;
        plant.fuel = self.fuel;
        let env = Environment {
            ambient: self.ambient,
            top_area_m2: self.top_area_m2,
            lid_open: false,
            food_load_w: 0.0,
        };
        let mut w = World::new(plant, env);
        w.jacks = self.jacks;
        w
    }
}
