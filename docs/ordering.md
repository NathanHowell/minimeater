# Ordering list: everything except the printed and CNC parts

Quantities are for **5 units** (the build size), with spares where a part is small or easy to lose. "Per unit" is what one box uses. Status says whether the part is pinned down in the repository (a named part or a size the model depends on) or still has to be chosen. Nothing here has been ordered.

The printed and machined parts are in `build/mech/3dprint` (JLC3DP: duct, hood, pan) and `build/mech/cnc` (JLCCNC: flap, crank, coupler), each with a README and a zip; see `docs/jlc3dp.md`.

## 1. JLCPCB: board and assembly

| What | Qty | Files | Notes |
|---|---|---|---|
| Board, 4 layers, with SMT assembly | 5 | `hw/fab/minimeat-gerber.zip`, `hw/fab/minimeat-bom.csv`, `hw/fab/minimeat-cpl.csv` | Every part in the BOM is a JLC part in stock (checked when each was chosen, `docs/parts/electronics-shortlist.md`); check stock again at order time. All SMT is on the top side. The fan and servo headers, and the test points, are not assembled (section 3). |

## 2. Mechanical hardware

| Part | Per unit | For 5 | Status | Notes |
|---|---|---|---|---|
| Noctua NF-A6x25 5V PWM fan | 1 | 5 | named | 60 × 60 × 25, 4-pin PWM with tach. It is mounted from the rear with 4 screws: check that the retail box includes them (`docs/parts/fan-noctua-nf-a6x25.md`). |
| Hitec HS-5065MG servo | 1 | 5 | named | 3-pin lead, 160 mm (`docs/parts/servo-hs-5065mg.md`). |
| Secraft 15 mm aluminium servo arm, Hitec 24T | 1 | 5 | named | The linkage uses its M2 hole at 12 mm radius. Comes with its centre screw. |
| Ø3 rod, 2.5 mm D flat, **42.9 mm ±0.1** | 1 | 5 + 1 | **unsourced** | Stainless or hardened steel. Cut a plain Ø3 rod to length and file or mill the flat (2.5 mm across), or buy 3 mm D-shaft and cut it. The length matters: the end stands 0.5 mm above the crank and 0.6 mm under the coupler (`docs/mech.md`). |
| M2 shoulder screw, Ø2 shoulder × 3 mm | 2 | 10 + 2 | **open question, see section 5** | One into the crank's tapped hole, one through the servo arm with a nyloc nut. |
| M2 nyloc nut | 1 | 5 + 5 | named | Under the servo arm. |
| M2 washer | 2 | 10 + 5 | named | |
| M2 × 3 hex-socket set screw, cup point | 1 | 5 + 5 | size is my pick | Radial on the rod's flat, in the crank's hub (tap drill Ø1.6). |
| M2 × 6 screw | 2 | 10 + 4 | named | Servo to its two bosses (6 mm pilot in the print): thread-forming for plastic. Check whether the servo's own screws will do. |
| M2.5 × 6 screw | 3 | 15 + 5 | named | The board on its three standoffs. (`docs/mech.md` said 4 until now; the board has 3 holes.) |
| M2.5 × 20 screw | 2 | 10 + 4 | named | Shell, front pair, from the belly. Pan-head or cheese-head: the head sits in a Ø4.6 channel, so a head up to 4.5 mm. |
| M2 × 16 screw | 2 | 10 + 4 | named | Shell, rear pair. |
| 34 × 2.5 O-ring | 1 | 5 + 5 | size named, **material is my pick** | Seals the duct's socket on the egg's tube, which can reach 65-90 °C (the `.2.5` thermal check is still open): FKM or silicone, not NBR. |
| M4 brass heat-set insert | 1 | 5 + 5 | named | Into the thumbscrew boss on the duct; check the bore size against the insert's data sheet, or use JLC3DP's insert service (`docs/jlc3dp.md`, bead `minimeat-dx8`). |
| M4 thumbscrew, stainless | 1 | 5 + 5 | length not chosen | Reaches the 34.93 mm tube through a short boss: measure on the first print. |
| Silicone port caps, tethered | 5 | 25 | **unsourced** | One for the USB-C port and four for the jacks; each has a tether loop through a Ø2 hole in the bay's end walls. Any tethered USB-C cap and 2.5 mm jack cap that fit the openings (USB-C 13.2 × 7.4, jack Ø5): measure the first print. |
| Clear Ø1.75 mm filament | ~15 mm | 1 piece | named | The LED's light pipe, glued flush in a Ø1.9 hole. |
| Retaining compound, removable grade (Loctite 641 or similar) | | 1 bottle | my pick | The flap on the rod. A permanent grade (638) would make the rod impossible to pull for sanding the duct. |

### Tool you may need

- **Ø31.6 mm reamer, +0.03/0, about 40 mm long.** The duct's bore at the flap station is reamed after printing; as printed (±0.3 mm) it would jam the flap or leak. You can instead ask a machine shop to ream the five ducts. See `docs/mech.md` and `docs/jlc3dp.md`.

## 3. Electronics, hand-soldered

| Part | Per unit | For 5 | Notes |
|---|---|---|---|
| Molex KK 254 header, 4 positions, 22-23-2041 | 1 | 5 + 2 | Fan. JLC's copy is out of stock, so this is Mouser or Digi-Key; left unpopulated in the JLC assembly. |
| Molex KK 254 header, 3 positions, 22-23-2031 | 1 | 5 + 2 | Servo. Same. |

## 4. To use the box

| What | Qty | Notes |
|---|---|---|
| USB-C to USB-C cable | 1 per unit | Not USB-A to USB-C: that always reads as a 500 mA source and the box then raises its `supply` fault (`docs/adr/0004-usb-c-power.md`). |
| USB-C charger that advertises 1.5 A or 3 A (7.5 W or more) | 1 per unit | A wall charger for overnight cooks: power banks often switch off under a light load. |
| Thermistor probes with a 2.5 mm plug | 1 pit probe + up to 3 food probes | ThermoWorks Pro-Series (about 100k) or a generic import probe; a Maverick ET-732 (about 1M) also reads. Probes come from other sources than the board. Overmold up to about 18 mm long, 8 mm across: longer ones stand proud of the bay. |

Not on this list because you already have them: the egg and its Ø34.93 intake tube the box slides onto, and a computer to flash the board over USB-C.

## 5. Open items

- **Shoulder screws.** ISO 7379 shoulder screws with a Ø2 shoulder have an M1.6 thread, as I recall (an M2 thread goes with a Ø3 shoulder). The model assumes a Ø2 pin and an M2 tapped hole in the crank, which would not match a standard part. Two ways out: (a) keep the Ø2 shoulder and tap the crank M1.6 (tap drill Ø1.25), and the horn end uses an M1.6 nyloc nut; or (b) Ø3 shoulder, M2 thread, and the coupler's holes become Ø3.1. This needs a decision before the CNC order, because it changes the crank's drawing (bead filed).
- The rod, the port caps and the thumbscrew length are not sourced.
- The O-ring material waits for the tube temperature measurement (`.2.5`).
