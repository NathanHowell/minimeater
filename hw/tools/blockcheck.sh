#!/bin/sh
# Check one layout block: hw/tools/blockcheck.sh <x0,y0,x1,y1> <out.png> "<title>"
# Save the live board first (Konnect run_drc with refill_zones + sync_live_board does refill+save).
# Prints DRC items inside the box (minus library-drift/field noise), writes a net-coloured F.Cu render.
set -e
cd "$(dirname "$0")/../.."
PY=/Applications/KiCad/KiCad.app/Contents/Frameworks/Python.framework/Versions/Current/bin/python3
T=${TMPDIR:-/tmp}/blockcheck.$$
mkdir -p "$T"
kicad-cli pcb drc --severity-all --format json -o "$T/drc.json" hw/minimeater.kicad_pcb >/dev/null 2>&1 || true
uv run python hw/tools/drcbox.py "$T/drc.json" "$1" | grep -v -E "does not match copy in library|Missing symbol field|unconnected end" || true
$PY hw/tools/pcbdump.py hw/minimeater.kicad_pcb "$T/b.json" 2>/dev/null
uv run --with matplotlib python hw/tools/render.py "$T/b.json" "$2" --bbox "$1" --layers F.Cu --ppmm 70 --title "$3"
rm -rf "$T"
