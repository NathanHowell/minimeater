"""Every layout block and the board at one scale, coloured by netclass, for placement reviews.

  uv run --with matplotlib --with pillow python hw/tools/montage.py <dump.json> <out.png> [--ppmm 20]

Each block is rendered with render.py (--refs, F.Cu and B.Cu, --by-class) and the board (outline, fixed parts, pours
hidden) beside them; the tiles are pasted at the same pixels per mm, so sizes compare directly.

Every net that leaves a block gets a port: an arrow at the pad (or track end) nearest the block's edge, pointing out
of that edge, labelled with the net and the blocks or fixed parts at its other end, and coloured by the first of
those. The fixed parts on the board get the same labels, pointing into the board. GND is left out (the plane takes
it); test points are not destinations (they are placed last).
"""

import argparse
import colorsys
import json
import os
import subprocess
import sys
import tempfile

from PIL import Image, ImageDraw

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from render import CLASS_COLOURS  # noqa: E402

HERE = os.path.dirname(os.path.abspath(__file__))
PRO = os.path.join(HERE, "..", "minimeater.kicad_pro")

# (tag, title, refs)
BLOCKS = [
    ("U4", "U4 buck 3V3", "U4,L1,C4,C5,C6,C7,C14,R3,R4"),
    ("U5", "U5 LDO 3V3A", "U5,C8,C9,C15,FB1"),
    ("U6/7", "U7 fan / U6 servo switches", "U6,U7,C10,C11,C12,C13,C16,C17,R5,R6,R8,R9"),
    ("U2", "U2 USB ESD", "U2,R1,R2"),
    ("VBUS", "VBUS entry", "D1,F1,C1,C2,C3"),
    ("U9", "U9/U14 probe ESD", "U9,U14"),
    ("U11", "U11 secure element", "U11,C34"),
    ("U8", "U8 probe ADC", "U8,C24,C25,C26,C27,C28,C29,C30,C31,C32,R17,R18,R19,R20,R21,R22,R23,R24,R25,R26,FB2"),
    ("U1", "U1 ESP32-C6 module", "U1,C18,C19,C20,C21,C22,R10"),
    ("J6p", "J6 fan parts", "R27,R28,R16,C35"),
    ("J7p", "J7 servo parts", "R29,R30,C36,C37"),
    ("BTN", "Boot button", "SW1,R13,C23"),
    ("PU", "Pull-ups", "R14,R15,R7,R11"),
    ("C33", "C33 (at U10)", "C33"),
    ("R12", "R12 (at D2)", "R12"),
]
FIXED = ["J1", "J2", "J3", "J4", "J5", "J6", "J7", "D2", "U10"]
SKIP_NETS = {"GND"}


def tag_colours(tags):
    out = {}
    for i, t in enumerate(tags):
        h = (i * 0.618034) % 1.0
        out[t] = colorsys.hls_to_rgb(h, 0.38, 0.85)
    return out


def bbox(points):
    xs, ys = [p[0] for p in points], [p[1] for p in points]
    return min(xs), min(ys), max(xs), max(ys)


def ports(d, owner, colours, members, inward=False, frame=None):
    """Ports for the parts in `members`: one per (net leaving them), at the point nearest the frame edge."""
    pads = [p for p in d["pads"] if p["ref"] in members and p["layer"] in ("F.Cu", "B.Cu") and p["net"]]
    if frame is None:
        frame = bbox([q for f in d["footprints"] if f["ref"] in members for c in f["courtyard"] for q in c["outline"]]
                     + [p["pos"] for p in pads])
    x0, y0, x1, y1 = frame
    out, seen = [], set()
    for p in pads:
        net = p["net"]
        if net in SKIP_NETS or net.startswith("unconnected") or net in seen:
            continue
        dest = sorted({owner.get(q["ref"]) for q in d["pads"] if q["net"] == net} - {None, owner.get(p["ref"])})
        if not dest:
            continue
        seen.add(net)
        def edge(c):
            return min((c[1] - y0, "up"), (y1 - c[1], "down"), (c[0] - x0, "left"), (x1 - c[0], "right"))
        # exits: the open ends of the net's tracks inside the frame (drawn on purpose as exit stubs), else the pad
        # nearest the frame edge
        ends = [e for t in d["tracks"] if t["net"] == net for e in (t["a"], t["b"])
                if x0 - 3 <= e[0] <= x1 + 3 and y0 - 3 <= e[1] <= y1 + 3]
        npads = [q for q in d["pads"] if q["net"] == net]
        open_ends = [e for e in ends if sum(1 for f in ends if abs(f[0] - e[0]) < 1e-3 and abs(f[1] - e[1]) < 1e-3) == 1
                     and not any(abs(q["pos"][0] - e[0]) < 0.5 and abs(q["pos"][1] - e[1]) < 0.5 for q in npads)]
        exits = open_ends or [min([q["pos"] for q in pads if q["net"] == net], key=lambda c: edge(c)[0])]
        name = net.lstrip("/")
        shown = dest if len(dest) <= 3 else dest[:3] + ["..."]
        centre = {f["ref"]: f["pos"] for f in d["footprints"]}
        for pos in exits:
            side = edge(centre[p["ref"]] if inward else pos)[1]
            if inward:
                side = {"up": "down", "down": "up", "left": "right", "right": "left"}[side]
            out.append({"pos": pos, "dir": side, "text": f"{name} > {','.join(shown)}", "color": colours[dest[0]]})
    return out


