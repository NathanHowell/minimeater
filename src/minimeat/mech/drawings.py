"""2D drawings of the machined parts for the CNC quote: three views, overall sizes, and a table of threads, bores and
tolerances, one A4 PDF per part named like its STEP file (`flap.pdf` beside `flap.step`: JLCCNC pairs a drawing with a 3D file
by filename). `uv run mech-export` writes them to `cnc/`.
The STEP files carry a thread only as a plain hole at the tap-drill diameter, so the callouts live here."""

from dataclasses import dataclass
from pathlib import Path

import matplotlib

matplotlib.use("Agg")
import matplotlib.pyplot as plt
import numpy as np
from matplotlib.backends.backend_pdf import PdfPages

from . import geom as G
from .geom import g
from .params import P
from .parts import MACHINED

MATERIAL = "304 stainless steel (6061-T6 aluminium if much cheaper)"
GENERAL = "General tolerance ISO 2768-m (+-0.1 on the sizes shown). Break sharp edges 0.1-0.3, no burrs. Threads ISO metric 6H. Quantity 5 of each."


@dataclass(frozen=True)
class Feature:
    """A hole or bore on a drawing: where it is (assembly frame), the way it goes, and what to cut."""

    at: tuple[float, float, float]
    axis: str  # "x", "y" or "z": the direction the hole runs
    call: str


def features():
    L, dm = P.linkage, P.damper
    xf = g.flap_x
    zs = (G.CRANK_Z0 + G.ARM_Z0) / 2
    zc = (G.CRANK_Z0 + G.ARM_Z1) / 2
    xc = g.flap_x - L.arm_r
    return {
        "flap": [
            Feature((xf, 0.0, g.bore_r - dm.flap_gap - 0.05), "z", f"Disc edge: Ø{2 * ((g.bore_r - dm.flap_gap) ** 2 - ((dm.rib_t - dm.flap_t) / 2) ** 2) ** 0.5:.1f} +-0.05 (sets the closed-flap leak), 2.0 thick, flat to 0.05"),
            Feature((xf, 0.0, 0.0), "z", f"D-bore through the rib: Ø{dm.shaft_d:.1f} +{dm.bore_clear:.2f}/0 with a flat {dm.shaft_flat:.1f} across (the rod's D); retaining compound, no thread"),
        ],
        "crank": [
            Feature((xf, 0.0, zc), "z", f"D-bore through hub and arm: Ø{dm.shaft_d:.1f} +{L.slip:.2f}/0, flat {dm.shaft_flat:.1f} across (slip fit on the rod)"),
            Feature((xf - L.arm_r, 0.0, (G.ARM_Z0 + G.ARM_Z1) / 2), "z", f"M2 x 0.4-6H tapped through the arm (tap drill Ø{L.set_screw_d:.1f}); an M2 shoulder screw's thread"),
            Feature((xf, L.hub_d / 2, zs), "y", f"M2 x 0.4-6H tapped radial, from the flat's side through the hub wall into the bore (tap drill Ø{L.set_screw_d:.1f}); set screw"),
        ],
        "coupler": [
            Feature((xc, 0.0, (G.COUPLER_Z0 + G.COUPLER_Z1) / 2), "z", f"Ø{L.pin_d + 2 * L.pin_hole_clear:.2f} +0.05/0 through (M2 shoulder screw's shoulder, free turning)"),
            Feature((xc, G.SERVO_Y, (G.COUPLER_Z0 + G.COUPLER_Z1) / 2), "z", f"Ø{L.pin_d + 2 * L.pin_hole_clear:.2f} +0.05/0 through; centres {G.SERVO_Y:.1f} +-0.05 from hole 1"),
        ],
    }


# (viewport origin, up, which world axes the view's x and y are)
VIEWS = {"front": ((0, -100, 0), (0, 0, 1), (0, 2)), "top": ((0, 0, 100), (0, 1, 0), (0, 1)), "right": ((100, 0, 0), (0, 0, 1), (1, 2))}


def _lines(edges, centre, axes):
    out = []
    for e in edges:
        pts = e.positions(np.linspace(0, 1, 40 if e.geom_type.name != "LINE" else 2))
        out.append(np.array([(p.X, p.Y) for p in pts]))
    return out


