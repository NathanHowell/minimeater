# System requirements and interface control document

Status: draft (2026-10-11), bead `minimeat-seq.1.2`. This is the contract between the firmware, the PCB and the enclosure. Where a value comes from an ADR, the ADR is named; where it is still a placeholder, it says so.

## Purpose and scope

- A HeaterMeter-style controller for one Big Green Egg Large, run at a 150-325 F pit. Grilling is out of scope.
- One printed box on the egg's 34.93 mm stainless intake tube holds the fan, a servo flap damper, the controller board and one USB-C port. There is no separate blower, and no 12 V supply.
- Offset-free MPC control, reported over MQTT (Home Assistant) and OTLP; the schema is `docs/protocol.md`.

## Power (ADR 0004)

| | |
|---|---|
| Input | USB-C, 5 V only, sink with 5.1 k Rd on both CC pins; no PD |
| Source class | read from CC on ADC1 pins: Default 500 mA, 1.5 A, 3 A; an unknown source is Default. The box needs 1.5 A or 3 A (7.5 W): on less, the fan and servo are off and the `supply` fault is raised. There is no reduced mode. Use a USB-C to USB-C cable: a USB-A to USB-C cable always reads as Default |
| Typical cook draw | about 0.12 A |
| Worst normal draw | about 0.8 A |
| Fault | servo stall 0.96-1.2 A, limited to about 0.6 A by a load switch |
| Rails | 5 V (VBUS), 3.3 V buck, filtered analog supply for the ADS122C04 |

Rules: the fan and the servo each have a power switch controlled by the MCU; the 5 V loads run only on a 1.5 A or 3 A source; a brownout resets the MCU and the cook resumes from flash.

## Probes (ADR 0002)

- Four interchangeable 2.5 mm stereo jacks for thermistor probes. Any jack can be the pit probe; the default is jack 0 for the pit and 1-3 for food.
- Probes in use: ThermoWorks Pro-Series and generic imports (about 100 k at 25 C) and legacy Maverick ET-732 (about 1 M at 25 C).
- TI ADS122C04 on I2C (same bus as the ambient sensor) with per-jack dividers. There is no K-type input in this revision (ADR 0002, amended).
- Ambient (outside air) sensor TMP117 on I2C, on the board's underside in a vented well, isolated from the duct air.
- Accuracy: +/-2 F at ambient and at boiling (`.4.7`).

## Actuators

| | Fan | Servo |
|---|---|---|
| Part (placeholder) | Noctua NF-A6x25 5V PWM (`.2.2`, bake-off `.2.11`) | Hitec HS-5065MG (`.2.9`) |
| Supply | 5 V through a load switch, about 190 mA | 5 V through a current-limited load switch, 180 mA moving, stall to 1.2 A |
| Control | 25 kHz PWM, open-collector tach to a pull-up and clamp | 50 Hz PWM, 900-2100 us, 3.3 V logic |
| Gate pin | fan power enable | servo power enable |

## Identity (ADR 0005)

- Microchip ATECC608B on the shared I2C bus at 0x60 (ADS122C04 at 0x40, TMP117 at 0x48). It holds a P-256 device key that never leaves the chip and the device certificate, for mutual TLS to the MQTT broker and signed OIDC client assertions.
- Provisioning locks the chip; the lock is one-way.

## Controls and indication

- One status LED (not on GPIO8 or GPIO9) and one button on GPIO9, which doubles as BOOT. Their positions on the shell are an enclosure decision.
- UART0 is test pads only.

## Mechanical envelope (`docs/mech.md`)

- Overall size is set by the enclosure design; the placeholder target is about 123 x 74 x 72 mm.
- Board: 46 x 54 x 1.6 mm, lying flat under the duct with its parts facing up, on 2.5 mm standoffs. Parts above the board up to 11 mm, nothing below it except lead stubs and the ambient sensor.
- Four M2.5 mounting holes at 3.5 mm in from the corners, 2.7 mm drill.
- Connectors on the board's -y long edge, in a row along the flow axis: USB-C and four jacks, 8.7 mm pitch for the jacks (`uv run mech-envelope` prints the exact positions). Plugs enter from the side through the port bay in the shell.
- The antenna side of the module faces away from the egg and the tube, with the keep-out clear of copper and of the duct.
- These numbers are provisional until the schematic and layout set the real outline (`.3.6`); the envelope file is the single source (`.1.6`).

## Environment

- Outdoors next to the egg: ambient from -10 C to 45 C, rain and splash, no immersion. Target IPX4 (`docs/mech.md`, to be proven by a spray test).
- Board and the ADS122C04 are limited to 85 C. The module is rated to 85 C.
- The egg shell models at 65-90 C at 225-325 F. The servo is rated 60 C maximum and sits near the tube, so the case must keep it cooler than the shell (`.2.9`, `.2.5`).
- The cook must survive a power cut and come back by itself: the setpoint, mode, schedule and food estimates are persisted.

## Interfaces (firmware)

- Pin assignment, strapping rules and GPIO budget: ADR 0001 (fan power enable takes the last spare GPIO, per ADR 0004).
- Wire format: `docs/protocol.md` (MQTT and Home Assistant discovery) and `docs/telemetry.md` (OTLP).
- Flashing, logs and provisioning: USB Serial/JTAG on GPIO12/13 over the same USB-C port. The C6 has one CDC port and no USB OTG, so console logs and Improv share it: `minimeat-fw/src/console.rs` sends whole log lines and whole Improv frames under one lock, and drops writes while no host is draining the port (the box on a wall adapter). The panic handler still prints directly.
- WiFi credentials: Improv Wi-Fi over that same serial port (`minimeat-app::improv`): a browser page (esp-web-tools, Home Assistant) sends the SSID and password with the cable in, the device joins, and only credentials that join are saved (`Request::Wifi`, never on MQTT). Whoever holds the cable may provision; there is no authorisation step. Forgetting the credentials is a stored empty SSID. The radio (`minimeat-fw/src/net.rs`, esp-radio and embassy-net) joins the saved network with DHCP, retries with backoff (1, 2, 4, 8, 16, 30, then 60 s) and re-joins when the link drops or the credentials change; a join from provisioning that fails leaves the saved network joined. WPA2 and WPA/WPA2 (and open) networks; WPA3-only networks are not supported by the driver's configuration. The device registers `minimeat-<mac low 3 bytes>` with the DHCP server and answers mDNS for `minimeat-<...>.local`; `.local` names in the MQTT and OTLP settings resolve over mDNS. SNTP (pool.ntp.org, every 15 min) sets the clock the OTLP exporter needs.

## Open items

- Fan and servo are placeholders until `.2.11` and the part checks (`.2.9`).
- Board outline, connector parts and positions: `.3.x`.
- Thermal break between the tube and the box: needs a tube temperature measurement (`.2.5`).
- Tube dimensions: calipers (`.2.8`); the design uses 34.93 mm OD and 31.6 mm ID.
