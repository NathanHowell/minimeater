"""A stand-in kamado for the animation (`film.py`): the egg's base and dome, the vent port the intake tube enters, the fire ring and
grate, a bed of charcoal lumps, the cooking grid and a brisket (mm, mech frame, Blender in metres). Illustrative, not a model of
a particular egg and not CFD: the minimeater's CFD ends at the tube, and the path in here is drawn by hand through the places the
air has to go (port, below the grate, up through a grate hole, through the bed, past the grid to the meat).

The egg's axis is vertical, `X0` mm past the tube's end. Sizes follow a large egg: a 300 mm fire ring (the model's `bed_area_m2`,
0.07 m2), a dome 456 mm across at its widest, 570 mm to the top vent."""

import math

import numpy as np

PORT = 55.0  # the vent port: from the tube's end to the cavity's wall, mm
PROFILE = ((-30, 0), (-29, 70), (-18, 112), (20, 140), (120, 200), (220, 228), (330, 224), (430, 192), (500, 132), (548, 62), (570, 30))  # (z, inner r)
WALL = 14.0
RING = (150.0, 40.0, 190.0)  # fire ring: inner radius, z0, z1
GRATE = (146.0, 58.0, 8.0)  # radius, z of the underside, thickness
BED = (140.0, 68.0, 172.0)  # coal bed: radius, z0, z1
GRID_Z = 250.0
MEAT = (150.0, 82.0, 34.0)  # brisket: half length (x), half width (y), half thickness (z)
LUMP = (13.0, 24.0)  # lump radius range
HOLES = 12  # holes in the grate's ring, at radius 70, one of them at 180 deg (the path's)
CONTACT_R = 70.0  # the air comes up through the hole at x = X0 - this


def catmull(ctl: np.ndarray, step: float) -> np.ndarray:
    """A Catmull-Rom spline through the control points `ctl` (n, 3), resampled about every `step` of arc length."""
    c = np.vstack([2 * ctl[0] - ctl[1], ctl, 2 * ctl[-1] - ctl[-2]])
    t = np.linspace(0, 1, 40)[:-1, None]
    seg = []
    for k in range(1, len(c) - 2):
        p0, p1, p2, p3 = c[k - 1], c[k], c[k + 1], c[k + 2]
        seg.append(0.5 * (2 * p1 + (p2 - p0) * t + (2 * p0 - 5 * p1 + 4 * p2 - p3) * t**2 + (3 * p1 - p0 - 3 * p2 + p3) * t**3))
    dense = np.vstack([*seg, ctl[-1:]])
    s = np.r_[0.0, np.cumsum(np.linalg.norm(np.diff(dense, axis=0), axis=1))]
    u = np.arange(0.0, s[-1], step)
    return np.stack([np.interp(np.append(u, s[-1]), s, dense[:, k]) for k in range(3)], axis=1)


def inner_radius(z):
    zs, rs = zip(*PROFILE)
    return float(np.interp(z, zs, rs))


def layout(tube_end_x: float, seed: int = 11) -> dict:
    """Where everything stands, from the tube's end (mm): the egg's axis `x0`, the hero lump's centre and radius, the lumps (centres,
    radii) clear of the air's corridor, and the point on the meat the plume reaches."""
    rng = np.random.default_rng(seed)
    x0 = tube_end_x + PORT + inner_radius(0.0)
    up = np.array([x0 - CONTACT_R, 0.0, 0.0])
    hero_r = 21.0
    hero = up + [0.0, 0.0, 118.0 + hero_r]  # its underside is where the O2 arrives
    corridor = (up[0], 0.0, 68.0, 200.0, 17.0 + hero_r)  # no other lump within this of the vertical line above the grate hole
    centres, radii = np.empty((0, 3)), np.empty(0)
    for _ in range(40000):
        r = rng.uniform(*LUMP)
        ang, rad = rng.uniform(0, 2 * math.pi), BED[0] * math.sqrt(rng.uniform(0, 1)) - r
        c = np.array([x0 + rad * math.cos(ang), rad * math.sin(ang), rng.uniform(BED[1] + r, BED[2] - r)])
        if np.hypot(c[0] - corridor[0], c[1]) < corridor[4] + r - 8.0 and corridor[2] <= c[2] <= corridor[3] + r:
            continue
        if np.linalg.norm(c - hero) < 0.8 * (r + hero_r) or (len(radii) and (np.linalg.norm(centres - c, axis=1) < 0.8 * (r + radii)).any()):
            continue
        centres, radii = np.vstack([centres, c]), np.append(radii, r)
        if len(radii) >= 330:
            break
    return {"x0": x0, "hero": hero, "hero_r": hero_r, "lumps": (centres, radii), "up": up,
            "meat_c": np.array([x0, 0.0, GRID_Z + 1.5 + MEAT[2]]), "plume_end": np.array([up[0], 0.0, GRID_Z + 1.5 + MEAT[2] - MEAT[2] * math.sqrt(1 - (CONTACT_R / MEAT[0]) ** 2)])}


