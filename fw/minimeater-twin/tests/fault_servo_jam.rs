//! A flap that jams (`minimeat-y8a`): the servo pushes against it while it should hold, `servo` is raised within a few
//! moves and cleared once free. It only warns: the cook carries on, and the load switch is not tripped.

mod servo_current;

use minimeater_sim::hal::Faults;
use servo_current::{check, with};

#[test]
fn a_jammed_flap_raises_servo_and_clears_once_free() {
    check(
        "jam",
        &with(Faults {
            servo_jam: true,
            ..Faults::default()
        }),
    );
}
