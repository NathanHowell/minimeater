"""Back-silk artwork (KiCad closed; KiCad's python), run after silk.py: the face seen with the case open. Usage:
silk_back.py hw/minimeater.kicad_pcb [--dry]
Probe numbers P0-P3 between each jack's pegs (the protocol's probes[0..3], jack order), the flame, wordmark and open-source-hardware mark, rev and date,
a QR code to the repository, the KK headers' pin names and USB between J1's shell legs.
Everything it adds sits in the group "silk_back", which a rerun replaces. Each item is checked against pads, holes,
vias and other back silk; a clash is printed and the board is not saved."""
import sys
import pcbnew

path = sys.argv[1]
dry = "--dry" in sys.argv
b = pcbnew.LoadBoard(path)
mm, tomm = pcbnew.FromMM, pcbnew.ToMM
GROUP = "silk_back"

for g in list(b.Groups()):
    if g.GetName() == GROUP:
        items = [i.Cast() for i in g.GetItems()]
        g.RemoveAll()
        for it in items:
            b.Remove(it)
        b.Remove(g)
group = pcbnew.PCB_GROUP(b)
group.SetName(GROUP)
b.Add(group)


def box(item, pad=0.0):
    r = item.GetEffectiveShape().BBox() if item.GetClass() == "PCB_TEXT" else item.GetBoundingBox()  # text: the ink
    return tomm(r.GetX()) - pad, tomm(r.GetY()) - pad, tomm(r.GetRight()) + pad, tomm(r.GetBottom()) + pad


# keep-clear boxes on the back: pads and holes (+0.2), vias (+0.15), back silk already there (+0.15)
obst = []
for fp in b.GetFootprints():
    for p in fp.Pads():
        if p.IsOnLayer(pcbnew.B_Cu) or p.GetAttribute() == pcbnew.PAD_ATTRIB_NPTH:
            obst.append((fp.GetReference() + "." + p.GetNumber(), box(p, 0.2)))
    for g in fp.GraphicalItems():
        if g.GetLayer() == pcbnew.B_SilkS:
            obst.append((fp.GetReference() + " silk", box(g, 0.15)))
    if fp.Reference().IsVisible() and fp.Reference().GetLayer() == pcbnew.B_SilkS:
        obst.append((fp.GetReference() + " ref", box(fp.Reference(), 0.15)))
for d in b.GetDrawings():
    if d.GetLayer() == pcbnew.B_SilkS and d.GetParentGroup() is None:
        obst.append(("silk " + getattr(d, "GetText", lambda: "")(), box(d, 0.15)))
for t in b.GetTracks():
    if t.GetClass() == "PCB_VIA":
        obst.append(("via", box(t, 0.15)))
for hx, hy in ((12.05, 28.5), (43.0, 29.8), (43.0, 49.6)):  # standoff rings
    obst.append(("H ring", (hx - 2.75, hy - 2.75, hx + 2.75, hy + 2.75)))
obst.append(("antenna keepout", (14.5, 51.25, 32.5, 57.25)))
EDGE = (0.2, 1.95, 44.3, 53.8)  # board outline less 0.2 mm (corners checked by eye)

clash = []


def place(item, what, inside=None):
    """Add item, noting any clash; `inside` names an item it may sit within (the flame's core)."""
    x0, y0, x1, y1 = box(item)
    if x0 < EDGE[0] or y0 < EDGE[1] or x1 > EDGE[2] or y1 > EDGE[3]:
        clash.append((what, "board edge"))
    for name, (a0, b0, a1, b1) in obst:
        if name == inside:
            continue
        if x0 < a1 and a0 < x1 and y0 < b1 and b0 < y1:
            clash.append((what, f"{name} {a0:.2f},{b0:.2f}-{a1:.2f},{b1:.2f}  item {x0:.2f},{y0:.2f}-{x1:.2f},{y1:.2f}"))
    b.Add(item)
    group.AddItem(item)
    obst.append((what, box(item, 0.15)))


