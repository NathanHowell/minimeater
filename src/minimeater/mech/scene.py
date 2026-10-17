"""The assembly as a scene for Blender (`assembly.glb`, `assembly_cut.glb`): PBR materials per part, the flap at any
angle, a cutaway through the flow axis, and the air path's stations as named empties for the camera and the molecule."""

from pathlib import Path

import numpy as np
import pyvista as pv
from build123d import Compound

from . import assembly as A
from . import board3d, mock
from . import shell as K
from .geom import FAN_X0, REAR_IN_X, REAR_X, g
from .glb import _mesh, write_glb
from .parts import box

# (colour, metallic, roughness). The shells and the duct are one material, MJF PA12S-HP: a dark grey, matte and grainy.
NYLON = ("#45484d", 0.0, 0.8)
STEEL = ("#c4c7ca", 1.0, 0.32)  # the made parts are 304 stainless (6061 aluminium if much cheaper): brushed
MATERIALS = {
    "hood": NYLON, "pan": NYLON, "duct": ("#4d5055", 0.0, 0.8),
    "fan": ("#1c1c1e", 0.0, 0.45), "servo": ("#2b2b2e", 0.0, 0.5), "horn": ("#e6e6e3", 0.0, 0.4),
    "flap": STEEL, "crank": STEEL, "coupler": STEEL, "pin_crank": STEEL, "pin_horn": STEEL, "rod": STEEL,
    "tube": ("#b87333", 1.0, 0.3),  # copper
    "oring": ("#141414", 0.0, 0.9),
    "connectors": ("#2b2b2b", 0.0, 0.5),
    # the board keeps the colours of its own model
    "pcb_substrate": ("#1d6b3a", 0.0, 0.35), "pcb_module": ("#3b3b3b", 0.0, 0.5), "pcb_usb": ("#c8c8c8", 1.0, 0.35),
    "pcb_jacks": ("#202020", 0.0, 0.5), "pcb_parts": ("#2a2a2a", 0.0, 0.5),
}
BOARD_PARTS = ("pcb_substrate", "pcb_module", "pcb_usb", "pcb_jacks", "pcb_parts")

# the section plane: everything on the -y side of it goes, which leaves the servo and the linkage on +y in place
CUT_Y = 0.0


def assembly(flap_deg=0.0, board=None):
    """Name -> shape of the product with the flap at `flap_deg` (0 open, 90 closed) and the servo following it, the fan as
    Noctua's model, and the board when `board` is its exported STEP (`board3d.export_board`). Without the fit-check
    stand-ins (the plugs, the board's keep-out)."""
    asm = {**A.printed(flap_deg), **A.bought(flap_deg), "hood": K.hood(), "pan": K.pan()}
    asm.pop("pcb_keepout")
    asm.pop("plugs")
    asm["fan"] = mock.fan_step()
    if board is not None:
        asm["pcb"] = board3d.placed(board)
    return asm


def airpath(tube):
    """The air path's stations as (name, (x, y, z)) along the flow axis, in order: the rear wall's gills, the plenum behind
    the fan, the fan's inlet and outlet faces, the end of the cone, the flap's shaft, the socket's shoulder and the
    intake tube's end."""
    x = (("grille", REAR_X), ("plenum", (REAR_IN_X + FAN_X0) / 2), ("fan_in", FAN_X0), ("fan_out", g.fan_face_x),
         ("bore", g.straight_x0), ("flap", g.flap_x), ("socket", g.socket_x0), ("tube_end", tube.bounding_box().max.X))
    return [(f"air.{i}.{name}", (v, 0.0, 0.0)) for i, (name, v) in enumerate(x)]


def _clip(solid, y):
    """The mesh of `solid` with everything below `y` clipped off, open at the plane."""
    v, t = _mesh(solid)
    pd = pv.PolyData(v, np.hstack([np.full((len(t), 1), 3), t]).ravel()).clip(normal=(0, -1, 0), origin=(0, y, 0)).triangulate()
    return np.asarray(pd.points), np.asarray(pd.regular_faces)


def _cut_solid(solid, y):
    """`solid` with everything below `y` removed: itself when it lies above, None when it lies below, else the boolean,
    which closes it at the plane. A boolean on an imported solid with a face OCC cannot mesh (the fan's frame) comes out
    as garbage outside the solid's own box; that one is clipped as a mesh instead, open at the plane."""
    b = solid.bounding_box()
    if b.min.Y >= y:
        return solid
    if b.max.Y <= y:
        return None
    out = solid & box(-1000, 1000, y, y + 1000, -1000, 1000)
    if out is None or out.volume < 1e-6:
        return None
    o = out.bounding_box()
    inside = all(lo - 1e-3 <= o_lo and o_hi <= hi + 1e-3 for lo, hi, o_lo, o_hi in
                 zip((b.min.X, b.min.Y, b.min.Z), (b.max.X, b.max.Y, b.max.Z), (o.min.X, o.min.Y, o.min.Z), (o.max.X, o.max.Y, o.max.Z)))
    return out if inside else _clip(solid, y)


def _cut(shape, y):
    """`shape` cut solid by solid at `y`: a list of what is left (shapes and meshes), empty if nothing is."""
    return [c for c in (_cut_solid(s, y) for s in shape.solids() or [shape]) if c is not None]


def look(name):
    return MATERIALS.get(name, ("#a0a0a0", 0.1, 0.6))


def parts(asm, cut_y=None):
    """The glTF parts of `asm` (a `scene.assembly`), the board in its five colours; cut at `cut_y` when given."""
    out = []
    for name, shape in asm.items():
        if name == "pcb":
            groups = board3d.pieces(shape)
            for part, solids in zip(BOARD_PARTS, groups):
                if cut_y is not None:  # hundreds of solids: cut each, the few that straddle the plane do the work
                    solids = [c for s in solids for c in _cut(s, cut_y)]
                    if solids:
                        out.append((part, solids, look(part)))
                elif solids:
                    out.append((part, Compound(obj=solids), look(part)))
            continue
        if cut_y is not None:
            shape = _cut(shape, cut_y)
        if shape:
            out.append((name, shape, look(name)))
    return out


def write(path: Path, asm, cut_y=None, flap_deg=0.0):
    """`asm` as a glb at `path`: the parts, the air path's stations as empties and the flap angle on the scene."""
    write_glb(path, parts(asm, cut_y), markers=airpath(asm["tube"]), extras={"flap_deg": flap_deg, "cut_y": cut_y})
