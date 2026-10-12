"""Print orientation of each part and an overhang check (the parts are meant to print without support)."""

import numpy as np
from build123d import Pos, Rot

from .parts import PARTS
from .shell import SHELLS

ALL = {**PARTS, **SHELLS}

# assembly frame -> print pose (the bed is z = 0, applied before the lift to z >= 0)
POSES = {
    "duct": Rot(0, -90, 0),  # flow axis up, fan flange on the bed: the flare and the collar's chamfer are for this
    "hood": Rot(0, -90, 0),  # standing on its rear face, nose up
    "pan": Rot(0, -90, 0),  # the same
}


def print_pose(name, shape=None):
    s = POSES[name] * (shape if shape is not None else ALL[name]())
    return Pos(0, 0, -s.bounding_box().min.Z) * s


def mesh(shape, tol=0.05, angular=0.2):
    # OCC occasionally leaves a face untriangulated at one tolerance (a sliver of a sphere), so retry coarser
    for k in (1, 2, 4, 8):
        try:
            v, t = shape.tessellate(tol * k, angular * k)
            return np.array([(p.X, p.Y, p.Z) for p in v]), np.array(t)
        except AttributeError:
            continue
    raise RuntimeError("tessellation failed")


def overhangs(shape, max_angle=45.0, min_area=1.0):
    """Downward-facing faces steeper than `max_angle` from vertical, off the bed, grouped into patches.

    Returns [{area, z0, z1, extent}] with the patch area in mm^2, its z range and its horizontal extent.
    """
    v, t = mesh(shape, 0.02, 0.1)  # fine: the designed 44 degree faces must not be flagged by facet skew
    tri = v[t]
    n = np.cross(tri[:, 1] - tri[:, 0], tri[:, 2] - tri[:, 0])
    area = np.linalg.norm(n, axis=1) / 2
    n = n / (np.linalg.norm(n, axis=1, keepdims=True) + 1e-12)
    bad = np.where((n[:, 2] < -np.sin(np.radians(max_angle)) - 1e-6) & (tri[:, :, 2].max(axis=1) > 0.05) & (area > 1e-6))[0]
    # group triangles that share a vertex
    parent = {i: i for i in bad}

    def find(a):
        while parent[a] != a:
            parent[a] = parent[parent[a]]
            a = parent[a]
        return a

    seen = {}
    for i in bad:
        for p in t[i]:
            k = int(p)
            if k in seen:
                parent[find(i)] = find(seen[k])
            else:
                seen[k] = i
    groups = {}
    for i in bad:
        groups.setdefault(find(i), []).append(i)
    out = []
    for idx in groups.values():
        pts = tri[idx].reshape(-1, 3)
        a = float(area[idx].sum())
        if a < min_area:
            continue
        ext = float(max(np.ptp(pts[:, 0]), np.ptp(pts[:, 1])))
        out.append({"area": round(a, 1), "z0": round(float(pts[:, 2].min()), 1), "z1": round(float(pts[:, 2].max()), 1),
                    "extent": round(ext, 1), "x": (round(float(pts[:, 0].min()), 1), round(float(pts[:, 0].max()), 1)),
                    "y": (round(float(pts[:, 1].min()), 1), round(float(pts[:, 1].max()), 1))})
    return sorted(out, key=lambda g: -g["area"])
