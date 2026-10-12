# PCB flow scripts

How the board was placed and routed (first pass). Run from a scratch directory; paths inside are under `/tmp/minimeat-place/`.

1. `parts.py <out.json>`: reads `hw/minimeat.kicad_sch` and the footprint libraries, writes each reference's footprint and courtyard box.
2. `layout.py`: the hand-chosen zones, packed and overlap-checked against the courtyards; writes `pos.json` and a picture. The positions go into KiCad with the Konnect `set_component_placements` tool.
3. Routing is done outside the live editor on a copy of the saved board, with KiCad's bundled Python (`pcbnew`) and Freerouting 2.4.1 (the jar from the KiCad plugin): `ExportSpecctraDSN`, `java -jar freerouting.jar -de x.dsn -do x.ses -mp 60`, `ImportSpecctraSES`.
4. `gndvias.py <in> <out>` adds a via and stub for ground pads the router left off the plane; `finish.py <in> <out>` adds the top-side ground pour, full connection for small ground pads, island removal and silkscreen text tidy.

The routed board replaces `hw/minimeat.kicad_pcb`; reload it in KiCad. Never edit the `.kicad_*` files by hand.

Closing the last open items (v8): `islandvias.py <in> <out>` puts a ground via in every top-pour fragment that has none (checked against copper on all layers). TIP2/TIP3 at U9 were joined by hand: a 0.6 mm via and an In2 stub for TIP2, an F.Cu run for TIP3; TIP1's tail from R19 was rerouted to make room.

Probe bias rework (after the ngspice check, `docs/sim/probe-frontend.md`): `probe_bias_rework.py <in> <out> <dir>` moves each R23-R26 pad 2 from AINx to TIPx, reroutes the affected links with a small maze router (`maze.py` on F.Cu, `maze3.py` with vias; copy them to `<dir>` as `mr.py` and `mr3.py`), refills the pours.

TMP117 isolation: `tmp117_keepout.py <in> <out> <dir>` drops the ground vias under the sensor, routes its two ground pins with traces to the ground via at C33 (F.Cu, with `maze3.py`) and adds a no-pour rule area (F.Cu, In1, B.Cu; tracks and vias allowed) over x 0.5-7.0 mm, y 50.9-53.6 mm, so the sensor follows the air at the louvre, not the board's ground planes. No board slot: J7 sits right above, and a slot would cost routing for little.

Fab outputs: `uv run python hw/tools/fab.py` writes `hw/fab/minimeat-gerber.zip`, `minimeat-bom.csv` (JLC columns, only parts with an LCSC number; the KK headers and test points are listed as not assembled) and `minimeat-cpl.csv` (top side, KiCad's rotation and Y; check rotations of polarised parts, the USB-C, the module and the ADC in JLC's viewer when ordering).

Pre-fab review fixes (`power_rework.py <in> <out>`): D1 turned 180 degrees with its pads' nets swapped (the TVS was backwards: pin 1 is the cathode), and the power copper widened segment by segment to the widest that keeps 0.2 mm to everything else (VBUS_RAW, VBUS_F, VSERVO, VFAN, the switch node up to 0.8 mm; 3V3 to 0.4 mm). Short pad necks and the crowded In2 stretches stay at 0.2 mm; order 1 oz inner copper.

Rerouting the narrow power stretches (`reroute_power.py <in> <out>`, run from `hw/tools` for `maze_w.py`): each chain of tracks under 0.35 mm on VBUS_RAW, VBUS_F, VSERVO and VFAN is taken out and routed again between its two end nodes at 0.5 mm (0.4 or 0.3 if that fails) on F.Cu, In2 and B.Cu with vias, around everything else; a chain that cannot be routed goes back as it was. Then `power_rework.py` (widens to 0.8 mm where there is room, refills), then `islandvias.py`. After it only five pad necks of 0.3-1.2 mm remain at 0.2 mm; DRC unchanged (the four USB-C hole clearances), no unconnected items, no schematic mismatches.

Net names: the seven auto-named nets got schematic labels (`LED_A`, `3V3A_FILT`, `BUCK_SW`, `VBUS_LDO`, `SERVO_ILIM`, `FAN_ILIM`, `ADC_RESET`); `rename_nets.py <in> <out>` gives the board the same names. The pins left as `unconnected-(...)` are the deliberate no-connects.
