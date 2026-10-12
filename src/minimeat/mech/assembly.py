"""The assembled box with its bought parts, for fit checks and renders."""

from build123d import Pos, Rot

from . import mock
from .geom import g
from .parts import PARTS


def about_shaft(shape, deg):
    """`shape` turned `deg` about the flap shaft axis (0 = open, parallel to the flow; 90 = closed)."""
    return Pos(g.flap_x, 0, 0) * Rot(0, 0, deg) * Pos(-g.flap_x, 0, 0) * shape


def printed(flap_deg=0.0):
    out = {n: f() for n, f in PARTS.items() if n not in ("flap", "link")}
    out["flap"] = about_shaft(PARTS["flap"](), flap_deg)
    out["link"] = about_shaft(PARTS["link"](), flap_deg)
    return out


def bought(flap_deg=0.0):
    pcb, keepout = mock.pcb()
    return {"fan": mock.fan(), "servo": mock.servo(), "horn": mock.horn(), "rod": about_shaft(mock.rod(), flap_deg), "tube": mock.tube(),
            "oring": mock.oring(), "pcb": pcb, "pcb_keepout": keepout}


def overlap(a, b):
    """Volume (mm^3) shared by two shapes."""
    i = a & b
    return 0.0 if i is None else i.volume


def interferences(parts, tol=0.5):
    """Pairs of shapes in `parts` that share more than `tol` mm^3: [(name_a, name_b, volume)]."""
    names = list(parts)
    out = []
    for i, a in enumerate(names):
        for b in names[i + 1 :]:
            v = overlap(parts[a], parts[b])
            if v > tol:
                out.append((a, b, round(v, 1)))
    return out
