"""Renders of the assembly for docs/mech.md (`uv run mech-render`)."""

import sys
from pathlib import Path

from build123d import Pos

from . import assembly as A
from . import skin as K
from .printing import print_pose
from .parts import PARTS, box
from .render import render

COLOURS = {"duct": "#9aa6b2", "flap": "#d9822b", "link": "#d9822b", "grille": "#7d8a96", "tray": "#5b7fa6",
           "lid": "#6f93ba", "fan": "#333333", "servo": "#2c4a8a", "horn": "#eeeeee", "rod": "#cccccc", "tube": "#c9c9c9",
           "oring": "#111111", "pcb": "#1f7a3a", "pcb_keepout": "#7fbf8f", "hood": "#2f6b45", "pan": "#262626"}


def main():
    out = Path(sys.argv[1] if len(sys.argv) > 1 else "build")
    out.mkdir(parents=True, exist_ok=True)
    printed, bought = A.printed(0.0), A.bought(0.0)
    parts = {**printed, **bought}
    parts.pop("pcb_keepout")
    iso = [(s, COLOURS[n]) for n, s in parts.items()]
    render(iso, out / "iso.png", view=(22, -52), title="minimeat box, egg to the right")
    render(iso, out / "iso_back.png", view=(25, 130), title="rear: fan inlet, probe jacks and USB-C")
    # section through y = 0, looking from +y
    sec = [(s - box(-200, 50, -80, 0, -80, 120), COLOURS[n]) for n, s in parts.items() if n not in ("fan",)]
    sec.append((parts["fan"] - box(-200, 50, -80, 0, -80, 120), COLOURS["fan"]))
    render(sec, out / "section.png", view=(0, -90), size=(11, 6), title="section y = 0, egg to the right")
    plate, x = [], 0.0
    for n in PARTS:
        s = print_pose(n)
        b = s.bounding_box()
        plate.append((Pos(x - b.min.X, -(b.min.Y + b.max.Y) / 2, 0) * s, COLOURS[n]))
        x += b.size.X + 8.0
    render(plate, out / "plate.png", view=(50, -80), size=(13, 5), tight=True, title="print orientation: duct, flap, link, grille, tray, lid")
    skin_views(out)
    print("wrote", ", ".join(sorted(p.name for p in out.glob("*.png"))))


def skin_views(out):
    """The exterior shells over the internals (docs/mech.md, "Exterior"). The renderer has no depth test, so the
    exterior views show only the exposed socket, and the section is the y >= 0 half seen from -y."""
    hood, pan = K.hood(), K.pan()
    parts = K.internals(0.0)
    parts.pop("pcb_keepout")
    nose = parts["duct"] & box(K.X_CAV_FRONT, 10, -40, 40, -40, 70)
    ext = [(hood, COLOURS["hood"]), (pan, COLOURS["pan"]), (nose, COLOURS["duct"]), (parts["tube"], COLOURS["tube"])]
    render(ext, out / "skin_iso.png", view=(22, -52), title="hood and pan, egg to the right")
    render(ext, out / "skin_rear.png", view=(14, 142), title="rear plate: grille, probe jacks, USB-C, label")
    render(ext, out / "skin_side.png", view=(0, -90), title="side: the parting line is the tube axis")
    keep = box(-200, 50, 0, 80, -80, 120)
    sec = [(hood & keep, COLOURS["hood"]), (pan & keep, COLOURS["pan"])] + [(s & keep, COLOURS[n]) for n, s in parts.items()]
    render(sec, out / "skin_section.png", view=(0, -90), size=(11, 7), title="section y = 0, egg to the right")
    plate, x = [], 0.0
    for n in K.SKIN:
        s = K.print_pose(n)
        b = s.bounding_box()
        plate.append((Pos(x - b.min.X, -(b.min.Y + b.max.Y) / 2, 0) * s, COLOURS[n]))
        x += b.size.X + 12.0
    render(plate, out / "skin_plate.png", view=(35, -60), size=(10, 6), tight=True, title="print orientation: hood top down, pan on its rear plate")


if __name__ == "__main__":
    main()
