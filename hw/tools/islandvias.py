import sys,math
sys.path.insert(0,'/Applications/KiCad/KiCad.app/Contents/Frameworks/Python.framework/Versions/Current/lib/python3.9/site-packages')
import pcbnew
mm=pcbnew.ToMM; F=pcbnew.FromMM
b=pcbnew.LoadBoard(sys.argv[1])
z=[z for z in b.Zones() if z.GetNetname()=='GND' and z.GetLayer()==pcbnew.F_Cu][0]
fp=z.GetFilledPolysList(pcbnew.F_Cu)
vias=[t.GetPosition() for t in b.GetTracks() if t.Type()==pcbnew.PCB_VIA_T and t.GetNetname()=='GND']
add=[]
def seg(px,py,ax,ay,bx,by):
    dx,dy=bx-ax,by-ay; l=dx*dx+dy*dy
    t=0 if l==0 else max(0,min(1,((px-ax)*dx+(py-ay)*dy)/l))
    return math.hypot(px-ax-t*dx,py-ay-t*dy)
obs=[]
for t in b.GetTracks():
    if t.GetNetname()=='GND': continue
    if t.Type()==pcbnew.PCB_VIA_T:
        obs.append((mm(t.GetX()),mm(t.GetY()),mm(t.GetX()),mm(t.GetY()),0.4))
    else:
        obs.append((mm(t.GetStart().x),mm(t.GetStart().y),mm(t.GetEnd().x),mm(t.GetEnd().y),mm(t.GetWidth())/2))
for f in b.GetFootprints():
    for p in f.Pads():
        if p.GetNetname()!='GND' and p.HasHole():
            obs.append((mm(p.GetX()),mm(p.GetY()),mm(p.GetX()),mm(p.GetY()),max(mm(p.GetSizeX()),mm(p.GetSizeY()))/2))
def clear(x,y): return all(seg(x,y,*o[:4])>=o[4]+0.3+0.25+0.05 for o in obs)

for i in range(fp.OutlineCount()):
    o=fp.Outline(i); bb=o.BBox()
    if any(o.PointInside(v,F(0.31)) for v in vias): continue
    if abs(o.Area())/1e12>50: continue
    best=None
    x0,y0,x1,y1=[mm(v) for v in (bb.GetX(),bb.GetY(),bb.GetRight(),bb.GetBottom())]
    n=0;x=x0
    while x<=x1:
        y=y0
        while y<=y1:
            ok=True
            for k in range(9):
                px,py=(x,y) if k==0 else (x+0.4*math.cos(k*math.pi/4),y+0.4*math.sin(k*math.pi/4))
                if not o.PointInside(pcbnew.VECTOR2I(F(px),F(py))): ok=False;break
                # must be in this outline
            if ok and clear(x,y) and bb.Contains(pcbnew.VECTOR2I(F(x),F(y))): best=(x,y); break
            y+=0.1
        if best: break
        x+=0.1
    print("frag",i,"area",round(abs(o.Area())/1e12,1),"via",best)
    if best: add.append(best)
for x,y in add:
    v=pcbnew.PCB_VIA(b); v.SetPosition(pcbnew.VECTOR2I(F(x),F(y))); v.SetWidth(F(0.6)); v.SetDrill(F(0.3)); v.SetNet(b.FindNet('GND')); v.SetViaType(pcbnew.VIATYPE_THROUGH); b.Add(v)
pcbnew.ZONE_FILLER(b).Fill(b.Zones())
pcbnew.SaveBoard(sys.argv[2],b)
