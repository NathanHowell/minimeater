"""The raytraced animation's scene: `assembly.glb`, cut in half through the flow axis, with one O2 molecule riding a CFD pathline
(`uv run --group scene mech-scene`; bpy is in the `scene` group, ADR 0007). Saves a .blend to hand-tune and can render stills.

Blender's frame is the mech frame (x along the flow, z up) in metres: the glTF importer undoes the export's y-up turn. The
pathlines (`cfd.pathlines`) are mm in the same frame.

Time. A pathline's own clock runs about 35 ms from the room to the tube, 10x faster in the rotor than in the plenum, so the film
re-times it: screen time advances by ds / speed^ALPHA (ALPHA 0 is constant screen speed, 1 is real time scaled), normalised to
the shot's length. In the rotor's zone screen time is a fixed multiple of real time and the rotor turns at a fixed rate on screen (omega times that
multiple), so the molecule, and every gas parcel (swarm.py), passes the blades as it did in the CFD.
The molecule is ball-and-stick, SCALE times life size (an O=O bond is 1.21 A), because at life size it is under a billionth of a
metre; the film says so."""

import argparse
import json
import math
from pathlib import Path

import numpy as np

from . import swarm

ANGSTROM = 1e-10
BOND, BALL, STICK, DOUBLE = 1.21, 0.4, 0.09, 0.2  # O=O bond length, ball radius, stick radius, the two sticks' offset (A)
SEED = 7
SECONDS = {"cfd": 22.0, "approach": 8.0, "combustion": 9.0, "rise": 8.0, "bump": 3.0}  # screen time of each part of the film
# (camera, the x (mm) at which the film cuts to it; None is 10 mm before the tube's end): the molecule starts in the room
EXPOSURE = {"egg": -1.2, "plume": -1.6}  # a shot's exposure where it differs from the scene's -0.8 (AgX): the egg's whites are bright
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
    tau = swarm.raw_clock(line, alpha, v_min)
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


