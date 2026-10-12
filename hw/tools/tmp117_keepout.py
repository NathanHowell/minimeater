import sys
sys.path.insert(0,sys.argv[3])
from mr import *
import mr3
b=pcbnew.LoadBoard(sys.argv[1])
def near(x,y,px,py,t=0.15): return abs(x-px)<t and abs(y-py)<t
for t in list(b.GetTracks()):
    if t.GetNetname()!='GND': continue
    if t.Type()==pcbnew.PCB_VIA_T:
        if near(mm(t.GetX()),mm(t.GetY()),1.3,52.1) or near(mm(t.GetX()),mm(t.GetY()),5.3,52.9): b.RemoveNative(t)
    else:
        s=(mm(t.GetStart().x),mm(t.GetStart().y));e=(mm(t.GetEnd().x),mm(t.GetEnd().y))
        if all(0<p[0]<5.6 and p[1]>51.5 for p in (s,e)): b.RemoveNative(t); print("rm",s,e)
u=b.FindFootprintByReference("U10")
p=lambda n:(mm(u.FindPadByNumber(n).GetX()),mm(u.FindPadByNumber(n).GetY()))
via=(7.5,51.6)
box=(0.5,49.0,12,53.8)
r4=mr3.route3(b,'GND',p("4"),[via],box=box); print("p4",r4)
r2=mr3.route3(b,'GND',p("2"),None,box=box); print("p2",r2)
z=pcbnew.ZONE(b); z.SetIsRuleArea(True)
ls=pcbnew.LSET(); [ls.AddLayer(l) for l in (pcbnew.F_Cu,pcbnew.In1_Cu,pcbnew.B_Cu)]
z.SetLayerSet(ls); z.SetDoNotAllowZoneFills(True); z.SetDoNotAllowTracks(False); z.SetDoNotAllowVias(False); z.SetDoNotAllowPads(False); z.SetDoNotAllowFootprints(False)
z.SetZoneName("TMP117_no_copper")
ol=z.Outline(); ol.NewOutline()
for x,y in ((0.5,50.9),(7.0,50.9),(7.0,53.6),(0.5,53.6)): ol.Append(F(x),F(y))
b.Add(z)
pcbnew.ZONE_FILLER(b).Fill(b.Zones())
pcbnew.SaveBoard(sys.argv[2],b)
