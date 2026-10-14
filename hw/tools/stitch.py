"""Edge GND stitching candidates (KiCad python): stitch.py <board> <out.json> [<scratch.kicad_pcb>]
0.6/0.3 GND vias 0.9 mm in from the outline, about 3 mm apart, each clear of other nets' pads and tracks, holes, the
F.Cu power zones, rule areas, courtyards (bar the KK headers), the antenna keepout, the standoff rings and the silk.
With a scratch path it also writes a copy with the vias added, for a refilled DRC before they go in through Konnect
(which cannot delete vias)."""
import sys, json, math, pcbnew
b = pcbnew.LoadBoard(sys.argv[1]); mm, t = pcbnew.FromMM, pcbnew.ToMM
X0, Y0, X1, Y1, R = -1.5, 1.75, 46.0, 54.0, 2.0
INSET, PITCH, MINGAP = 0.9, 3.0, 2.2
gnd = b.GetNetcodeFromNetname("GND")
L = [pcbnew.F_Cu, pcbnew.In1_Cu, pcbnew.In2_Cu, pcbnew.B_Cu]
def path():
    """Perimeter inset by INSET, clockwise from top-left, 0.1 mm steps (bottom corners rounded)."""
    pts = []; x0, y0, x1, y1 = X0 + INSET, Y0 + INSET, X1 - INSET, Y1 - INSET
    def seg(a, c):
        n = max(1, int(math.dist(a, c) / 0.1))
        pts.extend((a[0] + (c[0] - a[0]) * k / n, a[1] + (c[1] - a[1]) * k / n) for k in range(n))
    r = R - INSET
    seg((x0, y0), (x1, y0)); seg((x1, y0), (x1, Y1 - R))
    for k in range(16): a = math.pi / 2 * k / 16; pts.append((X1 - R + r * math.cos(a), Y1 - R + r * math.sin(a)))
    seg((X1 - R, y1), (X0 + R, y1))
    for k in range(16): a = math.pi / 2 + math.pi / 2 * k / 16; pts.append((X0 + R + r * math.cos(a), Y1 - R + r * math.sin(a)))
    seg((x0, Y1 - R), (x0, y0))
    return pts
items = []
for fp in b.GetFootprints():
    for p in fp.Pads(): items.append(p)
    for z in fp.Zones(): items.append(z)
for tr in b.GetTracks(): items.append(tr)
fills = [z for z in b.Zones() if not z.GetIsRuleArea() and z.GetNetCode() != gnd and z.GetLayerSet().Seq()[0] != pcbnew.In2_Cu]  # In2 planes take an antipad
rules = [z for z in b.Zones() if z.GetIsRuleArea()] + [z for fp in b.GetFootprints() for z in fp.Zones() if z.GetIsRuleArea()]
courts = [fp.GetCourtyard(l) for fp in b.GetFootprints() if fp.GetReference() not in ("J6", "J7") for l in (pcbnew.F_CrtYd, pcbnew.B_CrtYd)]  # tented vias under the KK bodies are fine
silk = []
for d in b.GetDrawings():
    if d.GetLayer() in (pcbnew.B_SilkS, pcbnew.F_SilkS): silk.append(d.GetEffectiveShape().BBox() if d.GetClass() == "PCB_TEXT" else d.GetBoundingBox())
for fp in b.GetFootprints():
    for f in (fp.Reference(),):
        if f.IsVisible(): silk.append(f.GetEffectiveShape().BBox())
holes = ((10.6, 28.7), (43.0, 29.8), (43.0, 49.6))
existing = [(t(v.GetPosition().x), t(v.GetPosition().y)) for v in b.GetTracks() if v.GetClass() == "PCB_VIA" and v.GetNetCode() == gnd]
def ok(x, y):
    c = pcbnew.VECTOR2I(mm(x), mm(y))
    if 14.5 - 0.5 < x < 32.5 + 0.5 and y > 51.25 - 0.5: return "antenna"
    for hx, hy in holes:
        if math.hypot(x - hx, y - hy) < 2.75 + 0.35: return "standoff"
    circ = pcbnew.SHAPE_CIRCLE(c, mm(0.3))
    for it in items:
        same = it.GetNetCode() == gnd if hasattr(it, "GetNetCode") else False
        if it.GetClass() == "PCB_VIA":
            if math.hypot(x - t(it.GetPosition().x), y - t(it.GetPosition().y)) < 1.0: return "via"
            continue
        if it.GetClass() == "PAD":
            for l in L:
                if it.IsOnLayer(l) and it.GetEffectiveShape(l).Collide(circ, mm(0.25)): return "pad " + it.GetParentFootprint().GetReference()
            if it.HasHole() and math.hypot(x - t(it.GetPosition().x), y - t(it.GetPosition().y)) < t(max(it.GetDrillSize().x, it.GetDrillSize().y)) / 2 + 0.15 + 0.2 + 0.3: return "hole"
            continue
        if it.GetClass() in ("PCB_TRACK", "PCB_ARC") and not same:
            if it.GetEffectiveShape(it.GetLayer()).Collide(circ, mm(0.15)): return "track " + it.GetNetname()
    for z in fills:
        for l in z.GetLayerSet().Seq():
            if z.GetFilledPolysList(l).Collide(c, mm(0.3 + 0.2)): return "zone " + z.GetZoneName()
    for z in rules:
        if z.Outline().Collide(c, mm(0.35)): return "rule " + z.GetZoneName()
    for cy in courts:
        if cy.OutlineCount() and cy.Collide(c, mm(0.35)): return "courtyard"
    for r in silk:
        if r.Inflate(mm(0.35)).Contains(c) if False else (mm(x) > r.GetX() - mm(0.4) and mm(x) < r.GetRight() + mm(0.4) and mm(y) > r.GetY() - mm(0.4) and mm(y) < r.GetBottom() + mm(0.4)): return "silk"
    for g in b.Groups():
        if g.GetName() == "silk_back":
            for it in g.GetItems():
                bb = it.GetBoundingBox()
                if bb.GetX() - mm(0.4) < mm(x) < bb.GetRight() + mm(0.4) and bb.GetY() - mm(0.4) < mm(y) < bb.GetBottom() + mm(0.4): return "art"
    return None
pts = path(); placed = []; why = {}
last = None; s_since = 99.0; prev = pts[0]
for i, (x, y) in enumerate(pts):
    s_since += math.dist(prev, (x, y)); prev = (x, y)
    near = [e for e in existing + placed if math.dist(e, (x, y)) < 1.2]
    if near: s_since = 0; continue
    if s_since < PITCH: continue
    r = ok(x, y)
    if r is None and all(math.dist(e, (x, y)) >= MINGAP for e in existing + placed):
        placed.append((round(x, 2), round(y, 2))); s_since = 0
    else: why[r] = why.get(r, 0) + 1
print(len(placed), "vias"); print(why)
json.dump(placed, open(sys.argv[2], "w"))
if len(sys.argv) > 3:
    for x, y in placed:
        v = pcbnew.PCB_VIA(b); v.SetPosition(pcbnew.VECTOR2I(mm(x), mm(y))); v.SetWidth(mm(0.6)); v.SetDrill(mm(0.3)); v.SetNetCode(gnd); b.Add(v)
    pcbnew.SaveBoard(sys.argv[3], b)
