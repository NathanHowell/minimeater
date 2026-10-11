//! Persistent configuration (`.4.9`) on the simulated board's flash: settings survive a reboot,
//! a bad record only costs that record, and a power cut at *any* point of a save leaves every record
//! either at its old or its new value. Loading never fails.

use core::cell::{Cell, RefCell};

use embassy_futures::block_on;
use embedded_storage_async::nor_flash::{ErrorType, NorFlash, NorFlashError, NorFlashErrorKind, ReadNorFlash};
use minimeat_app::config::{ConfigStore, ControllerKind, Mode, Mqtt, ProbeKind, Settings, Wifi};
use minimeat_core::control::{Fopdt, Schedule};
use minimeat_core::model::airflow::AirPath;
use minimeat_core::model::{Ambient, top_vent};
use minimeat_core::thermistor::{Preset, Thermistor};
use minimeat_sim::hal::{CONFIG_BYTES, World, io};
use minimeat_sim::{Dynamics, Environment, Plant};

fn world() -> RefCell<World> {
    let ambient = Ambient::default();
    let plant = Plant::cold(AirPath::default(), Dynamics::default(), &ambient);
    let env = Environment {
        ambient,
        top_area_m2: top_vent::SLIDER_QUARTER,
        lid_open: false,
        food_load_w: 0.0,
    };
    RefCell::new(World::new(plant, env))
}

fn boot(w: &RefCell<World>) -> Settings {
    block_on(ConfigStore::open(io(w).config)).1
}

/// A mid-cook state that differs from the defaults everywhere.
fn cooking() -> Settings {
    let mut s = Settings::default();
    s.cook.mode = Mode::Run;
    s.cook.sp_f = 250.0;
    s.cook.controller = ControllerKind::FfPi;
    s.cook.pit_probe = 2;
    s.cook.autotune = true;
    s.cook.hold = Some((250.0, 0.72));
    s.schedule.insert(Fopdt {
        sp_f: 250.0,
        k: 99.0,
        tau_s: 450.0,
        theta_s: 120.0,
    });
    s.lid.boost = 2.0;
    s.probes[1].name = "Brisket flat".try_into().unwrap();
    s.probes[1].kind = ProbeKind::Preset(Preset::MaverickEt732);
    s.probes[1].hi_f = Some(203.0);
    s.probes[3].kind = ProbeKind::Custom(Thermistor {
        a: 7.3431e-4,
        b: 2.1574e-4,
        c: 9.5157e-8,
    });
    s.wifi = Some(Wifi {
        ssid: "egg-net".try_into().unwrap(),
        pass: "hunter22".try_into().unwrap(),
    });
    s.mqtt = Some(Mqtt {
        host: "homeassistant.local".try_into().unwrap(),
        port: 1883,
        user: "minimeat".try_into().unwrap(),
        pass: "pw".try_into().unwrap(),
    });
    s
}

#[test]
fn fresh_flash_gives_defaults_and_settings_survive_reboot() {
    let w = world();
    assert_eq!(boot(&w), Settings::default());
    let s = cooking();
    block_on(async {
        let (mut store, _) = ConfigStore::open(io(&w).config).await;
        store.save(&s).await.unwrap();
    });
    assert_eq!(boot(&w), s);
}

#[test]
fn unchanged_settings_are_not_rewritten() {
    let w = world();
    let s = cooking();
    block_on(async {
        let (mut store, _) = ConfigStore::open(io(&w).config).await;
        store.save(&s).await.unwrap();
        let before = w.borrow().config;
        store.save(&s).await.unwrap();
        assert!(w.borrow().config == before, "a no-op save wrote flash");
    });
}

#[test]
fn an_invalid_record_costs_only_itself() {
    let w = world();
    let mut s = cooking();
    s.cook.sp_f = 400.0; // outside the 150-325F scope: rejected on load
    block_on(async {
        let (mut store, _) = ConfigStore::open(io(&w).config).await;
        store.save(&s).await.unwrap();
    });
    let got = boot(&w);
    assert_eq!(got.cook, Settings::default().cook);
    assert_eq!(got.schedule, s.schedule);
    assert_eq!(got.probes, s.probes);
}

#[test]
fn garbage_flash_boots_with_defaults() {
    let w = world();
    let mut x = 0x1234_5678u32;
    for b in w.borrow_mut().config.iter_mut() {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        *b = x as u8;
    }
    assert_eq!(boot(&w), Settings::default());
    // and the store works afterwards
    let s = cooking();
    block_on(async {
        let (mut store, _) = ConfigStore::open(io(&w).config).await;
        store.save(&s).await.unwrap();
    });
    assert_eq!(boot(&w), s);
}

/// Flash that loses power after `budget` bytes have been written: the write in progress lands
/// partially (whole words up to the cut), and every later operation fails.
struct Cut<F> {
    inner: F,
    budget: Cell<usize>,
    dead: Cell<bool>,
    erased: Cell<bool>,
}

#[derive(Debug)]
struct PowerLost;

impl NorFlashError for PowerLost {
    fn kind(&self) -> NorFlashErrorKind {
        NorFlashErrorKind::Other
    }
}

impl<F: NorFlash> ErrorType for Cut<F> {
    type Error = PowerLost;
}

impl<F: NorFlash> ReadNorFlash for Cut<F> {
    const READ_SIZE: usize = F::READ_SIZE;

    async fn read(&mut self, offset: u32, bytes: &mut [u8]) -> Result<(), PowerLost> {
        if self.dead.get() {
            return Err(PowerLost);
        }
        self.inner.read(offset, bytes).await.map_err(|_| PowerLost)
    }

    fn capacity(&self) -> usize {
        self.inner.capacity()
    }
}

