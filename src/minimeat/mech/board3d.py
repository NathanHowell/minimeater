"""The real board for the assembly: KiCad's own STEP export of hw/minimeat.kicad_pcb with its components, placed in the
mech frame. Needs KiCad (`kicad-cli`, and its bundled python for `hw/tools/models.py`); without it the assembly keeps
the board's box. Gitignored output only (`build/mech/pcb.step`)."""

import shutil
import subprocess
from pathlib import Path

from build123d import Compound, Plane, import_step, mirror

from . import geom as G
from .params import P

KICAD_PY = "/Applications/KiCad/KiCad.app/Contents/Frameworks/Python.framework/Versions/Current/bin/python3"
# the board's centre in KiCad's sheet coordinates (the board is 46 x 54 from the sheet's origin)
CENTRE = (P.pcb.x_len / 2, P.pcb.y_len / 2)


def available() -> bool:
    return shutil.which("kicad-cli") is not None and Path(KICAD_PY).exists()


def export_board(out: Path) -> Path | None:
    """Writes `out/pcb.step` (board frame, centre origin, KiCad's y flipped) and returns it, or None without KiCad."""
    if not available():
        return None
    out.mkdir(parents=True, exist_ok=True)
    tmp = Path("hw/minimeat_models.kicad_pcb")  # beside the project, so ${KIPRJMOD}/lib/3d resolves; gitignored
    subprocess.run([KICAD_PY, "hw/tools/models.py", "hw/minimeat.kicad_pcb", str(tmp)], check=True, capture_output=True)
    step = out / "pcb.step"
    subprocess.run(["kicad-cli", "pcb", "export", "step", "--subst-models", "--no-dnp",
                    "--user-origin", f"{CENTRE[0]}x{CENTRE[1]}mm", "-o", str(step), str(tmp)], check=True, capture_output=True)
    tmp.unlink()
    return step


def placed(step: Path):
    """The board in the assembly frame: KiCad's export has y up where the sheet has y down, the mech frame follows the
    sheet (connectors on -y), so mirror across y = 0, then the board's underside to PCB_Z0 and its middle to the pan."""
    s = mirror(import_step(step), Plane.XZ)
    xc = (G.g.board_x0 + G.g.board_x1) / 2
    s = s.moved(__import__("build123d").Location((xc, 0.0, G.PCB_Z0)))
    return frozen(s)


def frozen(board) -> Compound:
    """The board and every part on it as one flat compound, labelled "pcb": KiCad's STEP is an assembly of a few hundred
    instances, and a viewer's exploded view sends each of them off on its own. With no tree below it the board moves
    as one piece."""
    return Compound(obj=list(board.solids()), label="pcb")


def pieces(board):
    """(substrate, module, usb_c, jacks, others): the solids of a placed board sorted for colouring a render."""
    sub, mod, usb, jacks, rest = [], [], [], [], []
    for so in board.solids():
        b = so.bounding_box()
        x, y = b.size.X, b.size.Y
        if x > 30 and y > 30:
            sub.append(so)
        elif x > 15 and y > 20:
            mod.append(so)
        elif so.volume > 80 and 4.5 < x < 5.5 and 11 < y < 13:
            jacks.append(so)
        elif b.max.Y < G.PCB_Y0 + 9.5 and b.min.X < G.g.board_x0 + 11.5 and x > 4 and so.volume > 10:
            usb.append(so)
        else:
            rest.append(so)
    return sub, mod, usb, jacks, rest
