# The board (hw/, KiCad 10): fab outputs, checks and the layout tools. Runs at the repository root.
# The scripts marked "KiCad's python" use pcbnew and run on KiCad's bundled python (KICAD_PY); close the board in KiCad
# first, or they and KiCad will overwrite each other.
set working-directory := ".."

kicad_py := env("KICAD_PY", "/Applications/KiCad/KiCad.app/Contents/Frameworks/Python.framework/Versions/Current/bin/python3")
kicad_bin := "/Applications/KiCad/KiCad.app/Contents/MacOS"
board := "hw/minimeater.kicad_pcb"

# Gerbers and drill (zip), BOM and placement for JLCPCB, for the board and the assembly panel (hw/fab).
fab:
    PATH="{{ kicad_bin }}:$PATH" uv run python hw/tools/fab.py

# ERC and DRC (errors only), as CI runs them. kicad-cli from the PATH, else the kicad container under podman (MINIMEATER_KICAD_IMAGE).
check:
    #!/usr/bin/env bash
    set -euo pipefail
    PATH="{{ kicad_bin }}:$PATH"
    base=hw/minimeater
    mkdir -p build/hw
    if command -v kicad-cli >/dev/null; then
        kc() { kicad-cli "$@"; }
    else
        image=${MINIMEATER_KICAD_IMAGE:-docker.io/kicad/kicad:10.0}
        # the container has KiCad's stock libraries but no global library tables: seed them from its templates
        kc() {
            podman run --rm --userns=keep-id -v "$PWD":/work:Z -w /work -e HOME=/tmp "$image" sh -c \
                'd=/tmp/.config/kicad/10.0; mkdir -p $d; cp /usr/share/kicad/template/sym-lib-table /usr/share/kicad/template/fp-lib-table $d/; exec kicad-cli "$@"' kicad-cli "$@"
        }
    fi
    kc sch erc --exit-code-violations --output build/hw/erc.rpt "$base.kicad_sch"
    # errors only: the stock footprints differ between KiCad versions (lib_footprint_mismatch warnings), and the silk warnings are known
    kc pcb drc --severity-error --exit-code-violations --output build/hw/drc.rpt "$base.kicad_pcb"

# JLCPCB stock and price for parts by name or C-number, e.g. `just hw::stock ADS1220IPWR C2089026`.
stock +parts:
    uv run scripts/jlc_stock.py {{ parts }}

# Board geometry to JSON for the renders (KiCad's python): `just hw::pcbdump build/hw/dump.json`.
pcbdump out pcb=board:
    {{ kicad_py }} hw/tools/pcbdump.py {{ pcb }} {{ out }}

# Copper coloured by net, for layout reviews: `just hw::render build/hw/dump.json build/hw/board.png [--rats ...]`.
render dump out *args:
    uv run python hw/tools/render.py {{ dump }} {{ out }} {{ args }}

# Every layout block and the board at one scale: `just hw::montage build/hw/dump.json build/hw/montage.png [--ppmm 20]`.
montage dump out *args:
    uv run --with matplotlib --with pillow python hw/tools/montage.py {{ dump }} {{ out }} {{ args }}

# Each reference's footprint and courtyard box from the schematic and the libraries: `just hw::parts build/hw/parts.json`.
parts out:
    uv run python hw/tools/parts.py {{ out }}

# DRC violations inside a box (mm) from a JSON report: `just hw::drcbox build/hw/drc.json x0,y0,x1,y1`.
drcbox report box:
    uv run python hw/tools/drcbox.py {{ report }} {{ box }}

# Silk pass (KiCad's python): references at JLC's minimum size where they fit, test point and header labels.
silk pcb=board:
    {{ kicad_py }} hw/tools/silk.py {{ pcb }}

# Back-silk artwork (KiCad's python), after `silk`: probe numbers, logo, rev, the QR code, header pin names. `--dry` checks only.
silk-back *args:
    {{ kicad_py }} hw/tools/silk_back.py {{ board }} {{ args }}

# Edge GND stitching candidates (KiCad's python): `just hw::stitch build/hw/stitch.json [scratch.kicad_pcb]`.
stitch out *scratch:
    {{ kicad_py }} hw/tools/stitch.py {{ board }} {{ out }} {{ scratch }}

# The right-angle KK 254 header footprints (KiCad's python): hw/lib/minimeater.pretty/KK254-RA-0{3,4}.kicad_mod.
kk-ra:
    {{ kicad_py }} hw/tools/kk_ra.py

# A Molex KK 254 STEP into the footprint's frame: `just hw::kk-ra-step molex.stp hw/lib/3d/KK254-RA-04.step <pin1 x>`.
kk-ra-step src dst pin1_x:
    uv run python hw/tools/kk_ra_step.py {{ src }} {{ dst }} {{ pin1_x }}

# Copy the library footprints' 3D model references onto the placed ones (KiCad's python): `just hw::models in.kicad_pcb out.kicad_pcb`.
models src dst:
    {{ kicad_py }} hw/tools/models.py {{ src }} {{ dst }}

# The silk artwork, hw/art/logo.svg and qr.svg, from a font (Lilita One): `just hw::art LilitaOne-Regular.ttf`.
art font:
    uv run --with segno --with matplotlib python hw/tools/art.py {{ font }}
