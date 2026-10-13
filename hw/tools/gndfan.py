"""GND fan-out for cut-off pour: `gndfan.py <in> <out>` (KiCad's python, from `hw/tools`; zones filled in <in>).

A piece of the F.Cu GND pour holding GND pads but no via, no GND through-hole pad and no GND track end is joined to
nothing but those pads: routing walled it off. From one of its pads a short F.Cu stub goes to the nearest spot a via
fits (`ar.Lattice`, the same clearances as the router) and a via drops to the In1 plane. Pours refill after, in a
second process. Then a GND pad whose thermal relief the routes left one spoke (DRC `starved_thermal`) gets a solid
zone connection, and the pours refill again."""

import subprocess
import sys

import ar
import kdrc
import octo
import pcbnew
from octo import LAYERS, M

WIDTH = 0.25
REACH = 6.0  # mm around the pad to look for room for the via


def refill(b, path):
    """Save, then fill the pours in a second process (the fill crashes this one)."""
    pcbnew.SaveBoard(path, b)
    subprocess.run([sys.executable, "-c", f"""
import sys
sys.path.insert(0, {sys.path[0]!r})
import pcbnew
b = pcbnew.LoadBoard({path!r})
pcbnew.ZONE_FILLER(b).Fill(b.Zones())
pcbnew.SaveBoard({path!r}, b)
"""], check=False)


b = pcbnew.LoadBoard(sys.argv[1])
gnd = b.FindNet("GND").GetNetCode()
z = [z for z in b.Zones() if z.GetNetCode() == gnd and z.GetFirstLayer() == pcbnew.F_Cu and not z.GetIsRuleArea()][0]
fill = z.GetFilledPolysList(pcbnew.F_Cu)
pads = [p for f in b.GetFootprints() for p in f.Pads() if p.GetNetCode() == gnd and p.IsOnLayer(pcbnew.F_Cu)]
anchors = [t.GetPosition() for t in b.GetTracks() if t.GetNetCode() == gnd and t.Type() == pcbnew.PCB_VIA_T]
anchors += [p.GetPosition() for p in pads if p.HasHole()]
anchors += [e for t in b.GetTracks() if t.GetNetCode() == gnd and t.Type() == pcbnew.PCB_TRACE_T for e in (t.GetStart(), t.GetEnd())]
r = octo.Router(b)
added = 0
for k in range(fill.OutlineCount()):
    o = fill.Outline(k)
    inside = [p for p in pads if o.PointInside(p.GetPosition())]
    if not inside or any(o.PointInside(a) for a in anchors):
        continue
    p = inside[0]
    name = f"{p.GetParentFootprint().GetReference()}.{p.GetNumber()}"
    pos = (M(p.GetPosition().x), M(p.GetPosition().y))
    box = r.win(pos[0] - REACH, pos[1] - REACH, pos[0] + REACH, pos[1] + REACH)
    lat = ar.Lattice(r, gnd, WIDTH, box)
    sources = {nd: pos for nd in ar.padnodes(lat, p, 0, pos)}
    goals = {}
    for a in range(lat.na):
        for bb in range(lat.nb):
            i = a * lat.nb + bb
            if lat.via[i] and all(lat.node_l[li][i] for li in range(3)) and (0, a, bb) not in sources:
                goals[(0, a, bb)] = (a, bb)
    res = lat.search(sources, goals)
    if res is None or any(n[0] != 0 for n in res[0]):
        print(f"{name}: cut off, no room for a via within {REACH} mm", flush=True)
        continue
    path, (a, bb) = res
    v = lat.xy(a, bb)
    pts = octo.simplify([pos] + [lat.xy(x, y) for _, x, y in path])
    okr = lat.ok_r[0]
    pts = octo.tighten(pts, lambda q0, q1: r.seg_ok(okr, lat.box, q0, q1))
    r.add_tracks(pts, LAYERS[0], WIDTH, gnd)
    via = pcbnew.PCB_VIA(b)
    via.SetPosition(pcbnew.VECTOR2I(octo.F(v[0]), octo.F(v[1])))
    via.SetWidth(octo.F(ar.VIA_D))
    via.SetDrill(octo.F(ar.VIA_DRILL))
    via.SetViaType(pcbnew.VIATYPE_THROUGH)
    via.SetNetCode(gnd)
    b.Add(via)
    r.redraw_net(gnd)
    added += 1
    print(f"{name}: stub {octo.length(pts):.2f} mm to a via at ({v[0]:.2f}, {v[1]:.2f})", flush=True)
print(f"{added} GND vias added", flush=True)
refill(b, sys.argv[2])
b = pcbnew.LoadBoard(sys.argv[2])
uuids = {p.m_Uuid.AsString(): p for f in b.GetFootprints() for p in f.Pads()}
for v in kdrc.violations(b, sys.argv[2]):  # a thermal the routes left one spoke: a solid joint instead
    if v["type"] == "starved_thermal":
        for i in v["items"]:
            p = uuids.get(i["uuid"])
            if p is not None:
                p.SetLocalZoneConnection(pcbnew.ZONE_CONNECTION_FULL)
                print(f"{p.GetParentFootprint().GetReference()}.{p.GetNumber()}: starved thermal, full zone connection", flush=True)
refill(b, sys.argv[2])
