"""The airflow loop for the README (`just mech::flowloop`): a fixed cutaway (every part but the fan sectioned along the flow axis,
the fan whole) with the CFD's pathlines (`cfd/pathlines.py`) drawn as moving parcels with short trails, coloured by speed, from
the grille through the fan and the stator cone to the flap and the tube, left to right. Same canvas as the explode loop
(480 x 360, 72 frames, 60 ms, lossy WebP with alpha, `img2webp`), so the two sit side by side.

The parcels share one time scale: a loop is a slowed-down run of the longest line, and each parcel runs its own line at that
line's speeds, so the flap's wake is slow and the rotor's squeeze is quick. Every line carries a parcel, spread in phase with a
seeded jitter, and a parcel fades out when its line ends, so the stream is steady and the loop closes. A parcel behind a
solid part is hidden (a ray cast from the camera); the fan is see-through, so the swirl in the rotor shows. The rotor turns once
a loop, a slow stand-in for 3000 rpm. Not a flow simulation of the blades: the pathlines are of the steady MRF mean flow
(docs/cfd.md).

The frames stay in build/flowloop/<name>/frames (`--encode` makes the WebP again from them)."""

import argparse
import math
from pathlib import Path

import numpy as np
from PIL import Image

from . import explode

KEEP_WHOLE = ("fan_rotor",)  # not cut: the blades turn in full
GHOSTED = ("duct", "fan", "hood", "pan")  # their removed front half stays as a ghost (the cone and vanes are the duct's)
PASSABLE = ("fan", "fan_rotor")  # parcels are drawn through the fan, and through every ghost

# speed (m/s) to colour: slow cool to fast warm, mid-toned so it reads on a light and a dark page (sRGB stops)
RAMP = [(0.0, "#2c7fb8"), (1.5, "#41b6c4"), (3.0, "#7fcdbb"), (5.0, "#fdb863"), (9.0, "#e34a33")]


def ramp(speed):
    xs = [s for s, _ in RAMP]
    cols = np.array([[int(c[i:i + 2], 16) for i in (1, 3, 5)] for _, c in RAMP], float) / 255
    return np.stack([np.interp(speed, xs, cols[:, k]) for k in range(3)], axis=-1)


def parcels(lines, per_line: int, seed: int):
    """(line index, phase) of every parcel: `per_line` evenly spread phases, jittered, per line."""
    rng = np.random.default_rng(seed)
    idx = np.repeat(np.arange(len(lines)), per_line)
    phase = (np.tile(np.arange(per_line) / per_line, len(lines)) + rng.uniform(0, 1 / per_line, len(idx))) % 1.0
    return idx, phase


def clip(line, x_end):
    """`line` up to the first vertex past `x_end` (mm), cut there."""
    past = np.nonzero(line["p"][:, 0] > x_end)[0]
    k = int(past[0]) if len(past) else len(line["t"])
    k = max(k, 2)
    return {key: line[key][:k] for key in ("p", "t", "speed", "stage")}


def positions(lines, idx, phase, u: float, t_loop: float, tail: np.ndarray):
    """Positions (mm), speeds and fades of every parcel at loop phase `u`, for each tail sample (0 = the head).
    Returns arrays shaped (len(tail), n, ...)."""
    pts = np.zeros((len(tail), len(idx), 3))
    spd = np.zeros((len(tail), len(idx)))
    fade = np.zeros((len(tail), len(idx)))
    clock = ((u + phase) % 1.0) * t_loop  # each parcel's time on its line, s
    for j, k in enumerate(tail):  # tail samples in seconds behind the head
        for i in np.unique(idx):
            sel = idx == i
            ln = lines[i]
            t = clock[sel] - k
            ok = (t >= 0) & (t <= ln["t"][-1])
            tt = np.clip(t, 0, ln["t"][-1])
            pts[j, sel] = np.stack([np.interp(tt, ln["t"], ln["p"][:, c]) for c in range(3)], axis=-1)
            spd[j, sel] = np.interp(tt, ln["t"], ln["speed"])
            edge = np.minimum(t, ln["t"][-1] - t) / (0.06 * ln["t"][-1])  # a short fade in and out at the ends
            fade[j, sel] = np.where(ok, np.clip(edge, 0, 1), 0)
    return pts, spd, fade


