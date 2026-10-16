"""Writes the two order folders (printed parts, machined parts), the assembly and the envelope (`uv run mech-export [dir]`)."""

import json
import sys
import zipfile
from pathlib import Path

from build123d import Color, Compound, export_step, export_stl

from . import assembly as A
from . import board3d, drawings, mock
from .glb import write_glb
from . import shell as K
from .envelope import envelope
from .parts import MACHINED
from .printing import ALL, print_pose


BUILDS = 5  # the first run: five units

PRINT_NOTES = {
    "duct": "airway, fan flange, a hollow tail cone on three airfoil struts in the airway (open toward the fan: let the powder out), servo shelf, O-ring socket with a 10 mm extension whose 8-lobe bore grips the tube (a smooth sleeve outside, a slot open at the mouth between them: clear the powder): the hot part (the socket sits on the egg's intake tube); print flange down, as exported",
    "hood": "upper shell; print nose up on the rear face, as exported",
    "pan": "lower shell with the port bay and the board standoffs; print nose up on the rear face, as exported",
}
CNC_NOTES = {
    "flap": "damper blade: a Ø31.7 disc 2.0 thick with a 4.0 x 5.0 rib along its diameter (round-ended, Ø4); the rib's D-bore (Ø3 with a 2.5 across-flats flat, +0.03 mm over) takes the rod with retaining compound; the disc's edge is a full round (R1.0, half the thickness); edge diameter +-0.05 matters (closed-flap leak); no threads",
    "crank": "Ø8 hub with a D-bore slip fit (0.1 mm) for the rod's flat, an M2 set screw radial on the flat, an arm out to a tapped M2 hole, and a 3 mm wide stop lug under the arm (hits the duct's two posts at 0 and 90 deg, for homing)",
    "coupler": "flat bar 6 x 2 mm, two Ø3.1 holes on the 25 mm centres, a 0.9 mm boss under each (parallelogram linkage); deburr, no sharp edges",
    "rod": "the flap's shaft, QUANTITY 6 (not 5): Ø3 h7 (0/-0.010) with a D flat 2.5 across the full length (+-0.05), 42.7 +-0.1 long; if you can turn it from 3 mm bar or ground 3 mm rod rather than mill it from larger stock, please do; the flat is milled",
}


# viewer colours for the assembly files (the board keeps the colours of its own model)
COLORS = {
    "hood": "#9aa0a6", "pan": "#80868b", "duct": "#b8956a", "fan": "#2b2b2b", "servo": "#3a3a3a", "horn": "#d8d8d8",
    "flap": "#c0c4c8", "crank": "#c0c4c8", "coupler": "#c0c4c8", "pin_crank": "#c0c4c8", "pin_horn": "#c0c4c8",
    "rod": "#c0c4c8", "tube": "#b87333", "oring": "#202020",
}


def write_packages(out: Path):
    """The two order folders, each with the files to upload one by one, a README of quantities and notes, and the same
    files as one zip (some JLC pages take a zip, others one file at a time): `3dprint/` (STEP and STL of each printed part
    in its print pose) and `cnc/` (STEP of each made part, and `drawings.pdf`). Only what is ordered goes in them; the
    assembly and the stand-ins stay in `out`. Built with every export, so the files you send are the files the model made."""
    def readme(title, rows, extra):
        lines = [title, f"Quantity: {BUILDS} units of each part per file name below (a {BUILDS}-board build).", ""]
        lines += [f"{name}: {note}" for name, note in rows]
        return "\n".join(lines + ["", *extra, ""])

    def package(folder: Path, zip_name: str, text: str):
        (folder / "README.txt").write_text(text)
        with zipfile.ZipFile(folder / zip_name, "w", zipfile.ZIP_DEFLATED) as z:
            for f in sorted(folder.iterdir()):
                if f.name != zip_name:
                    z.write(f, f.name)

    package(out / "3dprint", "minimeater-3dprint.zip", readme("minimeater printed parts (print pose; mm)", list(PRINT_NOTES.items()), [
        "Material: MJF PA12S-HP (black nylon), all three parts; see docs/jlc3dp.md. No threaded inserts: all screws go",
        "straight into the plastic. Shell walls are 2.8 mm, the duct's 2.2-2.4. Ream the duct's bore at the flap station (x = -37.5, Ø32 H7 +0.025/0,",
        "about 40 mm of it) after printing, from the fan's end; stop the tip at x = -25, where the bore necks to the tube's Ø30.83: the machined flap's gap to it is 0.15 mm.",
    ]))
    package(out / "cnc", "minimeater-cnc.zip", readme("minimeater machined parts (mm)", list(CNC_NOTES.items()), [
        "<part>.pdf beside <part>.step: the drawing with the threads (M2 x 0.4-6H, tap drill Ø1.6), the D-bores and their tolerances.",
        "The STEP files carry a thread only as a plain hole at the tap-drill diameter; the drawing is the callout.",
        "Material: 304 stainless preferred (6061 aluminium if much cheaper). General tolerance +-0.1 mm unless a fit is",
        "named; the crank's D-bore is a slip fit on the rod's flat (+0.1 mm), the flap's +0.03 (retaining compound).",
        "The rod is the exception: its Ø3 is h7 and its flat +-0.05, not the general tolerance (the bores above are cut to it).",
    ]))


def export(out: Path):
    out.mkdir(parents=True, exist_ok=True)
    prints, cnc = out / "3dprint", out / "cnc"
    for d in (prints, cnc):
        d.mkdir(exist_ok=True)
    for old in out.glob("*.stl"):  # the layout before the order folders
        old.unlink()
    for name in (*ALL, *(f"{n}_machined" for n in MACHINED)):
        (out / f"{name}.step").unlink(missing_ok=True)
    for old in ("drawings.pdf", "minimeater-3dprint.zip", "minimeater-cnc.zip"):  # (and the layout before the order folders)
        (out / old).unlink(missing_ok=True)
    written = []
    for name in ALL:
        s = print_pose(name)
        export_step(s, str(prints / f"{name}.step"))
        export_stl(s, str(prints / f"{name}.stl"), tolerance=0.02, angular_tolerance=0.1)
        written.append(name)
    for name, build in MACHINED.items():  # the made parts that are not printed, for the CNC quote
        export_step(build(), str(cnc / f"{name}.step"))
        written.append(name)
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
        if name in COLORS:
            shape.color = Color(COLORS[name])
    product = Compound(label="minimeater", children=list(asm.values()))
    export_step(product, str(out / "assembly.step"))
    # the same assembly for Blender and other viewers (File > Import > glTF): binary glTF, metres, one node per part
    # (the board one per colour)
    glb = []
    for name, shape in asm.items():
        if name == "pcb":
            for part, solids, colour in zip(("pcb_substrate", "pcb_module", "pcb_usb", "pcb_jacks", "pcb_parts"), board3d.pieces(shape),
                                            ("#1d6b3a", "#3b3b3b", "#c8c8c8", "#202020", "#2a2a2a")):
                if solids:
                    glb.append((part, Compound(obj=solids), colour))
        else:
            glb.append((name, shape, COLORS.get(name, "#a0a0a0")))
    write_glb(out / "assembly.glb", glb)
    (out / "envelope.json").write_text(json.dumps(envelope(), indent=2))
    drawings.write(cnc)
    write_packages(out)
    return written


def main():
    out = Path(sys.argv[1] if len(sys.argv) > 1 else "build/mech")
    names = export(out)
    print(f"wrote {', '.join(names)}, assembly.step, assembly.glb and envelope.json to {out}; the order folders {out}/3dprint and {out}/cnc, each with a zip")
