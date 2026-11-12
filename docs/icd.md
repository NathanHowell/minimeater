# System requirements and interface control document

Status: draft (2026-10-11), bead `minimeat-seq.1.2`. This is the contract between the firmware, the PCB and the enclosure. Where a value comes from an ADR, the ADR is named; where it is still a placeholder, it says so.

## Purpose and scope

- A HeaterMeter-style controller for one Big Green Egg Large, run at a 150-325 F pit. Grilling is out of scope.
- One printed box on the egg's 34.89 mm stainless intake tube holds the fan, a servo flap damper, the controller board and one USB-C port. There is no separate blower, and no 12 V supply.
- Offset-free MPC control, reported over MQTT (Home Assistant) and OTLP; the schema is `docs/protocol.md`.

## Power (ADR 0004)

| | |
|---|---|
| Input | USB-C, 5 V only, sink with 5.1 k Rd on both CC pins; no PD |
| Source class | read from CC on ADC1 pins: Default 500 mA, 1.5 A, 3 A; an unknown source is Default. The box needs 1.5 A or 3 A (7.5 W): on less, the fan and servo are off and the `supply` fault is raised. There is no reduced mode. Use a USB-C to USB-C cable: a USB-A to USB-C cable always reads as Default |
| Typical cook draw | about 0.12 A |
| Worst normal draw | about 0.8 A |
| Fault | servo stall 0.96-1.2 A, limited to about 0.6 A by a load switch |
| Servo current | measured across a 0.1 ohm shunt (INA181A1 into an ADC1 pin): homing reads the stall in it; each move's peak and mean are published (`out.servo_ma`), and a servo still drawing 0.3 A once it should hold, or nothing on a move, raises the `servo` fault (`docs/protocol.md`) |
| Rails | 5 V (VBUS), 3.3 V buck, filtered analog supply for the ADS122C04 |

Rules: the fan and the servo each have a power switch controlled by the MCU; the 5 V loads run only on a 1.5 A or 3 A source; a brownout resets the MCU and the cook resumes from flash.

## Probes (ADR 0002)

- Four interchangeable 2.5 mm stereo jacks for thermistor probes. Any jack can be the pit probe; by default the firmware finds it (`pit_probe: auto`, ADR 0008) and a setting pins it to a jack.
- Probes in use: ThermoWorks Pro-Series and generic imports (about 100 k at 25 C) and legacy Maverick ET-732 (about 1 M at 25 C).
- TI ADS122C04 on I2C (same bus as the ambient sensor) with per-jack dividers. There is no K-type input in this revision (ADR 0002, amended).
- Ambient (outside air) sensor TMP117 on I2C, on the board's underside in a vented well, isolated from the duct air.
  - It reads a little of the board's heat, so the published `amb` is compensated: air = (raw - k * board) / (1 - k), with the board temperature from the ADC's die and k = 0.1 `[est]` (`ambient::SELF_HEAT`; the bench measures it, `minimeat-bxw`). The correction is bounded to 5 C. The twin's sensor picks up 0.12 of a 35 C board's excess, and the compensated reading lands within 0.5 C of the air (`tests/probes.rs`).
  - Over-temperature: `board_hot` is raised by the hotter of the ADC die and this sensor (80 C, clearing at 75 C). A raw reading outside -30..60 C is not used for the airflow plan (the 20 C fallback and the `ambient` fault), so a sensor that reads high cannot move the pit: with it 55 C out, the twin's pit stays within 1.5 F; without the gate it was 4.5 F off.
  - Feedforward: the reading sets the airflow map's ambient (rebuilt when it moves 2 C). The suite's `*-blind` scenarios run the same cooks with the sensor dead from the start (the 20 C fallback); the difference was inside the noise, see `docs/feedforward.md`.
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
  - LED (`minimeater-app::ui`), most urgent first: `identify` blinks at 5 Hz for 10 s; alarms nobody has acknowledged blink at 2 Hz; a fault blinks at 1 Hz; a cook in progress flashes briefly every 3 s; otherwise off.
  - Button: a short press acknowledges the alarms (`alarm_ack`); a press held for 10 s clears the saved WiFi network so the box can be provisioned again. Held for 30 s it resets every setting (cook, probes, the learned schedule, WiFi, MQTT and OTLP) to its default, saves, and reboots; the LED blinks at 10 s (WiFi cleared; let go here to keep the rest) and again at 30 s. The learned model goes with the rest: the factory state is the nominal schedule.
