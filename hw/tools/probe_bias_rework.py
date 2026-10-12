import sys
sys.path.insert(0,sys.argv[3])  # directory holding maze.py as mr.py and maze3.py as mr3.py
from mr import *
import mr3
b=pcbnew.LoadBoard(sys.argv[1])
def pad(ref,n):
    p=b.FindFootprintByReference(ref).FindPadByNumber(n); return (mm(p.GetX()),mm(p.GetY()))
def dist(a,c): return math.hypot(a[0]-c[0],a[1]-c[1])
def prune(net):
    """remove F.Cu tracks of net with a free end (no pad, via, or other track of the net there)"""
    while True:
        tr=[t for t in b.GetTracks() if t.GetNetname()==net and t.Type()!=pcbnew.PCB_VIA_T]
        pts=[(mm(p.GetX()),mm(p.GetY())) for f in b.GetFootprints() for p in f.Pads() if p.GetNetname()==net]
        pts+=[(mm(t.GetX()),mm(t.GetY())) for t in b.GetTracks() if t.Type()==pcbnew.PCB_VIA_T and t.GetNetname()==net]
        gone=None
        for t in tr:
            for e,o in ((t.GetStart(),t.GetEnd()),(t.GetEnd(),t.GetStart())):
                ep=(mm(e.x),mm(e.y))
                ok=any(dist(ep,q)<0.45 for q in pts) or any(u is not t and u.GetLayer()==t.GetLayer() and (dist(ep,(mm(u.GetStart().x),mm(u.GetStart().y)))<0.01 or dist(ep,(mm(u.GetEnd().x),mm(u.GetEnd().y)))<0.01) for u in tr)
                if not ok: gone=t;break
            if gone: break
        if not gone: return
        b.RemoveNative(gone)
for k in range(4):
    rb,rs,c=[("R23","R19","C29"),("R24","R20","C30"),("R25","R21","C31"),("R26","R22","C32")][k]
    ain="/AIN%d"%k; tip="/TIP%d"%(k+1)
    p2=pad(rb,"2")
    b.FindFootprintByReference(rb).FindPadByNumber("2").SetNet(b.FindNet(tip))
    for t in list(b.GetTracks()):
        if t.GetNetname()==ain and t.Type()!=pcbnew.PCB_VIA_T and t.GetLayer()==pcbnew.F_Cu and (dist(p2,(mm(t.GetStart().x),mm(t.GetStart().y)))<0.6 or dist(p2,(mm(t.GetEnd().x),mm(t.GetEnd().y)))<0.6): b.RemoveNative(t)
    prune(ain)
    # reconnect whatever the pruning left
    vias=[(mm(t.GetX()),mm(t.GetY())) for t in b.GetTracks() if t.Type()==pcbnew.PCB_VIA_T and t.GetNetname()==ain and mm(t.GetX())>=24]
    out=[]
    for name,start,goals in (("Rs",pad(rs,"2"),vias),("C",pad(c,"1"),None)):
        # already joined? try route only if it fails to be connected
        r=route(b,ain,start,goals)
        if r is None: r=mr3.route3(b,ain,start,goals)
        out.append((name,r))
    r=route(b,tip,p2,None)
    if r is None: r=mr3.route3(b,tip,p2,None)
    out.append(("tip",r))
    print(k,out)
pcbnew.ZONE_FILLER(b).Fill(b.Zones())
pcbnew.SaveBoard(sys.argv[2],b)
