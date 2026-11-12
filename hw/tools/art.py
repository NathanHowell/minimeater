"""Silkscreen artwork: `just hw::art <font.ttf>` writes hw/art/.

logo.svg: a flame (hollow inner tongue) and the wordmark "minimeater" (Lilita One, OFL, from google/fonts), as filled
paths; qr.svg: the repository URL as a QR code, the light modules and a two-module quiet zone as squares (dark
modules are the bare mask between them, so the code reads the normal way round). Sizes are in mm."""

import sys
from pathlib import Path

import segno
from matplotlib.font_manager import FontProperties
from matplotlib.path import Path as MPath
from matplotlib.textpath import TextPath

OUT = Path(__file__).resolve().parents[1] / "art"
URL = "https://github.com/NathanHowell/minimeater"

FLAME = ("M5 14 C1.5 14 0 11.5 0 9 C0 6 2.5 4.5 3 1.5 C5.5 3 6 5.5 5.6 7.2 C6.6 6.6 7.2 5.6 7.3 4.3 "
         "C9.2 6 10 7.8 10 9.6 C10 12.2 7.8 14 5 14 Z "
         "M5 12.9 C3.6 12.9 2.9 12 2.9 10.9 C2.9 9.6 4 8.7 4.6 7.5 C5.5 8.4 6 9.2 5.9 10.1 C6.4 9.9 6.8 9.4 7 8.9 "
         "C7.4 9.6 7.3 10.3 7.3 10.9 C7.3 12.1 6.4 12.9 5 12.9 Z")


def textpath_d(text, font, size, x0, base):
    tp = TextPath((0, 0), text, size=size, prop=FontProperties(fname=font))
    d = []
    for verts, code in tp.iter_segments(curves=True, simplify=False):
        pts = [(x0 + verts[i], base - verts[i + 1]) for i in range(0, len(verts), 2)]
        f = lambda p: f"{p[0]:.3f} {p[1]:.3f}"
        if code == MPath.MOVETO:
            d.append("M" + f(pts[0]))
        elif code == MPath.LINETO:
            d.append("L" + f(pts[0]))
        elif code == MPath.CURVE3:
            d.append("Q" + " ".join(map(f, pts)))
        elif code == MPath.CURVE4:
            d.append("C" + " ".join(map(f, pts)))
        elif code == MPath.CLOSEPOLY:
            d.append("Z")
    ext = tp.get_extents()
    return " ".join(d), x0 + ext.x1


def logo(font):
    s = 0.36  # the flame: 10 x 14 units -> 3.6 x 5.0 mm
    flame = f'<path transform="scale({s})" fill-rule="evenodd" d="{FLAME}"/>'
    word, x1 = textpath_d("minimeater", font, 4.6, 4.4, 4.9)
    w, h = x1 + 0.1, 5.1
    svg = (f'<svg xmlns="http://www.w3.org/2000/svg" width="{w:.2f}mm" height="{h:.2f}mm" viewBox="0 0 {w:.3f} {h:.3f}">'
           f'{flame}<path fill-rule="nonzero" d="{word}"/></svg>')
    (OUT / "logo.svg").write_text(svg)
    return w, h


def qr(size=11.0, quiet=2):
    q = segno.make(URL, error="m", micro=False)
    m = [list(r) for r in q.matrix]
    n = len(m) + 2 * quiet
    u = size / n
    light = lambda i, j: not (quiet <= i < n - quiet and quiet <= j < n - quiet and m[i - quiet][j - quiet])
    rects = []
    for i in range(n):  # rows of light runs
        j = 0
        while j < n:
            if light(i, j):
                k = j
                while k < n and light(i, k):
                    k += 1
                rects.append(f'<rect x="{j * u:.4f}" y="{i * u:.4f}" width="{(k - j) * u:.4f}" height="{u:.4f}"/>')
                j = k
            else:
                j += 1
    svg = f'<svg xmlns="http://www.w3.org/2000/svg" width="{size}mm" height="{size}mm" viewBox="0 0 {size} {size}">{"".join(rects)}</svg>'
    (OUT / "qr.svg").write_text(svg)
    return q.version, len(m), u


if __name__ == "__main__":
    OUT.mkdir(exist_ok=True)
    print("logo", logo(sys.argv[1]))
    print("qr version, modules, module mm", qr())
