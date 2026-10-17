"""The gas as a flow (Blender, `film.py`): hundreds of anonymous parcels move along the CFD's own pathlines, so the molecule is one
individual among them. Each parcel is a comet, a head and a tail that is its last fraction of a second, so a fast parcel has a long
tail and a slow one a short one.

Same clock for every parcel. The film re-times the molecule's line by ds / speed^alpha; here every line is re-timed by that law with
the molecule's constant (screen seconds per unit of ds / speed^alpha), so a parcel's screen speed is a function of the local speed alone
and the parcels on neighbouring lines move in step with it. A stream is steady: a parcel at phase `phi` is at `(t + phi) mod period` on
its line and the lines are out of step with each other, so the flow looks the same at any moment. A line's way on from the tube's end is
the molecule's hand-drawn one (`firebox.approach`) with the line's own lateral offset from the molecule's carried along.

The positions are baked to a .pc2 point cache (the vertex count must match the mesh: `parcels * TAIL` tail points then one carrier
point per parcel whose x is its speed); a Geometry Nodes modifier turns each parcel's chain of points into a tapered tube."""

import math
from pathlib import Path

import numpy as np

V_REF = 8.0  # m/s: the speed at which the rotor zone's clock meets the outside's
TAIL = 8  # points a parcel's tail is drawn through
TAIL_DT = 0.07 # s of screen time between them


def write_pc2(path: Path, xyz: np.ndarray):
    """`xyz` (frames, points, 3) as a Point Cache 2 file starting at frame 1, one sample a frame."""
    path.parent.mkdir(parents=True, exist_ok=True)
    with open(path, "wb") as f:
        f.write(b"POINTCACHE2\0" + np.array([1, xyz.shape[1]], "<i4").tobytes() + np.array([1.0, 1.0], "<f4").tobytes() + np.array([xyz.shape[0]], "<i4").tobytes())
        f.write(np.ascontiguousarray(xyz, "<f4").tobytes())


def raw_clock(line, alpha: float, v_min: float = 0.3) -> np.ndarray:
    """The integral of ds / speed^alpha along `line` at each vertex (0 at the first), except in the rotor's zone (`stage` 1 at both ends of
    a step), where it is the line's own real time, scaled to meet the outside's law at `V_REF` m/s. There screen time is a fixed multiple of
    real time (`rotor_rate`), so the blades turn through the same angle on screen as in the CFD while a parcel crosses the zone."""
    p, v = line["p"].astype(float), np.maximum(line["speed"].astype(float), v_min)
    dt = np.linalg.norm(np.diff(p, axis=0), axis=1) / np.maximum(0.5 * (v[1:] + v[:-1]), v_min) ** alpha
    if line.get("stage") is not None and line.get("t") is not None:
        stage = np.asarray(line["stage"])
        zone = (stage[1:] == 1) & (stage[:-1] == 1)
        dt = np.where(zone, np.diff(line["t"].astype(float)) * 1e3 * V_REF ** (1 - alpha), dt)
    return np.r_[0.0, np.cumsum(dt)]


def rotor_rate(omega: float, k: float, alpha: float) -> float:
    """The rotor's angular speed on screen (rad/s): `omega` (rad/s, real) times the zone's real seconds per screen second, which is
    `k` (the clock's constant: raw units per screen second) scaled as `raw_clock` scales the zone's real time."""
    return omega * k * V_REF ** (alpha - 1) * 1e-3


def entry(line, tau):
    """(screen time, the line's own real time) at its first vertex in the rotor's zone, or None if it never enters."""
    stage = line.get("stage")
    inside = np.flatnonzero(np.asarray(stage) == 1) if stage is not None else []
    if not len(inside):
        return None
    return float(tau[inside[0]]), float(line["t"][inside[0]])