def splat(canvas, xy, rgb, alpha, radius):
    """Soft discs (premultiplied) into `canvas` (h, w, 4), each point its own radius in px."""
    h, w = canvas.shape[:2]
    r = int(math.ceil(radius.max())) + 1
    oy, ox = np.mgrid[-r:r + 1, -r:r + 1]
    for (x, y), c, a, rad in zip(xy, rgb, alpha, radius):
        if a <= 0 or not (0 <= x < w and 0 <= y < h):
            continue
        xi, yi = int(round(x)), int(round(y))
        d2 = ((ox + xi - x) ** 2 + (oy + yi - y) ** 2) / (rad * rad)
        g = np.exp(-2.5 * d2) * a
        ys, xs = yi + oy, xi + ox
        m = (ys >= 0) & (ys < h) & (xs >= 0) & (xs < w)
        for k in range(3):
            canvas[ys[m], xs[m], k] += g[m] * c[k]
        canvas[ys[m], xs[m], 3] += g[m]


def composite(base: Image.Image, layer: np.ndarray) -> Image.Image:
    """`layer` (premultiplied RGBA, 0..1, may exceed 1) over `base` (RGBA)."""
    b = np.asarray(base.convert("RGBA"), float) / 255
    bp = np.concatenate([b[..., :3] * b[..., 3:], b[..., 3:]], axis=-1)
    a = np.clip(layer[..., 3:], 0, 1)
    scale = np.divide(a, layer[..., 3:], out=np.ones_like(a), where=layer[..., 3:] > 1e-6)
    lp = np.concatenate([np.clip(layer[..., :3] * scale, 0, 1), a], axis=-1)
    out = lp + bp * (1 - a)
    al = out[..., 3:]
    rgb = np.divide(out[..., :3], al, out=np.zeros_like(out[..., :3]), where=al > 1e-6)
    return Image.fromarray((np.concatenate([rgb, al], axis=-1) * 255).round().clip(0, 255).astype(np.uint8), "RGBA")


def smooth_shaded(o):
    """Smooth faces with a hard edge past 35 degrees (as `film.py`): the boolean's cap shares vertices with the curved cone, and
    would take its normals and render black."""
    import bmesh

    bm = bmesh.new()
    bm.from_mesh(o.data)
    for e in bm.edges:
        e.smooth = not (len(e.link_faces) == 2 and e.calc_face_angle(0.0) > math.radians(35))
    for f in bm.faces:
        f.smooth = True
    bm.to_mesh(o.data)
    bm.free()


def weld(o, dist=1e-5):
    """Merge the boolean's near-duplicate vertices along the cut (10 um): left apart, shadow rays catch on the seams and the cap
    (the stator cone's) is shadowed by itself and renders black."""
    import bmesh

    bm = bmesh.new()
    bm.from_mesh(o.data)
    bmesh.ops.remove_doubles(bm, verts=bm.verts, dist=dist)
    bm.to_mesh(o.data)
    bm.free()


