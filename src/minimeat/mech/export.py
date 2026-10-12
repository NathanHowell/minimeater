"""Writes STEP and STL for every printed part, in its print pose, plus the assembly (`uv run mech-export [dir]`)."""

import json
import sys
from pathlib import Path

from build123d import Compound, export_step, export_stl

from . import assembly as A
from . import shell as K
from .envelope import envelope
from .printing import ALL, print_pose


def export(out: Path):
    out.mkdir(parents=True, exist_ok=True)
    written = []
    for name in ALL:
        s = print_pose(name)
        export_step(s, str(out / f"{name}.step"))
        export_stl(s, str(out / f"{name}.stl"), tolerance=0.02, angular_tolerance=0.1)
        written.append(name)
    asm = {**A.printed(0.0), **A.bought(0.0), "hood": K.hood(), "pan": K.pan()}
    asm.pop("pcb_keepout")
    export_step(Compound(list(asm.values())), str(out / "assembly.step"))
    (out / "envelope.json").write_text(json.dumps(envelope(), indent=2))
    return written


def main():
    out = Path(sys.argv[1] if len(sys.argv) > 1 else "build/mech")
    names = export(out)
    print(f"wrote {', '.join(names)}, assembly.step and envelope.json to {out}")
