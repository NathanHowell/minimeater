# ADR 0008: Pit probe auto-detect, and the mismatch response

- Status: accepted (2026-10-21)
- Beads: `minimeat-seq.4.36`; the airflow cap it leaves open is filed as its own follow-up under `minimeat-seq.4`

## Context

The pit probe was a setting: `pit_probe` is a jack index 0 to 3 (or `tc`), default jack 0, in the `Cook` record. A cook with
the pit probe in another jack and food in jack 0 controlled on the food. If the selected probe is not `ok` for two control
periods the `pit_probe` fault shuts the flap and fan.

The reference model is `src/minimeater/model/pitprobe.py` (sim support in `model/sim.py`, tests in `tests/test_pitprobe.py`).

## Decision

**`pit_probe: auto` is the new default**, replacing jack 0. An explicit `0` to `3` or `tc` still wins.

- **Finder.** Each control period it looks at every jack and names the pit probe when, for 120 s, one probe is
  - at least 40 F above ambient (the fire is lit and the probe is in its gas),
  - at least 40 F above every other live probe (a lone probe passes this vacuously),
  - rising at least 1 F/min over a 3 min window, or within 25 F of the setpoint (a cook already at temperature).
  "Hottest wins" alone was rejected: a pit probe lost mid-cook would hand control to a food probe.
- **Locked for the cook** and persisted in the `Cook` record, so a power cut resumes on the same jack. A locked probe that
  goes missing raises the existing `pit_probe` fault (flap shut, fan off); the finder never looks again.
- **Unassigned period.** The flap is not shut: that starves the fire and ends light-off. The controller runs on the
  hottest probe, which is the pit probe once the fire is lit, and locks when the finder names one (a fresh controller start
  only if the locked jack is not the one it was using). With no probe reading at all it gets the model's feedforward airflow
  for the setpoint (plus the `Cook::hold` trim) with no feedback. The first design was feedforward only, under an
  over-temperature ceiling of `setpoint + 0.35 * (setpoint - ambient)` (the worst open-loop overshoot across the plant
  band, 0.33 of the gap), but the twin showed a cold egg barely lights at the feedforward flow: 66 min before the pit probe
  was seen and 94 min to setpoint, against 30 min on the controller. The ceiling went with it: feedback on the hottest
  probe holds the setpoint, and it is blind to the same food-probe case either way.
- **`pit_mismatch`: `off` | `warn` | `stop`, default `warn`.** It applies to auto and manual selection. A watchdog on the
  selected probe trips when, held continuously,
  - *slow*: the fan is running and the probe is 40 F or more under the setpoint for 60 min, or
  - *hot*: the probe is 50 F or more over the setpoint for 120 s.
  `warn` raises the advisory and keeps driving; `stop` shuts the flap and fan as `pit_probe` does. The trip latches until the
  cook ends or the selection changes. *Slow* does not count while the fire watch says the fire is out: a dead fire cools and
  is already `fire_out`, it is not a probe in the wrong place (the twin's `fuel-burnout` run would otherwise raise it).

## Evidence

Finder, pit probe plugged in: right jack in every layout tried (pit in each of jacks 0 to 3, a lone pit probe, food first with
the pit probe plugged in 15 min later) in every plant and setpoint; median 8 min after light-off, worst 17. A rise
threshold of 1.5 or 3 F/min missed the high-loss 225 F cases, hence 1.

Watchdog, closed loop on a locked jack (3 plants, 225 and 325 F, calm and cold wind):

| Probe on the locked jack | Within 40 F of setpoint | Max over setpoint |
|---|---|---|
| Pit probe | 12 to 39 min | +34 F |
| Chicken-like meat | 30 to 52 min | +115 F |
| 1 h meat | 41 to 104 min | +111 F |
| 3 h meat | 73 min, or never | +80 F |

## Known gap

With no pit probe plugged in, the finder cannot tell a food probe from a pit probe at any threshold tried: thin meat in a hot
pit climbs as fast as a slow light-off. (Today's jack-0 default has the same hole.) A food probe named or selected as the pit
makes the controller see a cold pit and drive full air, so the real pit runs 100 to 530 F over setpoint in the low-loss and
nominal plants (15 to 190 F in the high-loss one) before the watchdog can fire. Chicken-like meat passes the *slow* budget,
so only *hot* catches it, 40 to 60 min in. The watchdog is a warning or a late stop, not a prevention.

An airflow cap until the selected probe has been seen tracking would bound it; it is a follow-up bead because in a high-loss
plant feedforward alone leaves the pit about 70 F under and the cap must not hold a real pit there. Gas turbulence on a pit
probe versus a smooth meat probe might separate them, but the sim has no gas noise, so that needs a bench check.

## Alternatives

- **Hold the flap shut while unassigned**: rejected, it starves the fire and shuts the grill off.
- **A fixed over-temperature guard (350 F)**: rejected, it must follow the cook temperature.
- **Hottest probe wins**: rejected, see above.
- **No auto when the layout is ambiguous**: stays on feedforward until the user picks a jack; kept as a fallback if the
  watchdog proves too weak on the bench.

## Consequences

- `~/state` `pit` is the reading the loop controls on (the hottest probe while auto is still looking); `pit_jack` is the
  chosen or locked jack, `null` until auto has one. `pit_found` is a new event and `pit_mismatch` a new fault.
- Twin: `tests/pit_auto.rs` (brisket in jack 0, pit probe in jack 2: found in minutes, held at the setpoint) and
  `tests/pit_mismatch.rs` (the brisket's jack chosen by hand: `slow` trips at an hour, `stop` shuts the air).

- `Cook.pit_probe` gains an `Auto` value and the flash record a migration; the default changes from jack 0.
- `docs/protocol.md` (`pit_probe` auto, the `pit_mismatch` setting, the fault or event code) and `docs/icd.md` (default jack)
  change with the firmware port.
- Autotune and learning never take the pit away from the controller (unchanged).