def text(s, x, y, h, w=None, ang=0):
    """Back-silk text centred on (x, y) as seen from the back."""
    t = pcbnew.PCB_TEXT(b)
    t.SetText(s)
    t.SetLayer(pcbnew.B_SilkS)
    t.SetTextSize(pcbnew.VECTOR2I(mm(h), mm(h)))
    t.SetTextThickness(mm(w or max(0.15, h * 0.15)))
    t.SetMirrored(True)
    t.SetHorizJustify(pcbnew.GR_TEXT_H_ALIGN_CENTER)
    t.SetVertJustify(pcbnew.GR_TEXT_V_ALIGN_CENTER)
    t.SetTextAngleDegrees(ang)
    t.SetPosition(pcbnew.VECTOR2I(mm(x), mm(y)))
    place(t, s)
    return t


def text_from(s, x, y, h):
    """Back-silk text whose low-x end is at x (seen from the back it ends at x): labels inboard of a pin row."""
    t = pcbnew.PCB_TEXT(b)
    t.SetText(s)
    t.SetTextSize(pcbnew.VECTOR2I(mm(h), mm(h)))
    t.SetTextThickness(mm(max(0.15, h * 0.15)))
    t.SetVertJustify(pcbnew.GR_TEXT_V_ALIGN_CENTER)
    t.SetLayer(pcbnew.B_SilkS)
    t.SetMirrored(True)
    t.SetPosition(pcbnew.VECTOR2I(mm(x), mm(y)))
    x0, _, _, _ = box(t)
    t.Move(pcbnew.VECTOR2I(mm(x - x0), 0))
    place(t, s)
    return t


def spline(pts, n=10):
    """Closed Catmull-Rom spline through pts."""
    out, k = [], len(pts)
    for i in range(k):
        p0, p1, p2, p3 = pts[i - 1], pts[i], pts[(i + 1) % k], pts[(i + 2) % k]
        for j in range(n):
            t = j / n
            out.append(tuple(0.5 * (2 * p1[c] + (p2[c] - p0[c]) * t + (2 * p0[c] - 5 * p1[c] + 4 * p2[c] - p3[c]) * t * t
                                    + (3 * p1[c] - p0[c] - 3 * p2[c] + p3[c]) * t ** 3) for c in (0, 1)))
    return out


def poly(pts, cx, base, h, fill, width=0.0, what="flame", inside=None):
    """A back-silk polygon from unit coordinates (y up, as seen from the back), height h, bottom centre (cx, base)."""
    sh = pcbnew.PCB_SHAPE(b)
    sh.SetShape(pcbnew.SHAPE_T_POLY)
    sh.SetLayer(pcbnew.B_SilkS)
    sh.SetPolyPoints([pcbnew.VECTOR2I(mm(cx - u * h), mm(base - v * h)) for u, v in spline(pts)])  # -u: seen from the back
    sh.SetFilled(fill)
    sh.SetWidth(mm(width))
    place(sh, what, inside)


# the mark: a flame (outline, solid core), beside the wordmark
FLAME = [(0.02, 0), (0.26, 0.05), (0.38, 0.22), (0.36, 0.44), (0.25, 0.60), (0.31, 0.80), (0.14, 0.68), (0.06, 1.0),
         (-0.04, 0.84), (-0.16, 0.70), (-0.30, 0.80), (-0.30, 0.60), (-0.38, 0.40), (-0.32, 0.14), (-0.16, 0.03)]
CORE = [(0.02, 0.10), (0.17, 0.16), (0.22, 0.30), (0.16, 0.44), (0.08, 0.68), (0.0, 0.52), (-0.10, 0.44), (-0.20, 0.32),
        (-0.18, 0.18)]


def fit(s, h, near, region):
    """Text s at the free spot in region (x0, y0, x1, y1) nearest `near`, on a 0.1 mm grid."""
    x0, y0, x1, y1 = region
    spots = sorted(((x0 + i * 0.1, y0 + j * 0.1) for i in range(int((x1 - x0) / 0.1) + 1) for j in range(int((y1 - y0) / 0.1) + 1)),
                   key=lambda p: (p[0] - near[0]) ** 2 + (p[1] - near[1]) ** 2)
    t = pcbnew.PCB_TEXT(b)
    t.SetText(s)
    t.SetLayer(pcbnew.B_SilkS)
    t.SetTextSize(pcbnew.VECTOR2I(mm(h), mm(h)))
    t.SetTextThickness(mm(0.15))
    t.SetMirrored(True)
    t.SetHorizJustify(pcbnew.GR_TEXT_H_ALIGN_CENTER)
    t.SetVertJustify(pcbnew.GR_TEXT_V_ALIGN_CENTER)
    for x, y in spots:
        t.SetPosition(pcbnew.VECTOR2I(mm(x), mm(y)))
        a0, b0, a1, b1 = box(t, 0.15)
        if a0 > EDGE[0] and b0 > EDGE[1] and a1 < EDGE[2] and b1 < EDGE[3] and \
                not any(a0 < c1 and c0 < a1 and b0 < d1 and d0 < b1 for _, (c0, d0, c1, d1) in obst):
            place(t, s)
            return t
    clash.append((s, "no free spot"))


