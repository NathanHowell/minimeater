//! Core/surface temperature of the default brisket in a constant-temperature pit, hourly.
//! `just fw::meat-curve 250`

use minimeater_core::model::Ambient;
use minimeater_core::units::f_to_k;
use minimeater_sim::Meat;

fn main() {
    let pit_f: f64 = std::env::args()
        .nth(1)
        .map_or(250.0, |a| a.parse().expect("pit temperature in F"));
    let mut m = Meat::brisket();
    let amb = Ambient::default();
    let mut done = None;
    println!("hour  core F  surface F  heat W");
    for i in 0..(20 * 1800) {
        let q = m.step(2.0, f_to_k(pit_f), &amb);
        if i % 1800 == 0 {
            println!("{:4}  {:6.0}  {:9.0}  {:6.0}", i / 1800, m.core_f(), m.surface_f(), q);
        }
        if done.is_none() && m.core_f() >= 203.0 {
            done = Some(i as f64 * 2.0 / 3600.0);
        }
    }
    println!("203F at {done:.1?} h; evaporated {:.2} kg", m.evaporated_kg());
}
