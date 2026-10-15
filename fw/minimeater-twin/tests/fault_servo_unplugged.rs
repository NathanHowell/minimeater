//! An unplugged servo (`minimeat-y8a`): a real move draws nothing, `servo` is raised within a few moves and cleared once
//! it is back. The cook carries on.

mod servo_current;

use minimeater_sim::hal::Faults;
use servo_current::{check, with};

#[test]
fn an_unplugged_servo_raises_servo_and_clears_once_back() {
    check(
        "unplugged",
        &with(Faults {
            servo_unplugged: true,
            ..Faults::default()
        }),
    );
}
