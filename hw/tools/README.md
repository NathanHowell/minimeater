# PCB flow scripts

How the board was placed and routed (first pass). Run from a scratch directory; paths inside are under `/tmp/minimeat-place/`.

1. `parts.py <out.json>`: reads `hw/minimeat.kicad_sch` and the footprint libraries, writes each reference's footprint and courtyard box.
2. `layout.py`: the hand-chosen zones, packed and overlap-checked against the courtyards; writes `pos.json` and a picture. The positions go into KiCad with the Konnect `set_component_placements` tool.
3. Routing is done outside the live editor on a copy of the saved board, with KiCad's bundled Python (`pcbnew`) and Freerouting 2.4.1 (the jar from the KiCad plugin): `ExportSpecctraDSN`, `java -jar freerouting.jar -de x.dsn -do x.ses -mp 60`, `ImportSpecctraSES`.
4. `gndvias.py <in> <out>` adds a via and stub for ground pads the router left off the plane; `finish.py <in> <out>` adds the top-side ground pour, full connection for small ground pads, island removal and silkscreen text tidy.

The routed board replaces `hw/minimeat.kicad_pcb`; reload it in KiCad. Never edit the `.kicad_*` files by hand.