def build(glb: Path, lines_path: Path, out: Path, seconds=None, fps=24, alpha=0.5, scale=4e7, cut=True, flap_x=-37.5, tail_mm=15.0, fire=True):
    import bpy
    import bmesh  # noqa: I001 (bpy first: it provides bmesh)
    from mathutils import Vector

    from . import firebox, molecule, smoke
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
    omega = meta["omega_rad_s"]
    k1 = swarm.raw_clock(line, alpha)[-1] / secs["cfd"]
    omega_f = swarm.rotor_rate(omega, k1, alpha)  # the rotor turns at a fixed rate on screen: screen time in its zone is a multiple of real time
    in_h = swarm.entry(line, screen_time(line, secs["cfd"], alpha))
    theta0 = omega * in_h[1] - omega_f * in_h[0]  # the molecule meets the blades where the CFD had them
    rotor_angle = theta0 + omega_f * np.arange(n) / fps

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

    # the paths: each segment's line draws itself behind the molecule, a short bright trail
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

    # the gas: parcels on the CFD's lines (and the molecule's way on into the egg), and a plume of them rising off the coal. The molecule,
    # the CO2 and the flavour particles are individuals picked out of these flows
    reach = [ln for i, ln in enumerate(lines) if i != pk and ln["p"][-1, 0] >= tube_end]
    if fire:
        k2 = swarm.raw_clock(approach, 0.3)[-1] / secs["approach"]
        chains = [swarm.chain(ln, p_in, v_in, line["p"][-1].astype(float), k1, k2, alpha) for ln in reach]
        stop_pt = p_in[-1]
    else:
        chains = [(ln["p"].astype(float), ln["speed"], swarm.raw_clock(ln, alpha) / k1, swarm.entry(ln, swarm.raw_clock(ln, alpha) / k1)) for ln in reach]
        stop_pt = line["p"][-1].astype(float)
    xyz, period, per = swarm.air(chains, n, fps, rotor=(omega, omega_f, theta0))
    swarm.write_pc2(out.parent / "gas-air.pc2", xyz)
    n_air = len(chains) * per
    swarm.build(sc, "gas", out.parent / "gas-air.pc2", n_air, n, 0.3, ((meta["seed_plane_mm"], 0.0, 0.0), (1.0, 0.0, 0.0), 8.0), (stop_pt, 6.0, 22.0), vmax=8.0, glow=1.2)
    del xyz
    print(f"gas: {n_air} parcels from {len(chains)} lines, a loop of {period:.1f} s ({period * abs(omega_f) / (2 * math.pi):.0f} rotor turns of {2 * math.pi / abs(omega_f):.2f} s)")

    # the reaction at the coal
    if fire:
        f0, f1 = firsts[2] + 12, firsts[3] - 12
        smoke.build(sc, p_up, arcs[3], lay["plume_end"], fps, objs["brisket"], firsts[4] + 1)
        xyz = swarm.plume(p_up, v_up, firsts[3], n, fps, secs["rise"])
        swarm.write_pc2(out.parent / "gas-plume.pc2", xyz)
        tang = p_up[min(20, len(p_up) - 1)] - p_up[0]
        swarm.build(sc, "gas_plume", out.parent / "gas-plume.pc2", xyz.shape[1] // (swarm.TAIL + 1), n, 0.35,
                    (p_up[0], tuple(tang / np.linalg.norm(tang)), 20.0), (lay["plume_end"], 8.0, 25.0), swarm.SMOKE_COLOURS, vmax=1.0, glow=0.5)
        molecule.chemistry(parts, scale, f0 + 1, f1 + 1, fps, firebox.GAP * 1e-3 / (scale * molecule.ANGSTROM))
        f_bond = f0 + int(0.4 * (f1 - f0))

    # cameras: the wide one sees the whole cut, the follow one trails the molecule, the flap one watches the flap
    bb = np.array([[*(o.matrix_world @ Vector(c))] for o in sc.objects if o.type == "MESH" and not o.hide_render and not o.name.startswith(("kamado", "fire", "grate", "coals", "hero", "grid", "brisket")) for c in o.bound_box])
    centre, span = (bb.min(0) + bb.max(0)) / 2, np.ptp(bb, axis=0).max()

    def camera(name, loc, target, lens=50, dof=None):
        cam = bpy.data.objects.new(f"cam_{name}", bpy.data.cameras.new(name))  # not `name`: the glb has a part called flap
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
    camera("intake", gx + Vector((-0.10, -0.07, 0.05)), empty("grille_target", gx), lens=35)
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
        sc.timeline_markers.new(name, frame=frame).camera = bpy.data.objects[f"cam_{name}"]
    sc.camera = bpy.data.objects["cam_intake"]
    shot_at = dict(cuts)
    if fire:
        beacon(sc, mol, 22.0, (shot_at["wide"], shot_at["coal"] - 1))  # the molecule is 5 mm in the egg shot
    sc.view_settings.exposure = -0.8
    last = -0.8
    for name, frame in sorted(cuts, key=lambda c: c[1]):
        want = EXPOSURE.get(name, -0.8)
        if want != last:
            for f, v in ((max(1, frame - 12), last), (frame, want)):
                sc.view_settings.exposure = v
                sc.view_settings.keyframe_insert("exposure", frame=f)
            last = want
    sc.view_settings.exposure = -0.8
    if fire:
        sc["captions"] = json.dumps([
            [f"The molecule is drawn {scale:.0e} times life size".replace("e+0", " x 10^").replace("e+", " x 10^"), 1, n, 0],
            ["CFD: OpenFOAM, 4.2 CFM, 3000 rpm", 1, firsts[1], 1],
            ["Illustrative: the egg and the air's way in are not CFD", firsts[1] + 1, n, 1],
            ["C + O2 -> CO2: the oxygen bonds to a carbon of the coal", firsts[2] + 1, firsts[3], 2],
            ["Smoke flavour is wood chemistry, not CO2: it rides the plume", firsts[3] + 1, n, 2],
        ])
    bloom(sc)

    # light: a big soft key from the camera's side, a rim from behind, a dark studio world; a big one over the egg, and the coal's own glow
    lights = [("key", (-0.1, -0.35, 0.3), 25, 0.3, mid), ("rim", (0.15, 0.2, 0.25), 15, 0.25, mid)]
    if fire:
        lights += [("egg_key", (x0 - 0.1, -1.1, 0.9), 55, 0.8, bpy.data.objects["egg_target"]), ("egg_rim", (x0 + 0.5, 0.8, 0.7), 25, 0.6, bpy.data.objects["egg_target"])]
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
        xyz = swarm.sparks(lay["hero"], lay["hero_r"], firsts[1], firsts[3], f_bond, n, fps)
        swarm.write_pc2(out.parent / "gas-sparks.pc2", xyz)
        hr, up_ = lay["hero_r"], np.array([0.0, 0.0, 1000.0])
        swarm.build(sc, "embers", out.parent / "gas-sparks.pc2", xyz.shape[1] // (swarm.TAIL + 1), n, 0.6, (lay["hero"] - up_, (0.0, 0.0, 1.0), 1.0),
                    (lay["hero"], hr - 7.0, hr - 0.5), swarm.SPARK_COLOURS, vmax=1.0, glow=8.0)
        shimmer(sc, lay["hero"] - [0.0, 0.0, 40.0], 150.0, 55.0, fps, n)
    w = bpy.data.worlds.new("studio")
    w.use_nodes = True
    w.node_tree.nodes["Background"].inputs[0].default_value = (0.02, 0.025, 0.035, 1)
    w.node_tree.nodes["Background"].inputs[1].default_value = 1.0
    sc.world = w
    sc["pathline"] = int(pk)
    sc["scale"] = scale

    out.parent.mkdir(parents=True, exist_ok=True)
    bpy.ops.wm.save_as_mainfile(filepath=str(out))
    return sc


