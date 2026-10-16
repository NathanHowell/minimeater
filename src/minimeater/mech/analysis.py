"""Numbers the design is held to: closed-flap leakage, open-flap blockage, the O-ring, the envelope and the mass (mm, cm^2, g)."""

import math

from .geom import g
from .params import P
from .parts import PARTS, flap


def _d_area():
    """Area of the rod's D section: a round with a flat `shaft_flat` across."""
    r, d = P.damper.shaft_d / 2, P.damper.shaft_d
    h = d - P.damper.shaft_flat  # height of the segment the flat removes
    seg = r * r * math.acos((r - h) / r) - (r - h) * math.sqrt(2 * r * h - h * h)
    return math.pi * r * r - seg


def flap_leak_cm2(extra_gap=0.0):
    """Closed-flap leak area: the ring between the disc edge and the bore, plus the shaft bore around the rod.
    `extra_gap` widens the radial gap beyond the nominal (tolerances)."""
    dm = P.damper
    r_edge = math.sqrt((g.bore_r - dm.flap_gap - extra_gap) ** 2 - dm.disc_off ** 2)
    ring = math.pi * (g.bore_r**2 - r_edge**2)
    shaft = math.pi * 1.7**2 - _d_area()  # the Ø3.4 bore through the top wall; the bottom one is blind
    return (ring + shaft) / 100.0


def flap_gap_range(bore_tol):
    """(smallest, largest) radial gap between the machined disc and the bore, mm, for a bore good to +-`bore_tol` radially."""
    dm = P.damper
    return dm.flap_gap - bore_tol - dm.flap_tol, dm.flap_gap + bore_tol + dm.flap_tol


def flap_leak_worst_cm2(bore_tol):
    """Closed-flap leak at the widest gap the tolerances allow."""
    return flap_leak_cm2(flap_gap_range(bore_tol)[1] - P.damper.flap_gap)


def flap_blockage_open():
    """Fraction of the bore the open flap covers, seen along the flow: the disc edge-on plus the thicker rib."""
    dm = P.damper
    bb = flap().bounding_box()
    h = bb.size.Z
    return (dm.flap_t * h + (dm.rib_t - dm.flap_t) * h * 0.97) / (math.pi * g.bore_r**2)


def oring_squeeze():
    """(squeeze, stretch) of the joint's O-ring as fractions: the cross-section pinched between the tube and the
    groove bottom, and the ring's ID stretched over the tube. Static radial seals want 10-30 % and under 5 %."""
    from .parts import groove_r

    c = P.coupling
    gap = groove_r() - P.tube.od / 2
    return (c.oring_cs - gap) / c.oring_cs, (P.tube.od - c.oring_id) / c.oring_id


def cone_half_angle():
    """Degrees: the contraction from the fan's aperture to the bore."""
    return math.degrees(math.atan((P.fan.aperture_d / 2 - g.bore_r) / P.cone_len))


def sizes():
    """Overall bounding box of each chassis part in its assembly pose: {name: (dx, dy, dz)}."""
    out = {}
    for n, build in PARTS.items():
        b = build().bounding_box()
        out[n] = (round(b.size.X, 1), round(b.size.Y, 1), round(b.size.Z, 1))
    return out


def mass_budget():
    """{part: (grams, x of its centre)} for the box as installed, from the CAD's volumes (printed and machined) and the
    bought parts' data; the cables and plugs hanging off the ports are not in it."""
    from . import assembly as A
    from . import shell as K

    m = P.mass
    vol = lambda s: s.volume / 1000.0  # cm^3
    out = {}
    for name, s in {**A.printed(), "hood": K.hood(), "pan": K.pan()}.items():
        out[name] = (vol(s) * m.pa12, s.center().X)
    b = A.bought()
    for name in ("flap", "crank", "coupler", "rod", "pin_crank", "pin_horn"):
        out[name] = (vol(b[name]) * m.machined, b[name].center().X)
    out["fan"] = (m.fan_g, b["fan"].center().X)
    out["servo"] = (m.servo_g + m.horn_g, b["servo"].center().X)
    out["board"] = (vol(b["pcb"]) * m.fr4 + m.board_parts_g, b["pcb"].center().X)
    out["hardware"] = (m.hardware_g, m.hardware_x)
    return out


def mass_and_cg():
    """(grams, x of the centre of mass): x from the socket's mouth, the box toward -x."""
    parts = mass_budget()
    total = sum(g_ for g_, _ in parts.values())
    return total, sum(g_ * x for g_, x in parts.values()) / total


def tube_loads(engaged=None):
    """(N up at the tube's end, N down at the far end of the engagement, N.m at the egg): the box as a lever on the
    tube, its weight at the centre of mass, held at the tube's end (the shoulder) and `engaged` mm further on."""
    total, cg = mass_and_cg()
    w = total / 1000 * 9.81
    x_end = -P.coupling.depth
    span = engaged if engaged is not None else P.coupling.depth + P.coupling.ext_len
    up = w * (x_end + span - cg) / span
    return up, up - w, w * (x_end + P.tube.free_len - cg) / 1000
