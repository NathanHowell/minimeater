"""The film's molecule (Blender, `film.py`): O=O as ball-and-stick, and the chemistry of the combustion shot, C + O2 -> CO2, drawn
the way the books draw it: the carbon comes in across the bond, the two oxygens part to 2.32 angstrom either side of it, the O-O
double bond goes and two C=O double bonds (four sticks) grow in, so the product is linear O=C=O.

Everything is at `scale` times life size (an O=O bond is 1.21 angstrom, so 4e7 makes it 4.8 mm) and the film says so. The
molecule is an empty, `molecule`, with the atoms and sticks as children in its own frame (x along the O-O axis); a stick is a
unit-length cylinder along x that is placed by its centre and scaled in x to its length, so bonds can grow and break by key."""

import math

import numpy as np

ANGSTROM = 1e-10
R_O, R_C, R_STICK = 0.4, 0.42, 0.09  # ball-and-stick radii, angstrom
D_O2, D_CO = 1.21, 1.16  # O=O and C=O bond lengths, angstrom
DOUBLE = 0.2  # a double bond: two sticks this far apart


def materials():
    import bpy

    def mat(name, colour, rough, metal=0.0):
        m = bpy.data.materials.new(name)
        m.use_nodes = True
        b = m.node_tree.nodes["Principled BSDF"]
        b.inputs["Base Color"].default_value = (*colour, 1)
        b.inputs["Roughness"].default_value, b.inputs["Metallic"].default_value = rough, metal
        return m

    return {"O": mat("O", (0.55, 0.015, 0.01), 0.3), "C": mat("C", (0.03, 0.03, 0.035), 0.35), "bond": mat("bond", (0.82, 0.82, 0.84), 0.3, 0.6)}


def build(sc, scale: float):
    """The molecule: returns {"root", "o": [two balls], "c": the carbon ball, "o2": [two sticks], "co": [four sticks]}. As built it is O2
    (the carbon and the C=O sticks have zero size)."""
    import bpy
    from mathutils import Matrix

    k = scale * ANGSTROM
    m = materials()
    root = bpy.data.objects.new("molecule", None)
    root.rotation_mode = "XYZ"
    sc.collection.objects.link(root)

    def ball(name, mat, r, x):
        bpy.ops.mesh.primitive_uv_sphere_add(radius=r * k, location=(x * k, 0, 0), segments=48, ring_count=24)
        o = bpy.context.object
        o.name = name
        bpy.ops.object.shade_smooth()
        o.data.materials.append(m[mat])
        o.parent = root
        return o

    def stick(name, y):
        bpy.ops.mesh.primitive_cylinder_add(radius=R_STICK * k, depth=1.0, location=(0, y * k, 0), vertices=24)
        o = bpy.context.object
        o.data.transform(Matrix.Rotation(math.pi / 2, 4, "Y"))  # the mesh lies along x, so scale.x is its length
        o.name = name
        bpy.ops.object.shade_smooth()
        o.data.materials.append(m["bond"])
        o.parent = root
        return o

    parts = {"root": root, "o": [ball("O1", "O", R_O, -D_O2 / 2), ball("O2", "O", R_O, D_O2 / 2)], "c": ball("C", "C", R_C, 0.0),
             "o2": [stick("O2_bond_a", -DOUBLE / 2), stick("O2_bond_b", DOUBLE / 2)],
             "co": [stick(f"CO_bond_{s}{y}", y) for s in "lr" for y in (-DOUBLE / 2, DOUBLE / 2)]}
    for s in parts["o2"]:
        s.scale = (D_O2 * k, 1, 1)
    for s in parts["co"]:
        s.scale = (1e-6, 1, 1)
    parts["c"].scale = (1e-6,) * 3
    return parts


def chemistry(parts, scale: float, f0: int, f1: int, fps: int, z0: float = 3.4):
    """Keys the reaction over frames f0..f1 (the O2 at rest at the root, carbon arriving from +z where the coal is): the carbon grows and
    comes down from `z0` angstrom above (the coal's face) to the O-O bond's middle (first 40%), the oxygens part to the C=O length as the O-O sticks shrink and the four C=O
    sticks grow (40-75%), and the product holds (the rest). Returns the frame at which the bond forms (the oxygens' parting starts)."""
    k = scale * ANGSTROM
    a, b, c = f0, f0 + int(0.4 * (f1 - f0)), f0 + int(0.75 * (f1 - f0))
    ease = lambda u: u * u * (3 - 2 * u)  # noqa: E731

    def key(o, path, frame, value):
        setattr(o, path, value)
        o.keyframe_insert(path, frame=frame)

    carbon, (o1, o2) = parts["c"], parts["o"]
    key(carbon, "scale", a - 1, (1e-6,) * 3)
    key(carbon, "location", a - 1, (0, 0, z0 * k))
    for f in range(a, b + 1):
        u = ease((f - a) / (b - a))
        key(carbon, "scale", f, (max(u * 3, 1e-6) if u < 1 / 3 else 1.0,) * 3)  # grows over the first third of its approach
        key(carbon, "location", f, (0, 0, z0 * (1 - u) * k))
    for f in range(b, c + 1):
        u = ease((f - b) / (c - b))
        d = (D_O2 / 2 + u * (D_CO - D_O2 / 2)) * k
        key(o1, "location", f, (-d, 0, 0))
        key(o2, "location", f, (d, 0, 0))
        for s in parts["o2"]:
            key(s, "scale", f, (max((1 - u) * D_O2 * k, 1e-6), 1, 1))  # the O-O bond runs out ...
        for j, s in enumerate(parts["co"]):
            side = -1 if j < 2 else 1
            key(s, "scale", f, (max(u * d, 1e-6), 1, 1))  # ... as the C=O bonds grow out from the carbon to each oxygen
            key(s, "location", f, (side * u * d / 2, s.location.y, 0))
    for o in (o1, o2, carbon, *parts["o2"], *parts["co"]):  # hold to the end
        for path in ("location", "scale"):
            o.keyframe_insert(path, frame=f1)
    return b


def flicker(material, f0: int, f1: int, base: float, peak: float, f_peak: int, seed: int = 9, light=None, watts: float = 0.0):
    """Keys the ember's `Emission Strength` over f0..f1 every second frame: up from `base` to `peak` at `f_peak`, down to half way after,
    with a seeded flicker; a light's `energy` (`light`, `watts` per unit of strength) follows it."""
    rng = np.random.default_rng(seed)
    node = material.node_tree.nodes["Principled BSDF"].inputs["Emission Strength"]
    for f in range(f0, f1 + 1, 2):
        u = (f - f0) / max(1, f_peak - f0) if f <= f_peak else 1 - 0.5 * (f - f_peak) / max(1, f1 - f_peak)
        v = (base + (peak - base) * min(1.0, max(0.0, u))) * (1 + rng.normal(0, 0.12))
        node.default_value = v
        node.keyframe_insert("default_value", frame=f)
        if light is not None:
            light.energy = v * watts
            light.keyframe_insert("energy", frame=f)
