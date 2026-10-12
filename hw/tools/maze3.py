"""3-layer (F.Cu, In2.Cu, B.Cu) maze router with vias, on pcbnew items."""
import sys,heapq,math
sys.path.insert(0,'/Applications/KiCad/KiCad.app/Contents/Frameworks/Python.framework/Versions/Current/lib/python3.9/site-packages')
import numpy as np
import pcbnew
mm=pcbnew.ToMM; F=pcbnew.FromMM
G=0.05
LAY=[pcbnew.F_Cu,pcbnew.In2_Cu,pcbnew.B_Cu]
CLR=0.2; TW=0.1; VR=0.3; VCOST=6.0
class G3:
    def __init__(s,b,box):
        s.b=b;x0,y0,x1,y1=box;s.x0=x0;s.y0=y0;s.nx=int((x1-x0)/G);s.ny=int((y1-y0)/G)
        s.X,s.Y=np.meshgrid(x0+G*np.arange(s.nx),y0+G*np.arange(s.ny),indexing='ij')
    def seg(s,ax,ay,bx,by,d):
        dx,dy=bx-ax,by-ay;l=dx*dx+dy*dy
        t=0 if l==0 else np.clip(((s.X-ax)*dx+(s.Y-ay)*dy)/l,0,1)
        return np.hypot(s.X-ax-t*dx,s.Y-ay-t*dy)<=d
    def box(s,l,t,r,bm,d):
        return np.hypot(np.maximum(np.maximum(l-s.X,s.X-r),0),np.maximum(np.maximum(t-s.Y,s.Y-bm),0))<=d
    def build(s,net):
        b=s.b; own=[np.zeros(s.X.shape,bool) for _ in LAY]; obs=[np.zeros(s.X.shape,bool) for _ in LAY]
        for t in b.GetTracks():
            same=t.GetNetname()==net
            if t.Type()==pcbnew.PCB_VIA_T:
                x,y=mm(t.GetX()),mm(t.GetY())
                for li in range(3):
                    if same: own[li]|=s.seg(x,y,x,y,VR)
                    else: obs[li]|=s.seg(x,y,x,y,VR+CLR+TW)
            elif t.GetLayer() in LAY:
                li=LAY.index(t.GetLayer());w=mm(t.GetWidth())/2
                m=s.seg(mm(t.GetStart().x),mm(t.GetStart().y),mm(t.GetEnd().x),mm(t.GetEnd().y),w if same else w+CLR+TW)
                (own if same else obs)[li]|=m
        for f in b.GetFootprints():
            for p in f.Pads():
                bb=p.GetBoundingBox();l,tp,r,bm=[mm(v) for v in (bb.GetX(),bb.GetY(),bb.GetRight(),bb.GetBottom())]
                same=p.GetNetname()==net
                for li,L in enumerate(LAY):
                    if p.IsOnLayer(L):
                        m=s.box(l,tp,r,bm,0 if same else CLR+TW)
                        (own if same else obs)[li]|=m
                if p.HasHole() or p.GetAttribute()==pcbnew.PAD_ATTRIB_NPTH:
                    for li in range(3): obs[li]|=s.box(l,tp,r,bm,CLR+VR) if not same else obs[li]
        # board edge margin
        edge=(s.X<0.5)|(s.X>45.5)|(s.Y<0.5)|(s.Y>53.5)
        for li in range(3): obs[li]|=edge
        # In1 is a plane: never routed. vias need clearance on all layers -> via-ok mask
        viaok=~(obs[0]|obs[1]|obs[2])
        # via also needs not to overlap other-net copper: obs already inflated by VR+CLR+TW for vias only; widen for tracks
        return own,obs,viaok
    def ij(s,x,y): return int(round((x-s.x0)/G)),int(round((y-s.y0)/G))
