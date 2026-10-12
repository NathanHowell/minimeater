import sys, math, json
sys.path.insert(0,'/Applications/KiCad/KiCad.app/Contents/Frameworks/Python.framework/Versions/Current/lib/python3.9/site-packages')
import pcbnew
src, dst = sys.argv[1], sys.argv[2]
b = pcbnew.LoadBoard(src)
mm = pcbnew.FromMM; tomm = pcbnew.ToMM
gnd = b.FindNet('GND')
f = pcbnew.ZONE_FILLER(b); f.Fill(b.Zones())
cn = b.GetConnectivity(); cn.RecalculateRatsnest()
zone_item = [z for z in b.Zones() if z.GetNetname()=='GND' and not z.GetIsRuleArea()][0]
def cluster(item):
    return set(i.m_Uuid.AsString() for i in cn.GetConnectedItems(item))
plane = set()
for z in b.Zones():
    if z.GetNetname()=='GND' and not z.GetIsRuleArea(): plane |= cluster(z)
pads = []
for fp in b.GetFootprints():
    for pad in fp.Pads():
        if pad.GetNetname()=='GND' and pad.IsOnLayer(pcbnew.F_Cu) and pad.m_Uuid.AsString() not in plane:
            pads.append(pad)
print('gnd pads not on the plane:', len(pads), [p.GetParentFootprint().GetReference()+'-'+p.GetNumber() for p in pads])
# obstacles: all pads (any net) and vias/tracks as inflated boxes
obs = []
def box_of(it, infl):
    bb = it.GetBoundingBox(); return (tomm(bb.GetLeft())-infl, tomm(bb.GetTop())-infl, tomm(bb.GetRight())+infl, tomm(bb.GetBottom())+infl)
for fp in b.GetFootprints():
    for pad in fp.Pads(): obs.append((pad.GetNetname()+'|'+fp.GetReference()+pad.GetNumber(), box_of(pad, 0.3)))
for t in b.GetTracks():
    # long tracks: sample as several boxes along their length
    sx,sy,ex,ey=tomm(t.GetStart().x),tomm(t.GetStart().y),tomm(t.GetEnd().x),tomm(t.GetEnd().y)
    n=max(1,int(math.hypot(ex-sx,ey-sy)/0.3)); w=tomm(t.GetWidth())/2+0.35
    for k in range(n+1):
        x=sx+(ex-sx)*k/n; y=sy+(ey-sy)*k/n; obs.append((t.GetNetname()+'|trk',(x-w,y-w,x+w,y+w)))
edge = b.GetBoardEdgesBoundingBox(); ex0,ey0,ex1,ey1 = tomm(edge.GetLeft()),tomm(edge.GetTop()),tomm(edge.GetRight()),tomm(edge.GetBottom())
keep = []   # keepouts: rule areas
for z in b.Zones():
    if z.GetIsRuleArea(): keep.append(box_of(z, 0.0))
def free(x, y, r, own, own_id=None):
    if x-r < ex0+0.5 or x+r > ex1-0.5 or y-r < ey0+0.5 or y+r > ey1-0.5: return False
    for kb in keep:
        if kb[0]-r < x < kb[2]+r and kb[1]-r < y < kb[3]+r: return False
    for net, bb in obs:
        if own_id and net.endswith('|'+own_id): continue
        if bb[0]-r < x < bb[2]+r and bb[1]-r < y < bb[3]+r: return False
    return True
placed = 0; failed = []
for pad in pads:
    pc = pad.GetPosition(); px, py = tomm(pc.x), tomm(pc.y)
    bb = pad.GetBoundingBox(); hw = tomm(bb.GetWidth())/2; hh = tomm(bb.GetHeight())/2
    best = None
    pid_ = pad.GetParentFootprint().GetReference()+pad.GetNumber()
    for dist in (0.5, 0.7, 0.9, 1.2, 1.5, 2.0, 2.6, 3.4, 4.2):
        for k in range(24):
            a = k*math.pi/12
            x = px + (hw+dist)*math.cos(a); y = py + (hh+dist)*math.sin(a)
            # keep the via off the pad's own box
            if abs(x-px) < hw+0.45 and abs(y-py) < hh+0.45: continue
            if free(x, y, 0.45, None, pid_) and all(free(px+(x-px)*q/6, py+(y-py)*q/6, 0.15, None, pid_) for q in range(1,6)):
                best = (x, y); break
        if best: break
    if not best: failed.append((pad.GetParentFootprint().GetReference(), pad.GetNumber())); continue
    v = pcbnew.PCB_VIA(b); v.SetPosition(pcbnew.VECTOR2I(mm(best[0]), mm(best[1]))); v.SetNet(gnd)
    v.SetDrill(mm(0.3)); v.SetWidth(mm(0.6)); v.SetViaType(pcbnew.VIATYPE_THROUGH); b.Add(v)
    t = pcbnew.PCB_TRACK(b); t.SetStart(pc); t.SetEnd(pcbnew.VECTOR2I(mm(best[0]), mm(best[1]))); t.SetWidth(mm(0.25)); t.SetLayer(pcbnew.F_Cu); t.SetNet(gnd); b.Add(t)
    obs.append(('GND|via', (best[0]-0.8,best[1]-0.8,best[0]+0.8,best[1]+0.8)))
    for q in range(0,7):
        qx=px+(best[0]-px)*q/6; qy=py+(best[1]-py)*q/6; obs.append(('GND|trk',(qx-0.4,qy-0.4,qx+0.4,qy+0.4)))
    placed += 1
print('vias placed', placed, 'failed', failed)
f = pcbnew.ZONE_FILLER(b); f.Fill(b.Zones())
pcbnew.SaveBoard(dst, b)
