"""The real board for the assembly: KiCad's own STEP export of hw/minimeater.kicad_pcb with its components, placed in the
mech frame. Needs KiCad: the Mac app (`kicad-cli`, and its bundled python for `hw/tools/models.py`), or the kicad
container under podman (`MINIMEATER_KICAD_IMAGE`, default docker.io/kicad/kicad:10.0). The container has no KiCad stock
3D library, so the stock parts it cannot find (resistors, capacitors, the TVS and other small SMD parts) are missing from
the board there; the vendored models (the module, the connectors, the inductor, the switch, the LED) are all in. Without
either the assembly keeps the board's box. Gitignored output only (`build/mech/pcb.step`)."""

import os
import shutil
import subprocess
from pathlib import Path

from build123d import Axis, Compound, import_step

from . import geom as G
from .params import P

KICAD_PY = "/Applications/KiCad/KiCad.app/Contents/Frameworks/Python.framework/Versions/Current/bin/python3"
# the board's centre in KiCad's sheet coordinates (the board is 46 x 54 from the sheet's origin, plus usb_end to its -x side)
CENTRE = (P.pcb.x_len / 2 - P.pcb.usb_end, P.pcb.y_len / 2)


IMAGE = os.environ.get("MINIMEATER_KICAD_IMAGE", "docker.io/kicad/kicad:10.0")


def _native() -> bool:
    return shutil.which("kicad-cli") is not None and Path(KICAD_PY).exists()


def _container() -> bool:
    return shutil.which("podman") is not None and subprocess.run(["podman", "image", "exists", IMAGE]).returncode == 0


def available() -> bool:
    return _native() or _container()


def export_board(out: Path) -> Path | None:
    """Writes `out/pcb.step` (board frame, centre origin, KiCad's y flipped) and returns it, or None without KiCad."""
    if not available():
        return None
    out.mkdir(parents=True, exist_ok=True)
    tmp = Path("hw/minimeater_models.kicad_pcb")  # beside the project, so ${KIPRJMOD}/lib/3d resolves; gitignored
    step = out / "pcb.step"
    origin = f"{CENTRE[0]}x{CENTRE[1]}mm"
    export = ["kicad-cli", "pcb", "export", "step", "--subst-models", "--no-dnp", "--user-origin", origin, "-o", str(step), str(tmp)]
    if _native():
        subprocess.run([KICAD_PY, "hw/tools/models.py", "hw/minimeater.kicad_pcb", str(tmp)], check=True, capture_output=True)
        subprocess.run(export, check=True, capture_output=True)
    else:  # the repository mounted as the working directory, owned by the caller's user
        export[export.index(str(step))] = str(step.resolve().relative_to(Path.cwd()))
        script = f"python3 hw/tools/models.py hw/minimeater.kicad_pcb {tmp} && {' '.join(export)}"
        subprocess.run(["podman", "run", "--rm", "--userns=keep-id", "-v", f"{Path.cwd()}:/work:Z", "-w", "/work", "-e", "HOME=/tmp",
                        IMAGE, "sh", "-c", script], check=True, capture_output=True)
    tmp.unlink()
    return step


def placed(step: Path):
    """The board in the assembly frame. KiCad's export is the real board seen from above, parts up, with y up and the
    connectors at +y; the board lies in the pan turned over about x (parts down, connectors toward -y, the USB-C still
    at the rear). A mirror here would make a board that cannot be built. Its parts face goes to PCB_Z0 and its middle
    to the pan."""
    s = import_step(step).rotate(Axis.X, 180)
    sub = max(s.solids(), key=lambda so: so.bounding_box().size.X * so.bounding_box().size.Y).bounding_box()
    xc = (G.g.board_x0 + G.g.board_x1) / 2
    s = s.moved(__import__("build123d").Location((xc, 0.0, G.PCB_Z0 - sub.min.Z)))
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
