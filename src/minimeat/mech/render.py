"""Quick matplotlib renders of build123d shapes, for reviewing the design without a CAD viewer."""

import numpy as np
import matplotlib

matplotlib.use("Agg")
import matplotlib.pyplot as plt
from mpl_toolkits.mplot3d.art3d import Poly3DCollection

from .printing import mesh


def draw(ax, shape, color, alpha=1.0, light=(0.4, -0.5, 0.8)):
    v, t = mesh(shape, 0.15)
    tri = v[t]
    n = np.cross(tri[:, 1] - tri[:, 0], tri[:, 2] - tri[:, 0])
    n /= np.linalg.norm(n, axis=1, keepdims=True) + 1e-12
    l = np.array(light) / np.linalg.norm(light)
    shade = 0.45 + 0.55 * np.abs(n @ l)
    base = np.array(matplotlib.colors.to_rgb(color))
    cols = np.clip(base[None, :] * shade[:, None], 0, 1)
    pc = Poly3DCollection(tri, facecolors=np.c_[cols, np.full(len(cols), alpha)], linewidths=0)
    ax.add_collection3d(pc)
    return v


def render(items, path, view=(25, -60), size=(9, 7), title=None, bounds=None, tight=False):
    """items: [(shape, colour[, alpha])]. view: (elevation, azimuth) in degrees."""
    fig = plt.figure(figsize=size)
    ax = fig.add_subplot(111, projection="3d", proj_type="ortho")
    pts = []
    for it in items:
        shape, col = it[0], it[1]
        pts.append(draw(ax, shape, col, it[2] if len(it) > 2 else 1.0))
    allv = np.vstack(pts)
    lo, hi = (allv.min(0), allv.max(0)) if bounds is None else bounds
    c, r = (lo + hi) / 2, (hi - lo).max() / 2
    if tight:  # axis limits follow the shapes, drawn to scale: for long, flat layouts
        for setter, i in ((ax.set_xlim, 0), (ax.set_ylim, 1), (ax.set_zlim, 2)):
            setter(lo[i], hi[i])
        ax.set_box_aspect(tuple(hi - lo), zoom=1.0)
    else:
        ax.set_xlim(c[0] - r, c[0] + r)
        ax.set_ylim(c[1] - r, c[1] + r)
        ax.set_zlim(c[2] - r, c[2] + r)
        ax.set_box_aspect((1, 1, 1))
    ax.view_init(*view)
    ax.set_axis_off()
    if title:
        ax.set_title(title)
    fig.subplots_adjust(0, 0, 1, 0.96)
    fig.savefig(path, dpi=130)
    plt.close(fig)