def route3(b,net,start,goal_pts=None,box=(14,10,46,28),start_layer=0,exclude_r=0.7):
    g=G3(b,box); own,obs,_=g.build(net)
    # via feasibility: no other-net copper within VR+CLR+TW on any layer; recompute with track-inflation by VR instead of TW
    vo=[o.copy() for o in obs]  # obs already inflated for tracks by CLR+TW; via needs CLR+VR: add VR-TW extra
    # approximate: dilate obs by (VR-TW)=0.2mm
    from scipy.ndimage import binary_dilation
    k=int(round((VR-TW)/G)); st=np.ones((2*k+1,2*k+1),bool)
    vblock=np.zeros(g.X.shape,bool)
    for li in range(3): vblock|=binary_dilation(obs[li],st)
    goals=[np.zeros(g.X.shape,bool) for _ in LAY]
    for li in range(3):
        if goal_pts: 
            for x,y in goal_pts: goals[li]|=(np.hypot(g.X-x,g.Y-y)<=0.12)
            goals[li]&=own[li]
        else: goals[li]=own[li]&~(np.hypot(g.X-start[0],g.Y-start[1])<=exclude_r)
    si,sj=g.ij(*start); S=(si,sj,start_layer)
    blocked=[o.copy() for o in obs]; blocked[start_layer][si,sj]=False
    dist={S:0};prev={};pq=[(0,S)]
    dirs=[(1,0,1),(-1,0,1),(0,1,1),(0,-1,1),(1,1,1.414),(1,-1,1.414),(-1,1,1.414),(-1,-1,1.414)]
    end=None
    while pq:
        d,c=heapq.heappop(pq)
        if d>dist[c]+1e-9: continue
        i,j,l=c
        if goals[l][i,j]: end=c;break
        for di,dj,w in dirs:
            n=(i+di,j+dj,l)
            if not(0<=n[0]<g.nx and 0<=n[1]<g.ny) or blocked[l][n[0],n[1]]: continue
            if di and dj and (blocked[l][i+di,j] or blocked[l][i,j+dj]): continue
            nd=d+w
            if nd<dist.get(n,1e18): dist[n]=nd;prev[n]=c;heapq.heappush(pq,(nd,n))
        if not vblock[i,j]:
            for nl in range(3):
                if nl!=l and not blocked[nl][i,j]:
                    n=(i,j,nl);nd=d+VCOST
                    if nd<dist.get(n,1e18): dist[n]=nd;prev[n]=c;heapq.heappush(pq,(nd,n))
    if end is None: return None
    path=[end]
    while path[-1] in prev: path.append(prev[path[-1]])
    path=path[::-1]
    # emit
    segs=[];vias=[];cur=[path[0]]
    for a,c in zip(path,path[1:]):
        if a[2]!=c[2]: vias.append((g.x0+a[0]*G,g.y0+a[1]*G)); segs.append(cur); cur=[c]
        else: cur.append(c)
    segs.append(cur)
    for sg in segs:
        pts=[(g.x0+i*G,g.y0+j*G) for i,j,_ in sg]; l=sg[0][2]
        out=[pts[0]]
        for k_ in range(1,len(pts)-1):
            a,b_,c_=out[-1],pts[k_],pts[k_+1]
            if abs((b_[0]-a[0])*(c_[1]-b_[1])-(b_[1]-a[1])*(c_[0]-b_[0]))>1e-9: out.append(b_)
        out.append(pts[-1])
        for a,c_ in zip(out,out[1:]):
            if a==c_: continue
            t=pcbnew.PCB_TRACK(b);t.SetStart(pcbnew.VECTOR2I(F(a[0]),F(a[1])));t.SetEnd(pcbnew.VECTOR2I(F(c_[0]),F(c_[1])))
            t.SetWidth(F(0.2));t.SetLayer(LAY[l]);t.SetNet(b.FindNet(net));b.Add(t)
    for x,y in vias:
        v=pcbnew.PCB_VIA(b);v.SetPosition(pcbnew.VECTOR2I(F(x),F(y)));v.SetWidth(F(0.6));v.SetDrill(F(0.3));v.SetNet(b.FindNet(net));v.SetViaType(pcbnew.VIATYPE_THROUGH);b.Add(v)
    return len(path),len(vias)
