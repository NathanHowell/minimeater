"""The assembly as a scene for Blender (`assembly.glb`): PBR materials per part, the flap at any angle, the fan's rotor as a
node of its own (it spins about x), and the air path's stations as named empties for the camera and the molecule.

The cutaway is made in the scene, not here: a Boolean modifier (Exact, self intersection and hole tolerant) with a cutter box,
which leaves the rotor free to spin in the cut. Tried on every part of this file, one cut each: 0.03 s for the rod, 7.5 s for the
fan, 26 s for the board's 393 small parts (196k vertices), 51 s in all. Merge vertices first (1e-7 m): the mesh comes out
of OCC with a vertex per face corner."""

from pathlib import Path

from build123d import Compound

from . import assembly as A
from . import board3d, mock
from . import shell as K
from .geom import FAN_X0, REAR_IN_X, REAR_X, g
from .glb import write_glb

# (colour, metallic, roughness). The shells and the duct are one material, MJF PA12S-HP: a dark grey, matte and grainy.
NYLON = ("#45484d", 0.0, 0.8)
STEEL = ("#c4c7ca", 1.0, 0.32)  # the made parts are 304 stainless (6061 aluminium if much cheaper): brushed
MATERIALS = {
    "hood": NYLON, "pan": NYLON, "duct": ("#4d5055", 0.0, 0.8),
    "fan": ("#1c1c1e", 0.0, 0.45), "fan_rotor": ("#1c1c1e", 0.0, 0.45), "servo": ("#2b2b2e", 0.0, 0.5), "horn": ("#e6e6e3", 0.0, 0.4),
    "flap": STEEL, "crank": STEEL, "coupler": STEEL, "pin_crank": STEEL, "pin_horn": STEEL, "rod": STEEL,
    "tube": ("#b87333", 1.0, 0.3),  # copper
    "oring": ("#141414", 0.0, 0.9),
    "connectors": ("#2b2b2b", 0.0, 0.5),
    # the board keeps the colours of its own model
    "pcb_substrate": ("#1d6b3a", 0.0, 0.35), "pcb_module": ("#3b3b3b", 0.0, 0.5), "pcb_usb": ("#c8c8c8", 1.0, 0.35),
    "pcb_jacks": ("#202020", 0.0, 0.5), "pcb_parts": ("#2a2a2a", 0.0, 0.5),
}
BOARD_PARTS = ("pcb_substrate", "pcb_module", "pcb_usb", "pcb_jacks", "pcb_parts")


def assembly(flap_deg=0.0, board=None):
    """Name -> shape of the product with the flap at `flap_deg` (0 open, 90 closed) and the servo following it, the fan as
    Noctua's model in two parts (`fan`, and `fan_rotor` to spin about x), and the board when `board` is its exported STEP (`board3d.export_board`). Without the fit-check
    stand-ins (the plugs, the board's keep-out)."""
    asm = {**A.printed(flap_deg), **A.bought(flap_deg), "hood": K.hood(), "pan": K.pan()}
    asm.pop("pcb_keepout")
    asm.pop("plugs")
    asm["fan_rotor"], asm["fan"] = mock.fan_parts()
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


def look(name):
    return MATERIALS.get(name, ("#a0a0a0", 0.1, 0.6))


def parts(asm):
    """The glTF parts of `asm` (a `scene.assembly`), the board in its five colours."""
    out = []
    for name, shape in asm.items():
        if name == "pcb":
            out += [(part, Compound(obj=solids), look(part)) for part, solids in zip(BOARD_PARTS, board3d.pieces(shape)) if solids]
        else:
            out.append((name, shape, look(name)))
    return out


def write(path: Path, asm, flap_deg=0.0):
    """`asm` as a glb at `path`: the parts, the air path's stations as empties and the flap angle on the scene."""
    write_glb(path, parts(asm), markers=airpath(asm["tube"]), extras={"flap_deg": flap_deg})
