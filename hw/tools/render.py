"""Copper coloured by net, for layout reviews: `uv run python hw/tools/render.py <dump.json> <out.png> [options]`.

The dump comes from `pcbdump.py` (KiCad's python). Every net has its own colour (GND dark grey, power warm); pads
carry their net name when they are big enough on the page; footprints show their courtyard and reference. Tracks
on F.Cu are solid, In1/In2 dashed and lighter, B.Cu dotted. With --rats, each net's unrouted pads are joined by a
thin minimum spanning tree (GND left out unless --rats-gnd), so placement can be judged before routing.

  --refs U4,L1,...   frame these footprints (plus --pad mm, default 1.5)
  --bbox x0,y0,x1,y1 frame this box (mm)
  --layers F.Cu,In1.Cu,In2.Cu,B.Cu  which copper to draw (default all; F on top)
  --ppmm N           pixels per mm (default 60)
  --title text
  --by-class PRO     colour by netclass (patterns from this .kicad_pro) instead of by net, with a legend
"""

import argparse
import colorsys
import hashlib
import json
import math

import matplotlib

matplotlib.use("Agg")
import matplotlib.pyplot as plt
from matplotlib.patches import Circle, PathPatch
from matplotlib.path import Path

ORDER = ["B.Cu", "In2.Cu", "In1.Cu", "F.Cu"]
STYLE = {"F.Cu": ("-", 1.0), "In1.Cu": ("--", 0.45), "In2.Cu": ("--", 0.55), "B.Cu": (":", 0.5)}
POWER = ("VBUS", "3V3", "VFAN", "VSERVO", "BUCK_SW")


CLASS_COLOURS = {"GND": (0.33, 0.33, 0.36), "PowerHigh": (0.85, 0.15, 0.12), "Power": (0.95, 0.55, 0.1),
                 "USB": (0.15, 0.4, 0.9), "Analog": (0.2, 0.7, 0.3), "Default": (0.55, 0.35, 0.75)}
NETCLASS = None  # net -> class, set by --by-class


def colour(net):
    if NETCLASS is not None:
        return CLASS_COLOURS.get(NETCLASS.get(net, "Default"), CLASS_COLOURS["Default"]) if net else (0.75, 0.75, 0.75)
    if not net:
        return (0.75, 0.75, 0.75)
    if net == "GND":
        return (0.33, 0.33, 0.36)
    h = int(hashlib.md5(net.encode()).hexdigest()[:8], 16)
    if any(p in net for p in POWER):
        return colorsys.hls_to_rgb(((h % 1000) / 1000) * 0.12, 0.45, 0.85)  # reds/oranges
    return colorsys.hls_to_rgb(0.15 + ((h % 1000) / 1000) * 0.8, 0.42 + (h >> 12) % 3 * 0.06, 0.75)


def path(polys):
    verts, codes = [], []
    for p in polys:
        for ring in [p["outline"]] + p["holes"]:
            if len(ring) < 3:
                continue
            verts += ring + [ring[0]]
            codes += [Path.MOVETO] + [Path.LINETO] * (len(ring) - 1) + [Path.CLOSEPOLY]
    return Path(verts, codes) if verts else None


def short(net):
    return net.lstrip("/")