- UART0 is test pads only.

## Mechanical envelope (`docs/mech.md`)

- Overall size is set by the enclosure design; the placeholder target is about 123 x 74 x 72 mm.
- Board: 46 x 54 x 1.6 mm, lying flat under the duct with its parts facing up, on 2.5 mm standoffs. Parts above the board up to 11 mm, nothing below it except lead stubs and the ambient sensor.
- Four M2.5 mounting holes at 3.5 mm in from the corners, 2.7 mm drill.
- Connectors on the board's -y long edge, in a row along the flow axis: USB-C and four jacks, 8.7 mm pitch for the jacks (`just mech::envelope` prints the exact positions). Plugs enter from the side through the port bay in the shell.
- The antenna side of the module faces away from the egg and the tube, with the keep-out clear of copper and of the duct.
- These numbers are provisional until the schematic and layout set the real outline (`.3.6`); the envelope file is the single source (`.1.6`).

## Environment

- Outdoors next to the egg: ambient from -10 C to 45 C, rain and splash, no immersion. Target IPX4 (`docs/mech.md`, to be proven by a spray test).
- Board and the ADS122C04 are limited to 85 C. The module is rated to 85 C.
- The egg shell models at 65-90 C at 225-325 F. The servo is rated 60 C maximum and sits near the tube, so the case must keep it cooler than the shell (`.2.9`, `.2.5`).
- The cook must survive a power cut and come back by itself: the setpoint, mode, schedule and food estimates are persisted.

## Interfaces (firmware)

