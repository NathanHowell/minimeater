"""The exploded-view loop for the README (`uv run --group scene mech-explode`): the assembly turning once about the vertical
axis while it comes apart and goes back together, the enclosure faint when assembled (the chassis and the board show
through) and solid when exploded. Raytraced (Cycles, as `film.py`) on a transparent film from the same glb as the film
(`scene.write`, the real board when KiCad is here).

Writes an animated WebP (lossy, full alpha, `img2webp` from libwebp: Pillow's WebP has no animation here); a GIF's one-bit
alpha cannot carry the faint enclosure, and a GIF or an optimised APNG of the same loop is 1.3-1.5 MB to the WebP's 0.8.
The frames stay in build/explode/frames (`--encode` makes the WebP again from them)."""

import argparse
import math
import shutil
import subprocess
from pathlib import Path

import numpy as np
from PIL import Image

# offset (mm, assembly frame) of each part at full explode; parts not listed stay put (the duct, the flap and rod inside it)
_BOARD = (0, 0, -30)
OFFSET = {
    "hood": (0, 0, 58), "pan": (0, 0, -58),
    "pcb": _BOARD, "pcb_substrate": _BOARD, "pcb_module": _BOARD, "pcb_usb": _BOARD, "pcb_jacks": _BOARD, "pcb_parts": _BOARD,
    "fan": (-48, 0, 0), "fan_rotor": (-48, 0, 0),
    "servo": (0, 26, 10), "horn": (0, 26, 22),
    "crank": (0, 0, 14), "coupler": (0, 0, 24), "pin_crank": (0, 0, 34), "pin_horn": (0, 26, 34),
    "tube": (44, 0, 0), "oring": (24, 0, 0),
}
ENCLOSURE = ("hood", "pan")
# the loop's own palette (sRGB): every part mid-toned so it reads on GitHub's light (#ffffff) and dark (#0d1117) pages alike;
# the film keeps the real materials (`scene.MATERIALS`). (colour, metallic, roughness); parts not listed keep the glb's
PALETTE = {
    "hood": ("#3f9a6a", 0.0, 0.55), "pan": ("#5d6b7a", 0.0, 0.6), "duct": ("#9aa5b1", 0.0, 0.55),
    "fan": ("#5a6070", 0.0, 0.45), "fan_rotor": ("#6c7384", 0.0, 0.45), "servo": ("#3f6fd1", 0.0, 0.45), "horn": ("#d8d8d2", 0.0, 0.4),
    "flap": ("#e08a2e", 0.3, 0.35), "rod": ("#b9bec4", 1.0, 0.3), "crank": ("#b9bec4", 1.0, 0.3), "coupler": ("#b9bec4", 1.0, 0.3),
    "pin_crank": ("#d0d3d6", 1.0, 0.3), "pin_horn": ("#d0d3d6", 1.0, 0.3),
    "tube": ("#c27c3e", 1.0, 0.3), "oring": ("#55585e", 0.0, 0.8),
    "pcb_substrate": ("#2f9a55", 0.0, 0.35), "pcb_module": ("#8a8f96", 0.6, 0.4), "pcb_jacks": ("#4a4f57", 0.0, 0.5), "pcb_parts": ("#6a6f77", 0.0, 0.5),
}


def linear(hex_colour):
    c = [int(hex_colour[i:i + 2], 16) / 255 for i in (1, 3, 5)]
    return [v / 12.92 if v <= 0.04045 else ((v + 0.055) / 1.055) ** 2.4 for v in c]


def ease(u):
    return u * u * (3 - 2 * u)


def explode_at(t, hold=0.12):
    """0 assembled .. 1 exploded at loop phase t in [0, 1): out over the first half, back over the second, holding at each end."""
    tri = 1 - abs(2 * t - 1)  # 0 -> 1 -> 0
    return ease(min(max((tri - hold) / (1 - 2 * hold), 0.0), 1.0))


def write_glb(path: Path):
    """The product (no fit-check stand-ins) as a glb, the real board when KiCad is here."""
    from . import board3d, scene

    step = board3d.export_board(Path("build/mech"))
    asm = scene.assembly(0.0, board=step)
    if step is not None:
        asm.pop("connectors", None)  # the real board has its own
    path.parent.mkdir(parents=True, exist_ok=True)
    scene.write(path, asm)


