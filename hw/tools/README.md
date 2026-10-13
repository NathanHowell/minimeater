# PCB scripts

The board is laid out by hand through Konnect (live KiCad); see `hw/layout/README.md`. The first-pass autorouter and
its one-off rework scripts were retired (in git history before the re-layout).

- `pcbdump.py <board> <out.json>` (KiCad's python): pads, tracks, vias, zone fills, courtyards to JSON.
- `render.py <dump.json> <out.png> [--refs ... | --bbox ...] [--layers ...] [--rats]` (uv, matplotlib): copper
  coloured and labelled by net, for layout reviews.
- `blockcheck.sh <x0,y0,x1,y1> <out.png> "<title>"`: DRC items inside a block's box (`drcbox.py`) plus its F.Cu render.
- `parts.py <out.json>`: each reference's footprint and courtyard box from the schematic and libraries.
- `kdrc.py`: kicad-cli DRC from KiCad's python, returns the violations.
- `fab.py` (`uv run python hw/tools/fab.py`): gerber zip, JLC BOM and CPL into `hw/fab/`.
- `models.py`: 3D models for the mech assembly (`src/minimeater/mech/board3d.py`).
- `kk_ra_step.py`, `kk_ra.py`: the right-angle KK header STEP model and footprint.
- `connector_edge.py`: trims the connector edge so the jacks reach into the port wall (applied).

DRC: `hw/minimeater.kicad_dru` relaxes `hole_clearance` to 0.19 mm for J1 only (the GCT USB4105-GF-A land pattern
puts its GND pads 0.1944 mm from the locating pegs).
