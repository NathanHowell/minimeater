"""Pre-fab review fixes on the routed board: `power_rework.py <in> <out>`.

1. D1 (SMF5V0A TVS): pin 1 is the cathode, and the schematic had it on GND with the anode on VBUS_RAW, a forward diode
   across the supply. The schematic symbol is turned 180 degrees (pin 1 to VBUS_RAW); here the footprint turns 180
   degrees and its two pads swap nets, so the copper that ran to each pad still meets the right net.
2. Power copper from 0.2 mm to as wide as the neighbours allow: VBUS_RAW, VBUS_F, VSERVO, VFAN and the buck's switch node up to 0.8 mm
   (1.5 A budget, and the inner layers are 0.5 oz), 3V3 up to 0.4 mm. Segment by segment, each at the widest of its
   candidates that keeps CLEAR to every other-net track, via and pad and EDGE to the board edge; the zones refill after.
"""

import math
import sys

sys.path.insert(0, "/Applications/KiCad/KiCad.app/Contents/Frameworks/Python.framework/Versions/Current/lib/python3.9/site-packages")
import pcbnew

mm, F = pcbnew.ToMM, pcbnew.FromMM
CLEAR = 0.2
EDGE = 0.6
NETS = {  # net -> candidate widths, widest first
    "/VBUS_RAW": (0.8, 0.6, 0.5, 0.4, 0.3),
    "/VBUS_F": (0.8, 0.6, 0.5, 0.4, 0.3),
    "VSERVO": (0.8, 0.6, 0.5, 0.4, 0.3),
    "VFAN": (0.6, 0.5, 0.4, 0.3),
    "/BUCK_SW": (0.5, 0.4, 0.3),
    "3V3": (0.4, 0.3),
}


def seg_seg(a, b, c, d):
    def ps(p, a, b):
        dx, dy = b[0] - a[0], b[1] - a[1]
        l = dx * dx + dy * dy
        t = 0 if l == 0 else max(0, min(1, ((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / l))
        return math.hypot(p[0] - a[0] - t * dx, p[1] - a[1] - t * dy)

    def cross(a, b, c, d):
        o = lambda p, q, r: (q[0] - p[0]) * (r[1] - p[1]) - (q[1] - p[1]) * (r[0] - p[0])
        return o(a, b, c) * o(a, b, d) < 0 and o(c, d, a) * o(c, d, b) < 0

    if cross(a, b, c, d):
        return 0.0
    return min(ps(a, c, d), ps(b, c, d), ps(c, a, b), ps(d, a, b))


def seg_rect(a, b, r, step=0.05):
    n = max(1, int(math.hypot(b[0] - a[0], b[1] - a[1]) / step))
    best = 1e9
    for i in range(n + 1):
        x, y = a[0] + (b[0] - a[0]) * i / n, a[1] + (b[1] - a[1]) * i / n
        dx = max(r[0] - x, 0, x - r[2])
        dy = max(r[1] - y, 0, y - r[3])
        best = min(best, math.hypot(dx, dy))
    return best


def main(src, dst):
    b = pcbnew.LoadBoard(src)

    d1 = b.FindFootprintByReference("D1")
    if d1.FindPadByNumber("1").GetNetname() == "GND":  # not yet turned (the script is safe to run again)
        before = {p.GetNumber(): (mm(p.GetX()), mm(p.GetY())) for p in d1.Pads()}
        d1.SetOrientationDegrees(d1.GetOrientationDegrees() + 180)
        after = {p.GetNumber(): (mm(p.GetX()), mm(p.GetY())) for p in d1.Pads()}
        assert all(abs(after["1"][i] - before["2"][i]) < 0.01 and abs(after["2"][i] - before["1"][i]) < 0.01 for i in (0, 1)), (before, after)
        nets = {"1": "/VBUS_RAW", "2": "GND"}
        for p in d1.Pads():
            p.SetNet(b.FindNet(nets[p.GetNumber()]))
    print("D1:", {p.GetNumber(): p.GetNetname() for p in d1.Pads()})

    edge = b.GetBoardEdgesBoundingBox()
    ex = (mm(edge.GetX()), mm(edge.GetY()), mm(edge.GetRight()), mm(edge.GetBottom()))
    pads = []
    for f in b.GetFootprints():
        for p in f.Pads():
            bb = p.GetBoundingBox()
            pads.append((p.GetNetname(), {p.GetLayer()} if not p.HasHole() else None, (mm(bb.GetX()), mm(bb.GetY()), mm(bb.GetRight()), mm(bb.GetBottom()))))
    tracks = [t for t in b.GetTracks()]
    vias = [(t.GetNetname(), mm(t.GetX()), mm(t.GetY()), mm(t.GetWidth()) / 2) for t in tracks if t.Type() == pcbnew.PCB_VIA_T]
    segs = [t for t in tracks if t.Type() != pcbnew.PCB_VIA_T]
    tot = {}
    for net, cands in NETS.items():
        for t in segs:
            if t.GetNetname() != net:
                continue
            a = (mm(t.GetStart().x), mm(t.GetStart().y))
            e = (mm(t.GetEnd().x), mm(t.GetEnd().y))
            lay = t.GetLayer()
            for w in cands:
                ok = True
                # board edge
                for p in (a, e):
                    if p[0] - w / 2 < ex[0] + EDGE or p[1] - w / 2 < ex[1] + EDGE or p[0] + w / 2 > ex[2] - EDGE or p[1] + w / 2 > ex[3] - EDGE:
                        ok = False
                if ok:
                    for o in segs:
                        if o is t or o.GetNetname() == net or o.GetLayer() != lay:
                            continue
                        oa = (mm(o.GetStart().x), mm(o.GetStart().y))
                        oe = (mm(o.GetEnd().x), mm(o.GetEnd().y))
                        if seg_seg(a, e, oa, oe) < w / 2 + mm(o.GetWidth()) / 2 + CLEAR:
                            ok = False
                            break
                if ok:
                    for vn, vx, vy, vr in vias:
                        if vn != net and seg_seg(a, e, (vx, vy), (vx, vy)) < w / 2 + vr + CLEAR:
                            ok = False
                            break
                if ok:
                    for pn, pl, r in pads:
                        if pn == net or (pl is not None and lay not in pl):
                            continue
                        if seg_rect(a, e, r) < w / 2 + CLEAR:
                            ok = False
                            break
                if ok:
                    t.SetWidth(F(w))
                    tot.setdefault(net, {}).setdefault(w, 0.0)
                    tot[net][w] += t.GetLength() / 1e6
                    break
    for n, v in tot.items():
        print(n, {w: round(l, 1) for w, l in sorted(v.items())})
    pcbnew.ZONE_FILLER(b).Fill(b.Zones())
    pcbnew.SaveBoard(dst, b)


main(sys.argv[1], sys.argv[2])
