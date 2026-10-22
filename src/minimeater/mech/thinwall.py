"""Thin-wall check of the exported print STLs, measured the way JLC3DP's upload check does (`uv run mech-thinwall`).

Every part's surface is sampled densely (`spacing` mm apart on average). At each sample a ray goes inward along the
normal; the distance to the first face it hits is the wall there. Samples under `limit` are grouped into clusters,
and each sample is explained or not: a wedge of interior angle a under 90 degrees reads d tan a at d from its edge, so
a thin sample within t / tan a (plus a margin) of a sharp convex CAD edge that acute is a knife edge, not a wall.
Clusters with unexplained samples are the ones to look at: real thin walls, or smooth thin features (an airfoil's
nose) that no edge explains. docs/jlc3dp.md has the findings.
"""

import argparse
import math
from pathlib import Path

import numpy as np
import trimesh
from scipy.sparse import coo_matrix
from scipy.sparse.csgraph import connected_components
from scipy.spatial import cKDTree

PARTS = ("duct", "hood", "pan")


def wall(m, spacing, seed=1):
    """Surface samples, their outward normals and the inward ray's wall thickness (inf where it escapes)."""
    n = int(m.area / spacing**2)
    pts, fi = trimesh.sample.sample_surface(m, n, seed=seed)
    nrm = m.face_normals[fi]
    o = pts - nrm * 1e-4
    loc, ri, _ = m.ray.intersects_location(o, -nrm, multiple_hits=False)
    t = np.full(len(pts), np.inf)
    t[ri] = np.linalg.norm(loc - o[ri], axis=1)
    return pts, nrm, t


def acute_edges(m, spacing, max_interior=85.0):
    """Points along every convex edge whose interior angle is under `max_interior` degrees, with that angle."""
    a = m.face_adjacency_angles
    keep = m.face_adjacency_convex & (a > math.radians(180.0 - max_interior))
    seg = m.vertices[m.face_adjacency_edges[keep]]
    interior = math.pi - a[keep]
    pts, ang = [], []
    for (p0, p1), al in zip(seg, interior):
        k = max(1, int(np.linalg.norm(p1 - p0) / (spacing / 2)))
        s = np.linspace(0.0, 1.0, k + 1)[:, None]
        pts.append(p0 + s * (p1 - p0))
        ang.append(np.full(k + 1, al))
    if not pts:
        return np.zeros((0, 3)), np.zeros(0)
    return np.concatenate(pts), np.concatenate(ang)


def clusters(pts, radius):
    if len(pts) == 0:
        return np.zeros(0, int), 0
    pr = cKDTree(pts).query_pairs(radius, output_type="ndarray")
    g = coo_matrix((np.ones(len(pr)), (pr[:, 0], pr[:, 1])), shape=(len(pts), len(pts)))
    k, lab = connected_components(g, directed=False)
    return lab, k


def check(path, spacing=0.25, limit=1.0, margin=0.3):
    m = trimesh.load(path, force="mesh")
    pts, _, t = wall(m, spacing)
    per = m.area / len(pts)  # mm^2 a sample stands for
    thin = np.where(t < limit)[0]
    ep, ea = acute_edges(m, spacing)
    if len(ep):
        d, j = cKDTree(ep).query(pts[thin])
        explained = d <= t[thin] / np.tan(np.clip(ea[j], 1e-3, None)) + margin
    else:
        explained = np.zeros(len(thin), bool)
    lab, k = clusters(pts[thin], 3 * spacing)
    out = []
    for c in range(k):
        s = lab == c
        idx = thin[s]
        u = ~explained[s]
        tt = t[idx]
        out.append(dict(
            n=int(s.sum()), area=float(s.sum() * per), unexplained_area=float(u.sum() * per),
            t_min=float(tt.min()), t_med=float(np.median(tt)),
            t_med_unexplained=float(np.median(tt[u])) if u.any() else None,
            at=pts[idx].mean(0).round(1).tolist(), lo=pts[idx].min(0).round(1).tolist(),
            hi=pts[idx].max(0).round(1).tolist(),
            at_unexplained=pts[idx[u]].mean(0).round(1).tolist() if u.any() else None,
        ))
    out.sort(key=lambda r: -r["unexplained_area"])
    return dict(samples=len(pts), spacing=float(math.sqrt(per)), area=float(m.area), thin_area=float(len(thin) * per),
                clusters=out)


def report(name, r, min_area):
    print(f"{name}: {r['samples']} samples ({r['spacing']:.2f} mm), {r['thin_area']:.0f} of {r['area']:.0f} mm^2 under "
          f"the limit in {len(r['clusters'])} clusters")
    for c in r["clusters"]:
        if c["unexplained_area"] < min_area:
            continue
        print(f"  {c['unexplained_area']:6.1f} of {c['area']:6.1f} mm^2 unexplained, t min {c['t_min']:.2f} median "
              f"{c['t_med_unexplained']:.2f}  at {c['at_unexplained']}  box {c['lo']} .. {c['hi']}")
    rest = [c for c in r["clusters"] if c["unexplained_area"] < min_area]
    if rest:
        print(f"  + {len(rest)} clusters, {sum(c['area'] for c in rest):.0f} mm^2, knife edges (or under {min_area} mm^2 "
              f"unexplained); largest {max(c['area'] for c in rest):.1f} mm^2")


def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("dir", nargs="?", default="build/mech/3dprint")
    ap.add_argument("--spacing", type=float, default=0.25)
    ap.add_argument("--limit", type=float, default=1.0)
    ap.add_argument("--min-area", type=float, default=0.5, help="mm^2 of unexplained samples worth listing")
    a = ap.parse_args()
    for name in PARTS:
        report(name, check(Path(a.dir) / f"{name}.stl", a.spacing, a.limit), a.min_area)
