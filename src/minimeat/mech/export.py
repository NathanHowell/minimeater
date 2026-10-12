"""Writes STEP and STL for every printed part, in its print pose, plus the assembly (`uv run mech-export [dir]`)."""

import json
import sys
import zipfile
from pathlib import Path

from build123d import Compound, export_step, export_stl

from . import assembly as A
from . import board3d, drawings, mock
from . import shell as K
from .envelope import envelope
from .parts import MACHINED
from .printing import ALL, print_pose


BUILDS = 5  # the first run: five units

PRINT_NOTES = {
    "duct": "airway, fan flange, servo shelf, O-ring socket, M4 thumbscrew boss: the hot part (the socket sits on the egg's intake tube); print flange down, as exported",
    "hood": "upper shell; print nose up on the rear face, as exported",
    "pan": "lower shell with the port bay and the board standoffs; print nose up on the rear face, as exported",
}
CNC_NOTES = {
    "flap": "damper blade: a Ø31.2 disc 2.0 thick with a 4.0 x 5.0 rib along its diameter; the rib's D-bore (Ø3 with a 2.5 across-flats flat, +0.03 mm over) takes the rod with retaining compound; edge diameter +-0.05 matters (closed-flap leak); no threads",
    "crank": "Ø8 hub with a D-bore slip fit (0.1 mm) for the rod's flat, an M2 set screw radial on the flat, an arm out to a tapped M2 hole",
    "coupler": "flat bar 6 x 2 mm, two Ø2.1 holes on the 25 mm centres (parallelogram linkage); deburr, no sharp edges",
}


def write_zips(out: Path):
    """The order packages: `minimeat-3dprint.zip` (STEP and STL of each printed part in its print pose) and
    `minimeat-cnc.zip` (STEP of each made part), each with a README of quantities and notes. Always built with the
    parts, so the files you send are the files the model made."""
    def readme(title, rows, extra):
        lines = [title, f"Quantity: {BUILDS} units of each part per file name below (a {BUILDS}-board build).", ""]
        lines += [f"{name}: {note}" for name, note in rows]
        return "\n".join(lines + ["", *extra, ""])

    with zipfile.ZipFile(out / "minimeat-3dprint.zip", "w", zipfile.ZIP_DEFLATED) as z:
        for name in PRINT_NOTES:
            for ext in ("step", "stl"):
                z.write(out / f"{name}.{ext}", f"{name}.{ext}")
        z.writestr("README.txt", readme("minimeat printed parts (print pose; mm)", list(PRINT_NOTES.items()), [
            "Material: see docs/jlc3dp.md. Threaded inserts: one M4 per duct (the thumbscrew boss); all other screws go",
            "straight into the plastic. Walls are 2.4 mm. Ream the duct's bore at the flap station (x = -37.5, Ø31.6 +0.03/0,",
            "about 40 mm of it) after printing: the machined flap's gap to it is 0.15 mm.",
        ]))
    with zipfile.ZipFile(out / "minimeat-cnc.zip", "w", zipfile.ZIP_DEFLATED) as z:
        for name in CNC_NOTES:
            z.write(out / f"{name}_machined.step", f"{name}_machined.step")
        z.write(out / "drawings.pdf", "drawings.pdf")
        z.writestr("README.txt", readme("minimeat machined parts (mm)", list(CNC_NOTES.items()), [
            "drawings.pdf: one page per part with the threads (M2 x 0.4-6H, tap drill Ø1.6), the D-bores and their tolerances.",
            "The STEP files carry a thread only as a plain hole at the tap-drill diameter; the drawing is the callout.",
            "Material: 304 stainless preferred (6061 aluminium if much cheaper). General tolerance +-0.1 mm unless a fit is",
            "named; the crank's D-bore is a slip fit on the rod's flat (+0.1 mm), the flap's +0.03 (retaining compound).",
        ]))


def export(out: Path):
    out.mkdir(parents=True, exist_ok=True)
    written = []
    for name in ALL:
        s = print_pose(name)
        export_step(s, str(out / f"{name}.step"))
        export_stl(s, str(out / f"{name}.stl"), tolerance=0.02, angular_tolerance=0.1)
        written.append(name)
    for name, build in MACHINED.items():  # the made parts that are not printed, for the CNC quote
        export_step(build(), str(out / f"{name}_machined.step"))
        written.append(f"{name}_machined")
    asm = {**A.printed(0.0), **A.bought(0.0), "hood": K.hood(), "pan": K.pan()}
    asm.pop("pcb_keepout")
    # the plugs are fit-check stand-ins for the probe and USB-C plugs, not parts of the product: a file of their own
    export_step(asm.pop("plugs"), str(out / "plugs_mock.step"))
    asm["fan"] = mock.fan_step()
    board = board3d.export_board(out)
    if board is not None:
        asm["pcb"] = board3d.placed(board)
    # one labelled product per part under the assembly, so an exploded view moves each part as a unit; the board is
    # one flat piece (`board3d.frozen`), its few hundred components do not fly apart
    for name, shape in asm.items():
        shape.label = name
    export_step(Compound(label="minimeat", children=list(asm.values())), str(out / "assembly.step"))
    (out / "envelope.json").write_text(json.dumps(envelope(), indent=2))
    drawings.write(out)
    write_zips(out)
    return written


def main():
    out = Path(sys.argv[1] if len(sys.argv) > 1 else "build/mech")
    names = export(out)
    print(f"wrote {', '.join(names)}, assembly.step, envelope.json, minimeat-3dprint.zip and minimeat-cnc.zip to {out}")