def chain(line, p_in, v_in, hero_end, k1, k2, alpha):
    """One line's whole way: the CFD line then the hand-drawn approach carrying the line's offset from the molecule's (taken along the
    approach's own frame: sideways in y, and across the path in its plane). Returns (points mm, speed m/s, screen time s at each point, `entry` of the line)."""
    d = line["p"][-1].astype(float) - hero_end
    tang = np.gradient(p_in, axis=0)
    tang /= np.linalg.norm(tang, axis=1)[:, None]
    nrm = np.cross(tang, [0.0, 1.0, 0.0])
    s = np.r_[0.0, np.cumsum(np.linalg.norm(np.diff(p_in, axis=0), axis=1))]
    u = np.clip(s / s[-1], 0, 1)
    g = 1 - 0.4 * u * u * (3 - 2 * u)  # the stream narrows a little going up through the grate's hole
    tail = p_in + g[:, None] * (d[1] * np.array([0.0, 1.0, 0.0]) + d[2] * nrm)
    t1 = raw_clock(line, alpha) / k1
    t2 = raw_clock({"p": tail, "speed": v_in}, 0.3) / k2 + t1[-1]
    return np.vstack([line["p"].astype(float), tail[1:]]), np.r_[line["speed"], v_in[1:]], np.r_[t1, t2[1:]], entry(line, t1)


