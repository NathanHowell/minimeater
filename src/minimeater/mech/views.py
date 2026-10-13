"""Renders of the assembly for docs/mech.md (`uv run mech-render`)."""

import sys
from pathlib import Path

from build123d import Pos

from . import assembly as A
from . import board3d
from . import shell as K
from .printing import ALL, print_pose
from .parts import box
from . import geom as G
from .params import P
from .render import render

COLOURS = {"duct": "#9aa6b2", "flap": "#d9822b", "crank": "#b8b8c0", "coupler": "#bcbcc4", "fan": "#333333",
           "servo": "#2c4a8a", "horn": "#eeeeee", "pin_crank": "#dddddd", "pin_horn": "#dddddd", "rod": "#cccccc",
           "tube": "#c9c9c9", "oring": "#111111", "pcb": "#1f7a3a", "pcb_keepout": "#7fbf8f", "connectors": "#2b2b2b", "plugs": "#1a1a1a",
           "hood": "#2f6b45", "pan": "#444444"}
BOARD = {"substrate": "#1f7a3a", "module": "#a9adb3", "usb": "#cfd2d6", "jacks": "#262626", "parts": "#4a4a4a"}
NAMES = ["exterior.png", "underside.png", "rear.png", "section_axis.png", "section_across.png", "chassis.png", "plate.png"]


def real_board(out: Path):
    """The board as KiCad exports it, placed in the assembly, split into colour groups; None without KiCad."""
    step = board3d.export_board(out)
    if step is None:
        return None
    return board3d.pieces(board3d.placed(step))


def board_items(pieces, keep=None):
    """Render items for the real board's pieces (cut to `keep` when given)."""
    items = []
    for group, colour in zip(pieces, BOARD.values()):
        for so in group:
            if keep is not None:
                b = so.bounding_box()
                kb = keep.bounding_box()
                if (b.max.X < kb.min.X or b.min.X > kb.max.X or b.max.Y < kb.min.Y or b.min.Y > kb.max.Y
                        or b.max.Z < kb.min.Z or b.min.Z > kb.max.Z):
                    continue
                so = so & keep
                if so is None or so.volume < 1e-6:
                    continue
            items.append((so, colour))
    return items


def main():
    out = Path(sys.argv[1] if len(sys.argv) > 1 else "build")
    out.mkdir(parents=True, exist_ok=True)
    hood, pan = K.hood(), K.pan()
    parts = {**A.printed(0.0), **A.bought(0.0)}
    keepout = parts.pop("pcb_keepout")
    parts.pop("plugs")  # fit-check stand-ins for the mating plugs (tests/test_skin.py), not part of the product
    ext = [(hood, COLOURS["hood"]), (pan, COLOURS["pan"]), (parts["duct"], COLOURS["duct"]), (parts["tube"], COLOURS["tube"])]
    render(ext, out / "exterior.png", view=(24, -48), title="minimeater on the egg's intake tube (egg to the right)")
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
    pieces = real_board(Path("build/mech"))
    if pieces is not None:
        # the enclosure open (the hood off), the real board in the pan under the duct, the connectors in the port wall
        # the chassis translucent so the board shows under the duct
        inner = [(s, COLOURS[n], 0.22) for n, s in parts.items() if n not in ("pcb", "connectors", "tube", "oring")]
        render([(pan, "#8d8d8d")] + board_items(pieces) + inner, out / "assembled.png", view=(52, -62), size=(11, 8), max_edge=12.0,
               title="assembled, hood off, chassis translucent: the real board in the pan under the duct")
        # a horizontal section through the connector axes, from above: the housings standing in the port wall
        zj = G.PCB_Z0 - P.pcb.jack_h / 2
        keep = box(-90, 20, -45, -10, G.PCB_Z0 - 8, zj)
        render([(pan & keep, "#9a9a9a")] + board_items(pieces, keep) + [(A.mock.plugs() & keep, "#4b6fb5")],
               out / "ports.png", view=(90, -90), size=(12, 5), tight=True, max_edge=12.0,
               title="section through the connector axes (the port wall grey, plugs blue): the jacks' noses and the USB-C shell stand in the wall")
    plate, x = [], 0.0
    for n in ALL:
        s = print_pose(n, {"hood": hood, "pan": pan}.get(n))
        b = s.bounding_box()
        plate.append((Pos(x - b.min.X, -(b.min.Y + b.max.Y) / 2, 0) * s, COLOURS[n]))
        x += b.size.X + 16.0
    render(plate, out / "plate.png", view=(32, -62), size=(14, 6), tight=True, title="print orientation: duct, hood, pan")
    print("wrote", ", ".join(sorted(p.name for p in out.glob("*.png"))))


if __name__ == "__main__":
    main()
