//! The servo's current (`minimeat-y8a`): a cook publishes each move's peak and mean, and no fault.

mod servo_current;

use minimeater_app::pit::Event;
use minimeater_sim::hal::Faults;
use servo_current::{when, with};

#[test]
fn a_cook_reports_the_servo_current_and_no_fault() {
    let r = with(Faults::default());
    let m = r
        .samples
        .iter()
        .filter_map(|s| s.status.servo_ma)
        .max_by_key(|m| m.peak)
        .expect("a move was reported");
    println!("largest move: {m:?}");
    assert!((150..=260).contains(&m.peak), "{m:?}");
    assert!(m.mean < m.peak);
    assert_eq!(when(&r, |e| matches!(e, Event::Fault(_))), None, "{:?}", r.events);
}
