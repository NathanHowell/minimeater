"""Silk pass (KiCad closed; KiCad's python): references at JLC minimum size (1.0 mm, 0.15 stroke) beside their part only
where they fit clear of pads, holes, part bodies and other silk (connectors and ICs first), else hidden; the test
points and the KK headers get back-silk labels (net names, FAN/SERVO). Usage: silk.py hw/minimeater.kicad_pcb
Also drops the old staging In1 zone if it is still there."""
import sys, math
import pcbnew
path = sys.argv[1]
b = pcbnew.LoadBoard(path)
mm, tomm = pcbnew.FromMM, pcbnew.ToMM
zones = [z for z in b.Zones() if z.GetZoneName() == "STAGING_In1_GND"]
for z in zones: b.Remove(z)
print("removed zones", len(zones))

X0, Y0, X1, Y1, C = -1.5, 1.75, 46.0, 54.0, 0.1
NX, NY = int((X1 - X0) / C) + 1, int((Y1 - Y0) / C) + 1
grid = {"F": bytearray(NX * NY), "B": bytearray(NX * NY)}
def mark(side, x0, y0, x1, y1, pad=0.0):
    g = grid[side]
    for i in range(max(0, int((x0 - pad - X0) / C)), min(NX, int((x1 + pad - X0) / C) + 1)):
        for j in range(max(0, int((y0 - pad - Y0) / C)), min(NY, int((y1 + pad - Y0) / C) + 1)):
            g[j * NX + i] = 1
def free(side, x0, y0, x1, y1):
    if x0 < X0 + 0.5 or y0 < Y0 + 0.5 or x1 > X1 - 0.5 or y1 > Y1 - 0.5: return False
    if x1 > 44.0 - (y1 - 52.0) and y1 > 52.0: return False  # chamfered corners
    g = grid[side]
    for i in range(int((x0 - X0) / C), int((x1 - X0) / C) + 1):
        for j in range(int((y0 - Y0) / C), int((y1 - Y0) / C) + 1):
            if g[j * NX + i]: return False
    return True
def bbox(item):
    r = item.GetBoundingBox()
    return tomm(r.GetX()), tomm(r.GetY()), tomm(r.GetRight()), tomm(r.GetBottom())
def seg(side, a, c, w):
    n = max(1, int(math.dist(a, c) / C))
    for k in range(n + 1):
        x = a[0] + (c[0] - a[0]) * k / n; y = a[1] + (c[1] - a[1]) * k / n
        mark(side, x, y, x, y, w / 2 + 0.15)

sideof = lambda fp: "B" if fp.GetLayer() == pcbnew.B_Cu else "F"
fps = list(b.GetFootprints())
for fp in fps:
    s = sideof(fp)
    for p in fp.Pads():
        x0, y0, x1, y1 = bbox(p)
        for side in ("F", "B"):
            if p.IsOnLayer(pcbnew.F_Cu if side == "F" else pcbnew.B_Cu) or p.GetAttribute() == pcbnew.PAD_ATTRIB_NPTH:
                mark(side, x0, y0, x1, y1, 0.2)
    cy = fp.GetCourtyard(pcbnew.F_CrtYd if s == "F" else pcbnew.B_CrtYd)
    if cy.OutlineCount():
        r = cy.BBox(); mark(s, tomm(r.GetX()), tomm(r.GetY()), tomm(r.GetRight()), tomm(r.GetBottom()), 0.05)
    for g in fp.GraphicalItems():
        if g.GetLayer() in (pcbnew.F_SilkS, pcbnew.B_SilkS) and g.GetClass() != "PCB_TEXT":
            gs = "B" if g.GetLayer() == pcbnew.B_SilkS else "F"
            if g.GetShape() == pcbnew.SHAPE_T_SEGMENT:
                a, c = g.GetStart(), g.GetEnd()
                seg(gs, (tomm(a.x), tomm(a.y)), (tomm(c.x), tomm(c.y)), tomm(g.GetWidth()))
            else:
                x0, y0, x1, y1 = bbox(g); mark(gs, x0, y0, x1, y1, 0.15)
# mounting holes' standoff rings (screw heads / standoff tops on both faces)
for hx, hy in ((12.05, 28.5), (43.0, 29.8), (43.0, 49.6)):
    for side in ("F", "B"): mark(side, hx - 2.75, hy - 2.75, hx + 2.75, hy + 2.75)
