# ADR 0004: USB-C power architecture

- Status: accepted (2026-10-11)
- Bead: `minimeat-seq.1.8`
- Constraint: the PCB is assembled by JLCPCB, so every PCB part must be in JLC stock. Volume is about 5 boards.

## Context

One USB-C port on the box carries all power and also flashing, logs and debug (ADR 0001). There is no 12 V input and no HeaterMeter blower. The loads are:
- the ESP32-C6 module with WiFi;
- the probe front-end (ADR 0002);
- one 5 V PWM fan (Noctua NF-A6x25 as the placeholder, `.2.2`);
- one 5 V metal-gear servo (Hitec HS-5065MG, `.2.9`).

The supply is a phone charger, a laptop port or a power bank. We do not control it, and a cook must survive a supply that sags, drops out and comes back.

## Decision

**5 V only. No USB PD.**

| Load | Typical cook | Worst normal | Fault |
|---|---|---|---|
| ESP32-C6, WiFi (3.3 V, through the regulator) | about 80 mA | about 350 mA TX burst | |
| ADS122C04, TMP117, probe dividers | under 2 mA | 2 mA | |
| Fan, 5 V (0.94 W at full speed) | about 30 mA (model: under 0.15 W at the operating points) | about 190 mA | |
| Servo, 5 V (HS-5065MG: 180 mA running, 3 mA idle) | 0 (power gated between moves) | 220 mA while moving | stall 0.96-1.2 A |
| **Total from VBUS** | **about 0.12 A** | **about 0.8 A** (with an LDO for the 3.3 V rail; about 0.65 A with a buck) | up to about 1.8 A |

The currents are the parts' own figures and need checking at bring-up (`.3.9`). The servo figures are from a servo database, not Hitec's sheet.

- **Why not PD:** every candidate load is a 5 V part under 1 W, except for a servo stall. PD (9 or 12 V) would add a sink controller, a second regulator stage and more failure modes for no gain. It would only come back if the bake-off picks a 12 V fan (`.2.11`).
- **Source current:** the device does not trust the source. Each CC pin has a 5.1 k pull-down (Rd), and both CC pins are read on ADC1 pins (ADR 0001). The voltage on the CC pin identifies the source's pull-up: Default USB (about 0.2-0.66 V), 1.5 A (0.66-1.23 V) or 3 A (1.23-2.04 V). The firmware limits the loads to the class:
  - Default (500 mA): fan duty capped, servo moves only while WiFi is idle, servo and fan never run together.
  - 1.5 A or 3 A: no limit.
  - A source that is not recognised is treated as Default.
- **Servo power is gated and current-limited.** It comes from VBUS through a load switch with an enable pin (ADR 0001 already has one) and a current limit of about 0.6 A. That cuts hold current and jitter between moves, and it stops a stalled or jammed flap from drawing 1 A. The firmware raises a fault on a limit trip and stops driving the servo; it does not retry in a loop.
- **Fan power is also gated.** A 0 % PWM duty does not guarantee a stopped fan on every unit, and the model needs a fully stopped fan for natural draft. A second load switch on the fan's 5 V gives that. It uses the last spare GPIO from ADR 0001, so VBUS sense is dropped; the CC ADC readings already show the source is present.
- **3.3 V rail:** a small synchronous buck from VBUS, sized for the 350 mA WiFi burst with margin, with the ADS122C04's analog supply taken from it through a ferrite and an LDO so converter ripple stays out of the probe measurement. A plain LDO for the MCU would dissipate about 0.6 W at TX peak inside a box that may sit at 60 C. The exact parts are chosen in `.3.2`, from JLC stock.
- **Protection:** a TVS on VBUS and ESD arrays on D+/D- and CC, as USB-C requires. No reverse-polarity circuit is needed on a USB-C input. Bulk capacitance on VBUS (about 100-220 uF) absorbs the servo's start and the WiFi bursts. The bulk sits behind an input eFuse (TI TPS259531, amended 2026-10-14): soft start, a 2 A limit and a 5.7 V output clamp, so the cable sees only 100 nF at attach and its ringing cannot reach the 6 V parts (`docs/sim/power.md`). The brownout detector stays on, and the cook resumes from flash after any reset (the autotune and cook-resume rules apply).
- **Consequence for the case:** the HS-5065MG is rated -20 to 60 C. The egg shell models at 65-90 C at 225-325 F, and the servo sits near the tube. The case needs a thermal check, and an air gap or heat break to the duct, before the servo is trusted (`.2.9`, `.2.5`).

## Consequences

- **Power banks** often cut the output when the load stays under 50-100 mA. The typical cook draws about 120 mA with the fan off and WiFi idle, which is close to that. For overnight cooks, use a wall charger. If testing shows a power bank cuts out, add a small keep-alive load (a resistor and a MOSFET footprint) in `.3.2`.
- **Pin budget** (ADR 0001): fan power enable takes the last spare GPIO. Nothing else changes.
- **Part selection** (`.3.2`, `.3.5`): 5 V fan and servo, VBUS TVS, two load switches (servo 0.6 A limit, fan about 0.3 A), a buck, a low-noise LDO for the ADC, and a USB-C receptacle that suits the case's port panel.

## Amendment (2026-10-12): 1.5 A or nothing

