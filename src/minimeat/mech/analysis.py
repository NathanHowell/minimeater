"""Numbers the design is held to: closed-flap leakage, open-flap blockage, the O-ring and the envelope (mm, cm^2)."""

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


def flap_leak_cm2():
    """Closed-flap leak area: the ring between the disc edge and the bore, plus the shaft bore around the rod."""
    dm = P.damper
    r_edge = math.sqrt((g.bore_r - dm.flap_gap) ** 2 - ((dm.rib_t - dm.flap_t) / 2) ** 2)
    ring = math.pi * (g.bore_r**2 - r_edge**2)
    shaft = math.pi * 1.7**2 - _d_area()  # the Ø3.4 bore through the top wall; the bottom one is blind
    return (ring + shaft) / 100.0


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