def build(glb: Path, frames: int, res, samples: int, faint: float, elevation: float, lens: float):
    import bpy
    from mathutils import Vector

    from .film import settings

    bpy.ops.wm.read_factory_settings(use_empty=True)
    bpy.ops.import_scene.gltf(filepath=str(glb))
    sc = bpy.context.scene
    meshes = [o for o in sc.objects if o.type == "MESH"]

    # the parts' offsets as keys, every frame; the enclosure's alpha with them
    base = {o.name: o.location.copy() for o in meshes}
    for o in meshes:
        off = Vector(OFFSET.get(o.name, (0, 0, 0))) * 1e-3
        if o.parent is not None:
            off = o.parent.matrix_world.inverted().to_3x3() @ off
        if o.name in PALETTE:
            mat = o.active_material.copy()  # its own: recoloured
            o.active_material = mat
            node = next(n for n in mat.node_tree.nodes if n.type == "BSDF_PRINCIPLED")
            colour, metallic, rough = PALETTE[o.name]
            node.inputs["Base Color"].default_value = (*linear(colour), 1.0)
            node.inputs["Metallic"].default_value, node.inputs["Roughness"].default_value = metallic, rough
        for f in range(frames + 1):
            e = explode_at(f / frames)
            o.location = base[o.name] + off * e
            o.keyframe_insert("location", frame=f + 1)

    # framing: the sphere round the exploded extent, so nothing leaves the frame at any angle
    pts = []
    for o in meshes:
        off = Vector(OFFSET.get(o.name, (0, 0, 0))) * 1e-3
        for c in o.bound_box:
            w = o.matrix_world @ Vector(c)
            pts += [w, w + off]
    pts = np.array([[*p] for p in pts])
    lo, hi = pts.min(0), pts.max(0)
    centre = Vector((lo + hi) / 2)
    radius = float(np.linalg.norm(hi - lo) / 2)

    target = bpy.data.objects.new("target", None)
    target.location = centre
    rig = bpy.data.objects.new("rig", None)  # the camera and the lights ride it round
    rig.location = centre
    for o in (target, rig):
        sc.collection.objects.link(o)
    cam = bpy.data.objects.new("cam", bpy.data.cameras.new("cam"))
    cam.data.lens, cam.data.sensor_fit = lens, "AUTO"
    half = math.atan(18.0 / lens * min(1.0, res[1] / res[0]))  # the narrower half-angle of the 36 mm sensor
    dist = radius / math.sin(half) * 0.8
    el = math.radians(elevation)
    cam.parent = rig
    cam.location = (0.0, -dist * math.cos(el), dist * math.sin(el))
    tr = cam.constraints.new("TRACK_TO")
    tr.target, tr.track_axis, tr.up_axis = target, "TRACK_NEGATIVE_Z", "UP_Y"
    cam.data.clip_start, cam.data.clip_end = 0.001, dist * 4
    sc.collection.objects.link(cam)
    sc.camera = cam
    for name, loc, power, size in (("key", (-0.8, -1.0, 1.1), 10, 0.5), ("fill", (1.2, -0.6, 0.3), 3, 0.8), ("rim", (0.4, 1.2, 0.9), 14, 0.4), ("rim2", (-1.0, 1.0, 0.2), 8, 0.5)):
        li = bpy.data.objects.new(name, bpy.data.lights.new(name, "AREA"))
        li.data.energy, li.data.size = power * (radius / 0.12) ** 2, size * radius
        li.parent = rig
        li.location = Vector(loc) * dist * 0.6
        t = li.constraints.new("TRACK_TO")
        t.target, t.track_axis, t.up_axis = target, "TRACK_NEGATIVE_Z", "UP_Y"
        sc.collection.objects.link(li)
    for f, z in ((1, 0.0), (frames + 1, 2 * math.pi)):
        rig.rotation_euler = (0.0, 0.0, z)
        rig.keyframe_insert("rotation_euler", frame=f)
    _linear(rig)

    w = bpy.data.worlds.new("studio")
    w.use_nodes = True
    w.node_tree.nodes["Background"].inputs[0].default_value = (0.8, 0.82, 0.85, 1)
    w.node_tree.nodes["Background"].inputs[1].default_value = 0.12
    sc.world = w
    settings(sc, samples, res)
    sc.render.film_transparent = True
    sc.view_settings.view_transform, sc.view_settings.look, sc.view_settings.exposure = "AgX", "AgX - Punchy", -0.6  # AgX's roll-off, the palette's saturation
    sc.cycles.transparent_max_bounces = 24
    sc.render.image_settings.file_format = "PNG"
    sc.render.image_settings.color_mode = "RGBA"
    sc.frame_start, sc.frame_end = 1, frames
    return sc


def _linear(obj):
    """Linear keys on `obj`'s animation (Blender 4.4+ keeps them in the action's layered channelbags)."""
    act = obj.animation_data.action
    curves = list(getattr(act, "fcurves", []) or [])
    for layer in getattr(act, "layers", []):
        for strip in layer.strips:
            for bag in strip.channelbags:
                curves += list(bag.fcurves)
    for fc in curves:
        for k in fc.keyframe_points:
            k.interpolation = "LINEAR"


def opacity(f, frames, faint):
    """The enclosure's opacity at frame `f` (1-based)."""
    return faint + (1 - faint) * explode_at((f - 1) / frames)


