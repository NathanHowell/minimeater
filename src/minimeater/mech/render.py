"""Renders of build123d shapes for reviewing the design without a CAD viewer: VTK (PyVista) offscreen, with a depth
buffer, smooth shading split at the model's edges, and anti-aliasing.

The view is matplotlib's (elevation, azimuth) in degrees, kept from the first renderer so the callers carry over:
the camera looks at the shapes' centre from (cos e cos a, cos e sin a, sin e), z up, in parallel projection.
"""

import math

import numpy as np
import pyvista as pv

from .printing import mesh

pv.OFF_SCREEN = True
DPI = 130


def polydata(shape, tol=0.03, angular=0.1):
    """The shape's tessellation as PolyData, normals split at edges sharper than 35 degrees."""
    v, t = mesh(shape, tol, angular)
    faces = np.hstack([np.full((len(t), 1), 3), t]).ravel()
    pd = pv.PolyData(v, faces).clean()
    return pd.compute_normals(split_vertices=True, feature_angle=35.0, auto_orient_normals=False, consistent_normals=True)


def _frame(pts, c, d, up):
    """Half-height and half-width of the points seen along d with this up, and the point to aim at (c moved across
    the view to the middle of what is seen)."""
    right = np.cross(d, up)
    right /= np.linalg.norm(right)
    upv = np.cross(right, d)
    u, w = pts @ upv, pts @ right
    aim = c + upv * ((u.max() + u.min()) / 2 - c @ upv) + right * ((w.max() + w.min()) / 2 - c @ right)
    return (u.max() - u.min()) / 2, (w.max() - w.min()) / 2, aim


def render(items, path, view=(25, -60), size=(9, 7), title=None, bounds=None, tight=False, max_edge=None, light=(0.4, -0.5, 0.8)):
    """items: [(shape, colour[, alpha])]. view: (elevation, azimuth) in degrees; light: a key light's direction.
    `bounds` (lo, hi) frames that box instead of the shapes; `tight` frames the shapes closely. `max_edge` is unused
    (the first renderer split triangles for its painter's sort)."""
    w, h = int(size[0] * DPI), int(size[1] * DPI)
    p = pv.Plotter(off_screen=True, window_size=(w, h), lighting="none")
    p.set_background("white")
    pts = []
    for it in items:
        if it[0] is None or it[0].volume < 1e-6:
            continue
        pd = polydata(it[0])
        pts.append(pd.points)
        p.add_mesh(pd, color=it[1], opacity=it[2] if len(it) > 2 else 1.0, smooth_shading=True, specular=0.15,
                   specular_power=20, ambient=0.25, diffuse=0.75)
    allp = np.vstack(pts)
    lo, hi = (allp.min(0), allp.max(0)) if bounds is None else (np.asarray(bounds[0], float), np.asarray(bounds[1], float))
    c = (lo + hi) / 2
    e, a = math.radians(view[0]), math.radians(view[1])
    d = -np.array([math.cos(e) * math.cos(a), math.cos(e) * math.sin(a), math.sin(e)])  # looking direction
    up = np.array([0.0, 0.0, 1.0]) if abs(view[0]) < 89.0 else np.array([-math.cos(a), -math.sin(a), 0.0])
    corners = np.array([[x, y, z] for x in (lo[0], hi[0]) for y in (lo[1], hi[1]) for z in (lo[2], hi[2])])
    half_h, half_w, c = _frame(allp if (tight or bounds is None) else corners, c, d, up)
    span = np.linalg.norm(hi - lo)
    p.camera.position = tuple(c - d * 3 * span)
    p.camera.focal_point = tuple(c)
    p.camera.up = tuple(up)
    p.camera.enable_parallel_projection()
    top = 0.92 if title else 1.0  # room for the title
    p.camera.parallel_scale = max(half_h, half_w * h / w) * (1.06 if tight else 1.12) / top
    p.camera.clipping_range = (0.1, 6 * span + 10)
    key = np.asarray(light, float) / np.linalg.norm(light)
    p.add_light(pv.Light(position=tuple(c + key * span * 4), focal_point=tuple(c), intensity=0.75, light_type="scene light"))
    p.add_light(pv.Light(position=tuple(c - d * span * 4), focal_point=tuple(c), intensity=0.45, light_type="scene light"))
    p.add_light(pv.Light(position=tuple(c - key * span * 4 + up * span), focal_point=tuple(c), intensity=0.2, light_type="scene light"))
    if title:
        p.add_text(title, position="upper_edge", font_size=11, color="black")
    p.enable_anti_aliasing("ssaa")
    p.screenshot(str(path))
    p.close()
