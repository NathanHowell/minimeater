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
| PAC-HP | MJF | 80 °C (2023, no load stated) | | full colour; no |
| SLS nylons (3301PA, 3201PA-F, 1172 Pro) | SLS | blog claims up to 179 °C | | as MJF for our purposes |
| PEEK, PEKK | | | | only a news headline on their site, no listing or numbers found; ask if wanted |

My recommendation, to be settled after the thermal check:

- **`duct` and `flap`: MJF PA12-HP.** The hot, precise part gets the 175 °C material and the insert service's best-supported process.
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
