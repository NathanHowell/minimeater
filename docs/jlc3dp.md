# JLC3DP: materials and the threaded-insert service

Researched 2026-10-12 from JLC3DP's own help pages (links below); nothing here has been ordered. Numbers are the vendor's.

## What we need from the plastic

- The duct's socket slides onto the egg's intake tube and the O-ring seals there. That tube is hot (the `.2.5` thermal check has not been done), so heat deflection matters more than anything else for `duct`. The shells and the flap are cooler; the shells are outdoors in sun and weather.
- The parts were designed for FDM: 2.4 mm walls, 44-degree overhangs and teardrop holes so they print without supports. They print as well on MJF (no supports needed there at all).

## Materials

| Material | Process | HDT (vendor) | Limits | Notes |
|---|---|---|---|---|
| [ASA](https://jlc3dp.com/help/article/ASA-Plastic) | FDM | 97.8 °C | 250 mm cube, ±0.3 mm or 0.4 %, 1.6 mm wall (1.2 min), black or white | UV-stable, what the design assumed; marginal for the socket |
| [PA12-HP](https://jlc3dp.com/help/article/PA12-HP-Nylon) | MJF | 175 °C (ASTM D648, 0.45 MPa) | 370 × 276 × 360 mm, ±0.3 mm within 100 mm and ±0.4 % above, 1 mm wall, natural grey | no supports; load rating at 1.82 MPa not given |
| [PA11-HP](https://jlc3dp.com/help/article/pa11-hp-nylon) | MJF | 185 °C at 0.45 MPa, 54 °C at 1.82 MPa | | the two-load figure shows how much the number depends on stress |
| [JLC Temp Resin](https://jlc3dp.com/help/article/jlc-temp-resin) | SLA | 101 °C, Tg 130 °C | not for outdoor or UV; yellows with heat | brittle; inserts must be pressed or glued, not heat-set |
| [PA12S-HP](https://jlc3dp.com/help/article/pa12s-hp-nylon) | MJF | 175 °C (load not stated) | 370 × 276 × 360 mm, ±0.3 mm / 0.4 %, wall > 1 mm, black or grey | PA12-HP in black |
| PAC-HP | MJF | 80 °C (2023, no load stated) | | full colour; no |
| [1172 Pro](https://jlc3dp.com/help/article/Precimid-1172-Pro) | SLS | 179 °C at 0.45 MPa (ASTM D648) | 250 × 250 × 590 mm, wall ≥ 1 mm, white | the hottest nylon listed |
| [3301PA](https://jlc3dp.com/help/article/3301pa-nylon) | SLS | 147 °C at 0.45 MPa, 55 °C at 1.8 MPa | 350 × 350 × 400 mm, white | |
| [3401GB](https://jlc3dp.com/help/article/3401gb-nylon) | SLS | 150 °C at 0.45 MPa, 60 °C at 1.8 MPa | 340 × 340 × 400 mm, grey-white | glass-bead filled |
| [PEEK](https://jlc3dp.com/help/article/peek-plastic) | FDM | 144 °C at 1.8 MPa (ISO 75-f) | 580 × 480 × 480 mm, wall > 1.6 mm, ±0.3 mm, brown only, from $3 | the best heat figure at a real load; the dearest |
| [PEKK](https://jlc3dp.com/help/article/pekk-plastic) | FDM | 139 °C at 0.45 MPa | as PEEK | |
| [PPA-CF](https://jlc3dp.com/help/article/ppa-cf-plastic) | FDM | 84 °C at 0.45 MPa | as PEEK, black | no better than ASA |
| [PA12-CF](https://jlc3dp.com/help/article/PA12-CF-Plastic) | FDM | 105 °C at 1.8 MPa | 580 × 480 × 480 mm, wall 1.6 (1.2 min), black | |
| [9000HE](https://jlc3dp.com/help/article/9000he-resin) | SLA | 60 °C (ASTM D648, 66 psi) | | no |

Their pages give the load for each figure inconsistently: 0.45 MPa numbers (PA12-HP 175, 1172 Pro 179) are not comparable with 1.8 MPa ones (PEEK 144, PA12-CF 105). The duct carries almost no load, so the 0.45 MPa figures are the relevant ones for it, but two of the pages show how far a nylon falls at 1.8 MPa (3301PA: 147 to 55 °C).

My recommendation, to be settled after the thermal check:

- **`duct` and `flap`: MJF PA12-HP** (or PA12S-HP, the black one). The hot, precise part gets a 175 °C material and the insert service's best-supported process. Step up to FDM PEEK only if the socket measures hotter than about 140 °C; it is three times the price and brown, and the insert service is not stated for it.
- **`hood` and `pan`: ASA (FDM),** for weather and UV, with a walls margin over the 1.6 mm recommendation and under the 250 mm limit. (That UV is a weakness of grey nylon is general knowledge, not from JLC's pages, which are silent on it.) PA12-HP for everything is the single-material alternative.
- Tolerance is ±0.3 mm either way. The O-ring groove (1.9 mm deep, squeeze 18 % on paper, the test wants 10-30 %) would swing about ±12 % at that tolerance; widen the margin or order the duct with the groove machined if the first seals leak.

## The threaded-insert service

Sources: [service page](https://jlc3dp.com/help/article/threaded-insert-service), [design guide](https://jlc3dp.com/blog/threaded-inserts-3d-printing).

- Eight brass types only, no custom sizes: M3 (types 1-5, 3-6.4 mm long), M4 (6: `M4*5*4`, 7: `M4*5*8`) and M5 (8: `M5*6*5`). Hole diameters and depths are fixed per type (M4: Ø4.6, 6 or 10 mm deep; M3: Ø3.6-4.0; M5: Ø5.9).
- Rules: 3.0 mm minimum wall around the hole, a flat mounting surface of at least 14 × 14 mm. Orders must come with a 2D drawing (PNG or PDF) marking each insert's type and position. 500 or more inserts add lead time.
- Technologies: the guide rates MJF and SLS nylon "excellent" (heat-set), SLA resin unsuitable for heat (press or glue), FDM PLA workable. **ASA is not mentioned**; the service page does not say which processes it covers, and gives no price, minimum order or standard lead time. These are questions for their support chat.
- No M2 or M2.5. Our small screws (M2 servo, M2.5 board and shell, M2 shells) cannot use the service; they go into the plastic directly (thread-forming screws take well in nylon), or we fit our own inserts.

### What it does to our design

The only insert is the M4 for the thumbscrew in the duct's boss, and it does not fit the service's rules: the boss is a 9.4 mm block with a Ø5.6 hole (a typical heat-set size), so the wall is 1.9 mm, not 3.0, the hole is 1.0 mm wider than their Ø4.6, and the flat is not 14 × 14. Using the service means a bigger block (at least 10.6 mm across the hole, and 14 × 14 flat) and the Ø4.6 hole in the CAD. Tracked in bead `minimeat-dx8`.

### Should the M2 and M2.5 screws become M4 to use the service?

No. The service's M4 rules (a Ø4.6 hole, a 3.0 mm wall around it so a boss of at least 10.6 mm, a 14 × 14 mm flat) do not fit any of those places, and three of them cannot change at all:

- **Servo tabs** (2 × M2): the HS-5065MG's holes are Ø2.0.
- **Board** (M2.5 into three Ø5.5 standoffs): the PCB holes are Ø2.7; an M4 boss would be 10.6 mm across and need a 14 × 14 flat on a 46 × 52 mm board's worth of floor.
- **Shell columns** (2 × M2.5 × 20, 2 × M2 × 16, 6 mm bosses in 2.4 mm walls): the same problem. The hood and pan are also the ASA parts, where the service is not stated.

The M3 types (hole Ø3.6, a 9.6 mm boss, the same flat) are no better. Use the service for the one M4. For the small screws: thread-forming screws straight into the MJF nylon, or heat-set M2/M2.5 inserts we press in ourselves (about 12 per unit; cheap, a soldering iron on nylon) if the case is opened often, which it is: the button is pressed with it open.