def air(chains, n, fps, per=3, seed=3, rotor=None):
    """The stream's tracks: (n frames, parcels*TAIL + parcels, 3) with the tail points in metres and the parcels' speeds (m/s) at their
    heads in the carrier points' x, and the period of the loop.

    `rotor` = (omega, omega_f, theta0): the rotor's real and screen angular speeds and its angle at screen time 0. A line's points in the
    zone assume the rotor at omega * (the line's own time) there, so a parcel must reach the zone's entry when the film's rotor is at that
    angle: once a turn (2 pi / omega_f). The loop is a whole number of turns and a line's parcels are put at those entries, `per` of the
    turns evenly spread. Without `rotor`, or for a line that misses the zone, the phases are random."""
    rng = np.random.default_rng(seed)
    t = np.arange(n) / fps
    period = max(c[2][-1] for c in chains) + 1.0
    turn = 2 * math.pi / abs(rotor[1]) if rotor else None
    if rotor:
        turns = math.ceil(period / turn)
        period = turns * turn
        per = min(per, turns)
    out = np.zeros((n, len(chains) * per * (TAIL + 1), 3), np.float32)
    nm = len(chains) * per * TAIL
    for i, (p, v, tau, enter) in enumerate(chains):
        if rotor and enter:
            omega, omega_f, theta0 = rotor
            at = ((omega * enter[1] - theta0) / omega_f) % turn  # the screen times (mod a turn) the rotor is where the line expects it
            phi = (enter[0] - at + turn * (np.arange(per) * turns // per)) % period
        else:
            phi = (np.arange(per) + rng.uniform(0.1, 0.9, per)) / per * period
        u = (t[:, None] + phi[None, :]) % period  # (n, per)
        uk = np.maximum(u[:, :, None] - TAIL_DT * np.arange(TAIL), 0.0)  # (n, per, TAIL)
        pos = np.stack([np.interp(uk, tau, p[:, k]) for k in range(3)], axis=-1)  # (n, per, TAIL, 3)
        a = i * per * TAIL
        out[:, a : a + per * TAIL] = pos.reshape(n, per * TAIL, 3) * 1e-3
        out[:, nm + i * per : nm + (i + 1) * per, 0] = np.interp(u, tau, v)
    return out, period, per


def plume(path, speed, f_start, n, fps, secs, per_s=40.0, spread=(2.0, 11.0), seed=4):
    """The CO2 and smoke rising along `path` (mm, with `speed` m/s): parcels leave the start from frame `f_start` (0-based) at `per_s` a
    second, each taking about `secs` seconds (0.85 to 1.3 times as fast as the molecule) and spreading in a spiral that widens from
    `spread[0]` to `spread[1]` mm. Returns (n, parcels*TAIL + parcels, 3) as `air` does."""
    rng = np.random.default_rng(seed)
    t = np.arange(n) / fps
    t0 = f_start / fps
    count = int(per_s * (t[-1] - t0))
    start = t0 + np.sort(rng.uniform(0, t[-1] - t0, count))
    fast = rng.uniform(0.85, 1.3, count)
    r_w = np.sqrt(rng.uniform(0, 1, count))
    phi0 = rng.uniform(0, 2 * math.pi, count)
    turn = rng.uniform(0.5, 2.0, count) * rng.choice([-1, 1], count)
    s = np.r_[0.0, np.cumsum(np.linalg.norm(np.diff(path, axis=0), axis=1))]
    tang = np.gradient(path, axis=0)
    tang /= np.linalg.norm(tang, axis=1)[:, None]
    e1 = np.cross(tang, [0.0, 1.0, 0.0])
    e2 = np.cross(tang, e1)
    out = np.zeros((n, count * (TAIL + 1), 3), np.float32)
    nm = count * TAIL
    for j in range(count):
        tt = t[:, None] - TAIL_DT * np.arange(TAIL) - start[j]  # (n, TAIL) seconds since it left
        f = np.clip(tt * fast[j] / secs, 0.0, 1.0)
        arc = f * s[-1]
        rad = (spread[0] + (spread[1] - spread[0]) * f) * r_w[j]
        ang = phi0[j] + turn[j] * 2 * math.pi * f
        idx = lambda col: np.interp(arc, s, col)  # noqa: E731
        base = np.stack([idx(path[:, k]) for k in range(3)], axis=-1)
        a = np.stack([idx(e1[:, k]) for k in range(3)], axis=-1)
        b = np.stack([idx(e2[:, k]) for k in range(3)], axis=-1)
        out[:, j * TAIL : (j + 1) * TAIL] = (base + rad[..., None] * (np.cos(ang)[..., None] * a + np.sin(ang)[..., None] * b)) * 1e-3
        out[:, nm + j, 0] = np.interp(np.clip((t - start[j]) * fast[j] / secs, 0, 1) * s[-1], s, speed)
    return out


def sparks(centre, radius, f0, f1, f_peak, n, fps, seed=8, base=6.0, peak=40.0):
    """Embers thrown off the coal: sparks leave the lump (`centre` mm, `radius` mm) between frames f0 and f1 (0-based) at `base` a second,
    rising to `peak` at `f_peak`; each rises on a made-up buoyant track (50-110 mm/s up, 25 sideways, a sway) for 0.7-2.2 s and goes out.
    Returns (n, sparks*TAIL + sparks, 3) as `air` does, but the carrier's x is the spark's heat (1 new, 0 out): colour by it. A spark
    not yet born or already out is parked at `centre`, where the stream's fade (the lump's own radius) hides it."""
    rng = np.random.default_rng(seed)
    t = np.arange(n) / fps
    rate = np.where((t >= f0 / fps) & (t <= f1 / fps), np.interp(t, [f0 / fps, f_peak / fps, f1 / fps], [base, peak, base * 2]), 0.0)
    count = rng.poisson(rate / fps)
    start = np.concatenate([t[k] + rng.uniform(0, 1 / fps, c) for k, c in enumerate(count)])
    m = len(start)
    d = rng.normal(0, 1, (m, 3))
    d[:, 2] = np.maximum(d[:, 2] * 0.5 + 0.25, -0.2)  # off the sides and top, leaning up
    d /= np.linalg.norm(d, axis=1)[:, None]
    origin = np.asarray(centre, float) + (radius + 1.0) * d
    side = d[:, :2] / np.maximum(np.linalg.norm(d[:, :2], axis=1), 1e-6)[:, None]
    vel = np.c_[side * rng.uniform(5, 35, (m, 1)) + rng.normal(0, 8, (m, 2)), rng.uniform(50, 110, m)]  # up, and out from the lump
    life = rng.uniform(0.7, 2.2, m)
    sway, hz, ph = rng.uniform(1.0, 4.0, m), rng.uniform(2.0, 4.5, m), rng.uniform(0, 2 * math.pi, (m, 2))
    out = np.zeros((n, m * (TAIL + 1), 3), np.float32)
    nm = m * TAIL
    park = np.asarray(centre, float) * 1e-3
    for j in range(m):
        age = t - start[j]
        alive = (age >= 0) & (age <= life[j])
        ak = np.clip(age[:, None] - TAIL_DT * np.arange(TAIL), 0.0, life[j])  # (n, TAIL)
        pos = origin[j] + vel[j] * ak[..., None] + np.array([0.0, 0.0, 12.0]) * ak[..., None] ** 2  # buoyancy: 24 mm/s^2
        pos[..., 0] += sway[j] * (np.sin(2 * math.pi * hz[j] * ak + ph[j, 0]) - np.sin(ph[j, 0]))  # (from nothing at birth)
        pos[..., 1] += sway[j] * (np.sin(2 * math.pi * hz[j] * ak + ph[j, 1]) - np.sin(ph[j, 1]))
        pos = pos * 1e-3
        pos[~alive] = park
        out[:, j * TAIL : (j + 1) * TAIL] = pos
        out[:, nm + j, 0] = np.where(alive, 1 - np.clip(age / life[j], 0, 1), 0.0)
    return out


# the colour of a parcel by its speed: a CFD plot's blue to red
SPEED_COLOURS = ((0.0, (0.05, 0.12, 0.9)), (0.25, (0.0, 0.65, 1.0)), (0.5, (0.35, 1.0, 0.55)), (0.75, (1.0, 0.85, 0.1)), (1.0, (1.0, 0.25, 0.05)))

SPARK_COLOURS = ((0.0, (0.12, 0.015, 0.0)), (0.3, (0.8, 0.12, 0.01)), (0.65, (1.0, 0.45, 0.04)), (1.0, (1.0, 0.85, 0.45)))
SMOKE_COLOURS = ((0.0, (0.4, 0.42, 0.48)), (0.5, (0.7, 0.66, 0.6)), (1.0, (1.0, 0.8, 0.55)))


def build(sc, name, pc2: Path, parcels: int, frames: int, radius_mm: float, fade_in, fade_out, colours=SPEED_COLOURS, vmax=8.0, glow=2.0):
    """The object `name`: a mesh of `parcels` chains of TAIL points (and the carrier points) cached from `pc2` and drawn as comets
    `radius_mm` thick at the head. A parcel is faded to nothing at the stream's ends: `fade_in` = (point mm, direction, ramp mm) fades
    with the distance along `direction` from the point, `fade_out` = (point mm, inner mm, outer mm) with the distance from a point."""
    import bpy

    mm = 1e-3
    nm = parcels * TAIL
    mesh = bpy.data.meshes.new(name)
    mesh.from_pydata(np.zeros((nm + parcels, 3)).tolist(), [(i * TAIL + k, i * TAIL + k + 1) for i in range(parcels) for k in range(TAIL - 1)], [])
    ob = bpy.data.objects.new(name, mesh)
    sc.collection.objects.link(ob)
    mc = ob.modifiers.new("cache", "MESH_CACHE")
    mc.cache_format, mc.filepath, mc.interpolation = "PC2", str(Path(pc2).resolve()), "NONE"
    mc.time_mode, mc.play_mode, mc.frame_start = "FRAME", "SCENE", 1.0
    mc.forward_axis, mc.up_axis = "POS_Y", "POS_Z"

    ng = bpy.data.node_groups.new(f"{name}_comets", "GeometryNodeTree")
    ng.interface.new_socket("Geometry", in_out="INPUT", socket_type="NodeSocketGeometry")
    ng.interface.new_socket("Geometry", in_out="OUTPUT", socket_type="NodeSocketGeometry")
    nodes, links = ng.nodes, ng.links

    def node(kind, **kw):
        n = nodes.new(kind)
        for k, v in kw.items():
            setattr(n, k, v)
        return n

    def socket(n, label, out=False):
        return next(s for s in (n.outputs if out else n.inputs) if s.name == label and s.enabled)

    def link(a, b):
        links.new(a, b)

    def math_(op, a=None, b=None, **kw):
        n = node("ShaderNodeMath", operation=op, **kw)
        for k, x in enumerate((a, b)):
            if x is None:
                continue
            if isinstance(x, (int, float)):
                n.inputs[k].default_value = x
            else:
                link(x, n.inputs[k])
        return n.outputs[0]

    def vec(op, a, b=None):
        n = node("ShaderNodeVectorMath", operation=op)
        for k, x in enumerate((a, b)):
            if x is None:
                continue
            if isinstance(x, (tuple, list)):
                n.inputs[k].default_value = x
            else:
                link(x, n.inputs[k])
        return n

    def ramp(x, lo, hi):
        n = node("ShaderNodeMapRange", data_type="FLOAT", interpolation_type="SMOOTHSTEP", clamp=True)
        link(x, n.inputs["Value"])
        n.inputs["From Min"].default_value, n.inputs["From Max"].default_value = lo, hi
        return n.outputs["Result"]

    gin, gout = node("NodeGroupInput"), node("NodeGroupOutput")
    index, pos = node("GeometryNodeInputIndex").outputs[0], node("GeometryNodeInputPosition").outputs[0]
    # the head's speed, from the carrier point of this point's parcel
    which = math_("ADD", math_("FLOOR", math_("DIVIDE", index, float(TAIL))), float(nm))
    si = node("GeometryNodeSampleIndex", data_type="FLOAT_VECTOR", domain="POINT")
    link(gin.outputs[0], si.inputs["Geometry"])
    link(pos, socket(si, "Value"))
    link(which, si.inputs["Index"])
    speed = node("ShaderNodeSeparateXYZ")
    link(socket(si, "Value", out=True), speed.inputs[0])
    # 0 at the head to 1 at the tail's end
    along = math_("DIVIDE", math_("MODULO", index, float(TAIL)), float(TAIL - 1))
    # the fades: along `direction` from the start point, and by distance from the end point
    p0, d0, ramp_mm = fade_in
    f_in = ramp(vec("DOT_PRODUCT", vec("SUBTRACT", pos, tuple(np.asarray(p0) * mm)).outputs[0], tuple(d0)).outputs["Value"], 0.0, ramp_mm * mm)
    p1, r0, r1 = fade_out
    f_out = ramp(vec("DISTANCE", pos, tuple(np.asarray(p1) * mm)).outputs["Value"], r0 * mm, r1 * mm)
    fade = math_("MULTIPLY", f_in, f_out)
    geo = gin.outputs[0]
    for key, val in (("speed", speed.outputs[0]), ("tail", along), ("fade", fade)):
        st = node("GeometryNodeStoreNamedAttribute", data_type="FLOAT", domain="POINT")
        st.inputs["Name"].default_value = key
        link(geo, st.inputs["Geometry"])
        link(val, socket(st, "Value"))
        geo = st.outputs["Geometry"]
    mc_ = node("GeometryNodeMeshToCurve")
    link(geo, mc_.inputs["Mesh"])
    tail_f, fade_f = (node("GeometryNodeInputNamedAttribute", data_type="FLOAT") for _ in range(2))
    tail_f.inputs["Name"].default_value, fade_f.inputs["Name"].default_value = "tail", "fade"
    shape = math_("SQRT", math_("SUBTRACT", 1.0, math_("MULTIPLY", tail_f.outputs["Attribute"], tail_f.outputs["Attribute"])))  # a rounded head
    rad = math_("MULTIPLY", math_("MULTIPLY", shape, fade_f.outputs["Attribute"]), radius_mm * mm)
    prof = node("GeometryNodeCurvePrimitiveCircle", mode="RADIUS")
    prof.inputs["Resolution"].default_value, prof.inputs["Radius"].default_value = 8, 1.0
    cm = node("GeometryNodeCurveToMesh")
    link(mc_.outputs["Curve"], cm.inputs["Curve"])
    link(rad, cm.inputs["Scale"])
    link(prof.outputs["Curve"], cm.inputs["Profile Curve"])
    cm.inputs["Fill Caps"].default_value = True
    mat = bpy.data.materials.new(f"{name}_speed")
    mat.use_nodes = True
    nt = mat.node_tree
    b = nt.nodes["Principled BSDF"]
    attr = nt.nodes.new("ShaderNodeAttribute")
    attr.attribute_name = "speed"
    cr = nt.nodes.new("ShaderNodeValToRGB")
    cr.color_ramp.elements.remove(cr.color_ramp.elements[0])
    for i, (at, col) in enumerate(colours):
        el = cr.color_ramp.elements[0] if i == 0 else cr.color_ramp.elements.new(at)
        el.position, el.color = at, (*col, 1.0)
    dv = nt.nodes.new("ShaderNodeMath")
    dv.operation = "DIVIDE"
    dv.inputs[1].default_value = vmax
    nt.links.new(attr.outputs["Fac"], dv.inputs[0])
    nt.links.new(dv.outputs[0], cr.inputs["Fac"])
    nt.links.new(cr.outputs["Color"], b.inputs["Base Color"])
    nt.links.new(cr.outputs["Color"], b.inputs["Emission Color"])
    b.inputs["Emission Strength"].default_value, b.inputs["Roughness"].default_value = glow, 0.4
    sm = node("GeometryNodeSetMaterial")
    sm.inputs["Material"].default_value = mat
    link(cm.outputs["Mesh"], sm.inputs["Geometry"])
    link(sm.outputs["Geometry"], gout.inputs[0])
    gm = ob.modifiers.new("comets", "NODES")
    gm.node_group = ng
    return ob
