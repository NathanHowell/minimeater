"""`just mech::cfd::fields CASE [CASE ...]`: the flow field of finished grille cases, not just
their numbers. Each case keeps its last time step; this cuts slices through it in the OpenFOAM container
(`postProcess`, a few cores, a minute) and draws them side by side, one column per case:

- two sections across the gills (y = 0 and y = 15): speed, the in-plane flow as arrows;
- three planes facing the fan (just off the wall, mid plenum, 0.5 mm off the rotor): axial velocity (red in, blue
  back out), the cross flow as arrows, the hub, r 25 and the bore drawn in;
- the same planes' total pressure against the room's (where the loss is made).

Written to `<out>/fields-<names>.png`.
"""

import argparse
import math
from pathlib import Path

import matplotlib

matplotlib.use("Agg")
import matplotlib.pyplot as plt  # noqa: E402
import numpy as np  # noqa: E402
from scipy.interpolate import griddata  # noqa: E402
from scipy.spatial import cKDTree  # noqa: E402

from .. import geom as G  # noqa: E402
from .. import shell  # noqa: E402
from ..params import P  # noqa: E402
from . import runtime  # noqa: E402

X_EXIT = G.REAR_IN_X + 0.5
X_MID = (G.REAR_IN_X + G.FAN_X0) / 2
X_ROTOR = G.FAN_X0 - 0.5
SIDES = (0.0, 15.0)
FACES = (("exit", X_EXIT), ("mid", X_MID), ("rotor", X_ROTOR))


def slices_dict() -> str:
    planes = [f"y{y:g} {{ type cuttingPlane; planeType pointAndNormal; pointAndNormalDict {{ point (0 {y / 1000} 0); normal (0 1 0); }} interpolate false; }}" for y in SIDES]
    planes += [f"{n} {{ type cuttingPlane; planeType pointAndNormal; pointAndNormalDict {{ point ({x / 1000} 0 0); normal (1 0 0); }} interpolate false; }}" for n, x in FACES]
    body = "\n            ".join(planes)
    return f"""FoamFile {{ version 2.0; format ascii; class dictionary; object slices; }}
functions
{{
    slices {{ type surfaces; libs (sampling); surfaceFormat raw; fields (U p);
        surfaces {{
            {body}
        }} }}
}}
"""


def sample(case: Path, cpus: int = 4) -> Path:
    (case / "system" / "slices").write_text(slices_dict())
    script = "cd /case && export OMPI_ALLOW_RUN_AS_ROOT=1 OMPI_ALLOW_RUN_AS_ROOT_CONFIRM=1 && mpirun --oversubscribe -np 8 postProcess -parallel -dict system/slices -fields '(U p)' -latestTime > log.slices 2>&1"
    if runtime.run(case.resolve(), script, cpus, "8G"):
        raise SystemExit(f"{case}: postProcess failed (log.slices)")
    return max((case / "postProcessing" / "slices").iterdir(), key=lambda p: float(p.name))


def load(d: Path, name: str):
    u = np.loadtxt(d / f"U_{name}.raw", comments="#")
    p = np.loadtxt(d / f"p_{name}.raw", comments="#")
    return u[:, :3] * 1000, u[:, 3:6], p[:, 3]


def room_pt(case: Path) -> float:
    """The room's total pressure (kinematic), as the case's inlet patch average."""
    f = next((case / "postProcessing" / "inlet").glob("*/surfaceFieldValue.dat"))
    last = [ln for ln in f.read_text().splitlines() if ln and not ln.startswith("#")][-1].split()
    return float(last[-1])


def gridded(a, b, vals, extent, step=0.25, reach=0.8):
    """Samples (cell centres on the cut) on a regular grid, blank (solid) where the nearest sample is further than
    `reach` times that sample's own spacing to its neighbour: the mesh's cells grow from ~0.5 mm at the walls to 2."""
    ga, gb = np.mgrid[extent[0]:extent[1]:step, extent[2]:extent[3]:step]
    pts = np.column_stack([a, b])
    tree = cKDTree(pts)
    spacing = tree.query(pts, k=2)[0][:, 1]
    d, i = tree.query(np.column_stack([ga.ravel(), gb.ravel()]))
    out = []
    for v in vals:
        g = griddata(pts, v, (ga, gb), method="linear")
        g.ravel()[d > reach * spacing[i] + 0.1] = np.nan
        out.append(g)
    return ga, gb, out


