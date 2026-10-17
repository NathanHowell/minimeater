"""The raytraced animation's scene: `assembly.glb`, cut in half through the flow axis, with one O2 molecule riding a CFD pathline
(`uv run --group scene mech-scene`; bpy is in the `scene` group, ADR 0007). Saves a .blend to hand-tune and can render stills.

Blender's frame is the mech frame (x along the flow, z up) in metres: the glTF importer undoes the export's y-up turn. The
pathlines (`cfd.pathlines`) are mm in the same frame.

Time. A pathline's own clock runs about 35 ms from the room to the tube, 10x faster in the rotor than in the plenum, so the film
re-times it: screen time advances by ds / speed^ALPHA (ALPHA 0 is constant screen speed, 1 is real time scaled), normalised to
the shot's length. The rotor turns by omega * (the line's real time), so it stays in step with the molecule through the blades.
The molecule is ball-and-stick, SCALE times life size (an O=O bond is 1.21 A), because at life size it is under a billionth of a
metre; the film says so."""

import argparse
import math
from pathlib import Path

import numpy as np

ANGSTROM = 1e-10
BOND, BALL, STICK, DOUBLE = 1.21, 0.4, 0.09, 0.2  # O=O bond length, ball radius, stick radius, the two sticks' offset (A)
SEED = 7
SECONDS = {"cfd": 22.0, "approach": 8.0, "combustion": 9.0, "rise": 8.0, "bump": 3.0}  # screen time of each part of the film
# (camera, the x (mm) at which the film cuts to it; None is 10 mm before the tube's end): the molecule starts in the room
SHOTS = (("intake", -999.0), ("follow", -128.0), ("flap", -62.0), ("follow", -20.0), ("wide", None))


def pick(lines, r_mm=14.0):
    """The index of the line that goes furthest down the airway through the rotor, of those seeded nearest `r_mm` from the axis."""
    reach = np.array([ln["p"][-1, 0] if (ln["stage"] == 2).any() and (ln["stage"] == 1).any() else -1e9 for ln in lines])
    r = np.array([np.hypot(*ln["p"][0, 1:]) for ln in lines])
    ok = np.flatnonzero(reach >= reach.max() - 5.0)
    return int(ok[np.argmin(abs(r[ok] - r_mm))])


def trim(lines, x_mm):
    """`lines` cut where they pass x = `x_mm` (the duct case's domain runs on past the tube); lines that never get there are kept whole."""
    out = []
    for ln in lines:
        past = np.flatnonzero(ln["p"][:, 0] > x_mm)
        k = past[0] + 1 if len(past) else len(ln["p"])
        out.append({**ln, **{key: ln[key][:k] for key in ("p", "t", "speed", "stage")}})
    return out


def screen_time(line, seconds, alpha=0.5, v_min=0.3):
    """Screen time (s) at each vertex of `line`, 0 at the seed and `seconds` at the end."""
    p, v = line["p"].astype(float), np.maximum(line["speed"].astype(float), v_min)
    ds = np.linalg.norm(np.diff(p, axis=0), axis=1)
    dt = ds / np.maximum(0.5 * (v[1:] + v[:-1]), v_min) ** alpha
    tau = np.r_[0.0, np.cumsum(dt)]
    return tau / tau[-1] * seconds


def sample(line, tau, t):
    """(position mm, real time s, arc length mm) of `line` at the screen times `t`."""
    p = line["p"].astype(float)
    s = np.r_[0.0, np.cumsum(np.linalg.norm(np.diff(p, axis=0), axis=1))]
    pos = np.stack([np.interp(t, tau, p[:, k]) for k in range(3)], axis=1)
    return pos, np.interp(t, tau, line["t"].astype(float)), np.interp(t, tau, s)


def tumble(t, seed=SEED, rate=0.35, jitter=0.06):
    """Euler angles (rad) of the molecule at times `t`: a slow tumble about three axes at different rates, and thermal jitter as
    seeded noise (sampled at 8 Hz and interpolated), the same every run."""
    rng = np.random.default_rng(seed)
    base = rng.uniform(0, 2 * math.pi, 3)
    rates = rate * 2 * math.pi * rng.uniform(0.5, 1.5, 3) * rng.choice([-1, 1], 3)
    knots = np.arange(0, t[-1] + 1.0, 1 / 8)
    noise = [np.interp(t, knots, rng.normal(0, jitter, len(knots))) for _ in range(3)]
    return np.stack([base[k] + rates[k] * t + noise[k] for k in range(3)], axis=1)