def cut_away(sc):
    """Every part but the fan and its rotor cut once by a box removing y < 0 (as `film.py`): the section the camera looks into."""
    import bmesh
    import bpy

    cutter = bpy.data.objects.new("cutter", bpy.data.meshes.new("cutter"))
    bm = bmesh.new()
    bmesh.ops.create_cube(bm, size=4.0)
    bm.to_mesh(cutter.data)
    bm.free()
    sc.collection.objects.link(cutter)
    cutter.location = (0, -2.0, 0)
    cutter.hide_render = cutter.hide_viewport = True
    dg = bpy.context.evaluated_depsgraph_get()
    for o in [o for o in sc.objects if o.type == "MESH" and o is not cutter and o.name not in KEEP_WHOLE]:
        bm = bmesh.new()
        bm.from_mesh(o.data)
        bmesh.ops.remove_doubles(bm, verts=bm.verts, dist=1e-7)
        bm.to_mesh(o.data)
        bm.free()
        if o.name in GHOSTED:  # the front half, kept: the part intersected with the cutter's box
            g = o.copy()
            g.data = o.data.copy()
            g.name = "ghost_" + o.name
            sc.collection.objects.link(g)
            m = g.modifiers.new("keep", "BOOLEAN")
            m.operation, m.object, m.solver = "INTERSECT", cutter, "EXACT"
            m.use_self = m.use_hole_tolerant = True
            dg.update()
            g.data = bpy.data.meshes.new_from_object(g.evaluated_get(dg))
            g.modifiers.clear()
            weld(g)
            smooth_shaded(g)
        m = o.modifiers.new("cut", "BOOLEAN")
        m.operation, m.object, m.solver = "DIFFERENCE", cutter, "EXACT"
        m.use_self = m.use_hole_tolerant = True
        dg.update()
        o.data = bpy.data.meshes.new_from_object(o.evaluated_get(dg))
        o.modifiers.clear()
        weld(o)
        smooth_shaded(o)


def visible(sc, dg, origin, points):
    """Which `points` (m) the camera at `origin` sees: not behind a solid part. The fan is see-through, so the swirl in the rotor shows."""
    from mathutils import Vector

    out = np.ones(len(points), bool)
    for i, p in enumerate(points):
        a = Vector(origin)
        d = Vector(p) - a
        dist = d.length
        d.normalize()
        for _ in range(8):
            hit, loc, _n, _i, obj, _m = sc.ray_cast(dg, a, d, distance=max(dist - 1e-4, 0.0))
            if not hit:
                break
            if obj.name not in PASSABLE and not obj.name.startswith("ghost_"):
                out[i] = False
                break
            dist -= (loc - a).length + 1e-4
            a = loc + d * 1e-4
    return out


