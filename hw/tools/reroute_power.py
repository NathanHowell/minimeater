"""Reroute the power copper that `power_rework.py` could not widen: `reroute_power.py <in> <out>`.

For each power net (VBUS_F, VSERVO, VFAN, VBUS_RAW) a chain of narrow track segments between two nodes (a via, a pad or a
junction) is taken out and routed again between the same two points with `maze_w.py` (F.Cu, In2.Cu, B.Cu with vias) at the
widest of 0.5, 0.4 and 0.3 mm that finds a way, around everything else as it lies. A chain that cannot be improved goes back as
it was. Run `power_rework.py` afterwards to widen what the new routes left alone and refill the zones.
"""

import sys
from collections import defaultdict

sys.path.insert(0, "/Applications/KiCad/KiCad.app/Contents/Frameworks/Python.framework/Versions/Current/lib/python3.9/site-packages")
import maze_w
import pcbnew

mm, F = pcbnew.ToMM, pcbnew.FromMM
NETS = ("/VBUS_RAW", "/VBUS_F", "VSERVO", "VFAN")
NARROW = 0.35
MIN_LEN = 1.5  # shorter chains are pad necks
WIDTHS = (0.5, 0.4, 0.3)


def key(p):
    return (round(mm(p.x), 3), round(mm(p.y), 3))


def chains(b, net):
    tr = [t for t in b.GetTracks() if t.GetNetname() == net]
    segs = [t for t in tr if t.Type() != pcbnew.PCB_VIA_T]
    nodes = defaultdict(list)
    for t in segs:
        for p in (t.GetStart(), t.GetEnd()):
            nodes[(key(p), t.GetLayer())].append(t)
    stops = {key(t.GetPosition()) for t in tr if t.Type() == pcbnew.PCB_VIA_T}
    for f in b.GetFootprints():
        for p in f.Pads():
            if p.GetNetname() == net:
                stops.add(key(p.GetPosition()))
    narrow = [t for t in segs if mm(t.GetWidth()) < NARROW]
    seen, out = set(), []
    for t in narrow:
        if t.m_Uuid.AsString() in seen:
            continue
        chain = [t]
        seen.add(t.m_Uuid.AsString())
        for end in (0, 1):
            cur = t
            while True:
                p = cur.GetEnd() if end else cur.GetStart()
                if key(p) in stops:
                    break
                nxt = [o for o in nodes[(key(p), cur.GetLayer())] if o is not cur]
                if len(nxt) != 1 or mm(nxt[0].GetWidth()) >= NARROW or nxt[0].m_Uuid.AsString() in seen:
                    break
                cur = nxt[0]
                seen.add(cur.m_Uuid.AsString())
                (chain.append if end else lambda x: chain.insert(0, x))(cur)
        out.append(chain)
    return out


def ends(chain):
    cnt = defaultdict(int)
    for t in chain:
        for p in (t.GetStart(), t.GetEnd()):
            cnt[key(p)] += 1
    e = [k for k, v in cnt.items() if v == 1]
    return e if len(e) == 2 else None


def main(src, dst):
    b = pcbnew.LoadBoard(src)
    for net in NETS:
        todo = sorted(chains(b, net), key=lambda c: -sum(mm(t.GetLength()) for t in c))
        for ch in todo:
            length = sum(mm(t.GetLength()) for t in ch)
            e = ends(ch)
            if length < MIN_LEN or e is None:
                continue
            lay = ch[0].GetLayer()
            saved = [(t.GetStart(), t.GetEnd(), t.GetWidth(), t.GetLayer()) for t in ch]
            xs, ys = [p[0] for p in e], [p[1] for p in e]
            box = (max(0.0, min(xs) - 7), max(0.0, min(ys) - 7), min(46.0, max(xs) + 7), min(54.0, max(ys) + 7))
            for t in ch:
                b.RemoveNative(t)
            done = None
            for w in WIDTHS:
                done = maze_w.route3(b, net, e[0], [e[1]], box=box, start_layer=maze_w.LAY.index(lay), width=w)
                if done:
                    break
            if done:
                print(f"{net}: {length:.1f} mm on {b.GetLayerName(lay)} {e[0]} -> {e[1]}: rerouted at {w} mm ({done[0] * maze_w.G:.1f} mm path, {done[1]} vias)")
            else:
                for s, en, wd, ly in saved:
                    t = pcbnew.PCB_TRACK(b)
                    t.SetStart(s)
                    t.SetEnd(en)
                    t.SetWidth(wd)
                    t.SetLayer(ly)
                    t.SetNet(b.FindNet(net))
                    b.Add(t)
                print(f"{net}: {length:.1f} mm on {b.GetLayerName(lay)} {e[0]} -> {e[1]}: kept")
    pcbnew.ZONE_FILLER(b).Fill(b.Zones())
    pcbnew.SaveBoard(dst, b)


main(sys.argv[1], sys.argv[2])
