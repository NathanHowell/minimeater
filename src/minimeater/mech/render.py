"""Quick matplotlib renders of build123d shapes, for reviewing the design without a CAD viewer.

All the shapes of a view go into one Poly3DCollection so that matplotlib's painter's sort (by triangle depth)
works across them; big triangles are split first so the sort has little to get wrong.
"""

import numpy as np
import matplotlib

matplotlib.use("Agg")
import matplotlib.pyplot as plt
from mpl_toolkits.mplot3d.art3d import Poly3DCollection

from .printing import mesh


def _split(tri, max_edge):
    """Subdivide triangles until no edge is longer than `max_edge` (mm)."""
    out = []
    todo = list(tri)
    while todo:
        t = todo.pop()
        e = np.linalg.norm(np.roll(t, -1, axis=0) - t, axis=1)
        if e.max() <= max_edge or len(out) > 400000:
            out.append(t)
            continue
        i = int(e.argmax())  # split the longest edge
        a, b, c = t[i], t[(i + 1) % 3], t[(i + 2) % 3]
        m = (a + b) / 2
        todo.append(np.array([a, m, c]))
        todo.append(np.array([m, b, c]))
    return np.array(out)


def shaded(shape, color, alpha=1.0, light=(0.4, -0.5, 0.8), max_edge=6.0):
    """(triangles, rgba per triangle) for one shape."""
    v, t = mesh(shape, 0.08, 0.12)
    tri = _split(v[t], max_edge)
    n = np.cross(tri[:, 1] - tri[:, 0], tri[:, 2] - tri[:, 0])
    n /= np.linalg.norm(n, axis=1, keepdims=True) + 1e-12
    l = np.array(light) / np.linalg.norm(light)
    shade = 0.5 + 0.5 * np.abs(n @ l)
    base = np.array(matplotlib.colors.to_rgb(color))
    cols = np.clip(base[None, :] * shade[:, None], 0, 1)
    return tri, np.c_[cols, np.full(len(cols), alpha)]


def render(items, path, view=(25, -60), size=(9, 7), title=None, bounds=None, tight=False, max_edge=6.0, light=(0.4, -0.5, 0.8)):
    """items: [(shape, colour[, alpha])]. view: (elevation, azimuth) in degrees; light: its direction."""
    fig = plt.figure(figsize=size)
    ax = fig.add_subplot(111, projection="3d", proj_type="ortho")
    tris, cols = [], []
    for it in items:
        if it[0] is None or it[0].volume < 1e-6:
            continue
        t, c = shaded(it[0], it[1], it[2] if len(it) > 2 else 1.0, light=light, max_edge=max_edge)
        tris.append(t)
        cols.append(c)
    tri = np.vstack(tris)
    pc = Poly3DCollection(tri, facecolors=np.vstack(cols), linewidths=0, zsort="average")
    ax.add_collection3d(pc)
    allv = tri.reshape(-1, 3)
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
        ax.set_box_aspect((1, 1, 1), zoom=1.25)
    ax.view_init(*view)
    ax.set_axis_off()
    if title:
        ax.set_title(title)
    fig.subplots_adjust(0, 0, 1, 0.96)
    fig.savefig(path, dpi=130)
    plt.close(fig)