def smooth(pos, follow=0.12):
    """`pos` low-pass filtered (each frame moves `follow` of the way to the target) for a camera that trails the molecule."""
    out = np.empty_like(pos)
    out[0] = pos[0]
    for k in range(1, len(pos)):
        out[k] = out[k - 1] + follow * (pos[k] - out[k - 1])
    return out


def line_of(points, speed):
    """A pathline dict (p, speed, real time t) from points (mm) and speeds (m/s): the time is the integral of ds / v."""
    ds = np.linalg.norm(np.diff(points, axis=0), axis=1) * 1e-3
    v = np.maximum(0.5 * (speed[1:] + speed[:-1]), 1e-3)
    return {"p": points, "speed": speed, "t": np.r_[0.0, np.cumsum(ds / v)]}


def timeline(segs, fps):
    """The molecule's frames over the film's segments, each a (line, seconds, alpha): a line is a `pathline`'s dict, or one point for a hold
    (the molecule stays). Returns (positions mm, real times s, the arc length each segment has been run at each frame, each segment's first
    frame (0-based)). A segment's last point is the next one's first, so the film is continuous."""
    pos, real, firsts, n = [], [], [], 0
    spans = []
    for k, (ln, secs, alpha) in enumerate(segs):
        nf = round(secs * fps) + (k == len(segs) - 1)
        t = np.arange(nf) / fps
        t = np.minimum(t, secs)
        if len(ln["p"]) == 1:
            p, rt, arc = np.repeat(ln["p"].astype(float), nf, axis=0), np.full(nf, float(ln["t"][0])), np.zeros(nf)
        else:
            p, rt, arc = sample(ln, screen_time(ln, secs, alpha), t)
        firsts.append(n)
        spans.append((n, n + nf, arc, ln))
        pos.append(p)
        real.append(rt)
        n += nf
    arcs = []
    for a, b, arc, ln in spans:
        total = float(np.sum(np.linalg.norm(np.diff(ln["p"].astype(float), axis=0), axis=1)))
        full = np.zeros(n)
        full[a:b] = arc
        full[b:] = total
        arcs.append(full)
    return np.concatenate(pos), np.concatenate(real), arcs, firsts