GAP = 16.0  # mm: the O2 stops this far below the hero lump, the carbon comes out of its face to meet it


def approach(lay: dict, start: np.ndarray, v_start: float):
    """The air's way from the end of the CFD (`start`, mm, speed `v_start` m/s) to the hero lump: the port, the cavity under the grate,
    up through the grate's hole and the bed to where the O2 stops, GAP under the lump. (points mm, speed m/s, a point about every mm)."""
    x0, stop = lay["x0"], lay["hero"] - [0, 0, lay["hero_r"] + GAP]
    ctl = np.array([start, start + [10, 0, 0], [x0 - inner_radius(0) - 8, 0, 0], [x0 - 118, 0, 6], [x0 - 98, 0, 34], [x0 - CONTACT_R, 0, 52],
                    [x0 - CONTACT_R, 0, GRATE[1] + GRATE[2] + 6], stop - [0, 0, 25], stop])
    p = catmull(ctl, 1.0)
    s = np.r_[0.0, np.cumsum(np.linalg.norm(np.diff(p, axis=0), axis=1))]
    v = np.interp(s / s[-1], [0, 0.12, 0.35, 0.55, 0.62, 1.0], [v_start, v_start * 0.85, 0.5, 0.45, 1.1, 0.9])
    return p, v


def rise(lay: dict, seed: int = 5):
    """The CO2's way from where the O2 stopped, round the hero lump and up to the meat's underside (mm, m/s): buoyant, slow, swaying a little."""
    rng = np.random.default_rng(seed)
    h, r, end, ux = lay["hero"], lay["hero_r"], lay["plume_end"], lay["up"][0]
    z = np.linspace(h[2] + r + 30, end[2], 6)
    sway = rng.normal(0, 5, (6, 2))
    sway[-1] = 0
    ctl = np.array([[ux, 0, h[2] - r - GAP], [ux + r + 12, 0, h[2] - r], [ux + r + 12, 0, h[2] + r], *[[ux + sway[k, 0], sway[k, 1], z[k]] for k in range(6)]])
    ctl[-1] = end
    p = catmull(ctl, 1.0)
    return p, np.interp(np.arange(len(p)) / (len(p) - 1), [0, 0.3, 1.0], [0.4, 0.9, 0.7])


