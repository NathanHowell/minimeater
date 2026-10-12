"""Renders of the assembly for docs/mech.md (`uv run mech-render`)."""

import sys
from pathlib import Path

from build123d import Pos

from . import assembly as A
from . import shell as K
from .printing import ALL, print_pose
from .parts import box
from .render import render

COLOURS = {"duct": "#9aa6b2", "flap": "#d9822b", "crank": "#d9822b", "coupler": "#e0a050", "fan": "#333333",
           "servo": "#2c4a8a", "horn": "#eeeeee", "pin_crank": "#dddddd", "pin_horn": "#dddddd", "rod": "#cccccc",
           "tube": "#c9c9c9", "oring": "#111111", "pcb": "#1f7a3a", "pcb_keepout": "#7fbf8f", "plugs": "#1a1a1a",
           "hood": "#2f6b45", "pan": "#444444"}
NAMES = ["exterior.png", "underside.png", "rear.png", "section_axis.png", "section_across.png", "chassis.png", "plate.png"]


def main():
    out = Path(sys.argv[1] if len(sys.argv) > 1 else "build")
    out.mkdir(parents=True, exist_ok=True)
    hood, pan = K.hood(), K.pan()
    parts = {**A.printed(0.0), **A.bought(0.0)}
    keepout = parts.pop("pcb_keepout")
    ext = [(hood, COLOURS["hood"]), (pan, COLOURS["pan"]), (parts["duct"], COLOURS["duct"]), (parts["tube"], COLOURS["tube"]),
           (parts["plugs"], COLOURS["plugs"])]
    render(ext, out / "exterior.png", view=(24, -48), title="minimeat on the egg's intake tube (egg to the right)")
    render(ext, out / "underside.png", view=(-30, -125), title="from below: the port bay in the -y flank, the four screws", light=(-0.3, -0.6, -0.7))
    render(ext, out / "rear.png", view=(14, 152), title="rear: the louvred inlet, the two-tone seam", light=(-0.7, 0.4, 0.6))
    # section through the tube axis, looking from -y: everything with y > 0 removed
    keep = box(-200, 50, 0, 80, -80, 120)
    sec = [(s & keep, COLOURS[n]) for n, s in parts.items()] + [(hood & keep, COLOURS["hood"]), (pan & keep, COLOURS["pan"])]
    render(sec, out / "section_axis.png", view=(0, -90), size=(12, 7), title="section y = 0: fan, cone, flap, socket; the board under the duct")
    # section across at the flap station, looking from -x (the fan side): everything behind x = -45 removed
    keep = box(-45, 50, -80, 80, -80, 120)
    sec = [(s & keep, COLOURS[n]) for n, s in parts.items()] + [(hood & keep, COLOURS["hood"]), (pan & keep, COLOURS["pan"]),
                                                                (keepout & keep, COLOURS["pcb_keepout"], 0.5)]
    render(sec, out / "section_across.png", view=(0, 180), size=(9, 8), title="section x = -45, from the fan: duct, board below, servo and linkage on +y")
    chassis = [(s, COLOURS[n]) for n, s in parts.items() if n not in ("pcb", "plugs", "tube", "oring")]
    render(chassis, out / "chassis.png", view=(28, -55), title="the chassis: duct, fan, servo on its shelf, parallelogram linkage")
    plate, x = [], 0.0
    for n in ALL:
        s = print_pose(n, {"hood": hood, "pan": pan}.get(n))
        b = s.bounding_box()
        plate.append((Pos(x - b.min.X, -(b.min.Y + b.max.Y) / 2, 0) * s, COLOURS[n]))
        x += b.size.X + 16.0
    render(plate, out / "plate.png", view=(32, -62), size=(14, 6), tight=True, title="print orientation: duct, flap, crank, coupler, hood, pan")
    print("wrote", ", ".join(sorted(p.name for p in out.glob("*.png"))))


if __name__ == "__main__":
    main()