def build(glb: Path, lines_path: Path, out: Path, seconds=None, fps=24, alpha=0.5, scale=4e7, context=40, cut=True, flap_x=-37.5, tail_mm=15.0, fire=True):
    import bpy
    import bmesh  # noqa: I001 (bpy first: it provides bmesh)
    from mathutils import Vector

    from . import firebox, molecule
    from .cfd import pathlines

    secs = {**SECONDS, **(seconds or {})}
    bpy.ops.wm.read_factory_settings(use_empty=True)
    bpy.ops.import_scene.gltf(filepath=str(glb))
    tube_end = bpy.data.objects["air.7.tube_end"].matrix_world.translation.x * 1e3
    lines, meta = pathlines.load(lines_path)
    lines = trim(lines, tube_end + tail_mm)
    pk = pick(lines)
    line = lines[pk]
    segs = [(line, secs["cfd"], alpha)]
    lay = None
    if fire:  # the egg: the air goes on from the tube's end to the coal, the carbon meets it, the CO2 rises to the meat
        lay = firebox.layout(tube_end)
        p_in, v_in = firebox.approach(lay, line["p"][-1].astype(float), float(line["speed"][-1]))
        p_up, v_up = firebox.rise(lay)
        t0 = float(line["t"][-1])
        approach, rise = line_of(p_in, v_in), line_of(p_up, v_up)
        approach["t"] += t0
        rise["t"] += t0 + approach["t"][-1]
        stop = {"p": p_in[-1:], "speed": v_in[-1:], "t": approach["t"][-1:]}
        segs += [(approach, secs["approach"], 0.3), (stop, secs["combustion"], 0.0), (rise, secs["rise"], 0.0),
                 ({"p": p_up[-1:], "speed": v_up[-1:], "t": rise["t"][-1:]}, secs["bump"], 0.0)]
    pos, real, arcs, firsts = timeline(segs, fps)
    n = len(pos)
    t = np.arange(n) / fps
    rotor_angle = meta["omega_rad_s"] * real

    sc = bpy.context.scene
    sc.render.fps = fps
    sc.frame_start, sc.frame_end = 1, n
    if fire:
        objs = firebox.build(sc, lay)

    def smooth_shaded(o):
        bm = bmesh.new()
        bm.from_mesh(o.data)
        for e in bm.edges:
            e.smooth = not (len(e.link_faces) == 2 and e.calc_face_angle(0.0) > math.radians(35))
        for f in bm.faces:
            f.smooth = True
        bm.to_mesh(o.data)
        bm.free()

    if cut:  # a cutter box removes mech y < 0; every part is cut once, here, except the rotor which keeps its modifier live (it spins)
        cutter = bpy.data.objects.new("cutter", bpy.data.meshes.new("cutter"))
        bm = bmesh.new()
        bmesh.ops.create_cube(bm, size=4.0)
        bm.to_mesh(cutter.data)
        bm.free()
        sc.collection.objects.link(cutter)
        cutter.location = (0, -2.0, 0)
        cutter.hide_render = cutter.hide_viewport = True
        dg = bpy.context.evaluated_depsgraph_get()
        helpers = [o for o in sc.objects if o.hide_render]  # the cutter, the egg's port and the grate's holes cut nothing themselves
        for o in [o for o in sc.objects if o.type == "MESH" and o not in helpers]:
            bm = bmesh.new()
            bm.from_mesh(o.data)
            bmesh.ops.remove_doubles(bm, verts=bm.verts, dist=1e-7)
            bm.to_mesh(o.data)
            bm.free()
            m = o.modifiers.new("cut", "BOOLEAN")
            m.operation, m.object, m.solver = "DIFFERENCE", cutter, "EXACT"
            m.use_self = m.use_hole_tolerant = True
            if o.name != "fan_rotor":
                dg.update()
                o.data = bpy.data.meshes.new_from_object(o.evaluated_get(dg))
                o.modifiers.clear()
            smooth_shaded(o)

    rotor = bpy.data.objects["fan_rotor"]
    rotor.rotation_mode = "XYZ"
    for f in range(n):
        rotor.rotation_euler = (rotor_angle[f], 0.0, 0.0)
        rotor.keyframe_insert("rotation_euler", frame=f + 1)

    def material(name, colour, rough=0.4, metal=0.0, emit=0.0):
        m = bpy.data.materials.new(name)
        m.use_nodes = True
        b = m.node_tree.nodes["Principled BSDF"]
        b.inputs["Base Color"].default_value = (*colour, 1)
        b.inputs["Roughness"].default_value, b.inputs["Metallic"].default_value = rough, metal
        if emit:
            b.inputs["Emission Color"].default_value = (*colour, 1)
            b.inputs["Emission Strength"].default_value = emit
        return m

    # the molecule, tumbling slowly and jittering; at the coal it turns to lie along x for the reaction, and takes up the tumble after
    parts = molecule.build(sc, scale)
    mol = parts["root"]
    raw = tumble(t)
    eul = raw.copy()
    if fire:
        f_arrive, f_leave = firsts[2], firsts[3]
        target = 2 * math.pi * np.round(raw[f_arrive] / (2 * math.pi))
        ramp = fps
        w = np.ones(n)
        w[f_arrive:f_leave] = 0.0
        w[f_arrive - ramp : f_arrive] = np.linspace(1, 0, ramp)
        w[f_leave : f_leave + ramp] = np.linspace(0, 1, ramp)
        eul = target + w[:, None] * (raw - target)
    for f in range(n):
        mol.location = pos[f] * 1e-3
        mol.rotation_euler = eul[f]
        mol.keyframe_insert("location", frame=f + 1)
        mol.keyframe_insert("rotation_euler", frame=f + 1)

    # the paths: each segment's line draws itself behind the molecule, a short bright trail; the CFD's neighbours are faint context
    def curve(name, polys, depth, mat):
        cu = bpy.data.curves.new(name, "CURVE")
        cu.dimensions, cu.bevel_depth, cu.bevel_resolution, cu.resolution_u = "3D", depth, 2, 1
        cu.bevel_factor_mapping_start = cu.bevel_factor_mapping_end = "SPLINE"
        for q in polys:
            sp = cu.splines.new("POLY")
            sp.points.add(len(q) - 1)
            for pt, xyz in zip(sp.points, q * 1e-3):
                pt.co = (*xyz, 1.0)
        o = bpy.data.objects.new(name, cu)
        o.data.materials.append(mat)
        sc.collection.objects.link(o)
        return o

    glow = material("trail", (0.35, 0.75, 1.0), 0.5, emit=3.0)
    faint = material("context", (0.35, 0.6, 0.9), 0.5, emit=0.25)
    for name, (ln, _, _), arc in zip(("trail_room_to_tube", "trail_to_coal", "", "trail_rise", ""), segs, arcs):
        if not name:
            continue
        total = arc[-1]
        tr = curve(name, [ln["p"].astype(float)], 0.0007, glow)
        for f in range(n):
            done = arc[f] >= total and f > 0 and arc[f - 1] >= total
            tr.data.bevel_factor_end = 1.0 if done else arc[f] / total
            tr.data.bevel_factor_start = 1.0 if done else max(0.0, arc[f] - 25.0) / total  # 25 mm of tail, gone once the molecule has passed
            tr.data.keyframe_insert("bevel_factor_end", frame=f + 1)
            tr.data.keyframe_insert("bevel_factor_start", frame=f + 1)
    near = np.argsort([np.linalg.norm(ln["p"][0, 1:] - line["p"][0, 1:]) for ln in lines])[1 : context + 1]
    curve("context", [lines[i]["p"].astype(float) for i in near], 0.00010, faint)

    # the reaction at the coal
    if fire:
        f0, f1 = firsts[2] + 12, firsts[3] - 12
        molecule.chemistry(parts, scale, f0 + 1, f1 + 1, fps, firebox.GAP * 1e-3 / (scale * molecule.ANGSTROM))
        f_bond = f0 + int(0.4 * (f1 - f0))

    # cameras: the wide one sees the whole cut, the follow one trails the molecule, the flap one watches the flap
    bb = np.array([[*(o.matrix_world @ Vector(c))] for o in sc.objects if o.type == "MESH" and not o.hide_render and not o.name.startswith(("kamado", "fire", "grate", "coals", "hero", "grid", "brisket")) for c in o.bound_box])
    centre, span = (bb.min(0) + bb.max(0)) / 2, np.ptp(bb, axis=0).max()

    def camera(name, loc, target, lens=50, dof=None):
        cam = bpy.data.objects.new(name, bpy.data.cameras.new(name))
        cam.data.lens = lens
        sc.collection.objects.link(cam)
        cam.location = loc
        tr = cam.constraints.new("TRACK_TO")
        tr.target, tr.track_axis, tr.up_axis = target, "TRACK_NEGATIVE_Z", "UP_Y"
        if dof:
            cam.data.dof.use_dof, cam.data.dof.focus_object, cam.data.dof.aperture_fstop = True, target, dof
        return cam

    def empty(name, loc):
        e = bpy.data.objects.new(name, None)
        e.location = loc
        sc.collection.objects.link(e)
        return e

    mid = empty("mid", centre)
    camera("wide", centre + Vector((0.03, -span * 1.5, span * 0.4)), mid, lens=40)
    cam_path = smooth(pos * 1e-3) + np.array([0.01, -0.10, 0.04])
    follow = camera("follow", cam_path[0], mol, lens=50, dof=32.0)
    for f in range(n):
        follow.location = cam_path[f]
        follow.keyframe_insert("location", frame=f + 1)
    camera("flap", Vector((flap_x * 1e-3 + 0.01, -0.09, 0.04)), empty("flap_target", (flap_x * 1e-3, 0, 0)), lens=50, dof=32.0)
    gx = bpy.data.objects["air.0.grille"].matrix_world.translation
    intake = camera("intake", gx + Vector((-0.10, -0.07, 0.05)), empty("grille_target", gx), lens=35)
    # the cuts: the molecule starts outside the gills, is followed through the fan, the flap gets its close-up as it passes, and the
    # last shot of the machine pulls back once it is in the tube
    cuts = []
    for name, x_mm in SHOTS:
        x_mm = tube_end - 10.0 if x_mm is None else x_mm
        cuts.append((name, 1 + int(np.argmax(pos[: firsts[1] if fire else n, 0] >= x_mm))))  # the first frame past it (the first frame if none)
    if fire:
        x0 = lay["x0"] * 1e-3
        camera("egg", Vector((x0 + 0.05, -1.5, 0.32)), empty("egg_target", (x0 - 0.02, 0, 0.26)), lens=40)
        hero = Vector(lay["hero"] * 1e-3)
        camera("coal", hero + Vector((0.0, -0.15, -0.022)), empty("coal_target", hero + Vector((0.0, 0, -0.022))), lens=50, dof=24.0)
        upx = lay["up"][0] * 1e-3
        camera("plume", Vector((upx + 0.02, -0.26, 0.21)), empty("plume_target", (upx, 0, 0.21)), lens=45, dof=24.0)
        cuts += [("egg", firsts[1] + 1), ("coal", firsts[1] + 1 + int(0.55 * (firsts[2] - firsts[1]))), ("plume", firsts[3] + 1)]
    for name, frame in cuts:
        sc.timeline_markers.new(name, frame=frame).camera = bpy.data.objects[name]
    sc.camera = intake

    # light: a big soft key from the camera's side, a rim from behind, a dark studio world; a big one over the egg, and the coal's own glow
    lights = [("key", (-0.1, -0.35, 0.3), 25, 0.3, mid), ("rim", (0.15, 0.2, 0.25), 15, 0.25, mid)]
    if fire:
        lights += [("egg_key", (x0 - 0.1, -1.1, 0.9), 110, 0.8, bpy.data.objects["egg_target"]), ("egg_rim", (x0 + 0.5, 0.8, 0.7), 50, 0.6, bpy.data.objects["egg_target"])]
    for name, loc, power, size, aim in lights:
        li = bpy.data.objects.new(name, bpy.data.lights.new(name, "AREA"))
        li.data.energy, li.data.size = power, size
        li.location = loc
        tr = li.constraints.new("TRACK_TO")
        tr.target, tr.track_axis, tr.up_axis = aim, "TRACK_NEGATIVE_Z", "UP_Y"
        sc.collection.objects.link(li)
    if fire:
        ember = bpy.data.objects.new("ember_light", bpy.data.lights.new("ember_light", "POINT"))
        ember.data.shadow_soft_size, ember.data.color = 0.01, (1.0, 0.35, 0.08)
        ember.location = hero + Vector((0, -0.02, -0.02))
        sc.collection.objects.link(ember)
        molecule.flicker(bpy.data.materials["ember"], firsts[1] + 1, firsts[3] + 1, 0.6, 5.0, f_bond + 1, light=ember.data, watts=0.1)
    w = bpy.data.worlds.new("studio")
    w.use_nodes = True
    w.node_tree.nodes["Background"].inputs[0].default_value = (0.02, 0.025, 0.035, 1)
    w.node_tree.nodes["Background"].inputs[1].default_value = 1.0
    sc.world = w
    sc.view_settings.exposure = -0.8  # AgX (the default), the whites of the cut sections held under clipping
    sc["pathline"] = int(pk)
    sc["scale"] = scale

    out.parent.mkdir(parents=True, exist_ok=True)
    bpy.ops.wm.save_as_mainfile(filepath=str(out))
    return sc


