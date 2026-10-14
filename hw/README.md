# hw

KiCad project for the controller board. Schematic and layout start at `.3.x`.

- **Assembly:** JLCPCB, about 5 boards. Every PCB part must be in JLC stock (check with `uv run scripts/jlc_stock.py <keyword|C#>`). Where there is a choice, pick the higher-quality part unless it costs much more.
- **Inputs:** ADR 0001 (MCU and pin budget), ADR 0002 (probe front-end) and the ICD (`docs/icd.md`, `.1.2`).

## Board setup

- **Stackup:** 4 layers (F.Cu / In1.Cu GND plane / In2.Cu / B.Cu). The ground plane helps the module's antenna, USB and the buck converter. JLCPCB JLC04161H-7628, 1.6 mm, set in Board Setup: 1 oz outer (0.035), 7628 prepreg 0.2104 mm (er 4.4), 0.5 oz inner (0.0152), core 1.065 mm (er 4.6); ENIG finish (the 0.5 mm WSONs, the TSSOP and the module's pads want flat pads). Order with the same stackup and finish on JLC's form: it reads them from there, not from the file. "Mark on PCB": Remove Mark (the back silk has no order-number placeholder).
- **Rules:** min track and clearance 0.127 mm, via 0.6/0.3 mm, hole-to-hole 0.5 mm (JLCPCB 4-layer).
- **Outline:** 46 × 54 mm, 2 mm corner radius, origin at the top-left corner, +y down (46 because the USB-C and four jacks side by side need 45 mm). Three 2.7 mm mounting holes at (10.6, 28.7), (43.0, 29.8), (43.0, 49.6): the corners are taken by the connector row and the module.
- **Frame mapping:** the enclosure's frame (`uv run mech-envelope`, `docs/mech.md`) is centred, so KiCad (x, y) = envelope (x + 23, y + 27). The connector row on the envelope's -y edge is the top edge of the KiCad board (y = 0), with the USB-C at KiCad x = 5.7.
- **Editing:** only through the Konnect MCP tools; never edit the `.kicad_*` files by hand.
- **Assembly:** single-sided SMT at JLCPCB (cost): every SMT part on the top side. The through-hole fan and servo headers are hand-soldered. The TMP117 therefore sits on top (see the bead on the ambient vent path).
- **Placement:** done by script from the courtyard sizes (first pass; routing is next). Module at the +y end with its antenna toward the +y edge; fan and servo headers on the rear (x = 0) edge; the TMP117 at the rear +y corner; LED at x = 38 on the +y edge; three on-board fiducials; JLC adds the edge rails and tooling holes at order time.