def mix(without, with_shell, o):
    """The enclosure at opacity `o`: the render without it and the one with it opaque, mixed (premultiplied). Two opaque
    renders instead of one with a translucent material: Cycles' alpha is stochastic and its denoiser leaves the film's
    alpha channel alone, so a translucent shell over a transparent film comes out speckled."""
    def pre(im):
        a = np.asarray(im.convert("RGBA"), float) / 255
        return np.concatenate([a[..., :3] * a[..., 3:], a[..., 3:]], axis=-1)
    m = pre(without) * (1 - o) + pre(with_shell) * o
    alpha = m[..., 3:]
    rgb = np.divide(m[..., :3], alpha, out=np.zeros_like(m[..., :3]), where=alpha > 1e-6)
    return Image.fromarray((np.concatenate([rgb, alpha], axis=-1) * 255).round().clip(0, 255).astype(np.uint8), "RGBA")


def render_frame(sc, shell, frames_dir: Path, f: int, frames: int, faint: float, name="f"):
    """Frame `f` with the enclosure at its opacity: `frames_dir/{name}_{f:04d}.png` (the passes beside it)."""
    import bpy

    sc.frame_set(f)
    o = opacity(f, frames, faint)
    passes = {}
    for key, hidden in (("with", False), ("without", True)):
        if key == "without" and o >= 0.999:
            continue
        for s in shell:
            s.hide_render = hidden
        sc.render.filepath = str(frames_dir / f"{name}_{f:04d}.{key}.png")
        bpy.ops.render.render(write_still=True)
        passes[key] = Image.open(sc.render.filepath)
    for s in shell:
        s.hide_render = False
    img = passes["with"] if "without" not in passes else mix(passes["without"], passes["with"], o)
    img.save(frames_dir / f"{name}_{f:04d}.png")


def encode(frames_dir: Path, out: Path, size, ms: int, quality: int):
    """The frames, downscaled to `size`, as a looping lossy WebP with alpha."""
    if shutil.which("img2webp") is None:
        raise SystemExit("needs img2webp (libwebp: brew install webp, apt install webp)")
    small = frames_dir / "small"
    small.mkdir(exist_ok=True)
    names = []
    for p in sorted(frames_dir.glob("f_[0-9][0-9][0-9][0-9].png")):
        Image.open(p).convert("RGBA").resize(size, Image.LANCZOS).save(small / p.name)
        names.append(str(small / p.name))
    subprocess.run(["img2webp", "-loop", "0", "-lossy", "-q", str(quality), "-m", "6", "-d", str(ms), *names, "-o", str(out)],
                   check=True, capture_output=True)


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("out", nargs="?", default="docs/mech/explode.webp", type=Path)
    ap.add_argument("--size", type=int, nargs=2, default=(480, 360), help="the WebP's size (rendered at twice it)")
    ap.add_argument("--quality", type=int, default=65, help="img2webp's lossy quality")
    ap.add_argument("--encode", action="store_true", help="only make the WebP again from the frames already rendered")
    ap.add_argument("--frames", type=int, default=72)
    ap.add_argument("--ms", type=int, default=60, help="frame time")
    ap.add_argument("--samples", type=int, default=64)
    ap.add_argument("--faint", type=float, default=0.2, help="the enclosure's alpha when assembled")
    ap.add_argument("--elevation", type=float, default=24.0)
    ap.add_argument("--lens", type=float, default=60.0)
    ap.add_argument("--glb", type=Path, default=Path("build/explode/explode.glb"))
    ap.add_argument("--only", type=int, nargs="+", metavar="FRAME", help="render just these frames (a look) and stop")
    args = ap.parse_args()
    size = tuple(args.size)
    frames_dir = args.glb.parent / "frames"
    if args.encode:
        encode(frames_dir, args.out, size, args.ms, args.quality)
        print(f"wrote {args.out} ({args.out.stat().st_size / 1e6:.2f} MB)")
        return
    import bpy

    write_glb(args.glb)
    sc = build(args.glb, args.frames, (2 * size[0], 2 * size[1]), args.samples, args.faint, args.elevation, args.lens)
    frames_dir.mkdir(parents=True, exist_ok=True)
    shell = [bpy.data.objects[n] for n in ENCLOSURE]
    if args.only:
        for f in args.only:
            render_frame(sc, shell, frames_dir, f, args.frames, args.faint, name="look")
        print("wrote", ", ".join(f"look_{f:04d}.png" for f in args.only), "in", frames_dir)
        return
    for f in range(1, args.frames + 1):
        render_frame(sc, shell, frames_dir, f, args.frames, args.faint)
    args.out.parent.mkdir(parents=True, exist_ok=True)
    encode(frames_dir, args.out, size, args.ms, args.quality)
    print(f"wrote {args.out} ({args.out.stat().st_size / 1e6:.2f} MB)")


if __name__ == "__main__":
    main()