def settings(sc, samples, res):
    """Cycles on the GPU if there is one (Metal here; CPU otherwise), denoised, at `res` and `samples` a pixel."""
    import bpy

    sc.render.engine = "CYCLES"
    sc.cycles.samples, sc.cycles.use_denoising = samples, True
    prefs = bpy.context.preferences.addons["cycles"].preferences
    try:
        prefs.compute_device_type = "METAL"
        prefs.get_devices()
        sc.cycles.device = "GPU"
    except Exception:
        pass  # CPU
    sc.render.resolution_x, sc.render.resolution_y = res


def hold(sc, shot):
    """Film `shot` throughout, whatever the timeline's markers say."""
    import bpy

    for mk in sc.timeline_markers:
        mk.camera = None
    sc.camera = bpy.data.objects[shot]


def still(sc, path: Path, frame):
    import bpy

    sc.frame_set(frame)
    sc.render.filepath = str(path)
    bpy.ops.render.render(write_still=True)


def video(sc, path: Path, frames=None, step=1):
    """The animation (frames `frames` = (first, last), every `step`th) as an H.264 mp4."""
    import bpy

    if frames:
        sc.frame_start, sc.frame_end = frames
    sc.frame_step = step
    sc.render.fps = max(1, round(sc.render.fps / step))
    sc.render.image_settings.media_type = "VIDEO"
    sc.render.image_settings.file_format = "FFMPEG"
    sc.render.ffmpeg.format, sc.render.ffmpeg.codec, sc.render.ffmpeg.constant_rate_factor = "MPEG4", "H264", "MEDIUM"
    sc.render.filepath = str(path.with_suffix(""))
    bpy.ops.render.render(animation=True)
    for made in path.parent.glob(f"{path.stem}[0-9][0-9][0-9][0-9]-[0-9][0-9][0-9][0-9].mp4"):  # Blender adds the frame range
        made.replace(path)


