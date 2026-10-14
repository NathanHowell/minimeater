"""The flap's edge and rib variants the LES compares (`.2.18`, `.2.19`): the design's damper parameters with a few changed.

Each is a `Damper` (params.py) handed to `parts.flap`, so the print and the CNC drawing never see them. Whether a variant
is worth a machined runner-up is `.2.19`'s call; a serrated edge leaks when the flap is shut (every notch is an opening
in the 0.15 mm gap), which `leak_cm2` puts a number on.
"""

import math
from dataclasses import replace

from ..geom import g
from ..params import P

VARIANTS = {
    "round": {},  # the design: full-round disc edge, stadium rib
    "square": {"edge_round": False},  # a sharp-cornered disc edge: the fixed separation point
    "serrated": {"serr_n": 24, "serr_h": 1.0},  # V notches round the edge: breaks the shedding's spanwise coherence
    "rib-square": {"rib_round": 0.0},  # a blunt rib (what the print would have been)
    "rib-long": {"rib_w": 8.0},  # a longer chord on the rib: a streamlined body for the wake to close on
}


def damper(variant: str):
    if variant not in VARIANTS:
        raise SystemExit(f"unknown flap variant {variant!r}; one of {', '.join(VARIANTS)}")
    return replace(P.damper, **VARIANTS[variant])


def leak_cm2(variant: str) -> float:
    """The shut flap's leak: the 0.15 mm gap round the edge, plus the notches' open area (each a V serr_h deep through the
    disc's thickness: the area is serr_h^2 per notch, the disc clear of the rib)."""
    dm = damper(variant)
    r = g.bore_r - dm.flap_gap
    gap = 2 * math.pi * r * dm.flap_gap
    return (gap + dm.serr_n * dm.serr_h**2) / 100
