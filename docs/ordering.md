# Ordering list: everything except the printed and CNC parts

Quantities are for **5 units** (the build size), with spares where a part is small or easy to lose. "Per unit" is what one box uses. Status says whether the part is pinned down in the repository (a named part or a size the model depends on) or still has to be chosen. Nothing here has been ordered.

The printed and machined parts are in `build/mech/3dprint` (JLC3DP, all three in MJF PA12S-HP black: duct, hood, pan) and `build/mech/cnc` (JLCCNC: flap, crank, coupler), each with a README and a zip; see `docs/jlc3dp.md`.

## 1. JLCPCB: board and assembly

| What | Qty | Files | Notes |
|---|---|---|---|
| Board, 4 layers, with SMT assembly | 5 | `hw/fab/minimeater-panel-gerber.zip`, `hw/fab/minimeater-bom.csv`, `hw/fab/minimeater-panel-cpl.csv` | Order the one-up panel: it carries JLC's tooling holes (three 1.152 mm NPTH) and fiducials on 5 mm rails left and right, so choose "edge rails: added by customer" (the single-board files, `minimeater-gerber.zip` and `minimeater-cpl.csv`, are there if JLC prefers to add its own rails). Snap the rails off after delivery. Every part in the BOM is a JLC part in stock (checked when each was chosen, `docs/parts/electronics-shortlist.md`); check stock again at order time. All SMT is on the top side. The fan and servo headers, and the test points, are not assembled (section 3). |

## 2. Mechanical hardware

| Part | Per unit | For 5 | Status | Notes |
|---|---|---|---|---|
| Noctua NF-A6x25 5V PWM fan | 1 | 5 | named | 60 × 60 × 25, 4-pin PWM with tach. It is mounted from the rear with 4 screws: check that the retail box includes them (`docs/parts/fan-noctua-nf-a6x25.md`). |
| Hitec HS-5065MG servo | 1 | 5 | named | 3-pin lead, 160 mm (`docs/parts/servo-hs-5065mg.md`). |
| Secraft 15 mm aluminium servo arm, Hitec 24T | 1 | 5 | named | The linkage uses its M2 hole at 12 mm radius. Comes with its centre screw. |
| Ø3 rod, 2.5 mm D flat, **42.7 mm ±0.1** | 1 | 5 + 1 | JLCCNC, `rod-d-shaft.step`, SUS304: **$30.80 at quantity 1** (instant quote, 2026-10-13; get the price at 6) | Made with the flap, crank and coupler. The quote's general tolerance is ISO 2768 medium (±0.1 on Ø3), too loose for the flap's D-bore (+0.03 over, with retaining compound): the rod needs Ø3 h7 and the flat held to ±0.05, which `uv run mech-export` now puts on a drawing (`cnc/rod.step` and `cnc/rod.pdf`, quantity 6). Ask JLCCNC to turn it from 3 mm bar instead of milling it from larger stock. The length matters: the end stands 0.3 mm above the crank and 0.6 mm under the coupler (`docs/mech.md`). |
| M2 shoulder screw, Ø3 shoulder × 3 mm long, M2 thread | 2 | 10 + 2 | named (head size to check) | ISO 7379 style, stainless. One into the crank's tapped M2 hole, one through the servo arm's M2 hole with a nyloc nut. Head Ø5 × 2.0 (McMaster's/Zoro's STR601M3X4, which lists a 4 mm shoulder: get the 3 mm length); the hood clears the heads by 1.2 mm, so a head taller than 2.0 needs a check. The shoulder must be 3 mm (the bar and its boss are 2.9) and the thread long enough for the crank (3 mm arm) or the horn plus the nut: M2 × 4 or longer. |
| M2 nyloc nut | 1 | 5 + 5 | named | Under the servo arm. |
| M2 washer | 2 | 10 + 5 | named | |
| M2 × 3 hex-socket set screw, cup point | 1 | 5 + 5 | size is my pick | Radial on the rod's flat, in the crank's hub (tap drill Ø1.6). |
| M2 × 6 screw | 2 | 10 + 4 | named | Servo to its two bosses (6 mm pilot in the print): thread-forming for plastic. Check whether the servo's own screws will do. |
| M2.5 × 6 screw | 3 | 15 + 5 | named | The board on its three standoffs. (`docs/mech.md` said 4 until now; the board has 3 holes.) |
| M2.5 × 20 screw | 2 | 10 + 4 | named | Shell, front pair, from the belly. Pan-head or cheese-head: the head sits in a Ø4.6 channel, so a head up to 4.5 mm. |
| M2 × 16 screw | 2 | 10 + 4 | named | Shell, rear pair. |
| 34 × 2.5 O-ring | 1 | 5 + 5 | size named, **material is my pick** | Seals the duct's socket on the egg's tube, which can reach 65-90 °C (the `.2.5` thermal check is still open): FKM or silicone, not NBR. |
| Silicone port caps | 5 | 25 | **unsourced** | One for the USB-C port and four for the jacks; the bay has nowhere to tie them. Any USB-C cap and 2.5 mm jack cap that fit the openings (USB-C 13.2 × 7.4, jack Ø5): measure the first print. |
| Clear Ø1.75 mm filament | ~15 mm | 1 piece | named | The LED's light pipe, glued flush in a Ø1.9 hole. |
| Retaining compound, removable grade (Loctite 641 or similar) | | 1 bottle | my pick | The flap on the rod. A permanent grade (638) would make the rod impossible to pull for sanding the duct. |