def shimmer(sc, base_mm, height_mm, radius_mm, fps, n):
    """The heat shimmer: a column of glass over the coal whose normals are a noise that drifts upward, so what is behind it wobbles. Only
    camera and refraction rays see it (it casts no shadow and takes no light), and it thins to nothing at its sides and ends."""
    import bmesh
    import bpy

    mm = 1e-3
    me = bpy.data.meshes.new("shimmer")
    bm = bmesh.new()
    bmesh.ops.create_cone(bm, cap_ends=True, segments=48, radius1=radius_mm * mm, radius2=radius_mm * mm, depth=height_mm * mm)
    bm.to_mesh(me)
    bm.free()
    ob = bpy.data.objects.new("shimmer", me)
    sc.collection.objects.link(ob)
    ob.location = (base_mm[0] * mm, base_mm[1] * mm, (base_mm[2] + height_mm / 2) * mm)
    ob.visible_shadow = ob.visible_glossy = ob.visible_diffuse = False
    m = bpy.data.materials.new("shimmer")
    m.use_nodes = True
    nt = m.node_tree
    nt.nodes.clear()
    coord, mapping, noise = nt.nodes.new("ShaderNodeTexCoord"), nt.nodes.new("ShaderNodeMapping"), nt.nodes.new("ShaderNodeTexNoise")
    noise.inputs["Scale"].default_value, noise.inputs["Detail"].default_value = 30.0, 3.0
    bump, glass, clear, mix, out = (nt.nodes.new(k) for k in ("ShaderNodeBump", "ShaderNodeBsdfGlass", "ShaderNodeBsdfTransparent", "ShaderNodeMixShader", "ShaderNodeOutputMaterial"))
    bump.inputs["Strength"].default_value, bump.inputs["Distance"].default_value = 1.0, 0.007
    glass.inputs["IOR"].default_value, glass.inputs["Roughness"].default_value = 1.1, 0.0
    nt.links.new(coord.outputs["Object"], mapping.inputs["Vector"])
    nt.links.new(mapping.outputs["Vector"], noise.inputs["Vector"])
    nt.links.new(noise.outputs["Fac"], bump.inputs["Height"])
    nt.links.new(bump.outputs["Normal"], glass.inputs["Normal"])
    for f, z in ((1, 0.0), (n, -0.25 * n / fps)):  # a quarter metre a second upward through the noise
        mapping.inputs["Location"].default_value = (0, 0, z)
        mapping.inputs["Location"].keyframe_insert("default_value", frame=f)
    # the weight of the glass: 1 where the wall faces the camera, 0 at the column's edge-on sides, and 0 at its capped ends
    sep = nt.nodes.new("ShaderNodeSeparateXYZ")
    nt.links.new(coord.outputs["Object"], sep.inputs[0])

    def ramp(src, lo, hi, a, b):
        r = nt.nodes.new("ShaderNodeMapRange")
        r.interpolation_type, r.clamp = "SMOOTHSTEP", True
        for key, v in (("From Min", lo), ("From Max", hi), ("To Min", a), ("To Max", b)):
            r.inputs[key].default_value = v
        nt.links.new(src, r.inputs["Value"])
        return r.outputs["Result"]

    lw = nt.nodes.new("ShaderNodeLayerWeight")
    lw.inputs["Blend"].default_value = 0.5
    side = ramp(lw.outputs["Facing"], 0.0, 0.6, 1.0, 0.0)
    absz = nt.nodes.new("ShaderNodeMath")
    absz.operation = "ABSOLUTE"
    nt.links.new(sep.outputs["Z"], absz.inputs[0])
    ends = ramp(absz.outputs[0], 0.5 * height_mm / 2 * mm, height_mm / 2 * mm, 1.0, 0.0)
    both = nt.nodes.new("ShaderNodeMath")
    both.operation = "MULTIPLY"
    nt.links.new(side, both.inputs[0])
    nt.links.new(ends, both.inputs[1])
    nt.links.new(both.outputs[0], mix.inputs["Fac"])
    nt.links.new(clear.outputs[0], mix.inputs[1])
    nt.links.new(glass.outputs[0], mix.inputs[2])
    nt.links.new(mix.outputs[0], out.inputs["Surface"])
    me.materials.append(m)
    return ob


