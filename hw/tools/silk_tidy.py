"""Silkscreen tidy: python silk_tidy.py <in.kicad_pcb> <out.kicad_pcb>   (KiCad's bundled Python, on a copy of the board)

1. Footprint silk lines that cross, or lie outside, the board edge (the jacks' noses past the trimmed edge, the
   module's antenna end) are clipped to the board less a 0.2 mm margin; what is left shorter than 0.2 mm goes.
2. Every visible silk reference that sits on a pad, on other silk, on another reference or over the edge is moved to
   the nearest free spot (0 or 90 degrees, within 2 mm of the part's own outline and nearer to it than to any other part); where there is none it goes to
   F.Fab, like the passives' references. Fiducials' references always go to F.Fab.
"""
import sys, math
sys.path.insert(0, '/Applications/KiCad/KiCad.app/Contents/Frameworks/Python.framework/Versions/Current/lib/python3.9/site-packages')
import pcbnew

mm, MM = pcbnew.ToMM, pcbnew.FromMM
b = pcbnew.LoadBoard(sys.argv[1])
X0, Y0, X1, Y1 = 0.2, 1.75 + 0.2, 46 - 0.2, 54 - 0.2   # the board less the margin (the corners are 2 mm radius: no silk goes there)


def box(i, pad=0.0):
    r = i.GetBoundingBox()
    return (mm(r.GetLeft()) - pad, mm(r.GetTop()) - pad, mm(r.GetRight()) + pad, mm(r.GetBottom()) + pad)


def hit(a, c):
    return a[0] < c[2] and a[2] > c[0] and a[1] < c[3] and a[3] > c[1]


def outside(a):
    return a[0] < X0 or a[1] < Y0 or a[2] > X1 or a[3] > Y1


def clip(p, q):
    """Liang-Barsky: the part of segment pq inside the margin rectangle, or None."""
    (x0, y0), (x1, y1) = p, q
    t0, t1 = 0.0, 1.0
    for d, dist in ((x1 - x0, x0 - X0), (x0 - x1, X1 - x0), (y1 - y0, y0 - Y0), (y0 - y1, Y1 - y0)):
        # the point at t is inside this side when dist + t * d >= 0
        if d == 0:
            if dist < 0:
                return None
        elif d > 0:
            t0 = max(t0, -dist / d)
        else:
            t1 = min(t1, -dist / d)
    if t0 > t1:
        return None
    return (x0 + t0 * (x1 - x0), y0 + t0 * (y1 - y0)), (x0 + t1 * (x1 - x0), y0 + t1 * (y1 - y0))


# 1. footprint silk lines against the edge
trimmed = []
drop = []   # removed after the scan: removing while iterating upsets the bindings
for fp in b.GetFootprints():
    for g in [g for g in fp.GraphicalItems()]:
        if g.GetLayer() != pcbnew.F_SilkS or g.GetClass() != 'PCB_SHAPE' or g.GetShape() != pcbnew.SHAPE_T_SEGMENT:
            continue
        p, q = (mm(g.GetStart().x), mm(g.GetStart().y)), (mm(g.GetEnd().x), mm(g.GetEnd().y))
        if min(p[0], q[0]) >= X0 and max(p[0], q[0]) <= X1 and min(p[1], q[1]) >= Y0 and max(p[1], q[1]) <= Y1:
            continue
        c = clip(p, q)
        if c is None or math.dist(*c) < 0.2:
            drop.append((fp, g)); trimmed.append((fp.GetReference(), 'removed'))
        else:
            g.SetStart(pcbnew.VECTOR2I(MM(c[0][0]), MM(c[0][1]))); g.SetEnd(pcbnew.VECTOR2I(MM(c[1][0]), MM(c[1][1])))
            trimmed.append((fp.GetReference(), 'clipped'))
for fp, g in drop:
    fp.Remove(g)
print('edge:', trimmed)

# 2. references
refs = {fp.GetReference(): fp for fp in b.GetFootprints()}


def dist(a, c):
    return math.hypot(max(c[0] - a[2], a[0] - c[2], 0), max(c[1] - a[3], a[1] - c[3], 0))


def own_box(fp):
    r = fp.GetBoundingBox(False)   # without the texts
    return (mm(r.GetLeft()), mm(r.GetTop()), mm(r.GetRight()), mm(r.GetBottom()))