def main():
    ap = argparse.ArgumentParser(description="the molecule scene as a .blend, stills of it and the film")
    ap.add_argument("--glb", default="build/mech/assembly.glb", type=Path)
    ap.add_argument("--lines", default="build/pathlines/pathlines-flap0.npz", type=Path, help="pathlines from `just pathlines`")
    ap.add_argument("--out", default="build/scene/scene.blend", type=Path)
    ap.add_argument("--seconds", type=float, help="screen time of the CFD part of the path (the room to the tube)")
    ap.add_argument("--no-fire", action="store_true", help="the machine only: no egg, coal or meat")
    ap.add_argument("--alpha", type=float, default=0.5, help="re-timing: 0 constant screen speed .. 1 real time scaled")
    ap.add_argument("--scale", type=float, default=4e7, help="the molecule's size over life size")
    ap.add_argument("--no-cut", action="store_true", help="skip the cutaway (it takes about a minute)")
    ap.add_argument("--still", type=int, nargs="+", metavar="FRAME", help="render these frames beside --out")
    ap.add_argument("--shot", choices=("intake", "wide", "follow", "flap", "egg", "coal", "plume"), help="one camera throughout (default: the film's cuts)")
    ap.add_argument("--video", action="store_true", help="render the film to an mp4 beside --out")
    ap.add_argument("--frames", type=int, nargs=2, metavar=("FIRST", "LAST"), help="with --video: only these frames")
    ap.add_argument("--preview", action="store_true", help="640 x 360, 8 samples, every other frame at half the rate")
    ap.add_argument("--samples", type=int, help="a pixel (default 64, 8 for a preview)")
    args = ap.parse_args()
    sc = build(args.glb, args.lines, args.out, {"cfd": args.seconds} if args.seconds else None, alpha=args.alpha, scale=args.scale, cut=not args.no_cut, fire=not args.no_fire)
    print(f"wrote {args.out}: pathline {sc['pathline']}, {sc.frame_end} frames, cuts {[(m.name, m.frame) for m in sc.timeline_markers]}")
    settings(sc, args.samples or (8 if args.preview else 64), (640, 360) if args.preview else (1920, 1080))
    if args.shot:
        hold(sc, args.shot)
    for f in args.still or []:
        png = args.out.with_name(f"{args.shot or 'film'}-{f:04d}.png")
        still(sc, png, f)
        print(f"wrote {png}")
    if args.video:
        mp4 = args.out.with_suffix(".mp4")
        video(sc, mp4, args.frames, 2 if args.preview else 1)
        print(f"wrote {mp4}")
