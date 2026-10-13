---
name: pcb-relayout
description: The minimeater PCB re-layout workflow (epic minimeat-axk) - datasheet-correct IC blocks laid out off-board one at a time, then considered placement and hand routing reviewed from renders. Use whenever working on the board layout, a layout block, placement, routing, the staging area, or the hw/layout notes.
---

# PCB re-layout (epic minimeat-axk)

The first board was placed roughly and routed by a script: wrong rotations, traces where fills belong, far too many
vias, routes hopping across 2-4 layers, no real differential pair. It was stripped (commit 95ab894: no tracks or
vias; J1-J7, D2, U10 locked). We are rebuilding it by hand, in this order:

1. **Blocks.** Each IC with its own parts, laid out OFF the board in a staging area, exactly to its datasheet's
   layout guidance. One commit per IC. Never place a block on the board in this phase.
2. **Render** all blocks with traces coloured by net.
3. **Placement by thinking, not scripts.** Send the renders, outline, locked parts and netlist to an isolated
   subagent on the Fable model for recommendations; review them with the user before moving anything.
4. **Hand routing** from render reviews: short, few vias, no layer hopping, USB as a proper 90 ohm pair.
5. Fiducials and NPTHs (mounting holes) last; the mech model (`src/minimeater/mech`, `Pcb.holes`) follows them.

## Per-block procedure

1. `bd update <bead> --claim`. Find the block's parts from the schematic grouping (parts sit next to their IC on
   the sheet; each IC has a hidden `Function` field and most groups a note saying what it does).
2. Read the datasheet's layout section and example figure (`hw/datasheets/`, gitignored; fetch with
   `uv run --with curl_cffi python -I <script>` using `requests.get(url, impersonate="chrome")`; if a vendor blocks
   it, ask the user to drop the PDF in /tmp). Render the figure page with `pdftoppm -f N -l N -r 130 -png` and look
   at it. Note the pinout and orientation the figure uses.
3. Choose the rotation that reproduces the figure; get real pad positions with Konnect `get_component_pads` after
   placing (KiCad rot 270 maps local (x, y) to (-y, x): a 2-pad part's pad 1 goes up).
4. Place with Konnect `set_component_placements` (one undo step), at a block origin in the staging area.
5. Copper: `add_zone` fills for power nodes (pad_connection solid, priority 2 for power, 1 for GND), `route_trace`
   for signals, `add_via` 0.6/0.3 for GND to the In1 plane. GND via-in-pad at the pad centre when the pad fits it
   (0805 ground pads do; 0402 and SOT-23 pads don't: via beside). A tie between pins under an SOT-23 body (EN to
   VIN) fits at 0.25 mm down the middle, 0.35 mm from both pin rows: no via needed.
6. Check: Konnect `run_drc` with `refill_zones: true, sync_live_board: true, limit: 1` (refills and saves), then
   `hw/tools/blockcheck.sh x0,y0,x1,y1 hw/layout/<ic>.png "<title>"`. Expect only "missing connection" items that
   lead out of the block; anything else in the box is a defect. Look at the PNG.
7. Append the block's section to `hw/layout/README.md` (datasheet, figure, each rule and how the layout meets it),
   commit the board + `hw/layout`, push, `bd close`, `bd dolt push`.

## Tools and rules

- KiCad files are edited ONLY through Konnect (live KiCad over IPC for the PCB; the S-expression engine for the
  schematic). Never sed/Write a `.kicad_*` file. Konnect cannot mirror a placed symbol; swap labels instead.
- Konnect `update_pcb_from_schematic` refuses pad-net changes it thinks are routed; then ask the user to run
  Update PCB from Schematic (F8) in pcbnew.
- `hw/minimeater_models.kicad_pro/.prl` (left by the mech 3D export) make Konnect refuse schematic writes
  ("cannot establish unique project ownership"); move them out of `hw/`, they are gitignored temporaries.
- Renders: `hw/tools/pcbdump.py` (KiCad's python:
  `/Applications/KiCad/KiCad.app/Contents/Frameworks/Python.framework/Versions/Current/bin/python3`) then
  `uv run --with matplotlib python hw/tools/render.py <dump> <png> --bbox|--refs ... [--rats]`.
- Schematic changes: Konnect sch tools, then `kicad-cli sch erc` (must be 0) and look at a crop of
  `kicad-cli sch export svg` rasterised with `rsvg-convert`. Text notes go in clear space.
- Commit and push to master after each block; no attribution lines; `bd dolt push`.

## Staging area

A temporary second board outline (Edge.Cuts rectangle x 0-130, y 64-124) and an In1 GND zone
`STAGING_In1_GND` over it: KiCad only fills zones inside an outline, and block vias need a plane. Both are
removed once every block is on the board. Block origins so far (y = 75 row): U4 (20, 75), U5 (40, 75), U7 (60, 75),
U6 (75, 75). Next free: x >= 88 on that row, then a second row at y ~ 95.

## Netclasses (hw/minimeater.kicad_pro)

Default 0.2/0.15; PowerHigh 0.8/0.2 (VBUS_RAW, VBUS_F, VFAN, VSERVO, BUCK_SW); Power 0.5/0.2 (3V3, 3V3A, 3V3A_FILT,
VBUS_LDO); GND 0.4/0.15; USB 0.27 track / 0.15 gap (90 ohm edge-coupled on F.Cu over In1, JLC 7628 prepreg
~0.21 mm); Analog 0.2/0.25 (TIPx, AINx, VREF_PROBE). Vias 0.6/0.3. Stack: F.Cu, In1 = GND plane, In2 signal, B.Cu.

## Decisions made with the user

- Blocks are laid out per datasheet, local fills and traces only; GND vias allowed (via-in-pad at the pad centre
  when the pad is big enough).
- U1 (ESP32-C6 module) may be unlocked and moved: only its antenna keep-out constrains it. J1-J7, D2, U10 keep their
  places (layout constraints).
- U2/U3 USBLC6: J1 side on pins 6/4, MCU side on 1/3 (same I/O inside), so the pair runs J1 -> U2 -> MCU with one
  crossing; C38 100 nF C_BUS added on VBUS_RAW at their VBUS pins (ST fig. 17).
- J1's pads interleave D+ D- D+ D- at 0.5 mm: the D+/D- crossing is two vias with a ~1 mm hop on In2, just
  below the pad row (not a trace under the connector).
- U9 TPD4E05U06: NC pins tied to the tips (1/10, 2/9, 4/7, 5/6) for the datasheet's straight-through routing.
- The router scripts are retired; layout is by hand from renders.
- Hardware rules from memory still hold: JLCPCB assembly, every part in JLC stock, all SMT on F.Cu, prefer the
  better part.
