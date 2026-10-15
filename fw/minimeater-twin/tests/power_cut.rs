//! A power cut mid-cook (`.4.24`'s acceptance): the settings the control task saved are what the next boot finds in
//! flash, and the cook carries on from them: still in run at the same setpoint, the airflow that was holding the pit
//! still applied, nothing closed or started over. The flash is the twin's, kept across the "reboot" as the chip's is.

use std::cell::RefCell;

use embassy_futures::block_on;
use minimeater_app::config::{ConfigStore, Mode};
use minimeater_app::pit::{Command, PERIOD_S, Phase, Pit};
use minimeater_core::model::Ambient;
use minimeater_core::model::airflow::AirPath;
use minimeater_sim::hal::{World, io};
use minimeater_twin::Scenario;

#[test]
fn a_cook_resumes_from_the_flash_after_a_power_cut() {
    let sc = Scenario::kamado_hold(1.0);
    let world: &'static RefCell<World> = Box::leak(Box::new(RefCell::new(sc.world())));
    let ambient = Ambient::default();
    let path = AirPath::default();

    // the first boot: a cook is started and holds the pit for a while
    let (mut store, mut settings) = block_on(ConfigStore::open(io(world).config));
    block_on(store.save(&sc.settings)).expect("settings saved");
    let mut pit = Pit::new(&sc.settings, path, ambient);
    pit.command(Command::Mode(Mode::Run)).unwrap();
    pit.command(Command::Sp(275.0)).unwrap();
    let mut last = None;
    for _ in 0..180 {
        let (out, _) = pit.step(Some(275.0), ambient, PERIOD_S);
        last = Some(out);
    }
    let before = last.unwrap();
    assert!(matches!(before.phase, Phase::Hold), "{:?}", before.phase);
    pit.persist(&mut settings);
    block_on(store.save(&settings)).expect("saved");

    // the cut: everything in RAM is gone, only the flash is left
    drop((store, pit, settings));
    let (_store, settings) = block_on(ConfigStore::open(io(world).config));
    assert_eq!(settings.cook.mode, Mode::Run);
    assert_eq!(settings.cook.sp_f, 275.0);
    assert!(settings.cook.hold.is_some(), "the airflow that held the pit was saved");

    // the next boot: the pit is a little under the setpoint, as the egg cools in a dead fan's absence
    let mut pit = Pit::new(&settings, path, ambient);
    let (out, _) = pit.step(Some(272.0), ambient, PERIOD_S);
    assert!(matches!(out.phase, Phase::Startup | Phase::Hold), "{:?}", out.phase);
    assert!(out.q > 0.0 && out.act.damper > 0.0, "{out:?}");
    // seeded with the holding airflow: within a few CFM of where it was, not a restart from closed
    assert!(
        (out.q - before.q).abs() < 1.0,
        "resumes at {:.2} CFM, held at {:.2}",
        out.q,
        before.q
    );
}
