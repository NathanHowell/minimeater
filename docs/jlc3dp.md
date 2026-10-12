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

- **`duct`: MJF PA12-HP** (or PA12S-HP, the black one). The hot part gets a 175 °C material and the insert service's best-supported process. Step up to FDM PEEK only if the socket measures hotter than about 140 °C; it is three times the price and brown, and the insert service is not stated for it.
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

### The flap is machined, not printed

The disc's edge and its D-bore need ±0.05 mm; the vendors print to ±0.3 mm. The flap is a CNC part in `minimeat-cnc.zip` with the crank and the coupler, and the printed duct's bore at the flap station is reamed to size after printing (Ø31.6 +0.03/0, about 40 mm: an adjustable or fixed reamer in a drill press). Without the ream the gap swings from jammed to a 0.38 cm² leak (`tests/test_mech.py::test_flap_fit_needs_the_bore_reamed`).

## JLC3DP's thin-wall heatmap (checked 2026-10-12)

The upload page colours walls under 1.2 mm yellow and under 0.5 mm red. A first upload of `duct`, `hood` and `pan` lit up in several places. Each one was checked by casting a ray inward from 40,000 sample points on the surface of each part (the distance to the opposite face is the wall there) and clustering the thin ones; edge samples are noisy, so what follows is what the clusters and the geometry agree on.

Fixed:

- **The 0.2 mm web between the USB-C opening and the first jack's** (the one that was too thin): the two openings are now one cut (`shell.port_openings`). I made it by widening the USB-C opening to 13.2 mm; the web was my mistake.
- **Louvre blades, 0.81 mm** (the 3.0 mm slots cut at 44° leave 5.3 × cos 44° - 3.0): slots 3.2 at a 6.2 mm vertical pitch, blades 1.26 mm, open area 1520 mm² (it was 1583; the test wants 1500). The louvre field is 4 mm taller so the last slot leaves no sliver.
- **The seam, 0.8-1.0 mm**: the hood's outer skirt is the wall less the pan's tongue and 0.2 clearance. The shell wall is now 2.8 mm (seven perimeters) with a 1.3 mm tongue and rib, so the skirt is 1.3 mm. The envelope grows to 75.2 × 73.9.
- **The rod's blind pocket** left a 0.6 mm skin under the duct; now 1.2 mm (the rod is 0.6 mm shorter, 42.4 mm).
- **The shelf's rear edge**, a knife edge: a 1.2 mm vertical land now stops the lean.
- **The rear screw columns' sleeve, 0.08 mm** (second upload): the pan's M2 head channel (radius 2.15) was nearly tangent to the cavity wall at y 29.9, so a sheet of that thickness stood between them over 20 mm of height. The pair moved out to |y| 33.3 (`rear_screw_y`): 1.2 mm to the cavity, 1.3 mm to the hull, and the fan's screw heads still clear.
- **The front shell screw columns**: the head channel (Ø5.8) nearly equalled the column (Ø6), a 0.1 mm sleeve. The column is 6.7 mm with a Ø4.6 channel, a 1.05 mm sleeve. 6.7 is the most that works: bigger columns make the pan fail to tessellate in OpenCascade.

Left, and why:

- **Knife edges** where a 44° face or a fillet meets a surface (the teardrop tips of the servo bosses, the bay's 45° end wall and floor edge, louvre blade tips at the faces): they come with the supportless geometry. The flagged volume is small and a slicer or MJF handles them. Roughly 100 duct samples and the bay's end-wall clusters in the pan are of this kind.
- **The hood roof's rear edge above the grille**: the louvre field's cutter runs 4 mm into the cavity and leaves the roof's rear underside as a 44° wedge that comes to a point (red, 0.0-0.5 mm, 20 mm wide). Ending the cutter at the wall instead leaves a square 2.8 mm edge, which prints as an overhang over the 60 mm² per-patch limit in `test_prints_without_support`, so it stays.
- **Blade tips and the grille's rim**: each louvre blade is a parallelogram with 46° corners, so its ends are flagged yellow along every edge, and the 1.2 mm rear lip sits exactly on the threshold. Both come with the 44° louvres.
- **A 3 mm² patch where the rear screw channel meets the fan shim block's bottom face** (z -26 to -30, 0.1-0.3 mm): small, and the block is a locating aid, not a structural part.
- **The eave and its drip lip** at about 1.0 mm.
- **Around the ambient vents** (x -87 to -60) the seam skirt is 1.1 mm.

All of it passes MJF PA12's 1.0 mm minimum; the ASA minimum is 1.2, and these leftovers would still be flagged for ASA. That is one more reason to order the shells in PA12-HP/PA12S-HP as well.
