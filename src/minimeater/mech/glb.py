"""A binary glTF (`.glb`) of the assembly for Blender and other viewers: one mesh per part (the board one per colour),
metres, y up as glTF wants (Blender's importer turns it back to z up). build123d's own `export_gltf` writes one
mesh per face, which is 15,000 objects here."""

import json
import struct

import numpy as np

from .printing import mesh


def _mesh(shape):
    """Vertices (mm) and triangles of `shape`; OCC will not triangulate an odd face of an imported model (the fan's,
    the board's) at any tolerance, and that face is left out of the viewer file rather than failing the export."""
    verts, tris, base = [], [], 0
    for solid in shape.solids() or [shape]:
        try:
            v, t = mesh(solid, 0.05, 0.2)
        except RuntimeError:
            continue
        verts.append(v)
        tris.append(np.asarray(t, dtype=np.int64) + base)
        base += len(v)
    if not verts:
        return np.zeros((0, 3)), np.zeros((0, 3), dtype=np.uint32)
    return np.vstack(verts), np.vstack(tris).astype(np.uint32)


def write_glb(path, parts):
    """`parts`: a list of (name, shape, "#rrggbb"). Each becomes one node with one mesh and one material."""
    blob, views, accessors, meshes, nodes, materials = bytearray(), [], [], [], [], []

    def add(data, target, **acc):
        while len(blob) % 4:
            blob.append(0)
        views.append({"buffer": 0, "byteOffset": len(blob), "byteLength": data.nbytes, "target": target})
        blob.extend(data.tobytes())
        accessors.append({"bufferView": len(views) - 1, **acc})
        return len(accessors) - 1

    for name, shape, colour in parts:
        v, t = _mesh(shape)
        if not len(t):
            continue
        v = v * 0.001
        v = np.c_[v[:, 0], v[:, 2], -v[:, 1]].astype(np.float32)  # mech z up -> glTF y up
        pos = add(v, 34962, componentType=5126, count=len(v), type="VEC3", min=v.min(0).tolist(), max=v.max(0).tolist())
        idx = add(t.astype(np.uint32).ravel(), 34963, componentType=5125, count=t.size, type="SCALAR")
        rgb = [int(colour[i:i + 2], 16) / 255 for i in (1, 3, 5)]
        materials.append({"name": name, "pbrMetallicRoughness": {"baseColorFactor": [*rgb, 1.0], "metallicFactor": 0.1,
                                                                "roughnessFactor": 0.6}, "doubleSided": True})
        meshes.append({"name": name, "primitives": [{"attributes": {"POSITION": pos}, "indices": idx,
                                                     "material": len(materials) - 1}]})
        nodes.append({"name": name, "mesh": len(meshes) - 1})
    doc = {"asset": {"version": "2.0", "generator": "minimeater mech-export"}, "scene": 0,
           "scenes": [{"nodes": list(range(len(nodes)))}], "nodes": nodes, "meshes": meshes, "materials": materials,
           "accessors": accessors, "bufferViews": views, "buffers": [{"byteLength": len(blob)}]}
    js = json.dumps(doc, separators=(",", ":")).encode()
    js += b" " * (-len(js) % 4)
    blob.extend(b"\0" * (-len(blob) % 4))
    total = 12 + 8 + len(js) + 8 + len(blob)
    with open(path, "wb") as f:
        f.write(struct.pack("<4sII", b"glTF", 2, total))
        f.write(struct.pack("<I4s", len(js), b"JSON") + js)
        f.write(struct.pack("<I4s", len(blob), b"BIN\0") + bytes(blob))