impl<F: NorFlash> NorFlash for Cut<F> {
    const WRITE_SIZE: usize = F::WRITE_SIZE;
    const ERASE_SIZE: usize = F::ERASE_SIZE;

    async fn erase(&mut self, from: u32, to: u32) -> Result<(), PowerLost> {
        if self.dead.get() {
            return Err(PowerLost);
        }
        // an erase is the expensive step: count it as one sector's worth of budget, cut = not done
        self.erased.set(true);
        ERASED.with(|e| e.set(true));
        let cost = F::ERASE_SIZE / 16;
        if self.budget.get() < cost {
            self.dead.set(true);
            return Err(PowerLost);
        }
        self.budget.set(self.budget.get() - cost);
        self.inner.erase(from, to).await.map_err(|_| PowerLost)
    }

    async fn write(&mut self, offset: u32, bytes: &[u8]) -> Result<(), PowerLost> {
        if self.dead.get() {
            return Err(PowerLost);
        }
        let left = self.budget.get();
        if bytes.len() <= left {
            self.budget.set(left - bytes.len());
            return self.inner.write(offset, bytes).await.map_err(|_| PowerLost);
        }
        let partial = left - left % F::WRITE_SIZE;
        if partial > 0 {
            self.inner
                .write(offset, &bytes[..partial])
                .await
                .map_err(|_| PowerLost)?;
        }
        self.dead.set(true);
        Err(PowerLost)
    }
}

#[test]
fn power_cut_at_any_point_of_a_save_loses_nothing_old() {
    let old = cooking();
    let mut new = old.clone();
    new.cook.sp_f = 275.0;
    new.cook.autotune = false;
    new.schedule.insert(Fopdt {
        sp_f: 275.0,
        k: 88.0,
        tau_s: 430.0,
        theta_s: 125.0,
    });
    new.probes[0].offset_f = -1.5;
    let (mut cuts, mut erases_cut) = (0, 0);
    let prefill = |history: usize| {
        let w = world();
        block_on(async {
            let (mut store, _) = ConfigStore::open(io(&w).config).await;
            for i in 0..history {
                let mut s = old.clone();
                s.cook.manual_fan = (i % 100) as f64;
                store.save(&s).await.unwrap();
            }
            store.save(&old).await.unwrap();
        });
        w.borrow().config
    };
    // the first history length at which saving `new` has to garbage-collect (erase a page)
    let full = (300..2000)
        .step_by(5)
        .find(|&h| {
            let w = world();
            w.borrow_mut().config = prefill(h);
            ERASED.with(|e| e.set(false));
            block_on(async {
                let flash = Cut {
                    inner: io(&w).config,
                    budget: Cell::new(usize::MAX),
                    dead: Cell::new(false),
                    erased: Cell::new(false),
                };
                let (mut store, _) = ConfigStore::open(flash).await;
                store.save(&new).await.unwrap();
            });
            ERASED.with(|e| e.get())
        })
        .expect("the store never filled up");
    // a nearly empty store, and one that must garbage-collect during the save
    for history in [40usize, full] {
        let w = world();
        let snapshot = prefill(history);
        for budget in (0..CONFIG_BYTES / 4).step_by(3) {
            w.borrow_mut().config = snapshot;
            ERASED.with(|e| e.set(false));
            let flash = Cut {
                inner: io(&w).config,
                budget: Cell::new(budget),
                dead: Cell::new(false),
                erased: Cell::new(false),
            };
            let (finished, erased) = block_on(async {
                let (mut store, loaded) = ConfigStore::open(flash).await;
                assert_eq!(loaded, old, "budget {budget}: the store did not load before the cut");
                let ok = store.save(&new).await.is_ok();
                (ok, store_erased(store))
            });
            cuts += usize::from(!finished);
            erases_cut += usize::from(!finished && erased);
            let got = boot(&w);
            assert!(
                got.cook == old.cook || got.cook == new.cook,
                "budget {budget}: cook {:?}",
                got.cook
            );
            assert!(
                got.schedule == old.schedule || got.schedule == new.schedule,
                "budget {budget}: schedule {:?}",
                got.schedule
            );
            assert!(got.probes[0] == old.probes[0] || got.probes[0] == new.probes[0]);
            assert_eq!(
                (&got.probes[1..], &got.wifi, &got.mqtt),
                (&old.probes[1..], &old.wifi, &old.mqtt)
            );
            if finished {
                assert_eq!(got, new, "budget {budget}");
                break;
            }
        }
    }
    println!("{cuts} cut points, {erases_cut} after an erase had started");
    assert!(cuts > 150, "only {cuts} cut points exercised");
    assert!(erases_cut > 0, "no cut landed during garbage collection");
}

fn store_erased<F: NorFlash>(_: ConfigStore<Cut<F>>) -> bool {
    ERASED.with(|e| e.get())
}

thread_local! {
    static ERASED: Cell<bool> = const { Cell::new(false) };
}

#[test]
fn schedule_written_by_autotune_round_trips() {
    let w = world();
    let mut s = Settings::default();
    for (i, sp) in [150.0, 175.0, 200.0, 225.0, 250.0, 275.0, 300.0, 325.0]
        .into_iter()
        .enumerate()
    {
        s.schedule.insert(Fopdt {
            sp_f: sp,
            k: 100.0 + i as f64,
            tau_s: 450.0,
            theta_s: 110.0,
        });
    }
    assert_eq!(s.schedule.models().len(), Schedule::CAPACITY);
    block_on(async {
        let (mut store, _) = ConfigStore::open(io(&w).config).await;
        store.save(&s).await.unwrap();
    });
    assert_eq!(boot(&w).schedule, s.schedule);
    const { assert!(CONFIG_BYTES >= 2 * 4096) };
}