def render(dump, out, ppmm, extra):
    subprocess.run([sys.executable, os.path.join(HERE, "render.py"), dump, out, "--ppmm", str(ppmm), "--layers", "F.Cu,B.Cu",
                    "--by-class", PRO, "--labels", "refs", "--no-legend"] + extra, check=True)
    return Image.open(out)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("dump")
    ap.add_argument("out")
    ap.add_argument("--ppmm", type=float, default=20)
    ap.add_argument("--board", default="-2,0,47,55.5")
    a = ap.parse_args()
    d = json.load(open(a.dump))
    owner = {r: tag for tag, _, refs in BLOCKS for r in refs.split(",")}
    owner.update({r: r for r in FIXED})
    colours = tag_colours([b[0] for b in BLOCKS] + FIXED)
    tmp = tempfile.mkdtemp()

    def portfile(name, pts):
        path = os.path.join(tmp, name)
        json.dump(pts, open(path, "w"))
        return path

    bx = list(map(float, a.board.split(",")))
    edges = [q for e in d["edges"] for q in e if q[1] < 60]
    board = render(a.dump, os.path.join(tmp, "board.png"), a.ppmm,
                   ["--bbox=" + a.board, "--skip-zones", "GND", "--title", "Board: outline and fixed parts",
                    "--ports", portfile("board.json", ports(d, owner, colours, set(FIXED), inward=True,
                                                            frame=bbox(edges)))])
    tiles = []
    for i, (tag, name, refs) in enumerate(BLOCKS):
        pts = ports(d, owner, colours, set(refs.split(",")))
        tiles.append(render(a.dump, os.path.join(tmp, f"b{i}.png"), a.ppmm,
                            ["--refs", refs, "--only", refs, "--pad", "9", "--title", f"{name}  [{tag}]",
                             "--ports", portfile(f"b{i}.json", pts)]))
    # shelf-pack the blocks to the right of the board, rows no wider than the board is tall
    gap, width_cap = 12, int(board.width * 2.6)
    rows, row, w = [], [], 0
    for t in sorted(tiles, key=lambda t: -t.height):
        if row and w + t.width > width_cap:
            rows.append(row)
            row, w = [], 0
        row.append(t)
        w += t.width + gap
    rows.append(row)
    rw = max(sum(t.width + gap for t in r) for r in rows)
    rh = sum(max(t.height for t in r) + gap for r in rows)
    canvas = Image.new("RGB", (board.width + gap * 2 + rw, max(board.height, rh) + 30), "white")
    canvas.paste(board, (0, 30))
    y = 30
    for r in rows:
        x = board.width + gap * 2
        for t in r:
            canvas.paste(t, (x, y))
            x += t.width + gap
        y += max(t.height for t in r) + gap
    draw = ImageDraw.Draw(canvas)
    draw.text((8, 8), f"All blocks and the board at one scale ({a.ppmm:g} px/mm). Copper colour = netclass:",
              fill="black")
    x = 420
    for name, c in CLASS_COLOURS.items():
        draw.rectangle([x, 6, x + 14, 18], fill=tuple(int(v * 255) for v in c))
        draw.text((x + 18, 8), name, fill="black")
        x += 30 + 7 * len(name)
    draw.text((x + 20, 8), "Arrows: net leaving the block > where it goes (arrow colour = first destination)",
              fill="black")
    canvas.save(a.out)


if __name__ == "__main__":
    main()