def render(args):
    import bpy
    from bpy_extras.object_utils import world_to_camera_view
    from mathutils import Vector

    from . import board3d, scene
    from .cfd import pathlines

    lines, meta = pathlines.load(args.lines)
    lines = lines[:: args.stride]
    out_dir = args.work / args.name
    frames_dir = out_dir / "frames"
    frames_dir.mkdir(parents=True, exist_ok=True)

    glb = out_dir / "flowloop.glb"
    step = board3d.export_board(Path("build/mech"))
    asm = scene.assembly(args.flap, board=step)
    if step is not None:
        asm.pop("connectors", None)
    scene.write(glb, asm)

    size = tuple(args.size)
    res = (2 * size[0], 2 * size[1])
    explode.OFFSET = {}  # nothing comes apart
    sc = explode.build(glb, args.frames, res, args.samples, 1.0, args.elevation, args.lens)
    rig = bpy.data.objects["rig"]  # a fixed camera: no turn, only the azimuth
    rig.animation_data_clear()
    rig.rotation_euler = (0.0, 0.0, math.radians(args.azimuth))
    cam = sc.camera
    cam.location = cam.location * args.zoom
    cut_away(sc)
    ghosts = [o for o in sc.objects if o.name.startswith("ghost_")]
    rotor = bpy.data.objects.get("fan_rotor")
    if rotor is not None:
        rotor.rotation_mode = "XYZ"
    dg = bpy.context.evaluated_depsgraph_get()

    # the lines end at the tube's mouth (the duct case runs on past it)
    tube = bpy.data.objects["tube"]
    x_end = max((tube.matrix_world @ Vector(c)).x for c in tube.bound_box) * 1e3 - args.trim
    lines = [clip(ln, x_end) for ln in lines]
    t_loop = max(float(ln["t"][-1]) for ln in lines)

    idx, phase = parcels(lines, args.per_line, args.seed)
    tail = np.linspace(0.0, args.tail_ms * 1e-3, args.tail_samples)
    for f in args.only or range(1, args.frames + 1):
        sc.frame_set(f)
        u = (f - 1) / args.frames
        if rotor is not None:
            rotor.rotation_euler = (-2 * math.pi * u, 0.0, 0.0)  # one turn a loop, in the rotor's sense (a slow stand-in for 3000 rpm)
        dg = bpy.context.evaluated_depsgraph_get()
        passes = {}
        for key, hidden in (("with", False), ("without", True)):  # the ghosts at full opacity, and not there at all
            for g in ghosts:
                g.hide_render = hidden
            sc.render.filepath = str(frames_dir / f"r_{f:04d}.{key}.png")
            bpy.ops.render.render(write_still=True)
            passes[key] = Image.open(sc.render.filepath)
        base = explode.mix(passes["without"], passes["with"], args.ghost)

        pts, spd, fade = positions(lines, idx, phase, u, t_loop, tail)
        origin = cam.matrix_world.translation
        layer = np.zeros((res[1], res[0], 4))
        for j in range(len(tail) - 1, -1, -1):  # tails first, the heads on top
            world = pts[j] * 1e-3
            ndc = np.array([world_to_camera_view(sc, cam, Vector(p)) for p in world])
            seen = visible(sc, dg, origin, world)
            xy = np.stack([ndc[:, 0] * res[0], (1 - ndc[:, 1]) * res[1]], axis=-1)
            k = 1 - j / len(tail)
            alpha = fade[j] * k * k * args.opacity * seen
            radius = args.radius * (0.55 + 0.45 * k) * np.ones(len(ndc))
            splat(layer, xy, ramp(spd[j]), alpha, radius)
        composite(base, layer).save(frames_dir / (f"look_{f:04d}.png" if args.only else f"f_{f:04d}.png"))
        if f % 6 == 0:
            print(f"frame {f}/{args.frames}", flush=True)


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("out", type=Path, help="the WebP, e.g. docs/mech/flow-open.webp")
    ap.add_argument("--lines", type=Path, default=Path("build/pathlines/pathlines-flap0.npz"))
    ap.add_argument("--flap", type=float, default=0.0, help="the flap's angle in the assembly, deg (the lines' case)")
    ap.add_argument("--name", default="open", help="work directory under --work")
    ap.add_argument("--work", type=Path, default=Path("build/flowloop"))
    ap.add_argument("--size", type=int, nargs=2, default=(480, 360), help="the WebP's size (rendered at twice it)")
    ap.add_argument("--quality", type=int, default=65)
    ap.add_argument("--frames", type=int, default=72)
    ap.add_argument("--ms", type=int, default=60)
    ap.add_argument("--samples", type=int, default=48)
    ap.add_argument("--ghost", type=float, default=0.05, help="opacity of the removed front half of the duct, the fan's frame and the shells")
    ap.add_argument("--azimuth", type=float, default=0.0, help="camera swing about the vertical from the cut side, deg")
    ap.add_argument("--zoom", type=float, default=0.92, help="camera distance, against the sphere that holds the whole assembly")
    ap.add_argument("--elevation", type=float, default=14.0)
    ap.add_argument("--lens", type=float, default=60.0)
    ap.add_argument("--per-line", type=int, default=1, help="parcels on every line")
    ap.add_argument("--stride", type=int, default=1, help="use every Nth line")
    ap.add_argument("--trim", type=float, default=2.0, help="stop the lines this far inside the tube's mouth, mm")
    ap.add_argument("--radius", type=float, default=6.5, help="parcel radius at 2x resolution, px")
    ap.add_argument("--opacity", type=float, default=1.0)
    ap.add_argument("--tail-ms", type=float, default=30.0, help="trail length in the flow's own time")
    ap.add_argument("--tail-samples", type=int, default=7)
    ap.add_argument("--seed", type=int, default=1)
    ap.add_argument("--only", type=int, nargs="+", metavar="FRAME", help="render just these frames (look_NNNN.png) and stop")
    ap.add_argument("--encode", action="store_true", help="only make the WebP again from the frames already rendered")
    args = ap.parse_args()
    frames_dir = args.work / args.name / "frames"
    if not args.encode:
        render(args)
        if args.only:
            return
    explode.encode(frames_dir, args.out, tuple(args.size), args.ms, args.quality)
    print(f"wrote {args.out} ({args.out.stat().st_size / 1e6:.2f} MB)")


if __name__ == "__main__":
    main()
