//! The status LED and the button (`.4.30`), the box's only local interface (ADR 0001).
//!
//! The LED shows the most urgent of: an `identify` request (fast blink for 10 s), alarms nobody has acknowledged (2 Hz),
//! a fault (1 Hz), a cook in progress (a short flash every 3 s, so it is seen to be alive) or nothing. The button
//! acknowledges the alarms on a short press and, held for 10 s, clears the saved WiFi network so that the box can be
//! provisioned again; held for 30 s it resets every setting to its default and reboots. The LED blinks (as for
//! `identify`) at 10 s and again at 30 s.

use embassy_futures::select::{Either, select};
use embassy_time::{Duration, Instant, Timer};
use embedded_hal::digital::{InputPin, OutputPin};
use embedded_hal_async::digital::Wait;

use crate::config::Mode;
use crate::pit::Command;
use crate::proto::request::Request;
use crate::tasks::Bus;

/// How long `identify` blinks, ms (docs/protocol.md).
pub const IDENTIFY_MS: u64 = 10_000;
/// A press this long clears the WiFi network, ms.
pub const LONG_PRESS_MS: u64 = 10_000;
/// A press this long resets every setting to its default, ms.
pub const RESET_PRESS_MS: u64 = 30_000;
/// A closure shorter than this is contact bounce, ms.
const DEBOUNCE_MS: u64 = 30;
/// The LED is re-evaluated at least this often, ms: how late it follows a change of status.
const REFRESH_MS: u64 = 1000;
const REQUEST_ID: u16 = 0xFF12;

/// What the LED is doing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pattern {
    Off,
    /// A cook is running.
    Cooking,
    Fault,
    /// Unacknowledged alarms.
    Alarm,
    Identify,
}

impl Pattern {
    /// The most urgent pattern for these conditions.
    pub fn pick(identify: bool, alarm: bool, fault: bool, cooking: bool) -> Self {
        if identify {
            Pattern::Identify
        } else if alarm {
            Pattern::Alarm
        } else if fault {
            Pattern::Fault
        } else if cooking {
            Pattern::Cooking
        } else {
            Pattern::Off
        }
    }

    /// Period and lit time, ms.
    const fn timing(self) -> Option<(u64, u64)> {
        match self {
            Pattern::Off => None,
            Pattern::Cooking => Some((3000, 100)),
            Pattern::Fault => Some((1000, 500)),
            Pattern::Alarm => Some((500, 250)),
            Pattern::Identify => Some((200, 100)),
        }
    }

    /// Whether the LED is lit `t_ms` into the pattern's clock.
    pub fn lit(self, t_ms: u64) -> bool {
        self.timing().is_some_and(|(period, on)| t_ms % period < on)
    }

    /// Time to the next change of the LED from `t_ms`, ms (`None` if it never changes).
    pub fn next_edge(self, t_ms: u64) -> Option<u64> {
        self.timing().map(|(period, on)| {
            let phase = t_ms % period;
            if phase < on { on - phase } else { period - phase }
        })
    }
}

/// Drives the LED from the latest status and the `identify` request.
pub async fn led<L: OutputPin>(mut led: L, bus: &Bus) -> ! {
    let mut identify_until: Option<Instant> = None;
    loop {
        if bus.identify.try_take().is_some() {
            identify_until = Some(Instant::now() + Duration::from_millis(IDENTIFY_MS));
        }
        let now = Instant::now();
        if identify_until.is_some_and(|t| now >= t) {
            identify_until = None;
        }
        let (alarm, fault, cooking) = bus.status.try_get().map_or((false, false, false), |s| {
            (s.alarm_led, s.faults.iter().next().is_some(), s.cook.mode != Mode::Off)
        });
        let pattern = Pattern::pick(identify_until.is_some(), alarm, fault, cooking);
        let t = now.as_millis();
        let _ = if pattern.lit(t) { led.set_high() } else { led.set_low() };
        let wait = pattern.next_edge(t).map_or(REFRESH_MS, |e| e.min(REFRESH_MS));
        if let Either::Second(()) = select(Timer::after_millis(wait.max(1)), bus.identify.wait()).await {
            identify_until = Some(Instant::now() + Duration::from_millis(IDENTIFY_MS));
        }
    }
}

/// Watches the button (low while pressed): a short press acknowledges the alarms, a press held for [`LONG_PRESS_MS`]
/// clears the WiFi network, and one held for [`RESET_PRESS_MS`] resets every setting.
pub async fn button<P: InputPin + Wait>(mut pin: P, bus: &Bus) -> ! {
    loop {
        let _ = pin.wait_for_low().await;
        Timer::after_millis(DEBOUNCE_MS).await;
        if !pin.is_low().unwrap_or(false) {
            continue;
        }
        match select(pin.wait_for_high(), Timer::after_millis(LONG_PRESS_MS)).await {
            Either::First(_) => {
                bus.send(REQUEST_ID, Request::Pit(Command::AlarmAck));
            }
            Either::Second(()) => {
                // the LED blinks at each step, so that the user knows where to let go
                bus.send(REQUEST_ID, Request::Wifi(None));
                bus.identify.signal(());
                if let Either::Second(()) =
                    select(pin.wait_for_high(), Timer::after_millis(RESET_PRESS_MS - LONG_PRESS_MS)).await
                {
                    bus.send(REQUEST_ID, Request::Defaults);
                    bus.identify.signal(());
                    let _ = pin.wait_for_high().await;
                }
            }
        }
        Timer::after_millis(DEBOUNCE_MS).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_most_urgent_pattern_wins() {
        assert_eq!(Pattern::pick(true, true, true, true), Pattern::Identify);
        assert_eq!(Pattern::pick(false, true, true, true), Pattern::Alarm);
        assert_eq!(Pattern::pick(false, false, true, true), Pattern::Fault);
        assert_eq!(Pattern::pick(false, false, false, true), Pattern::Cooking);
        assert_eq!(Pattern::pick(false, false, false, false), Pattern::Off);
    }

    #[test]
    fn patterns_blink_at_their_rates_and_say_when_they_next_change() {
        assert!(!Pattern::Off.lit(0) && Pattern::Off.next_edge(0).is_none());
        // identify: 5 Hz, half on
        assert!(Pattern::Identify.lit(0) && !Pattern::Identify.lit(100) && Pattern::Identify.lit(200));
        assert_eq!(Pattern::Identify.next_edge(30), Some(70));
        assert_eq!(Pattern::Identify.next_edge(130), Some(70));
        // the cook's flash is brief
        let on = (0..3000).filter(|t| Pattern::Cooking.lit(*t)).count();
        assert_eq!(on, 100);
        // following next_edge visits every change and nothing else
        for p in [Pattern::Cooking, Pattern::Fault, Pattern::Alarm, Pattern::Identify] {
            let (mut t, mut state, mut edges) = (0, p.lit(0), 0);
            while t < 6000 {
                t += p.next_edge(t).unwrap();
                let now = p.lit(t);
                assert_ne!(now, state, "{p:?} at {t}");
                state = now;
                edges += 1;
            }
            assert!(edges >= 4, "{p:?}");
        }
    }
}
