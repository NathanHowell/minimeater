"""A printed wall between the USB-C and the first jack: `usb_shift.py <in> <out> [d]` (run from `hw/tools` for `maze3.py`).

The USB-C's opening in the enclosure's port wall (13.2 mm, for the cable's overmold) came within 0.2 mm of the first
jack's, too thin to print, so the two were one slot. The board grows by `d` (default 1.5 mm) on the USB-C's end and
the connector moves out by `d`: the opening gains `d` of wall (1.7 mm).

The connector's fan-out above y = 10 mm (the tracks and vias of VBUS_RAW, CC1_J, CC2_J, USB_DP_J, USB_DN_J, and the
ground pad's track) is taken out and routed again from the new pads to the copper it was joined to (`maze3.route3`,
F.Cu, In2 and B.Cu with vias); the VBUS_RAW runs are widened to what they were. The outline's left side, its top-left
corner and the two long sides follow. The pours refill, in a second process (the fill crashes this one, as does
the exit; the board is saved first).
"""

import subprocess
import sys

sys.path.insert(0, "/Applications/KiCad/KiCad.app/Contents/Frameworks/Python.framework/Versions/Current/lib/python3.9/site-packages")
import pcbnew

import maze3

F, M = pcbnew.FromMM, pcbnew.ToMM
D = float(sys.argv[3]) if len(sys.argv) > 3 else 1.5
XMAX, YCUT = 10.7, 10.0  # the box that is taken out and routed again, board mm
SIGNALS = ("/VBUS_RAW", "/CC1_J", "/CC2_J", "/USB_DP_J", "/USB_DN_J")
WIDE = {"/VBUS_RAW": 0.5}
EDGE, W, H, R = 1.75, 46.0, 54.0, 2.0
b = pcbnew.LoadBoard(sys.argv[1])
P = lambda x, y: pcbnew.VECTOR2I(F(x), F(y))
xy = lambda p: (round(M(p.x), 4), round(M(p.y), 4))
inside = lambda q: q[0] <= XMAX and q[1] < YCUT

# what goes: in the box, the signal nets' tracks and vias, and the ground pad's track (its via stays)
gone, anchors = [], {n: [] for n in SIGNALS + ("GND",)}
for t in list(b.GetTracks()):
    n = t.GetNetname()
    if n not in anchors:
        continue
    if t.Type() == pcbnew.PCB_VIA_T:
        if n != "GND" and inside(xy(t.GetPosition())):
            gone.append(t)
        continue
    a, e = xy(t.GetStart()), xy(t.GetEnd())
    if n == "GND" and not (inside(a) and inside(e) and a[0] > 7):  # the right ground pad's two-piece track only
        continue
    if inside(a) or inside(e):
        gone.append(t)
        if not (inside(a) and inside(e)):
            anchors[n].append(e if inside(a) else a)
        elif n == "GND":
            pass
width = {n: 0.2 for n in anchors}
for t in gone:
    if t.Type() != pcbnew.PCB_VIA_T:
        width[t.GetNetname()] = max(width[t.GetNetname()], M(t.GetWidth()))
for t in gone:
    b.RemoveNative(t)
anchors["GND"] = [(9.93, 9.39)]  # the via the pad's track ended on
print("removed", len(gone), "anchors", {n: sorted(set(a)) for n, a in anchors.items()})

j1 = b.FindFootprintByReference("J1")
j1.Move(pcbnew.VECTOR2I(-F(D), 0))

pads = {}
for p in j1.Pads():
    if p.GetAttribute() == pcbnew.PAD_ATTRIB_SMD and p.GetNetname() in anchors:
        if p.GetNetname() == "GND" and xy(p.GetPosition())[0] < 5:
            continue  # the left ground pad rides the pour
        pads.setdefault(p.GetNetname(), set()).add(xy(p.GetPosition()))

box = (0.5, 6.0, 13.0, 17.0)
for n in SIGNALS + ("GND",):
    first = True
    maze3.TW = WIDE.get(n, 0.2) / 2  # the router keeps its clearance from the track's edge
    for pos in sorted(pads.get(n, ())):
        before = {t.m_Uuid.AsString() for t in b.GetTracks()}
        r = maze3.route3(b, n, pos, sorted(set(anchors[n])) if first else None, box=box)
        print(n, pos, "route", r)
        first = False
        if r is None:
            sys.exit(f"{n}: no route from {pos}")
        if n in WIDE:
            for t in b.GetTracks():
                if t.m_Uuid.AsString() not in before and t.Type() != pcbnew.PCB_VIA_T:
                    t.SetWidth(F(WIDE[n]))

for d in list(b.GetDrawings()):
    if d.GetLayerName() == "Edge.Cuts":
        b.Remove(d)


def line(a, c):
    s = pcbnew.PCB_SHAPE(b)
    s.SetShape(pcbnew.SHAPE_T_SEGMENT)
    s.SetLayer(pcbnew.Edge_Cuts)
    s.SetWidth(F(0.05))
    s.SetStart(P(*a))
    s.SetEnd(P(*c))
    b.Add(s)


def arc(a, mid, c):
    s = pcbnew.PCB_SHAPE(b)
    s.SetShape(pcbnew.SHAPE_T_ARC)
    s.SetLayer(pcbnew.Edge_Cuts)
    s.SetWidth(F(0.05))
    s.SetArcGeometry(P(*a), P(*mid), P(*c))
    b.Add(s)


x0 = -D
k = R * (1 - 0.7071067811865476)
line((x0, EDGE), (W, EDGE))
line((W, EDGE), (W, H - R))
arc((W, H - R), (W - k, H - k), (W - R, H))
line((W - R, H), (x0 + R, H))
arc((x0 + R, H), (x0 + k, H - k), (x0, H - R))
line((x0, H - R), (x0, EDGE))

pcbnew.SaveBoard(sys.argv[2], b)
sys.stdout.flush()
subprocess.run([sys.executable, "-c", f"""
import sys
sys.path.insert(0, {sys.path[0]!r})
import pcbnew
b = pcbnew.LoadBoard({sys.argv[2]!r})
pcbnew.ZONE_FILLER(b).Fill(b.Zones())
pcbnew.SaveBoard({sys.argv[2]!r}, b)
"""], check=False)
