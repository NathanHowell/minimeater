"""The smoke shot (Blender, `film.py`): the CO2 is carried up the rise path in a plume of smoke, and flavour compounds (small amber
particles, kept apart from the CO2 on purpose: real smoke flavour is wood's pyrolysis products, phenols and carbonyls, riding the
plume, not the CO2) swirl up beside the molecule, land on the meat's underside and tint it where they land.

Illustrative, like the firebox: the plume is a volume in a tube along the path with a noise density that scrolls, not a smoke
simulation."""

import math

import numpy as np

N_FLAVOUR = 28
SEED = 21


def along(path: np.ndarray, s: np.ndarray) -> np.ndarray:
    """Points of `path` (mm, about a mm apart) at arc lengths `s`."""
    a = np.r_[0.0, np.cumsum(np.linalg.norm(np.diff(path, axis=0), axis=1))]
    return np.stack([np.interp(s, a, path[:, k]) for k in range(3)], axis=1)


def flavour_tracks(path: np.ndarray, arc: np.ndarray, target_c: np.ndarray, fps: int, seed: int = SEED):
    """The flavour particles' positions (mm) for every frame: (N_FLAVOUR, frames, 3) and the first frame each is in view. Each rides the path
    a little behind the molecule (lag 5-60 mm) on a spiral of its own (radius 3-9 mm), and when the molecule arrives (`arc` reaches the end) it
    comes down to its own spot on the meat within 16 mm of `target_c`, over a second."""
    rng = np.random.default_rng(seed)
    total = arc[-1]
    lag = rng.uniform(5, 60, N_FLAVOUR)
    rad = rng.uniform(3, 9, N_FLAVOUR)
    turns = rng.uniform(1.5, 3.5, N_FLAVOUR) * rng.choice([-1, 1], N_FLAVOUR)
    phase = rng.uniform(0, 2 * math.pi, N_FLAVOUR)
    spot = target_c + np.c_[rng.uniform(-16, 16, N_FLAVOUR), rng.uniform(-6, 6, N_FLAVOUR), rng.uniform(-1.5, 0, N_FLAVOUR)]
    out = np.zeros((N_FLAVOUR, len(arc), 3))
    first = np.zeros(N_FLAVOUR, int)
    arrived = int(np.argmax(arc >= total)) if (arc >= total).any() else len(arc)
    started = np.flatnonzero(arc > 0)
    f_start = started[0] if len(started) else len(arc)
    for i in range(N_FLAVOUR):
        s = np.clip(arc - lag[i], 0, total)
        c = along(path, s)
        ang = phase[i] + turns[i] * 2 * math.pi * s / total + 0.8 * np.arange(len(arc)) / fps
        pos = c + np.c_[rad[i] * np.cos(ang) * 0.3, rad[i] * np.cos(ang), rad[i] * np.sin(ang) * 0.3] * (np.clip(s / 30, 0, 1)[:, None])
        w = np.clip((np.arange(len(arc)) - arrived) / fps, 0, 1)
        w = (w * w * (3 - 2 * w))[:, None]
        out[i] = pos * (1 - w) + spot[i] * w
        first[i] = f_start
    return out, first


