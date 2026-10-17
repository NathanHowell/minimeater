"""A binary glTF (`.glb`) of the assembly for Blender and other viewers: one mesh per part (the board one per colour),
metres, y up as glTF wants (Blender's importer turns it back to z up), PBR materials and smooth normals split at the
model's edges (the renderer's 35 degrees). build123d's own `export_gltf` writes one mesh per face, which is 15,000
objects here."""

import json
import struct

import numpy as np
import pyvista as pv

from .printing import mesh


def _by_face(solid):
    """Vertices and triangles of the faces of `solid` that OCC will triangulate. The fan's frame has one 0.05 mm2 sliver
    of a plane (negative area, on a screw boss) that it never does, at any tolerance; the rest of its 563 faces are fine."""
    verts, tris, base = [], [], 0
    for f in solid.faces():
        try:
            v, t = f.tessellate(0.05, 0.2)
        except (AttributeError, RuntimeError):
            continue
        if not t:
            continue
        verts.append(np.array([(p.X, p.Y, p.Z) for p in v]))
        tris.append(np.asarray(t, dtype=np.int64) + base)
        base += len(v)
    if not verts:
        raise RuntimeError("tessellation failed")
    return np.vstack(verts), np.vstack(tris)


def _mesh(shape):
    """Vertices (mm) and triangles of `shape`: a shape, a mesh already made as (vertices, triangles), or a list of them. A
    solid OCC will not triangulate whole (the fan's, the board's) is done face by face, leaving out the faces it will not do
    rather than the solid, or failing the export."""
    verts, tris, base = [], [], 0
    for item in shape if isinstance(shape, list) else [shape]:
        if isinstance(item, tuple):
            meshes = [item]
        else:
            meshes = []
            for solid in item.solids() or [item]:
                try:
                    meshes.append(mesh(solid, 0.05, 0.2))
                except RuntimeError:
                    try:
                        meshes.append(_by_face(solid))
                    except RuntimeError:
                        continue
        for v, t in meshes:
            verts.append(v)
            tris.append(np.asarray(t, dtype=np.int64) + base)
            base += len(v)
    if not verts:
        return np.zeros((0, 3)), np.zeros((0, 3), dtype=np.uint32)
    return np.vstack(verts), np.vstack(tris).astype(np.uint32)


def _smooth(v, t):
    """The mesh with its vertices merged and then split where the surface bends more than 35 degrees, and a normal at each."""
    pd = pv.PolyData(v, np.hstack([np.full((len(t), 1), 3), t]).ravel()).clean()
    pd = pd.compute_normals(split_vertices=True, feature_angle=35.0, auto_orient_normals=False, consistent_normals=True)
    return np.asarray(pd.points), np.asarray(pd.regular_faces), np.asarray(pd.point_data["Normals"])


def _turn(v):
    """Mech frame (z up) to glTF's (y up)."""
    return np.c_[v[:, 0], v[:, 2], -v[:, 1]]


def write_glb(path, parts, markers=(), extras=None):
    """`parts`: a list of (name, shape, look) where look is "#rrggbb" or (colour, metallic, roughness); each becomes one
    node with one mesh and one material. `markers`: (name, (x, y, z) in mm) empty nodes, in order, with their index in
    the node's extras. `extras`: a dict kept on the scene."""
    blob, views, accessors, meshes, nodes, materials = bytearray(), [], [], [], [], []

    def add(data, target, **acc):
        while len(blob) % 4:
            blob.append(0)
        views.append({"buffer": 0, "byteOffset": len(blob), "byteLength": data.nbytes, "target": target})
        blob.extend(data.tobytes())
        accessors.append({"bufferView": len(views) - 1, **acc})
        return len(accessors) - 1

    for name, shape, look in parts:
        v, t = _mesh(shape)
        if not len(t):
            continue
        v, t, n = _smooth(v, t)
        v = (_turn(v) * 0.001).astype(np.float32)  # mm -> m
        n = _turn(n).astype(np.float32)
        pos = add(v, 34962, componentType=5126, count=len(v), type="VEC3", min=v.min(0).tolist(), max=v.max(0).tolist())
        nor = add(n, 34962, componentType=5126, count=len(n), type="VEC3")
        idx = add(t.astype(np.uint32).ravel(), 34963, componentType=5125, count=t.size, type="SCALAR")
        colour, metallic, roughness = (look, 0.1, 0.6) if isinstance(look, str) else look
        rgb = [int(colour[i:i + 2], 16) / 255 for i in (1, 3, 5)]
        materials.append({"name": name, "pbrMetallicRoughness": {"baseColorFactor": [*rgb, 1.0], "metallicFactor": metallic,
                                                                "roughnessFactor": roughness}, "doubleSided": True})
        meshes.append({"name": name, "primitives": [{"attributes": {"POSITION": pos, "NORMAL": nor}, "indices": idx,
                                                     "material": len(materials) - 1}]})
        nodes.append({"name": name, "mesh": len(meshes) - 1})
    for i, (name, xyz) in enumerate(markers):
        nodes.append({"name": name, "translation": (_turn(np.array([xyz], dtype=float))[0] * 0.001).tolist(), "extras": {"step": i}})
    doc = {"asset": {"version": "2.0", "generator": "minimeater mech-export"}, "scene": 0,
           "scenes": [{"nodes": list(range(len(nodes))), **({"extras": extras} if extras else {})}], "nodes": nodes, "meshes": meshes, "materials": materials,
           "accessors": accessors, "bufferViews": views, "buffers": [{"byteLength": len(blob)}]}
    js = json.dumps(doc, separators=(",", ":")).encode()
    js += b" " * (-len(js) % 4)
    blob.extend(b"\0" * (-len(blob) % 4))
    total = 12 + 8 + len(js) + 8 + len(blob)
    with open(path, "wb") as f:
        f.write(struct.pack("<4sII", b"glTF", 2, total))
        f.write(struct.pack("<I4s", len(js), b"JSON") + js)
        f.write(struct.pack("<I4s", len(blob), b"BIN\0") + bytes(blob))