### Tool you may need

- **Ø32 mm H7 machine reamer (+0.025/0), 40 mm or more of flute.** The duct's straight bore is Ø32.0 and is reamed at the flap station after printing; as printed (±0.3 mm) it would jam the flap or leak. Ø32 is a standard size (for example the Hoffmann machine reamer H7 Ø32, [164000-32](https://www.hoffmann-group.com/IE/en/brownlo/p/164000-32)); a plain HSS straight-flute one from an industrial supplier or a general import brand is enough for five nylon ducts, and a fixed reamer costs much less than the adjustable one. Or ask a machine shop to ream or bore the five. See `docs/mech.md` and `docs/jlc3dp.md`.

## 3. Electronics, hand-soldered

| Part | Per unit | For 5 | Notes |
|---|---|---|---|
| Molex KK 254 header, right angle, 4 positions, 22-05-3041 | 1 | 5 + 2 | Fan. Hand-soldered from the board's back, not in the JLC assembly; the plug enters toward the rear edge. |
| Molex KK 254 header, right angle, 3 positions, 22-05-3031 | 1 | 5 + 2 | Servo. Same. |

## 4. To use the box

| What | Qty | Notes |
|---|---|---|
| USB-C to USB-C cable | 1 per unit | Not USB-A to USB-C: that always reads as a 500 mA source and the box then raises its `supply` fault (`docs/adr/0004-usb-c-power.md`). |
| USB-C charger that advertises 1.5 A or 3 A (7.5 W or more) | 1 per unit | A wall charger for overnight cooks: power banks often switch off under a light load. |
| Thermistor probes with a 2.5 mm plug | 1 pit probe + up to 3 food probes | ThermoWorks Pro-Series (about 100k) or a generic import probe; a Maverick ET-732 (about 1M) also reads. Probes come from other sources than the board. Overmold up to about 18 mm long, 8 mm across: longer ones stand proud of the bay. |

Not on this list because you already have them: the egg and its Ø34.93 intake tube the box slides onto, and a computer to flash the board over USB-C.

## 5. Open items

- The port caps are not sourced. The rod is quoted (above), not yet ordered.
- The shoulder screw's head size and thread length are to confirm with the supplier.
- The O-ring material waits for the tube temperature measurement (`.2.5`).
