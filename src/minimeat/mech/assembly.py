"""The assembled chassis with its bought parts, for fit checks and renders."""

import math

from build123d import Pos, Rot

from . import mock
from .geom import g
from . import geom as G
from .params import P
from .parts import PARTS


def about_shaft(shape, deg):
    """`shape` turned `deg` about the flap shaft axis (0 = open, parallel to the flow; 90 = closed)."""
    return Pos(g.flap_x, 0, 0) * Rot(0, 0, deg) * Pos(-g.flap_x, 0, 0) * shape


def about_servo(shape, deg):
    """`shape` turned `deg` about the servo's output axis: the horn follows the crank one to one."""
    return Pos(G.SERVO_X, G.SERVO_Y, 0) * Rot(0, 0, deg) * Pos(-G.SERVO_X, -G.SERVO_Y, 0) * shape


def coupler_shift(deg):
    """The coupler translates on the parallelogram: its pins ride the arms."""
    r, a = P.linkage.arm_r, math.radians(deg)
    return Pos(r * (1 - math.cos(a)), -r * math.sin(a), 0)


def printed(flap_deg=0.0):
    out = {"duct": PARTS["duct"]()}
    out["flap"] = about_shaft(PARTS["flap"](), flap_deg)
    out["crank"] = about_shaft(PARTS["crank"](), flap_deg)
    out["coupler"] = coupler_shift(flap_deg) * PARTS["coupler"]()
    return out


def bought(flap_deg=0.0):
    pcb, keepout = mock.pcb()
    pin_c, pin_h = mock.pins()
    return {"fan": mock.fan(), "servo": mock.servo(), "horn": about_servo(mock.horn(), flap_deg),
            "pin_crank": about_shaft(pin_c, flap_deg), "pin_horn": about_servo(pin_h, flap_deg),
            "rod": about_shaft(mock.rod(), flap_deg), "tube": mock.tube(), "oring": mock.oring(),
            "pcb": pcb, "pcb_keepout": keepout, "plugs": mock.plugs()}


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