def build(sc, lay: dict):
    """Adds the egg and what is in it to `sc` (Blender): returns the objects by name, the hero lump among them."""
    import bpy
    import bmesh  # noqa: I001
    from mathutils import Vector

    rng = np.random.default_rng(3)
    x0 = lay["x0"]
    mm = 1e-3

    def mat(name, colour, rough=0.5, metal=0.0, emit=None):
        m = bpy.data.materials.new(name)
        m.use_nodes = True
        b = m.node_tree.nodes["Principled BSDF"]
        b.inputs["Base Color"].default_value = (*colour, 1)
        b.inputs["Roughness"].default_value, b.inputs["Metallic"].default_value = rough, metal
        return m

    def link(name, bm, material, loc=(0, 0, 0)):
        me = bpy.data.meshes.new(name)
        bm.to_mesh(me)
        bm.free()
        ob = bpy.data.objects.new(name, me)
        ob.location = loc
        ob.data.materials.append(material)
        sc.collection.objects.link(ob)
        for p in me.polygons:
            p.use_smooth = True
        return ob

    out = {}
    # the egg: the profile spun about the vertical axis, thickened inward, a hole for the port
    bm = bmesh.new()
    prof = [bm.verts.new((r * mm, 0, z * mm)) for z, r in PROFILE]
    edges = [bm.edges.new((a, b)) for a, b in zip(prof, prof[1:])]
    bmesh.ops.spin(bm, geom=prof + edges, cent=(0, 0, 0), axis=(0, 0, 1), angle=2 * math.pi, steps=96, use_merge=True)
    bmesh.ops.remove_doubles(bm, verts=bm.verts, dist=1e-6)
    bmesh.ops.recalc_face_normals(bm, faces=bm.faces)
    egg = link("kamado", bm, mat("egg_ceramic", (0.06, 0.28, 0.2), 0.12), (x0 * mm, 0, 0))
    sol = egg.modifiers.new("wall", "SOLIDIFY")
    sol.thickness, sol.offset = WALL * mm, 1.0
    port = bpy.data.objects.new("port", bpy.data.meshes.new("port"))
    bm = bmesh.new()
    bmesh.ops.create_cone(bm, cap_ends=True, segments=48, radius1=22 * mm, radius2=22 * mm, depth=300 * mm)
    bmesh.ops.rotate(bm, verts=bm.verts, cent=(0, 0, 0), matrix=__import__("mathutils").Matrix.Rotation(math.pi / 2, 3, "Y"))
    bm.to_mesh(port.data)
    bm.free()
    sc.collection.objects.link(port)
    port.location = ((x0 - 230) * mm, 0, 0)
    port.hide_render = port.hide_viewport = True
    bo = egg.modifiers.new("port", "BOOLEAN")
    bo.operation, bo.object, bo.solver = "DIFFERENCE", port, "EXACT"
    out["egg"] = egg

    # the fire ring and the grate with its ring of holes (one under the air's corridor)
    bm = bmesh.new()
    bmesh.ops.create_cone(bm, cap_ends=False, segments=96, radius1=(RING[0] + 16) * mm, radius2=(RING[0] + 16) * mm, depth=(RING[2] - RING[1]) * mm)
    ring = link("fire_ring", bm, mat("ring_ceramic", (0.72, 0.68, 0.6), 0.55), (x0 * mm, 0, (RING[1] + RING[2]) / 2 * mm))
    ring.modifiers.new("t", "SOLIDIFY").thickness = -16 * mm
    out["fire_ring"] = ring
    bm = bmesh.new()
    bmesh.ops.create_cone(bm, cap_ends=True, segments=96, radius1=GRATE[0] * mm, radius2=GRATE[0] * mm, depth=GRATE[2] * mm)
    grate = link("grate", bm, mat("cast_iron", (0.05, 0.05, 0.055), 0.6, 0.8), (x0 * mm, 0, (GRATE[1] + GRATE[2] / 2) * mm))
    for k in range(HOLES):
        a = math.pi + 2 * math.pi * k / HOLES
        hole = bpy.data.objects.new(f"hole{k}", bpy.data.meshes.new(f"hole{k}"))
        bm = bmesh.new()
        bmesh.ops.create_cone(bm, cap_ends=True, segments=24, radius1=17 * mm, radius2=17 * mm, depth=3 * GRATE[2] * mm)
        bm.to_mesh(hole.data)
        bm.free()
        sc.collection.objects.link(hole)
        hole.location = ((x0 + CONTACT_R * math.cos(a)) * mm, CONTACT_R * math.sin(a) * mm, (GRATE[1] + GRATE[2] / 2) * mm)
        hole.hide_render = hole.hide_viewport = True
        m = grate.modifiers.new(f"h{k}", "BOOLEAN")
        m.operation, m.object, m.solver = "DIFFERENCE", hole, "EXACT"
    out["grate"] = grate

    # the coals: lumps of displaced icospheres joined into one object (the cutaway cuts it once); the hero lump is its own object
    char = mat("charcoal", (0.012, 0.011, 0.01), 0.85)
    glow = glowing(mat)

    def lump(bm, c, r):
        v0 = len(bm.verts)
        g = bmesh.ops.create_icosphere(bm, subdivisions=2, radius=1.0)
        scale = rng.uniform(0.75, 1.25, 3)
        phase = rng.uniform(0, 6.3, 3)
        for v in g["verts"]:
            d = Vector(v.co)
            bump = 1 + 0.22 * math.sin(4 * d.x + phase[0]) * math.sin(4 * d.y + phase[1]) * math.sin(4 * d.z + phase[2]) + 0.1 * rng.normal()
            v.co = Vector((d.x * scale[0], d.y * scale[1], d.z * scale[2])) * bump * r * mm + Vector(c) * mm
        return g

    bm = bmesh.new()
    cs, rs = lay["lumps"]
    for c, r in zip(cs, rs):
        lump(bm, c, r)
    out["coals"] = link("coals", bm, char)
    bm = bmesh.new()
    lump(bm, lay["hero"], lay["hero_r"])
    out["hero_lump"] = link("hero_lump", bm, glow)
    # the cooking grid and the brisket
    bm = bmesh.new()
    for y in np.arange(-130, 141, 20.0):  # none at y = 0, where the plume reaches the meat
        c = bmesh.ops.create_cone(bm, cap_ends=True, segments=12, radius1=2.5 * mm, radius2=2.5 * mm, depth=2 * math.sqrt(max(0, 150**2 - y * y)) * mm)
        bmesh.ops.rotate(bm, verts=c["verts"], cent=(0, 0, 0), matrix=__import__("mathutils").Matrix.Rotation(math.pi / 2, 3, "Y"))
        bmesh.ops.translate(bm, verts=c["verts"], vec=(0, y * mm, 0))
    out["grid"] = link("grid", bm, mat("grid_steel", (0.5, 0.5, 0.52), 0.35, 1.0), (x0 * mm, 0, (GRID_Z - 2.5) * mm))
    bm = bmesh.new()
    bmesh.ops.create_uvsphere(bm, u_segments=64, v_segments=32, radius=1.0)
    for v in bm.verts:
        d = Vector(v.co)
        bump = 1 + 0.035 * math.sin(14 * d.x) * math.sin(11 * d.y + 1) + 0.03 * math.sin(9 * d.z + 2 * d.x)
        v.co = Vector((d.x * MEAT[0], d.y * MEAT[1], d.z * MEAT[2])) * bump * mm
    meat = link("brisket", bm, mat("brisket", (0.16, 0.07, 0.035), 0.55), tuple(lay["meat_c"] * mm))
    out["brisket"] = meat
    return out


def glowing(mat):
    """The hero lump's material: charcoal with a noise-driven ember glow whose strength is the `Glow` input of its Emission node, for
    the combustion shot to key."""
    import bpy

    m = bpy.data.materials.new("ember")
    m.use_nodes = True
    nt = m.node_tree
    b = nt.nodes["Principled BSDF"]
    b.inputs["Base Color"].default_value = (0.012, 0.011, 0.01, 1)
    b.inputs["Roughness"].default_value = 0.85
    tex = nt.nodes.new("ShaderNodeTexNoise")
    tex.inputs["Scale"].default_value, tex.inputs["Detail"].default_value = 90.0, 4.0
    ramp = nt.nodes.new("ShaderNodeValToRGB")
    ramp.color_ramp.elements[0].position, ramp.color_ramp.elements[1].position = 0.45, 0.8
    ramp.color_ramp.elements[0].color, ramp.color_ramp.elements[1].color = (0, 0, 0, 1), (1.0, 0.32, 0.04, 1)
    nt.links.new(tex.outputs["Fac"], ramp.inputs["Fac"])
    nt.links.new(ramp.outputs["Color"], b.inputs["Emission Color"])
    b.inputs["Emission Strength"].default_value = 0.6
    return m