def mst(points):
    if len(points) < 2:
        return []
    inside, edges = {0}, []
    while len(inside) < len(points):
        best = min(((math.dist(points[i], points[j]), i, j) for i in inside for j in range(len(points)) if j not in inside))
        inside.add(best[2])
        edges.append((points[best[1]], points[best[2]]))
    return edges


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("dump")
    ap.add_argument("out")
    ap.add_argument("--refs")
    ap.add_argument("--bbox")
    ap.add_argument("--pad", type=float, default=1.5)
    ap.add_argument("--layers", default="F.Cu,In1.Cu,In2.Cu,B.Cu")
    ap.add_argument("--ppmm", type=float, default=60)
    ap.add_argument("--rats", action="store_true")
    ap.add_argument("--rats-gnd", action="store_true")
    ap.add_argument("--title", default="")
    ap.add_argument("--by-class")
    ap.add_argument("--skip-zones", default="", help="comma-separated zone names not to draw (e.g. the board pours)")
    ap.add_argument("--labels", choices=["net", "refs", "none"], default="net")
    ap.add_argument("--no-legend", action="store_true")
    ap.add_argument("--only", help="draw only these footprints; tracks, vias and fills clipped to their box + 1 mm")
    ap.add_argument("--ports", help="JSON list of {pos, dir (up/down/left/right), text, color}: arrows and labels for "
                                    "nets leaving the frame")
    a = ap.parse_args()
    if a.by_class:
        global NETCLASS
        pro = json.load(open(a.by_class))
        NETCLASS = {}
        for pat in pro["net_settings"].get("netclass_patterns") or []:
            NETCLASS[pat["pattern"]] = pat["netclass"]
    d = json.load(open(a.dump))
    layers = [l for l in ORDER if l in a.layers.split(",")]

    if a.refs:
        refs = set(a.refs.split(","))
        pts = [q for f in d["footprints"] if f["ref"] in refs for c in f["courtyard"] for q in c["outline"]]
        pts += [q for p in d["pads"] if p["ref"] in refs for c in p["poly"] for q in c["outline"]]
        x0, y0 = min(p[0] for p in pts) - a.pad, min(p[1] for p in pts) - a.pad
        x1, y1 = max(p[0] for p in pts) + a.pad, max(p[1] for p in pts) + a.pad
    elif a.bbox:
        x0, y0, x1, y1 = map(float, a.bbox.split(","))
    else:
        pts = [q for e in d["edges"] for q in e] or [p["pos"] for p in d["pads"]]
        x0, y0 = min(p[0] for p in pts) - a.pad, min(p[1] for p in pts) - a.pad
        x1, y1 = max(p[0] for p in pts) + a.pad, max(p[1] for p in pts) + a.pad
    inbox = lambda p: x0 <= p[0] <= x1 and y0 <= p[1] <= y1
    if a.only:
        keep = set(a.only.split(","))
        d["pads"] = [p for p in d["pads"] if p["ref"] in keep]
        d["footprints"] = [f for f in d["footprints"] if f["ref"] in keep]

    w, h = x1 - x0, y1 - y0
    dpi = 100
    fig = plt.figure(figsize=(w * a.ppmm / dpi, h * a.ppmm / dpi + 0.4), dpi=dpi)
    ax = fig.add_axes([0, 0, 1, h / (h + 0.4 * dpi / a.ppmm)])
    ax.set_xlim(x0, x1)
    ax.set_ylim(y1, y0)
    ax.set_aspect("equal")
    ax.set_facecolor("#fbfaf6")
    ax.tick_params(labelsize=7)
    ax.grid(True, color="#e6e3da", lw=0.4)
    lw = lambda mm: mm * a.ppmm * 72 / dpi  # mm -> points

    for e in d["edges"]:
        ax.plot([p[0] for p in e], [p[1] for p in e], color="#c9a227", lw=1.2, zorder=1)
    skip = set(filter(None, a.skip_zones.split(",")))
    for z in d["zones"]:
        if z["layer"] not in layers or z.get("name") in skip:
            continue
        if z["keepout"]:
            p = path(z["outline"])
            if p:
                ax.add_patch(PathPatch(p, fill=False, hatch="xx", ec="#d08080", lw=0.6, zorder=2))
            continue
        p = path(z["fill"])
        if p:
            al = 0.45 if z["layer"] == "F.Cu" else 0.07
            ax.add_patch(PathPatch(p, fc=colour(z["net"]), ec="none", alpha=al, zorder=2))
    for f in d["footprints"]:
        for c in f["courtyard"]:
            ring = c["outline"] + c["outline"][:1]
            ax.plot([q[0] for q in ring], [q[1] for q in ring], color="#b8b2a3" if not f["locked"] else "#8a6f2a",
                    lw=0.5, zorder=3)
    for li, layer in enumerate(layers):
        ls, al = STYLE[layer]
        for t in d["tracks"]:
            if t["layer"] != layer:
                continue
            pts = t["arc"] if "arc" in t else [t["a"], t["b"]]
            ax.plot([q[0] for q in pts], [q[1] for q in pts], color=colour(t["net"]), lw=lw(t["w"]),
                    solid_capstyle="round", ls=ls, alpha=al, zorder=4 + li)
        for p in d["pads"]:
            if p["layer"] != layer:
                continue
            pp = path(p["poly"])
            if pp:
                ax.add_patch(PathPatch(pp, fc=colour(p["net"]), ec="black", lw=0.3, alpha=al if layer != "F.Cu" else 0.95,
                                       zorder=4 + li))
    for v in d["vias"]:
        ax.add_patch(Circle(v["pos"], v["d"] / 2, fc=colour(v["net"]), ec="black", lw=0.4, zorder=9))
        ax.add_patch(Circle(v["pos"], v["drill"] / 2, fc="white", ec="none", zorder=9.1))

    if a.rats:
        nets = {}
        for p in d["pads"]:
            if p["layer"] in ("F.Cu", "B.Cu") and p["net"] and (p["net"] != "GND" or a.rats_gnd) \
                    and not p["net"].startswith("unconnected"):
                nets.setdefault(p["net"], {})[(p["ref"], p["num"])] = p["pos"]
        for net, pads in nets.items():
            for pa, pb in mst(list(pads.values())):
                if inbox(pa) or inbox(pb):
                    ax.plot([pa[0], pb[0]], [pa[1], pb[1]], color=colour(net), lw=0.7, ls=(0, (3, 2)), zorder=10)

    seen = set()
    for p in d["pads"] if a.labels == "net" else []:
        if p["layer"] not in ("F.Cu", "B.Cu") or not inbox(p["pos"]) or not p["net"] or (p["ref"], p["num"]) in seen:
            continue
        seen.add((p["ref"], p["num"]))
        o = p["poly"][0]["outline"] if p["poly"] else []
        if not o:
            continue
        sz = min(max(q[0] for q in o) - min(q[0] for q in o), max(q[1] for q in o) - min(q[1] for q in o))
        fs = min(7.0, sz * a.ppmm * 72 / dpi / 3.2)
        if fs < 3.2:
            continue
        name = short(p["net"]) if not p["net"].startswith("unconnected") else "nc"
        ax.text(p["pos"][0], p["pos"][1], f"{p['num']}\n{name}", ha="center", va="center", fontsize=fs,
                color="white", fontweight="bold", zorder=12, linespacing=0.9)
    for f in d["footprints"] if a.labels != "none" else []:
        if inbox(f["pos"]) and f["courtyard"]:
            ys = [q[1] for c in f["courtyard"] for q in c["outline"]]
            ax.text(f["pos"][0], min(ys) - 0.08, f["ref"] + ("" if f["value"] == f["ref"] else f"  {f['value']}"),
                    ha="center", va="bottom", fontsize=7, color="#3a3a3a", zorder=13)
    if a.only:
        from matplotlib.patches import Rectangle
        keep = set(a.only.split(","))
        pts = [q for f in d["footprints"] if f["ref"] in keep for c in f["courtyard"] for q in c["outline"]]
        pts += [q for p in d["pads"] if p["ref"] in keep for c in p["poly"] for q in c["outline"]]
        cx0, cy0 = min(q[0] for q in pts) - 1.0, min(q[1] for q in pts) - 1.0
        cx1, cy1 = max(q[0] for q in pts) + 1.0, max(q[1] for q in pts) + 1.0
        clip = Rectangle((cx0, cy0), cx1 - cx0, cy1 - cy0, transform=ax.transData)
        for art in list(ax.lines) + list(ax.patches) + list(ax.texts):
            art.set_clip_path(clip)
    if a.ports:
        step = {"up": (0, -1), "down": (0, 1), "left": (-1, 0), "right": (1, 0)}
        placed = []
        for pt in json.load(open(a.ports)):
            dx, dy = step[pt["dir"]]
            x, y = pt["pos"]
            # stagger labels that would sit side by side (pins closer than a label is tall)
            k = sum(1 for q in placed if q[0] == pt["dir"] and abs((q[1] - x) * dy - (q[2] - y) * dx) < 0.9
                    and abs((q[1] - x) * dx + (q[2] - y) * dy) < 1.5)
            placed.append((pt["dir"], x, y))
            ln = 1.2 + 3.6 * (k % 2)
            ex, ey = x + dx * ln, y + dy * ln
            ax.annotate("", xy=(ex, ey), xytext=(x, y), zorder=14,
                        arrowprops=dict(arrowstyle="-|>", color=pt["color"], lw=1.2, mutation_scale=8))
            vert = dy != 0
            ax.text(ex + dx * 0.15, ey + dy * 0.15, pt["text"], color=pt["color"], fontsize=6, fontweight="bold",
                    rotation=90 if vert else 0, zorder=15,
                    ha={-1: "right", 1: "left", 0: "center"}[dx], va={-1: "bottom", 1: "top", 0: "center"}[dy],
                    bbox=dict(boxstyle="round,pad=0.12", fc="white", ec="none", alpha=0.85))
    if NETCLASS is not None and not a.no_legend:
        from matplotlib.patches import Patch
        ax.legend(handles=[Patch(color=c, label=k) for k, c in CLASS_COLOURS.items()], loc="lower right", fontsize=8,
                  framealpha=0.9)
    fig.suptitle(a.title or f"{a.dump}  layers: {','.join(layers)}", fontsize=9, y=0.995)
    fig.savefig(a.out, dpi=dpi)


if __name__ == "__main__":
    main()
