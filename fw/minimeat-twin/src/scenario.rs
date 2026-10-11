//! Twin scenarios: the board and egg at power-on, the settings in flash, and what happens when.

use minimeat_app::config::{Mode, Settings};
use minimeat_app::pit::Command;
use minimeat_app::tasks::Bus;
use minimeat_core::food::Cut;
use minimeat_core::model::airflow::AirPath;
use minimeat_core::model::{Ambient, top_vent};
use minimeat_core::thermistor::Preset;
use minimeat_sim::hal::{Wiring, World};
use minimeat_sim::{Dynamics, Environment, Meat, Plant};

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
}

impl Action {
    pub(crate) fn apply(self, w: &mut World, bus: &Bus) {
        match self {
            Action::Command(c) => {
                if bus.commands.try_send(c).is_err() {
                    log::warn!("twin: command queue full, dropped {c:?}");
                }
            }
            Action::Lid(open) => w.env.lid_open = open,
            Action::Weather(a) => w.env.ambient = a,
            Action::Meat { slot, meat } => w.plant.meats[slot] = meat,
            Action::Wire { jack, wiring } => w.jacks[jack] = wiring,
        }
    }
}

#[derive(Clone)]
pub struct Scenario {
    pub name: &'static str,
    pub path: AirPath,
    pub ambient: Ambient,
    /// In flash at power-on.
    pub settings: Settings,
    pub jacks: [Wiring; 4],
    /// (seconds from power-on, action).
    pub actions: Vec<(f64, Action)>,
    pub hours: f64,
}

impl Scenario {
    pub const NAMES: [&'static str; 1] = ["kamado-brisket"];

    pub fn named(name: &str) -> Option<Self> {
        match name {
            "kamado-brisket" => Some(Self::kamado_brisket()),
            _ => None,
        }
    }

    /// A packer brisket on a cold egg: light-off to 250F, the brisket on after an hour, the lid up
    /// for a minute every couple of hours to spritz, until it is done.
    pub fn kamado_brisket() -> Self {
        let pit_probe = Preset::ThermoworksPro.thermistor();
        let mut settings = Settings::default();
        settings.cook.mode = Mode::Off;
        settings.probes[0].name = "Pit".try_into().unwrap();
        settings.probes[1].name = "Brisket".try_into().unwrap();
        settings.probes[1].cut = Cut::Brisket;
        settings.probes[1].hi_f = Some(203.0);
        let h = 3600.0;
        let mut actions = vec![
            (0.0, Action::Command(Command::Sp(250.0))),
            (0.0, Action::Command(Command::Mode(Mode::Run))),
            (
                1.0 * h,
                Action::Meat {
                    slot: 0,
                    meat: Some(Meat::brisket()),
                },
            ),
            (
                1.0 * h,
                Action::Wire {
                    jack: 1,
                    wiring: Wiring::Food(0, pit_probe),
                },
            ),
        ];
        for at in [1.0, 4.0, 6.0, 8.0, 10.0] {
            actions.push((at * h - 30.0, Action::Lid(true)));
            actions.push((at * h + 30.0, Action::Lid(false)));
        }
        Self {
            name: "kamado-brisket",
            path: AirPath::default(),
            ambient: Ambient::default(),
            settings,
            jacks: [Wiring::Pit(pit_probe), Wiring::Empty, Wiring::Empty, Wiring::Empty],
            actions,
            hours: 18.0,
        }
    }

    pub(crate) fn world(&self) -> World {
        let plant = Plant::cold(self.path, Dynamics::default(), &self.ambient);
        let env = Environment {
            ambient: self.ambient,
            top_area_m2: top_vent::SLIDER_QUARTER,
            lid_open: false,
            food_load_w: 0.0,
        };
        let mut w = World::new(plant, env);
        w.jacks = self.jacks;
        w
    }
}
