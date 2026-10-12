# hw

KiCad project for the controller board. Schematic and layout start at `.3.x`.

- **Assembly:** JLCPCB, about 5 boards. Every PCB part must be in JLC stock (check with `uv run scripts/jlc_stock.py <keyword|C#>`). Where there is a choice, pick the higher-quality part unless it costs much more.
- **Inputs:** ADR 0001 (MCU and pin budget), ADR 0002 (probe front-end) and the ICD (`docs/icd.md`, `.1.2`).

## Board setup

- **Stackup:** 4 layers (F.Cu / In1.Cu GND plane / In2.Cu / B.Cu). The ground plane helps the module's antenna, USB and the buck converter.
- **Rules:** min track and clearance 0.127 mm, via 0.6/0.3 mm, hole-to-hole 0.5 mm (JLCPCB 4-layer).
- **Outline:** 44 × 54 mm, 2 mm corner radius, origin at the top-left corner, +y down. Four 2.7 mm holes at (3.5, 3.5), (40.5, 3.5), (3.5, 50.5), (40.5, 50.5).
- **Frame mapping:** the enclosure's frame (`uv run mech-envelope`, `docs/mech.md`) is centred, so KiCad (x, y) = envelope (x + 22, y + 27). The connector row on the envelope's -y edge is the top edge of the KiCad board (y = 0), with the USB-C at KiCad x = 4.9.
- **Editing:** only through the Konnect MCP tools; never edit the `.kicad_*` files by hand.
- **Assembly:** single-sided SMT at JLCPCB (cost): every SMT part on the top side. The through-hole fan and servo headers are hand-soldered. The TMP117 therefore sits on top (see the bead on the ambient vent path).
