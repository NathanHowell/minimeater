"""Run ngspice in batch mode on a netlist and read back what it wrote with `wrdata`."""

import shutil
import subprocess
import tempfile
from pathlib import Path

import numpy as np


def available() -> bool:
    return shutil.which("ngspice") is not None


def run(netlist: str, outputs: list[str], ps_compat: bool = False, timeout: float = 300) -> dict[str, np.ndarray]:
    """Runs `netlist` (which must end with a control block that has no `quit`) and returns, for each name in
    `outputs`, the 2-column array (x, y) that `wrdata` wrote. The control block must call
    `wrdata <name>.dat <expr>` for each output; files land in a temporary directory. `ps_compat` runs ngspice in its PSpice
    compatibility mode (vendor models: `ti_models.py`)."""
    with tempfile.TemporaryDirectory() as d:
        cir = Path(d) / "c.cir"
        cir.write_text(netlist)
        if ps_compat:
            (Path(d) / ".spiceinit").write_text("set ngbehavior=psa\n")
        p = subprocess.run(["ngspice", "-b", str(cir)], cwd=d, capture_output=True, text=True, timeout=timeout)
        if p.returncode != 0:
            raise RuntimeError(p.stdout[-2000:] + p.stderr[-2000:])
        out = {}
        for name in outputs:
            f = Path(d) / f"{name}.dat"
            if not f.exists():
                raise RuntimeError(f"ngspice wrote no {name}.dat:\n{p.stdout[-2000:]}")
            out[name] = np.loadtxt(f)
        return out