def arrows(ax, a, b, ua, ub, extent, n=28):
    ga, gb, (fa, fb) = gridded(a, b, (ua, ub), extent, step=(extent[1] - extent[0]) / n)
    ax.quiver(ga, gb, fa, fb, color="k", scale=60, width=0.003, alpha=0.7)


def circles(ax):
    for r, ls in ((P.fan.hub_d / 2, ":"), (25.0, "--"), (P.fan.aperture_d / 2, "-")):
        t = np.linspace(0, 2 * math.pi, 200)
        ax.plot(r * np.cos(t), r * np.sin(t), "k", lw=0.6, ls=ls)


def draw(cases: list[Path], out: Path):
    rows = len(SIDES) + 2 * len(FACES)
    fig, axs = plt.subplots(rows, len(cases), figsize=(4.2 * len(cases), 3.9 * rows), squeeze=False)
    for j, case in enumerate(cases):
        d = sample(case)
        pt0 = room_pt(case)
        axs[0, j].set_title(case.name.replace("grille-current-gap0-4.2cfm", "baseline"), fontsize=10)
        for i, y in enumerate(SIDES):
            xyz, u, _ = load(d, f"y{y:g}")
            ext = (shell.X_REAR - 2, G.FAN_X0 + 3, -36, 34)
            sp = np.linalg.norm(u, axis=1)
            ga, gb, (g,) = gridded(xyz[:, 0], xyz[:, 2], (sp,), ext)
            ax = axs[i, j]
            im = ax.pcolormesh(ga, gb, g, cmap="viridis", vmin=0, vmax=8, shading="auto")
            arrows(ax, xyz[:, 0], xyz[:, 2], u[:, 0], u[:, 2], ext)
            ax.axvline(G.FAN_X0, color="r", lw=0.6)
            ax.set_aspect("equal"); ax.set_xlim(ext[:2]); ax.set_ylim(ext[2:])
            ax.set_ylabel(f"y = {y:g}: speed, m/s", fontsize=8)
            fig.colorbar(im, ax=ax, shrink=0.7)
        for k, (name, _) in enumerate(FACES):
            xyz, u, p = load(d, name)
            ext = (-35, 35, -35, 33)
            ga, gb, (cx, pt) = gridded(xyz[:, 1], xyz[:, 2], (u[:, 0], p + 0.5 * (u ** 2).sum(1) - pt0), ext)
            ax = axs[len(SIDES) + k, j]
            im = ax.pcolormesh(ga, gb, cx, cmap="RdBu_r", vmin=-4, vmax=4, shading="auto")
            arrows(ax, xyz[:, 1], xyz[:, 2], u[:, 1], u[:, 2], ext)
            circles(ax); ax.set_aspect("equal")
            ax.set_ylabel(f"{name}: axial m/s (+ into the fan)", fontsize=8)
            fig.colorbar(im, ax=ax, shrink=0.7)
            ax = axs[len(SIDES) + len(FACES) + k, j]
            im = ax.pcolormesh(ga, gb, 1.204 * pt, cmap="magma", vmin=-12, vmax=2, shading="auto")
            circles(ax); ax.set_aspect("equal")
            ax.set_ylabel(f"{name}: total p vs room, Pa", fontsize=8)
            fig.colorbar(im, ax=ax, shrink=0.7)
    fig.tight_layout()
    fig.savefig(out, dpi=90)
    return out


def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("cases", nargs="+", type=Path)
    ap.add_argument("--out", type=Path, default=Path("build/cfd-grille-chev"))
    a = ap.parse_args()
    names = "+".join(c.name.replace("grille-current-gap0-4.2cfm", "").lstrip("-") or "base" for c in a.cases)
    print(draw(a.cases, a.out / f"fields-{names}.png"))


if __name__ == "__main__":
    main()
