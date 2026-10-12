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
| ADS1220, TMP117, probe dividers | under 2 mA | 2 mA | |
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
- **3.3 V rail:** a small synchronous buck from VBUS, sized for the 350 mA WiFi burst with margin, with the ADS1220's analog supply taken from it through a ferrite and an LDO so converter ripple stays out of the probe measurement. A plain LDO for the MCU would dissipate about 0.6 W at TX peak inside a box that may sit at 60 C. The exact parts are chosen in `.3.2`, from JLC stock.
- **Protection:** a TVS on VBUS and ESD arrays on D+/D- and CC, as USB-C requires. No reverse-polarity circuit is needed on a USB-C input. Bulk capacitance on VBUS (about 100-220 uF) absorbs the servo's start and the WiFi bursts. The brownout detector stays on, and the cook resumes from flash after any reset (the autotune and cook-resume rules apply).
- **Consequence for the case:** the HS-5065MG is rated -20 to 60 C. The egg shell models at 65-90 C at 225-325 F, and the servo sits near the tube. The case needs a thermal check, and an air gap or heat break to the duct, before the servo is trusted (`.2.9`, `.2.5`).

## Consequences

- **Power banks** often cut the output when the load stays under 50-100 mA. The typical cook draws about 120 mA with the fan off and WiFi idle, which is close to that. For overnight cooks, use a wall charger. If testing shows a power bank cuts out, add a small keep-alive load (a resistor and a MOSFET footprint) in `.3.2`.
- **Pin budget** (ADR 0001): fan power enable takes the last spare GPIO. Nothing else changes.
- **Part selection** (`.3.2`, `.3.5`): 5 V fan and servo, VBUS TVS, two load switches (servo 0.6 A limit, fan about 0.3 A), a buck, a low-noise LDO for the ADC, and a USB-C receptacle that suits the case's port panel.
