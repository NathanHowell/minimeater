"""Every layout block and the board at one scale, coloured by netclass, for placement reviews.

  uv run --with matplotlib --with pillow python hw/tools/montage.py <dump.json> <out.png> [--ppmm 20]

Each block is rendered with render.py (--refs, F.Cu, --by-class) and the board (outline, fixed parts, pours hidden)
beside them; the tiles are pasted at the same pixels per mm, so sizes compare directly.
"""

import argparse
import os
import subprocess
import sys
import tempfile

from PIL import Image, ImageDraw

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from render import CLASS_COLOURS  # noqa: E402

HERE = os.path.dirname(os.path.abspath(__file__))
PRO = os.path.join(HERE, "..", "minimeater.kicad_pro")

BLOCKS = [
    ("U4 buck 3V3", "U4,L1,C4,C5,C6,C7,C14,R3,R4"),
    ("U5 LDO 3V3A", "U5,C8,C9,C15,FB1"),
    ("U7 fan / U6 servo switches", "U6,U7,C10,C11,C12,C13,C16,C17,R5,R6,R8,R9"),
    ("U2/U3 USB ESD", "U2,U3,C38,R1,R2"),
    ("VBUS entry", "D1,F1,C1,C2,C3"),
    ("U9 probe ESD", "U9"),
    ("U11 secure element", "U11,C34"),
    ("U8 probe ADC", "U8,C24,C25,C26,C27,C28,C29,C30,C31,C32,R17,R18,R19,R20,R21,R22,R23,R24,R25,R26,FB2"),
    ("U1 ESP32-C6 module", "U1,C18,C19,C20,C21,C22,R10"),
    ("J6 fan parts", "R27,R28,R16,C35"),
    ("J7 servo parts", "R29,R30,C36,C37"),
    ("Boot button", "SW1,R13,C23"),
    ("Pull-ups", "R14,R15,R7,R11"),
    ("Test points (B.Cu)", "TP1,TP2,TP3,TP4,TP5,TP6,TP7,TP8"),
    ("C33 (at U10)", "C33"),
    ("R12 (at D2)", "R12"),
]


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
    tmp = tempfile.mkdtemp()
    board = render(a.dump, os.path.join(tmp, "board.png"), a.ppmm,
                   ["--bbox=" + a.board, "--skip-zones", "GND", "--title", "Board: outline and fixed parts"])
    tiles = [render(a.dump, os.path.join(tmp, f"b{i}.png"), a.ppmm, ["--refs", refs, "--pad", "1.5", "--title", name])
             for i, (name, refs) in enumerate(BLOCKS)]
    # shelf-pack the blocks to the right of the board, rows no wider than the board is tall
    gap, width_cap = 12, int(board.height * 1.1)
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
    draw.text((8, 8), f"All blocks and the board at one scale ({a.ppmm:g} px/mm), colour = netclass:", fill="black")
    x = 330
    for name, c in CLASS_COLOURS.items():
        draw.rectangle([x, 6, x + 14, 18], fill=tuple(int(v * 255) for v in c))
        draw.text((x + 18, 8), name, fill="black")
        x += 30 + 7 * len(name)
    canvas.save(a.out)


if __name__ == "__main__":
    main()