# probes, in jack order (J2-J5 = probes[0..3]); between each jack's two pegs
for i, jx in enumerate((15.2, 23.9, 32.6, 41.3)):
    text(f"P{i}", jx, 6.75, 1.5, 0.22)

# wordmark under the jacks, centred on them; rev/date below it
text("minimeater", 28.25, 11.5, 2.0, 0.3)
poly(FLAME, 40.3, 14.9, 4.6, False, 0.25)
poly(CORE, 40.3, 14.9, 4.6, True, what="core", inside="flame")
# the open-source-hardware mark (the design is CERN-OHL-S-2.0, LICENSE) balancing the flame on the wordmark's other side
OSHW = "/Applications/KiCad/KiCad.app/Contents/SharedSupport/footprints/Symbol.pretty"
for g in pcbnew.FootprintLoad(OSHW, "OSHW-Symbol_6.7x6mm_SilkScreen").GraphicalItems():
    sh = pcbnew.PCB_SHAPE(b)
    sh.SetShape(pcbnew.SHAPE_T_POLY)
    sh.SetPolyShape(g.GetPolyShape())
    sh.SetFilled(True)
    sh.SetWidth(0)
    sh.SetLayer(pcbnew.F_SilkS)
    r = sh.GetBoundingBox()
    sh.Move(pcbnew.VECTOR2I(-r.GetCenter().x, -r.GetCenter().y))
    sh.Scale(4.4 / tomm(r.GetHeight()))
    sh.Flip(pcbnew.VECTOR2I(0, 0), pcbnew.FLIP_DIRECTION_LEFT_RIGHT)  # onto the back, reading right from the back
    sh.Move(pcbnew.VECTOR2I(mm(15.4), mm(12.6)))
    place(sh, "OSHW")
text("rev A  2026-10", 28.25, 14.75, 1.0)

# QR to the repository, knocked out of a silk square (dark modules = bare mask, like ink on paper)
q = pcbnew.PCB_BARCODE(b)
q.SetLayer(pcbnew.F_SilkS)  # drawn on the front, then flipped: on the back it reads unmirrored from the back
q.SetKind(pcbnew.BARCODE_T_QR_CODE)
q.SetErrorCorrection(pcbnew.BARCODE_ECC_T_L)
q.SetText("HTTPS://GITHUB.COM/NATHANHOWELL/MINIMEATER")  # upper case: alphanumeric mode keeps it at version 2
q.SetShowText(False)
q.SetWidth(mm(10.0))
q.SetHeight(mm(10.0))
q.SetMargin(pcbnew.VECTOR2I(mm(0.8), mm(0.8)))
q.SetIsKnockout(True)
q.SetPosition(pcbnew.VECTOR2I(mm(22.8), mm(36.2)))
q.AssembleBarcode()
q.Flip(q.GetPosition(), pcbnew.FLIP_DIRECTION_LEFT_RIGHT)
place(q, "QR")

# KK pin names, inboard of each pin (pin 1 is the square pad, nearest the board's rear)
# (nudged off the pin centre where a via sits beside it)
for y, s in ((35.95, "GND"), (33.05, "5V"), (30.51, "TACH"), (28.1, "PWM"),
             (43.85, "GND"), (41.91, "5V"), (39.37, "SIG")):
    text_from(s, 5.6, y, 1.0)

text("USB", 4.2, 4.6, 1.2, 0.18)

for what, name in clash:
    print("clash:", what, "x", name)
x0, y0, x1, y1 = box(q)
print(f"QR box {x0:.2f},{y0:.2f} - {x1:.2f},{y1:.2f}")
if clash or dry:
    print("not saved")
    sys.exit(1 if clash else 0)
pcbnew.SaveBoard(path, b)
print("saved", len(list(group.GetItems())), "items")
