"""`uv run minimeater-cfd <case>` (`just cfd <case>`): meshes and solves one duct case in the OpenFOAM container
(Apple's `container` on the Mac, podman or docker elsewhere: runtime.py) and reports the flow and the duct's loss.

The loss is read off the developed pressure line: a straight line fitted through the static pressure on the last
planes of the tube, extrapolated back to the shoulder, is where the pressure would be if the tube's flow were
developed from its first millimetre. What the duct costs above that, referenced to the tube's velocity head:

  k_static = (p_in - p_line(shoulder)) / q    the static rise the fan must make at its outlet face (a fan's curve is static
                                               pressure); it includes the acceleration from the fan's annulus to the tube
  k_total  = (pt_in - pt_line(shoulder)) / q   the total-pressure loss: what is dissipated

with q = rho v^2 / 2 at the tube's mean velocity. The tube's own friction is not in either. Neither has the grille, the
cavity behind the fan or the tube's entry from the box (flow.Intake.k_entry is box -> tube, which this geometry replaces
with the socket's shoulder).
"""

import argparse
import json
import math
import shutil
import sys
import time
from dataclasses import asdict, replace
from pathlib import Path

import numpy as np

from . import geometry, runtime
from .case import Case, tube_planes, write

RHO = 1.204  # kg/m^3, air at 20 C
TAIL = 500  # iterations averaged for the result (a partly closed flap's wake never settles in a steady solve)
CASES = {
    "open": Case(flap_deg=0.0),
    "open-swirl": Case(flap_deg=0.0, rpm=2000.0),
    "open-vortex": Case(flap_deg=0.0, vortex=18.0),  # the fan's exit swirl, Euler at the model's operating line (docs/cfd.md)
    "flap-30": Case(flap_deg=30.0),
    "flap-45": Case(flap_deg=45.0),
    "flap-60": Case(flap_deg=60.0),
}


def _dat(case_dir: Path, fo: str) -> tuple[list[str], np.ndarray]:
    """The columns and rows of a function object's .dat file."""
    f = next((case_dir / "postProcessing" / fo).glob("*/*.dat"))
    cols, rows = None, []
    for line in f.read_text().splitlines():
        if line.startswith("#"):
            cols = line[1:].split()
        elif line.strip():
            rows.append([float(v) for v in line.split()])
    return cols, np.array(rows)


def _tail(case_dir: Path, fo: str, col: str) -> tuple[float, float]:
    """Mean and spread (max - min) of a column over the last TAIL iterations."""
    cols, a = _dat(case_dir, fo)
    v = a[-TAIL:, cols.index(col)]
    return float(v.mean()), float(v.max() - v.min())


def results(case_dir: Path, c: Case) -> dict:
    s = geometry.stations(c.tube_d)
    d = 2 * s.r_tube / 1000
    area = math.pi * d * d / 4
    p_in, _ = _tail(case_dir, "inlet", "weightedAreaAverage(p)")
    pt_in, pt_in_spread = _tail(case_dir, "inlet", "weightedAreaAverage(total(p))")
    p_out, _ = _tail(case_dir, "outlet", "weightedAreaAverage(p)")
    pt_out, _ = _tail(case_dir, "outlet", "weightedAreaAverage(total(p))")
    q_out, _ = _tail(case_dir, "flow", "sum(phi)")
    v = q_out / area
    q = 0.5 * v * v  # kinematic
    xs = np.array(tube_planes(s, c.tube_d)) / 1000
    ps = np.array([_tail(case_dir, f"plane{i}", "areaAverage(p)")[0] for i in range(len(xs))])
    fit = xs >= xs[-1] - 5 * d  # the last six planes: the developed stretch
    slope, icpt = np.polyfit(xs[fit], ps[fit], 1)
    p_line = slope * s.x_tube / 1000 + icpt
    alpha = (pt_out - p_out) / q  # the developed profile's energy coefficient
    cols, a = _dat(case_dir, "inlet")
    return {
        "case": asdict(c),
        "iterations": int(a[-1, 0]),
        "flow_cfm": q_out * 60 / 0.0283168,
        "v_tube_m_s": v,
        "dp_static_pa": RHO * (p_in - p_out),
        "dp_total_pa": RHO * (pt_in - pt_out),
        "k_static": float((p_in - p_line) / q),
        "k_total": float((pt_in - (p_line + alpha * q)) / q),
        "tube_friction": float(-slope * d / q),  # Darcy f from the developed line
        "tube_friction_blasius": 0.316 / (v * d / 1.5e-5) ** 0.25,
        "alpha_out": alpha,
        "pt_in_spread_pa": RHO * pt_in_spread,  # unsteadiness of the last TAIL iterations
    }


def run(name: str, c: Case, out: Path, cpus: int, mem: str) -> dict:
    case_dir = (out / name).resolve()
    if case_dir.exists():
        shutil.rmtree(case_dir)
    t0 = time.time()
    counts = geometry.write_stl(case_dir / "constant" / "triSurface", c.flap_deg, c.tube_d)
    write(case_dir, c)
    print(f"{name}: geometry {counts}, {time.time() - t0:.0f} s", flush=True)
    if runtime.run(case_dir, "/case/Allrun", cpus, mem):
        sys.exit(f"{name}: OpenFOAM failed (logs in {case_dir})")
    res = results(case_dir, c)
    res["wall_s"] = round(time.time() - t0)
    (case_dir / "results.json").write_text(json.dumps(res, indent=2))
    return res


def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("case", choices=sorted(CASES), help="a named case")
    ap.add_argument("--flap", type=float, help="flap angle, deg (0 open)")
    ap.add_argument("--cfm", type=float, help="flow")
    ap.add_argument("--rpm", type=float, help="inlet swirl, rpm")
    ap.add_argument("--vortex", type=float, help="inlet free-vortex swirl, r c_theta / Q in 1/m")
    ap.add_argument("--base", type=float, help="background cell, mm (coarser is faster)")
    ap.add_argument("--iterations", type=int)
    ap.add_argument("--np", type=int, default=8, help="MPI ranks (and container CPUs)")
    ap.add_argument("--mem", default="8G", help="container memory")
    ap.add_argument("--out", type=Path, default=Path("build/cfd"))
    a = ap.parse_args()
    c = replace(CASES[a.case], np=a.np)
    for field, val in (("flap_deg", a.flap), ("cfm", a.cfm), ("rpm", a.rpm), ("vortex", a.vortex), ("base", a.base),
                       ("iterations", a.iterations)):
        if val is not None:
            c = replace(c, **{field: val})
    res = run(a.case, c, a.out, a.np, a.mem)
    print(json.dumps(res, indent=2))
