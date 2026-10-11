# hw

KiCad project for the controller board. Schematic and layout start at `.3.x`.

- **Assembly:** JLCPCB, about 5 boards. Every PCB part must be in JLC stock (check with `uv run scripts/jlc_stock.py <keyword|C#>`). Where there is a choice, pick the higher-quality part unless it costs much more.
- **Inputs:** ADR 0001 (MCU and pin budget), ADR 0002 (probe front-end) and the ICD (`docs/icd.md`, `.1.2`).