def build(sc, path: np.ndarray, arc: np.ndarray, target_c: np.ndarray, fps: int, meat, f_arrive: int):
    """Adds the plume, the flavour particles and the meat's tint to `sc`, keyed to `arc` (the frames' distance along `path`). `meat` is the
    brisket object; `f_arrive` the first frame (1-based) the molecule is at the meat."""
    import bpy

    mm = 1e-3
    n = len(arc)
    total = arc[-1]

    # the plume: a closed tube along the path holding a scrolling noise density; it grows with the molecule
    cu = bpy.data.curves.new("plume", "CURVE")
    cu.dimensions, cu.bevel_depth, cu.bevel_resolution, cu.resolution_u, cu.use_fill_caps = "3D", 0.012, 3, 1, True
    cu.bevel_factor_mapping_start = cu.bevel_factor_mapping_end = "SPLINE"
    sp = cu.splines.new("POLY")
    sp.points.add(len(path) - 1)
    for pt, xyz in zip(sp.points, path * mm):
        pt.co = (*xyz, 1.0)
    ob = bpy.data.objects.new("smoke_plume", cu)
    sc.collection.objects.link(ob)
    m = bpy.data.materials.new("smoke")
    m.use_nodes = True
    nt = m.node_tree
    for node in list(nt.nodes):
        if node.type == "BSDF_PRINCIPLED":
            nt.nodes.remove(node)
    out = nt.nodes["Material Output"]
    vol = nt.nodes.new("ShaderNodeVolumePrincipled")
    vol.inputs["Color"].default_value = (0.72, 0.74, 0.78, 1)
    vol.inputs["Anisotropy"].default_value = 0.3
    coord, mapping = nt.nodes.new("ShaderNodeTexCoord"), nt.nodes.new("ShaderNodeMapping")
    noise = nt.nodes.new("ShaderNodeTexNoise")
    noise.inputs["Scale"].default_value, noise.inputs["Detail"].default_value, noise.inputs["Roughness"].default_value = 70.0, 6.0, 0.6
    ramp = nt.nodes.new("ShaderNodeMapRange")
    ramp.inputs["From Min"].default_value, ramp.inputs["From Max"].default_value = 0.35, 0.75
    ramp.inputs["To Min"].default_value, ramp.inputs["To Max"].default_value = 0.0, 90.0
    nt.links.new(coord.outputs["Object"], mapping.inputs["Vector"])
    nt.links.new(mapping.outputs["Vector"], noise.inputs["Vector"])
    nt.links.new(noise.outputs["Fac"], ramp.inputs["Value"])
    nt.links.new(ramp.outputs["Result"], vol.inputs["Density"])
    nt.links.new(vol.outputs["Volume"], out.inputs["Volume"])
    for f in (1, n):
        mapping.inputs["Location"].default_value = (0, 0, -0.4 * f / 24)  # the noise drifts up through the tube, a metre every 2.5 s
        mapping.inputs["Location"].keyframe_insert("default_value", frame=f)
    ob.data.materials.append(m)
    for f in range(n):
        cu.bevel_factor_start = 0.0
        cu.bevel_factor_end = arc[f] / total
        cu.keyframe_insert("bevel_factor_end", frame=f + 1)

    # the flavour: amber particles on spirals beside the molecule, then settled on the meat
    tracks, first = flavour_tracks(path, arc, target_c, fps)
    amber = bpy.data.materials.new("flavour")
    amber.use_nodes = True
    b = amber.node_tree.nodes["Principled BSDF"]
    b.inputs["Base Color"].default_value = (0.9, 0.32, 0.03, 1)
    b.inputs["Emission Color"].default_value = (1.0, 0.3, 0.02, 1)
    b.inputs["Emission Strength"].default_value = 0.5
    b.inputs["Roughness"].default_value = 0.3
    f0 = int(first[0])
    for i in range(N_FLAVOUR):
        bpy.ops.mesh.primitive_uv_sphere_add(radius=0.0012 * (0.7 + 0.6 * (i % 5) / 4), location=(0, 0, 0), segments=16, ring_count=8)
        o = bpy.context.object
        o.name = f"flavour{i}"
        bpy.ops.object.shade_smooth()
        o.data.materials.append(amber)
        o.hide_render = True
        o.keyframe_insert("hide_render", frame=1)
        o.hide_render = False
        o.keyframe_insert("hide_render", frame=f0 + 1)
        for f in range(f0, n):
            o.location = tracks[i, f] * mm
            o.keyframe_insert("location", frame=f + 1)

    # the meat takes the flavour where it lands: a darker, redder tint (the smoke ring's pink is a chemistry of its own, not shown) that spreads
    # from the contact point once the particles are there
    mt = meat.data.materials[0]
    nt = mt.node_tree
    bsdf = nt.nodes["Principled BSDF"]
    base = bsdf.inputs["Base Color"].default_value
    coord, mapping = nt.nodes.new("ShaderNodeTexCoord"), nt.nodes.new("ShaderNodeMapping")
    grad = nt.nodes.new("ShaderNodeTexGradient")
    grad.gradient_type = "SPHERICAL"
    local = target_c * mm - np.asarray(meat.location)  # the contact point in the meat's own frame, m
    mapping.vector_type = "POINT"
    mapping.inputs["Scale"].default_value = (1 / 0.03,) * 3  # the tint reaches 30 mm
    mapping.inputs["Location"].default_value = tuple(-local / 0.03)
    nt.links.new(coord.outputs["Object"], mapping.inputs["Vector"])
    nt.links.new(mapping.outputs["Vector"], grad.inputs["Vector"])
    amount = nt.nodes.new("ShaderNodeValue")
    mixer = nt.nodes.new("ShaderNodeMix")
    mixer.data_type = "RGBA"
    mul = nt.nodes.new("ShaderNodeMath")
    mul.operation = "MULTIPLY"
    nt.links.new(grad.outputs["Fac"], mul.inputs[0])
    nt.links.new(amount.outputs[0], mul.inputs[1])
    nt.links.new(mul.outputs[0], mixer.inputs["Factor"])
    mixer.inputs["A"].default_value = base
    mixer.inputs["B"].default_value = (0.2, 0.035, 0.012, 1)
    nt.links.new(mixer.outputs["Result"], bsdf.inputs["Base Color"])
    for f, v in ((1, 0.0), (f_arrive, 0.0), (min(n, f_arrive + 2 * fps), 1.0), (n, 1.0)):
        amount.outputs[0].default_value = v
        amount.outputs[0].keyframe_insert("default_value", frame=f)
    return ob
