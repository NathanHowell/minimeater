"""Tiny F.Cu maze router on pcbnew items: route(net, start_item_cells, goal_items)."""
import sys,heapq,math
sys.path.insert(0,'/Applications/KiCad/KiCad.app/Contents/Frameworks/Python.framework/Versions/Current/lib/python3.9/site-packages')
import numpy as np
import pcbnew
mm=pcbnew.ToMM; F=pcbnew.FromMM
G=0.05
class Grid:
    def __init__(s,b,x0,y0,x1,y1):
        s.b=b; s.x0=x0; s.y0=y0; s.nx=int((x1-x0)/G); s.ny=int((y1-y0)/G)
        s.X,s.Y=np.meshgrid(x0+G*np.arange(s.nx),y0+G*np.arange(s.ny),indexing='ij')
    def seg_mask(s,ax,ay,bx,by,d):
        dx,dy=bx-ax,by-ay;l=dx*dx+dy*dy
        t=np.zeros_like(s.X) if l==0 else np.clip(((s.X-ax)*dx+(s.Y-ay)*dy)/l,0,1)
        return np.hypot(s.X-ax-t*dx,s.Y-ay-t*dy)<=d
    def box_mask(s,l,t,r,bm,d):
        dxm=np.maximum(np.maximum(l-s.X,s.X-r),0);dym=np.maximum(np.maximum(t-s.Y,s.Y-bm),0)
        return np.hypot(dxm,dym)<=d
    def items(s,net):
        """masks: (own copper cells, obstacle cells for other nets) on F.Cu."""
        b=s.b; own=np.zeros(s.X.shape,bool); obs=np.zeros(s.X.shape,bool)
        C=0.2+0.1+0.02
        for t in b.GetTracks():
            same=t.GetNetname()==net
            if t.Type()==pcbnew.PCB_VIA_T:
                r=mm(t.GetWidth(pcbnew.F_Cu))/2 if hasattr(t,'GetWidth') else 0.3
                m=s.seg_mask(mm(t.GetX()),mm(t.GetY()),mm(t.GetX()),mm(t.GetY()),r+(0 if same else C))
            elif t.GetLayer()==pcbnew.F_Cu:
                w=mm(t.GetWidth())/2
                m=s.seg_mask(mm(t.GetStart().x),mm(t.GetStart().y),mm(t.GetEnd().x),mm(t.GetEnd().y),w+(0 if same else C))
            else: continue
            if same: own|=m
            else: obs|=m
        for f in b.GetFootprints():
            for p in f.Pads():
                if not p.IsOnLayer(pcbnew.F_Cu): continue
                bb=p.GetBoundingBox(); l,tp,r,bm=[mm(v) for v in (bb.GetX(),bb.GetY(),bb.GetRight(),bb.GetBottom())]
                same=p.GetNetname()==net
                m=s.box_mask(l,tp,r,bm,0 if same else C)
                if same: own|=m
                else: obs|=m
        return own,obs
    def ij(s,x,y): return int(round((x-s.x0)/G)),int(round((y-s.y0)/G))
def astar(g,obs,starts,goals,allow=None):
    ny=g.ny;nx=g.nx
    blocked=obs.copy()
    for (i,j) in starts: blocked[i,j]=False
    open_=[];dist={};prev={}
    for st in starts: dist[st]=0;heapq.heappush(open_,(0,st))
    gi,gj=np.nonzero(goals); gc=(gi.mean(),gj.mean())
    dirs=[(1,0,1),(-1,0,1),(0,1,1),(0,-1,1),(1,1,1.414),(1,-1,1.414),(-1,1,1.414),(-1,-1,1.414)]
    while open_:
        d,c=heapq.heappop(open_)
        if d>dist.get(c,1e18)+1e-9 and False: continue
        if goals[c]:
            path=[c]
            while path[-1] in prev: path.append(prev[path[-1]])
            return path[::-1]
        for di,dj,w in dirs:
            n=(c[0]+di,c[1]+dj)
            if not(0<=n[0]<nx and 0<=n[1]<ny) or blocked[n]: continue
            if di and dj and (blocked[c[0]+di,c[1]] or blocked[c[0],c[1]+dj]): continue
            nd=dist[c]+w
            if nd<dist.get(n,1e18):
                dist[n]=nd;prev[n]=c;heapq.heappush(open_,(nd,n))
    return None
def simplify(g,path):
    pts=[(g.x0+i*G,g.y0+j*G) for i,j in path]
    out=[pts[0]]
    for k in range(1,len(pts)-1):
        a,b,c=out[-1],pts[k],pts[k+1]
        if abs((b[0]-a[0])*(c[1]-b[1])-(b[1]-a[1])*(c[0]-b[0]))>1e-9: out.append(b)
    out.append(pts[-1]); return out
def add_track(b,net,pts,w=0.2):
    for a,c in zip(pts,pts[1:]):
        t=pcbnew.PCB_TRACK(b);t.SetStart(pcbnew.VECTOR2I(F(a[0]),F(a[1])));t.SetEnd(pcbnew.VECTOR2I(F(c[0]),F(c[1])))
        t.SetWidth(F(w));t.SetLayer(pcbnew.F_Cu);t.SetNet(b.FindNet(net));b.Add(t)
def route(b,net,start_xy,goal_xy_list,bbox=(22,13,43,27)):
    """start_xy: a point on a pad of the net; goal: points of copper of the net to reach (any cell of own copper component at those points)."""
    g=Grid(b,*bbox); own,obs=g.items(net)
    # goal: own-copper cells within 0.4mm of each goal point
    goals=np.zeros(own.shape,bool)
    for x,y in (goal_xy_list or []): goals|=np.hypot(g.X-x,g.Y-y)<=0.12
    goals&=own
    if not goal_xy_list: goals=own&~(np.hypot(g.X-start_xy[0],g.Y-start_xy[1])<=0.7)
    si=[g.ij(*start_xy)]
    path=astar(g,obs,si,goals)
    if path is None: return None
    pts=simplify(g,path)
    add_track(b,net,pts)
    return pts
