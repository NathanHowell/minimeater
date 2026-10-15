//! The simulated board behaves like the real one through the `minimeater-app` traits: the generic
//! application code (`selftest::survey`) runs on it, and probes, actuators, flash and button map onto
//! the plant the way the hardware would.

use core::cell::RefCell;

use embassy_futures::block_on;
use embedded_hal::digital::OutputPin;
use embedded_hal::pwm::SetDutyCycle;
use embedded_storage_async::nor_flash::{NorFlash, ReadNorFlash};
use minimeater_app::hal::{FlashRegion, HalError, Servo, Tach};
use minimeater_app::selftest::survey;
use minimeater_core::model::airflow::AirPath;
use minimeater_core::model::{Ambient, top_vent};
use minimeater_core::thermistor::{Jack, Preset};
use minimeater_core::units::k_to_f;
use minimeater_sim::hal::{Wiring, World, io};
use minimeater_sim::{Dynamics, Environment, Meat, Plant};

fn world() -> World {
    let ambient = Ambient::default();
    let mut plant = Plant::steady(AirPath::default(), Dynamics::default(), &ambient, 225.0, 0.0).unwrap();
    plant.meats[0] = Some(Meat::brisket());
    let env = Environment {
        ambient,
        top_area_m2: top_vent::SLIDER_QUARTER,
        lid_open: false,
        food_load_w: 0.0,
    };
    let mut w = World::new(plant, env);
    w.jacks = [
        Wiring::Pit(Preset::ThermoworksPro.thermistor()),
        Wiring::Food(0, Preset::MaverickEt732.thermistor()),
        Wiring::Empty,
        Wiring::Short,
    ];
    w
}

fn probe_f(j: Result<Jack, HalError>, p: Preset) -> f64 {
    let Ok(Jack::Probe(r)) = j else { panic!("{j:?}") };
    k_to_f(p.thermistor().t_k(r))
}

#[test]
fn survey_reads_the_plant_through_the_app() {
    let w = RefCell::new(world());
    let mut io = io(&w);
    let r = block_on(survey(&mut io));
    assert!((probe_f(r.jacks[0], Preset::ThermoworksPro) - 225.0).abs() < 1e-6);
    assert!((probe_f(r.jacks[1], Preset::MaverickEt732) - w.borrow().plant.meats[0].unwrap().core_f()).abs() < 1e-6);
    assert_eq!(r.jacks[2], Ok(Jack::Open));
    assert_eq!(r.jacks[3], Ok(Jack::Short));
    // the survey is the sensor's own reading: 20C air plus its share of the 35C board's heat
    assert!((r.ambient_c.unwrap() - 21.8).abs() < 1e-9);
    assert_eq!(r.usb_ma, Some(3000));
}

#[test]
fn servo_moves_flap_only_while_powered() {
    let w = RefCell::new(world());
    let mut io = io(&w);
    io.servo.set_pulse_us(2000);
    assert_eq!(w.borrow().flap_deg, 0.0, "unpowered servo must not move");
    io.servo_power.set_high().unwrap();
    assert_eq!(w.borrow().flap_deg, 90.0);
    io.servo.set_pulse_us(1500);
    assert_eq!(w.borrow().flap_deg, 45.0);
    io.servo_power.set_low().unwrap();
    io.servo.set_pulse_us(1000);
    assert_eq!(w.borrow().flap_deg, 45.0, "gear friction holds the flap");
    assert!((w.borrow().damper() - (1.0 - 45f64.to_radians().cos())).abs() < 1e-12);
}

#[test]
fn fan_duty_drives_plant_and_tach() {
    let w = RefCell::new(world());
    let mut io = io(&w);
    io.fan.set_duty_cycle_percent(50).unwrap();
    w.borrow_mut().advance(1.0);
    assert_eq!(
        io.tach.rpm(),
        Some(0.0),
        "an unpowered fan does not turn whatever the duty"
    );
    io.fan_power.set_high().unwrap();
    for _ in 0..10 {
        w.borrow_mut().advance(1.0);
    }
    let rpm = io.tach.rpm().unwrap();
    assert!((rpm - 0.5 * 3000.0).abs() < 2.0, "{rpm}");
    assert_eq!(io.tach.rpm(), None, "tach is reset on read");
    // below the fan's minimum speed (600 rpm = 20%) it stalls
    io.fan.set_duty_cycle_percent(10).unwrap();
    w.borrow_mut().advance(1.0);
    assert_eq!(io.tach.rpm(), Some(0.0));
}

#[test]
fn config_flash_has_nor_semantics_and_regions_bound_it() {
    let w = RefCell::new(world());
    let io = io(&w);
    let mut region = FlashRegion::new::<4096, 8192>(io.config);
    let mut b = [0u8; 4];
    block_on(async {
        region.erase(0, 4096).await.unwrap();
        region.write(0, &[0x0F, 0xF0, 0xAA, 0x55]).await.unwrap();
        // without an erase, writing can only clear bits
        region.write(0, &[0xF0, 0xFF, 0xFF, 0xFF]).await.unwrap();
        region.read(0, &mut b).await.unwrap();
        assert!(region.write(8192, &[0; 4]).await.is_err(), "past the region");
        assert!(region.write(2, &[0; 4]).await.is_err(), "unaligned");
    });
    assert_eq!(b, [0x00, 0xF0, 0xAA, 0x55]);
    assert_eq!(w.borrow().config[4096..4100], b, "region offset applied");
    assert_eq!(region.capacity(), 8192);
}

#[test]
fn button_wait_wakes_on_press() {
    use embedded_hal_async::digital::Wait;
    let w = RefCell::new(world());
    let mut io = io(&w);
    // the press arrives while the task is parked: poll once, press, then the future completes
    let mut pressed = core::pin::pin!(io.button.wait_for_low());
    let waker = core::task::Waker::noop();
    let mut cx = core::task::Context::from_waker(waker);
    assert!(pressed.as_mut().poll(&mut cx).is_pending());
    w.borrow_mut().set_button(true);
    assert!(pressed.as_mut().poll(&mut cx).is_ready());
}