mark("F", 14.5, 51.25, 32.5, 57.25); mark("B", 14.5, 51.25, 32.5, 57.25)

def rank(fp):
    r = fp.GetReference()
    for i, pre in enumerate(("J", "U", "SW", "D", "L", "F", "FB", "TP", "C", "R")):
        if r.startswith(pre): return (i, r)
    return (99, r)
LABEL = {"TP1": "5V", "TP2": "3V3", "TP3": "GND", "TP4": "3V3A", "TP5": "RX", "TP6": "GND", "TP7": "TX", "TP8": "EN",
         "TP9": "SDA", "TP10": "SCL", "J6": "FAN", "J7": "SERVO"}  # on the back: what you probe or plug in, with the case open
shown, hidden = [], []
for fp in sorted(fps, key=rank):
    ref = fp.Reference(); r = fp.GetReference()
    if r.startswith(("H", "FID")) or not r:
        ref.SetVisible(False); continue
    s = "B" if r in ("J6", "J7") else sideof(fp)  # THT headers: labelled on the back, the face seen with the case open
    ref.SetTextSize(pcbnew.VECTOR2I(mm(1.0), mm(1.0))); ref.SetTextThickness(mm(0.15))
    ref.SetLayer(pcbnew.B_SilkS if s == "B" else pcbnew.F_SilkS)
    cy = fp.GetCourtyard(pcbnew.F_CrtYd if sideof(fp) == "F" else pcbnew.B_CrtYd)
    if cy.OutlineCount():
        rb = cy.BBox(); fx0, fy0, fx1, fy1 = tomm(rb.GetX()), tomm(rb.GetY()), tomm(rb.GetRight()), tomm(rb.GetBottom())
    else:
        fx0, fy0, fx1, fy1 = bbox(fp)
    cx, cyy = (fx0 + fx1) / 2, (fy0 + fy1) / 2
    txt = LABEL.get(r, r)
    w = 0.8 * len(txt) + 0.15  # glyph advance at 1 mm, plus stroke
    h = 1.15
    cands = []
    gaps = (0.15, 0.4, 0.8, 1.2, 1.6) if rank(fp)[0] <= 6 else (0.15, 0.4, 0.8)
    for gap in gaps:
        cands += [(0, fx0 + w / 2, fy0 - gap - h / 2), (0, fx1 - w / 2, fy0 - gap - h / 2), (0, fx0 + w / 2, fy1 + gap + h / 2), (0, fx1 - w / 2, fy1 + gap + h / 2),
                  (0, fx0 - gap - w / 2, fy0 + h / 2), (0, fx1 + gap + w / 2, fy0 + h / 2), (0, fx0 - gap - w / 2, fy1 - h / 2), (0, fx1 + gap + w / 2, fy1 - h / 2)]
        cands += [(0, cx, fy0 - gap - h / 2), (0, cx, fy1 + gap + h / 2), (90, fx0 - gap - h / 2, cyy), (90, fx1 + gap + h / 2, cyy),
                  (0, fx0 - gap - w / 2, cyy), (0, fx1 + gap + w / 2, cyy)]
    done = False
    for ang, x, y in cands:
        tw, th = (w, h) if ang == 0 else (h, w)
        x0, y0, x1, y1 = x - tw / 2, y - th / 2, x + tw / 2, y + th / 2
        if free(s, x0, y0, x1, y1):
            if r in LABEL:
                ref.SetVisible(False)
                t = pcbnew.PCB_TEXT(b); t.SetText(txt); t.SetLayer(pcbnew.B_SilkS if s == "B" else pcbnew.F_SilkS)
                t.SetTextSize(pcbnew.VECTOR2I(mm(1.0), mm(1.0))); t.SetTextThickness(mm(0.15))
                t.SetTextAngleDegrees(ang); t.SetPosition(pcbnew.VECTOR2I(mm(x), mm(y))); t.SetMirrored(s == "B")
                b.Add(t)
            else:
                ref.SetVisible(True)
                ref.SetTextAngleDegrees(ang)
                ref.SetPosition(pcbnew.VECTOR2I(mm(x), mm(y)))
                ref.SetMirrored(s == "B")
            mark(s, x0, y0, x1, y1, 0.15)
            shown.append(r); done = True
            break
    if not done:
        ref.SetVisible(False); hidden.append(r)
print("shown", len(shown), " ".join(shown))
print("hidden", len(hidden), " ".join(hidden))
pcbnew.SaveBoard(path, b)