def beacon(sc, parent, radius_mm, shown):
    """A soft glowing shell round the molecule (an edge glow, clear in the middle) where it would be lost in the picture; rendered only
    between frames `shown` = (first, last)."""
    import bpy

    bpy.ops.mesh.primitive_uv_sphere_add(radius=radius_mm * 1e-3, segments=48, ring_count=24)
    ob = bpy.context.object
    ob.name = "beacon"
    ob.parent = parent
    ob.visible_shadow = False
    m = bpy.data.materials.new("beacon")
    m.use_nodes = True
    nt = m.node_tree
    nt.nodes.clear()
    fres, power, clear, emit, mix, out = (nt.nodes.new(k) for k in ("ShaderNodeLayerWeight", "ShaderNodeMath", "ShaderNodeBsdfTransparent", "ShaderNodeEmission", "ShaderNodeMixShader", "ShaderNodeOutputMaterial"))
    fres.inputs["Blend"].default_value = 0.3
    power.operation = "POWER"
    power.inputs[1].default_value = 1.6
    nt.links.new(fres.outputs["Facing"], power.inputs[0])
    emit.inputs["Color"].default_value, emit.inputs["Strength"].default_value = (1.0, 0.82, 0.4, 1.0), 6.0
    nt.links.new(power.outputs[0], mix.inputs["Fac"])
    nt.links.new(clear.outputs[0], mix.inputs[1])
    nt.links.new(emit.outputs[0], mix.inputs[2])
    nt.links.new(mix.outputs[0], out.inputs["Surface"])
    ob.data.materials.append(m)
    for f, v in ((1, True), (shown[0] - 1, True), (shown[0], False), (shown[1], False), (shown[1] + 1, True), (sc.frame_end, True)):
        ob.hide_render = v
        ob.keyframe_insert("hide_render", frame=f)
    return ob


def bloom(sc):
    """A glare on the render's brightest parts (the coal, the sparks, the beacon) in the compositor."""
    import bpy

    ng = bpy.data.node_groups.new("bloom", "CompositorNodeTree")
    ng.interface.new_socket("Image", in_out="OUTPUT", socket_type="NodeSocketColor")
    rl, gl, out = ng.nodes.new("CompositorNodeRLayers"), ng.nodes.new("CompositorNodeGlare"), ng.nodes.new("NodeGroupOutput")
    gl.inputs["Type"].default_value = "Bloom"
    gl.inputs["Threshold"].default_value, gl.inputs["Strength"].default_value, gl.inputs["Size"].default_value = 5.0, 0.5, 0.6
    ng.links.new(rl.outputs["Image"], gl.inputs["Image"])
    ng.links.new(gl.outputs["Image"], out.inputs[0])
    sc.compositing_node_group = ng


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
    sc.camera = bpy.data.objects[f"cam_{shot}"]