def obstacles(skip):
    o = []
    for r, fp in refs.items():
        for p in fp.Pads():
            o.append((r + ' pad', box(p, 0.1)))
        for g in fp.GraphicalItems():
            if g.GetLayer() == pcbnew.F_SilkS:
                o.append((r + ' silk', box(g, 0.05)))
        if r not in skip:
            t = fp.Reference()
            if t.IsVisible() and t.GetLayer() == pcbnew.F_SilkS:
                o.append((r + ' ref', box(t, 0.05)))
        if r.startswith('H'):
            o.append((r + ' hole', (mm(fp.GetX()) - 1.65, mm(fp.GetY()) - 1.65, mm(fp.GetX()) + 1.65, mm(fp.GetY()) + 1.65)))
    for d in b.GetDrawings():
        if d.GetLayer() == pcbnew.F_SilkS:
            o.append(('board silk', box(d, 0.05)))
    return o


bad = []
obs0 = obstacles(set())
for r, fp in refs.items():
    t = fp.Reference()
    if not t.IsVisible() or t.GetLayer() != pcbnew.F_SilkS:
        continue
    if r.startswith('FID'):
        bad.append(r); continue
    bx = box(t)
    if outside(bx) or any(hit((bx[0] + .01, bx[1] + .01, bx[2] - .01, bx[3] - .01), o[1]) for o in obs0 if o[0] not in (r + ' ref', r + ' silk')):
        bad.append(r)
print('references to move:', bad)

for r in bad:
    fp = refs[r]; t = fp.Reference()
    if r.startswith('FID'):
        t.SetLayer(pcbnew.F_Fab); print(r, '-> F.Fab'); continue
    obs = obstacles(set(bad[bad.index(r):]) )   # references not yet placed do not count
    ob = own_box(fp)
    others = [own_box(f) for rr, f in refs.items() if rr != r and not rr.startswith(('FID', 'H'))]
    best = None
    ox, oy = mm(t.GetX()), mm(t.GetY())
    cx, cy = (ob[0] + ob[2]) / 2, (ob[1] + ob[3]) / 2
    for rot in (0, 90):
        t.SetTextAngleDegrees(rot)
        for ix in range(-70, 71):
            for iy in range(-70, 71):
                x, y = cx + ix * 0.1, cy + iy * 0.1
                t.SetPosition(pcbnew.VECTOR2I(MM(x), MM(y)))
                bx = box(t, 0.05)
                if outside(bx):
                    continue
                gap = dist(bx, ob)
                if gap > 2.0 or min(dist(bx, o) for o in others) < gap + 0.3 or any(hit(bx, o[1]) for o in obs if not o[0].startswith(r + ' ')) or hit(bx, ob):   # not under the part itself (its body would cover the text)
                    continue
                s = gap + 0.15 * (rot == 90) + 0.01 * math.hypot(x - ox, y - oy)
                if best is None or s < best[0]:
                    best = (s, x, y, rot)
    if best:
        t.SetTextAngleDegrees(best[3]); t.SetPosition(pcbnew.VECTOR2I(MM(best[1]), MM(best[2])))
        print(r, '->', round(best[1], 2), round(best[2], 2), best[3])
    else:
        t.SetLayer(pcbnew.F_Fab); t.SetTextAngleDegrees(0); t.SetPosition(pcbnew.VECTOR2I(MM(cx), MM(cy)))
        print(r, '-> F.Fab (no free spot)')

# 3. two single items: the module's "Antenna Area" note sat across the edge, and L1's pin-1 dot on its pad
for fp in b.GetFootprints():
    if fp.GetReference() == 'U1':
        for g in [g for g in fp.GraphicalItems()]:
            if g.GetClass() == 'PCB_TEXT' and g.GetText() == 'Antenna Area':
                g.SetPosition(pcbnew.VECTOR2I(g.GetX(), MM(52.8)))   # was 53.7: its lower half was past the edge
        fp.Reference().SetLayer(pcbnew.F_Fab)   # under the module's body, over its ground pad
    if fp.GetReference() == 'L1':
        for g in [g for g in fp.GraphicalItems()]:
            if g.GetLayer() == pcbnew.F_SilkS and g.GetClass() == 'PCB_SHAPE' and g.GetShape() == pcbnew.SHAPE_T_CIRCLE:
                g.SetCenter(pcbnew.VECTOR2I(MM(0.4), MM(26.5)))   # beside pad 1, not on it
                g.SetEnd(pcbnew.VECTOR2I(MM(0.4 + 0.06), MM(26.5)))

pcbnew.SaveBoard(sys.argv[2], b)