def _page(pdf, name, shape, feats):
    bb = shape.bounding_box()
    centre = np.array([(bb.min.X + bb.max.X) / 2, (bb.min.Y + bb.max.Y) / 2, (bb.min.Z + bb.max.Z) / 2])
    s = shape.moved(__import__("build123d").Location(tuple(-centre)))
    size = np.array([bb.size.X, bb.size.Y, bb.size.Z])
    fig = plt.figure(figsize=(11.69, 8.27))
    fig.suptitle(f"minimeat {name}: machined part, mm", x=0.04, ha="left", fontsize=13)
    scale = min(120 / max(size[0], 1), 80 / max(size[2] + size[1], 1), 10 / 1.0)
    gap = 18.0
    # sheet layout (mm on the page, 297 x 210): front at the top left, the top view under it, the right view top right
    # of them, the table bottom right
    place = {"front": (22 + size[0] * scale / 2, 185 - size[2] * scale / 2),
             "top": (22 + size[0] * scale / 2, 185 - size[2] * scale - gap - size[1] * scale / 2),
             "right": (170 + size[1] * scale / 2, 185 - size[2] * scale / 2)}
    ax = fig.add_axes([0, 0, 1, 1])
    ax.set_xlim(0, 297)
    ax.set_ylim(0, 210)
    ax.set_aspect("equal")
    ax.axis("off")
    for view, (origin, up, (ia, ib)) in VIEWS.items():
        vis, hid = s.project_to_viewport(origin, up, look_at=(0, 0, 0))
        cx, cy = place[view]
        for col, ls, group in (("k", "-", vis), ("0.5", (0, (3, 2)), hid)):
            for line in _lines(group, centre, (ia, ib)):
                ax.plot(cx + line[:, 0] * scale, cy + line[:, 1] * scale, color=col, ls=ls, lw=0.9 if col == "k" else 0.5)
        w, h = (size[ia], size[ib]) if view != "front" else (size[0], size[2])
        if view == "top":
            w, h = size[0], size[1]
        if view == "right":
            w, h = size[1], size[2]
        # overall dimensions
        ax.annotate("", (cx - w * scale / 2, cy - h * scale / 2 - 4), (cx + w * scale / 2, cy - h * scale / 2 - 4), arrowprops=dict(arrowstyle="<->", lw=0.6))
        ax.text(cx, cy - h * scale / 2 - 7, f"{w:.1f}", ha="center", fontsize=8)
        ax.annotate("", (cx + w * scale / 2 + 4, cy - h * scale / 2), (cx + w * scale / 2 + 4, cy + h * scale / 2), arrowprops=dict(arrowstyle="<->", lw=0.6))
        ax.text(cx + w * scale / 2 + 6, cy, f"{h:.1f}", va="center", rotation=90, fontsize=8)
        ax.text(cx - w * scale / 2, cy + h * scale / 2 + 3, view.upper(), fontsize=8, color="0.3")
        for i, f in enumerate(feats, 1):
            p = np.array(f.at) - centre
            vx, vy = {"front": (p[0], p[2]), "top": (p[0], p[1]), "right": (p[1], p[2])}[view]
            ax.plot(cx + vx * scale, cy + vy * scale, "o", mfc="none", mec="tab:red", ms=7, mew=0.8)
            ax.text(cx + vx * scale + 2, cy + vy * scale + 2, str(i), color="tab:red", fontsize=9, weight="bold")
    # the table
    y = 112
    ax.text(150, y, "Features", fontsize=10, weight="bold")
    for i, f in enumerate(feats, 1):
        y -= 9
        ax.text(150, y, f"{i}", color="tab:red", weight="bold", fontsize=9)
        ax.text(156, y, "\n".join(_wrap(f.call, 80)), fontsize=8, va="top")
        y -= 4.2 * (len(_wrap(f.call, 80)) - 1)
    y -= 14
    for line in (f"Material: {MATERIAL}", "Finish: as machined; deburr; passivate if stainless.", GENERAL):
        for w_ in _wrap(line, 90):
            ax.text(150, y, w_, fontsize=8)
            y -= 5
        y -= 2
    pdf.savefig(fig)
    plt.close(fig)


def _wrap(text, n):
    words, lines, cur = text.split(), [], ""
    for w in words:
        if len(cur) + len(w) + 1 > n and cur:
            lines.append(cur)
            cur = w
        else:
            cur = f"{cur} {w}".strip()
    return lines + [cur]


def write(out: Path) -> list[Path]:
    feats = features()
    paths = []
    for name, build in MACHINED.items():
        path = out / f"{name}.pdf"
        with PdfPages(path) as pdf:
            _page(pdf, name, build(), feats[name])
        paths.append(path)
    return paths
