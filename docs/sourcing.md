# Sourcing: where to buy the mechanical hardware

Companion to `docs/ordering.md` (what and how many). Researched 2026-10-12 from supplier pages and web searches; **no stock or prices were confirmed** (distributor pages mostly would not load), so check each link at order time. Assumes a US buyer: McMaster-Carr as the main source, and Mouser or Digi-Key for the electronics. The fastener plan assumes the consolidation in the ordering list's follow-up (M2 for the board, servo and shells; no set screw): the M2.5 lines drop out if it is adopted.

## Found: buy as listed

| Part | Source | Part number | Notes |
|---|---|---|---|
| M2 shoulder screw, Ø3 shoulder × **3 mm**, M2 thread | McMaster-Carr | 316 stainless **90269A187** (18-8: 90265A100) | The size exists as a stock part: 3 mm shoulder, 4 mm thread, head Ø5 × 2. Zoro and Accu only list 4 mm shoulders. 316 over 18-8 (quality rule). **Check:** the thread is 4 mm; the horn side takes the horn, a washer and a nyloc nut on it, so confirm the stack against the horn's thickness. |
| Low-profile alternative | McMaster | 18-8 **90318A412** (head Ø5 × **0.7**, 3 mm shoulder, 4 mm thread); 90323A602 is head × 1.3 | The hood roof is 0.43 mm above the horn pin's head (open item); a 0.7 head buys about 1.3 mm back. `pin_head_d` is 5.5 in `params.py`: it becomes 5.0. Hex drive 1.5 mm, 95,000 psi. |
| M4 brass heat-set insert | McMaster | **94180A351** (installed length 4.7 mm, pack 100) or 94180A353 (7.9 mm, pack 50) | Tapered, ASTM B16. Check the hole against the boss (`docs/mech.md`); McMaster's M4 install tip is 92160A323. JLC3DP's insert service is the alternative (`minimeat-dx8`). |
| 34 × 2.5 FKM O-ring | RS PRO 1965799 (79 Shore A); Tameson S24CW-X10 (80 A, 10 pcs, EU) | | Standard size (OD 39). McMaster carries metric O-rings too: search 34 mm ID × 2.5 mm width in Viton. |
| Hitec HS-5065MG servo | Horizon Hobby HRC35065S ($37.99); Aloft Hobbies ($31.50); MotionRC | | Several US hobby shops list it. |
| Secraft 15 mm aluminium arm, Hitec 24T | Gator RC, SE-SA-100 ($3.50) | | **Only 2 were listed in stock** and no other retailer was found: buy 5 + 1 now, or choose a second source. |
| Noctua NF-A6x25 5V PWM | (US retailers not confirmed) PCCaseGear, QuietPC and others abroad | NF-A6x25-5V-PWM | Search Amazon, Newegg, B&H, Noctua's own store. |
| Molex KK header, right angle, 22-05-3041 (4P) and 22-05-3031 (3P) | Mouser, Digi-Key, Heilind, Farnell, RS | | Right angle so the plugs lie flat under the board (the vertical 22-23 parts are 11.8 mm tall). Molex's STEP files are in `hw/lib/3d` (turned into the footprint frame by `hw/tools/kk_ra_step.py`). Stock not checked. |
| Loctite 641 | Grainger, HISCO (50 ml), TestEquity, Amazon; Silmid (EU, 10 ml €10.73) | 641 | 10 ml is plenty. HISCO has a 10-bottle minimum: buy the 10 ml elsewhere. |

## Found, but not settled

| Part | What was found | Options |
|---|---|---|
| M4 thumbscrew, stainless | No M4 knurled thumb screw turned up at McMaster; its M4 spade-head items are thumb **nuts**. AFT Fasteners lists Kipp K0140.042X10 (DIN 464 knurled, M4 × 10, stainless; $63.82 per 10). Aspen Fasteners has stainless DIN 465 (slotted, high) in 5 and 6 mm only. | DIN 464 M4 × 10 or × 12 in 304/316 is the part to ask for: any fastener house. First measure the length (the order list already says so). |
| M2 thread-forming screws (6 and 16 mm) | McMaster has the family (Phillips and Torx, 18-8, flat, pan and others) but I could not read its M2 rows. The M3 flat head T8 is 95893A191, so M2 exists in the same group. | Open McMaster's "Thread-Forming Screws for Plastic", filter M2, pan head, 316 or 18-8 stainless, lengths 6 and 16. Or buy the plain M2 machine-screw assortment (Albany County 1060-096 has M2 × 6 and × 12 but no × 16) and run the printed pilots as machine thread. |
| M2 nyloc nuts, M2 washers | Not searched; stocked by McMaster, Grainger, any fastener house in A2 and A4. | A stainless M2 assortment kit with nuts and washers would cover both. |

## Not found: the real problems

1. **Ø3 rod with a 2.5 mm D flat, 42.7 ± 0.1.** Quoted at JLCCNC: SUS304, ISO 2768 medium, $30.80 for one (2026-10-13); see `docs/ordering.md` for the tolerance to add. No stock 3 mm D-shaft turned up (the stainless D-shafts on offer are ¼ in, from ServoCity, SparkFun and McMaster). Options:
   - **Have it made.** Ask JLCCNC for six pieces in the same order as the flap, crank and coupler: turning is offered with ±0.02 mm, and stainless with the grade to be confirmed on the instant quote. This is what I'd do.
   - **Cut and mill it yourself:** Misumi's 3 mm SUS440C straight shaft is sold cut to length (100 mm and up), then mill the flat.
   - Pololu's "3 mm D-shaft" category is wheels and collets; it does not sell the shaft.
2. **Silicone port caps.** The jacks are open Ø5 holes beside the housings, so the usual 3.5 mm and USB-C dust plugs only partly fit. USB-C: PortPlugs (a 5-pack with matching 3.5 mm plugs was listed at Newegg), and silicone USB-C cover inserts from Adafruit, Element14 and others. Jacks: a generic Ø5 silicone flanged plug (McMaster's tapered or flanged silicone plugs) is the likely fit; the 3.5 mm plugs are probably too small. Buy samples and measure on the first print; the bay's end walls no longer have tether holes.
3. **Ø32 H7 reamer.** The reamer the order list points to (Hoffmann 164000-32) is a Morse-taper machine reamer around CHF 200; a Somta 32 mm is $300 and was sold out. Five nylon ducts do not justify that: get a local machine shop to ream five bores to Ø32 H7 at the flap station, or buy the reamer for a hand-feed on a drill press only if you will reuse it.
4. **Clear Ø1.75 filament for the light pipe.** A few centimetres of clear PETG or PLA filament from any filament seller or Amazon.

## Order plan

- **McMaster:** shoulder screws, M4 inserts, M2 screws, nyloc nuts and washers, port plugs, O-rings (one account, one shipment).
- **JLCCNC:** the rod, with the flap, crank and coupler.
- **Mouser or Digi-Key:** the KK headers.
- **Hobby shops:** servo, arm, fan.
- **Amazon or a fastener house:** the thumbscrew, retaining compound, filament, USB-C caps.