The Default-class rules above ("fan duty capped, servo moves only while WiFi is idle, servo and fan never run together") are replaced by a hard requirement. The damper servo and the fan are both needed for control: the model's low-airflow regime is natural draft throttled by the damper with the fan stopped, and its high-airflow regime is the damper open with the fan modulated. With either one missing the pit cannot be held, so a half-working mode is worse than none.

- **Rule:** the source must advertise 1.5 A or 3 A on CC (7.5 W or more). Anything else, including no source or an unreadable CC, is a `supply` fault: the fan and the servo are off, the probes, alarms and MQTT keep running, and the fault says why. There is no reduced mode and no sequencing.
- **Budget behind 1.5 A:** the 3.3 V rail at the radio's TX burst (about 0.25 A at 5 V), the fan (0.19 A) and a servo at its 0.6 A limit come to about 1.1 A, and the servo's turn-on surge (0.9 A for about a millisecond, `docs/sim/power.md`) fits inside 1.5 A. A bulk-capacitor fix for a 500 mA source is not possible: the surge needs about 0.5 mC and the steady worst case already exceeds 500 mA.
- **Cables:** use USB-C to USB-C. A USB-A to USB-C cable carries a fixed 56k pull-up that always reads as Default, even from a capable charger, so it faults. A PD controller would not change this (the cable carries no PD signalling), and BC1.2 charger detection on D+/D- cannot prove 1.5 A; neither is used.
- **Firmware:** `drive::SupplyWatch` debounces the CC reading (two good reads to accept, three bad to reject); the actuator task holds the fan at 0 and the servo unpowered until it accepts. Twin test: `fault_supply`.
- **Servo hold:** the servo is power-gated between moves, so the flap is held by the gear train alone; check at bring-up that the flap does not creep in the fan's airflow (`.3.9`).

## Amendment (2026-10-12): load-switch fault and fan power in firmware

- **Fan power:** GPIO11 drives the fan's TPS2553 enable: the task sets it high only while the fan duty is above zero and the supply is good, so 0 % really stops the fan.
- **Fault line:** GPIO3 reads the two switches' shared open-drain fault output. Three one-second samples in a row low raise `load` and cut both loads until the next reboot (`drive::LoadWatch`). There is no retry: the TPS2553 thermal-cycles by itself in current limit, and the servo's turn-on surge (under a millisecond) is far shorter than the sampling. The line is shared, so the fault does not name the load.
- Twin tests: `fault_load` (the latch, the flap held, the fan stopped) and `fault_supply`.

## Amendment (2026-10-15): the fault line is the servo's stall signal while homing

Hard-stop homing (`minimeat-xdn`, `drive::Homing`) needs to know when the servo has stalled against a stop, and the board has no current sense on the servo. The servo's TPS2553 limits at 0.6 A, against about 0.2 A running and about 1.2 A stalled, so a stall holds the shared fault line low. Homing runs only with no cook (never when resuming one after a power cut) and holds the fan off, so the line is then the servo's alone; the load watch is left out of the run (it would latch both loads on the first stall) and the servo is unpowered for a second after each stall to let the switch go. The line is read ten times in each 0.1 s step, and a stall is two steps with the line seen low among the last five (not necessarily adjacent), after the first half second following a power-up.

What the TPS2552/TPS2553 datasheet (SLVS841F) says, read 2026-10-15: FAULT is asserted for overcurrent, over-temperature and reverse voltage; for overcurrent it asserts after a deglitch of 5 to 10 ms (7.5 typical) and de-asserts after the same, so a fault is stretched, not shortened; over-temperature asserts at once. The TPS2553 (not the -1) stays in constant-current mode and holds FAULT low for as long as the overload lasts, thermal-cycling at 135 C. In limit the output falls to the limit times the load's resistance (the datasheet's own formula, IOS x RLOAD), so a servo stalled at 4 ohm (about 1.1 A wanted at 4.5 V) is held at 0.6 A and about 2.4 V. The limit at R5 = 43.2 kohm is 0.60 A typical, interpolated from the table's 520 mA at 49.9 kohm (475 to 565) and 1295 mA at 20 kohm: about 0.55 to 0.65 A. The retailers' listing for the HS-5065MG gives 1.2 A stalled and 0.22 A running, so a full stall is at least 1.8 times the limit and the running current under half of it. Still unverified, and the reason for the chatter-tolerant detector: how far past a stop the servo must be pushed before it draws the limit, and whether a servo held at 2.4 V resets its logic and restarts (the line then comes and goes). Both are for the bench (`minimeat-xdn` bench bead). Dissipation in limit is about (4.5 - 2.4) x 0.6 = 1.3 W in a SOT-23-6, so a stall of more than a moment reaches the thermal limit; a run stalls twice for a fraction of a second each, and the servo is unpowered for a second after each. Twin: the sim's servo stalls (fault line low) once pushed `ServoMech::trip_us` past either hard stop.

Follow-up (2026-10-15, `minimeat-vpg`, `-sp4`, `-9jn`, `-55j`): the board now has the servo current sense the pre-fab bead recommended. A 0.1 ohm 0402 shunt (R36) sits between the switch's output and its caps (VSERVO_SW) and the servo side (VSERVO: C36's 100 uF reservoir, C37, J7), so the reservoir's charging peaks stay out of the reading; an INA181A1 (U13, gain 20) gives 2 V/A, 1.2 V at the 0.6 A limit, through a 1k/100 nF filter (1.6 kHz) to GPIO2 (ADC1_CH2). The shunt drops 60 mV at the limit. The firmware still homes on the fault line until `minimeat-sul` reads the current.