def still(sc, path: Path, frame):
    import bpy

    sc.frame_set(frame)
    sc.render.filepath = str(path)
    bpy.ops.render.render(write_still=True)


def encode(sc, path: Path):
    """Set `sc` to write an H.264 mp4 to `path` (Blender adds the frame range to the name: `finish` renames it)."""
    sc.render.image_settings.media_type = "VIDEO"
    sc.render.image_settings.file_format = "FFMPEG"
    sc.render.ffmpeg.format, sc.render.ffmpeg.codec, sc.render.ffmpeg.constant_rate_factor = "MPEG4", "H264", "MEDIUM"
    sc.render.filepath = str(path.with_suffix(""))


def finish(path: Path):
    for made in path.parent.glob(f"{path.stem}[0-9][0-9][0-9][0-9]-[0-9][0-9][0-9][0-9].mp4"):
        made.replace(path)


def video(sc, path: Path, frames=None, step=1):
    """The animation (frames `frames` = (first, last), every `step`th) as an H.264 mp4."""
    import bpy

    if frames:
        sc.frame_start, sc.frame_end = frames
    sc.frame_step = step
    sc.render.fps = max(1, round(sc.render.fps / step))
    encode(sc, path)
    bpy.ops.render.render(animation=True)
    finish(path)


def captions(raw: Path, out: Path, items, fps: int, step=1, first=1):
    """`raw` (the rendered film) with a caption strip along the bottom for each of `items` = (text, first film frame, last, row up from the bottom), in Blender's
    sequencer: no ray tracing, so captions can be reworded and re-laid in a few seconds. `step` is the video's frame step and `first` the film frame it starts at."""
    import bpy

    bpy.ops.wm.read_factory_settings(use_empty=True)
    sc = bpy.context.scene
    sc.render.fps = fps
    sc.view_settings.view_transform = "Standard"  # the film has been through AgX already
    sc.sequencer_colorspace_settings.name = "sRGB"
    sed = sc.sequence_editor_create()
    mov = sed.strips.new_movie("film", str(raw), 1, 1)
    mov.colorspace_settings.name = "sRGB"
    sc.render.resolution_x, sc.render.resolution_y, sc.render.resolution_percentage = mov.elements[0].orig_width, mov.elements[0].orig_height, 100
    sc.frame_start, sc.frame_end = 1, mov.frame_final_duration
    for k, (text, a, b, row) in enumerate(items):
        a, b = max(1, (a - first) // step + 1), min((b - first) // step + 1, sc.frame_end)
        if b < 1 or a > sc.frame_end:
            continue
        st = sed.strips.new_effect(f"caption{k}", "TEXT", 2 + k, a, length=max(1, b - a + 1))  # a channel each
        st.text, st.font_size = text, int(0.034 * sc.render.resolution_y)
        st.color, st.use_box, st.box_color, st.box_margin = (1, 1, 1, 1), True, (0, 0, 0, 0.45), 0.012
        st.anchor_x, st.anchor_y, st.location = "CENTER", "BOTTOM", (0.5, 0.03 + 0.075 * row)
    encode(sc, out)
    bpy.ops.render.render(animation=True)
    finish(out)


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
        mp4, raw = args.out.with_suffix(".mp4"), args.out.with_suffix(".raw.mp4")
        step = 2 if args.preview else 1
        items = json.loads(sc["captions"]) if "captions" in sc else []
        fps = max(1, round(sc.render.fps / step))
        video(sc, raw, args.frames, step)
        captions(raw, mp4, items, fps, step, args.frames[0] if args.frames else 1)
        print(f"wrote {mp4} (the uncaptioned render is {raw})")