- Pin assignment, strapping rules and GPIO budget: ADR 0001 (fan power enable takes the last spare GPIO, per ADR 0004; GPIO4-7 are the probe jacks' rings, ADR 0002's amendment of 2026-10-18).
- Wire format: `docs/protocol.md` (MQTT and Home Assistant discovery) and `docs/telemetry.md` (OTLP).
- Flashing, logs and provisioning: USB Serial/JTAG on GPIO12/13 over the same USB-C port. The C6 has one CDC port and no USB OTG, so console logs and Improv share it: `minimeater-fw/src/console.rs` sends whole log lines and whole Improv frames under one lock, and drops writes while no host is draining the port (the box on a wall adapter). The panic handler still prints directly.
- Status endpoint (`minimeater-app::http`, `minimeat-g7r`): read-only plain HTTP on port 80 of the LAN, no credentials and nothing that writes (settings stay on the serial REPL and MQTT; a write route would need TLS, ADR 0005). `GET /health` answers `200` and `{"ok":true,"id","sw","hw","up"}` once the control task has published a status (`503` before), for a monitor; `GET /status` answers the `~/state` object of `docs/protocol.md` (temperatures, setpoint, outputs, faults, WiFi signal). Any other method on those paths is `405` (`Allow: GET`), any other path `404`. One client at a time, one request per connection (`Connection: close`); a client that has not finished its headers in 5 s, or sends more than 1 KB of them, is dropped without an answer. It only reads the bus, so it cannot slow control or change the cook. Reach it at `http://minimeater-<mac low 3 bytes>.local/status`. Twin: `Action::Http` in a scenario, answers in `Report::http` (`tests/http.rs`).
- Serial REPL (`minimeater-app::repl`, `minimeat-s7v`): on the same serial port, line commands for what must not cross the network: `wifi set <ssid> [password]`, `mqtt set <host> [port] [user] [password]`, `otlp set <host> [port] [interval_s]`, each with `clear`, `mqtt ca [start | add <base64> | save | clear]` (the DER certificate of the CA that must have signed the broker's certificate, sent as base64 words of 64 characters like `identity cert`, at most 480 bytes; once saved MQTT uses TLS with the device certificate, `clear` returns to plaintext; `config` shows `tls on` or `tls off`), plus `config` (passwords hidden), `status`, `setpoint`, `identify`, `reboot`, `identity`, `identity csr`, `identity provision lock`, `identity rekey replace` (a new key, the certificate erased) and `identity cert [start | add <base64> | save]` (the device key in the ATECC608B, `docs/provisioning.md`: look, print the signed certificate request, lock the chip in, and keep the CA's certificate in slot 8 as base64 words of 64 characters; the lock is one way, so it needs the word `lock`, and `cert save` refuses a certificate that is not for this chip's key), `servo [home | jog <us> | jog +<n> | jog -<n> | mark closed | mark open | save | done | clear]` (the damper servo's bring-up, `docs/mech.md`: `home` drives the servo into the two hard stops itself, about 20 s, and saves the pulses it finds (the firmware does the same at power-on whenever none is saved, a resumed cook included, and a cook that starts meanwhile waits for it; a run that fails is reported as `servo_home` and `servo_cal`, docs/protocol.md); or hold a pulse width of 500 to 2500 us, note the closed and the open stop, save them; they are kept in flash and the actuators use them from the next second; a jog is refused while a cook is running and ends when one starts; `config` shows `servo` with the stored pulses or `nominal`), `factory-reset erase` (the button's 30 s reset: every setting to its default, then a reboot; without the word `erase` it only says what it would do) and `help`. Press Enter for the prompt (`minimeater> `). A word may be quoted (`"my net"`) so a password can hold spaces. Tab completes commands, their words and typical values, and a second Tab lists the matches one to a line; a byte that cannot be typed rings the bell; the up arrow recalls the last four lines (lines that set a credential are not kept). A log line that arrives while a command is half typed does not split it: the log writer (`console.rs`) asks `repl::LIVE` and, when a prompt is up, erases the line, writes the log line and draws the prompt and the typed text again. Improv frames and log lines share the port: `minimeater-app::serial` runs one `winnow` grammar over the bytes that yields an Improv frame (`improv::frame`, with its length and checksum, newline included) or a keystroke for the editor, so a byte that could start a frame waits until the grammar can tell (at most the six marker bytes, released after 200 ms of quiet) and no frame byte reaches the editor. The command line is a `winnow` grammar on the text (`repl::parse`). A setting goes to the control task as a request (`Request::Mqtt`, `Request::Otlp`, `Request::Wifi`, `Request::Defaults`: never on MQTT) and is saved; clearing the broker or the collector is a stored empty host, like the WiFi tombstone. Use any serial terminal at the board's USB-C port (`screen /dev/cu.usbmodem* 115200`). Twin: `Action::Serial` types text at the twin's port and `Report::serial` holds what the app wrote (`tests/serial.rs`); the chip is absent there, so `identity` times out with `no answer from the secure element`.
- WiFi credentials: Improv Wi-Fi over that same serial port (`minimeater-app::improv`, its frame and RPC grammar in `winnow`): a browser page (esp-web-tools, Home Assistant) sends the SSID and password with the cable in, the device joins, and only credentials that join are saved (`Request::Wifi`, never on MQTT). Whoever holds the cable may provision; there is no authorisation step. Forgetting the credentials is a stored empty SSID. The radio (`minimeater-fw/src/net.rs`, esp-radio and embassy-net) joins the saved network with DHCP, retries with backoff (1, 2, 4, 8, 16, 30, then 60 s) and re-joins when the link drops or the credentials change; a join from provisioning that fails leaves the saved network joined. WPA2 and WPA/WPA2 (and open) networks; WPA3-only networks are not supported by the driver's configuration. The device registers `minimeater-<mac low 3 bytes>` with the DHCP server and answers mDNS for `minimeater-<...>.local`; `.local` names in the MQTT and OTLP settings resolve over mDNS. SNTP (pool.ntp.org, every 15 min) sets the clock the OTLP exporter needs.

## Open items

- Fan and servo are placeholders until `.2.11` and the part checks (`.2.9`).
- Board outline, connector parts and positions: `.3.x`.
- Thermal break between the tube and the box: needs a tube temperature measurement (`.2.5`).
- Tube dimensions: measured 34.89 mm OD, 30.83 mm ID, 31.94 mm free length (`.2.8`); roundness, end condition and clearance on the egg still to check.
