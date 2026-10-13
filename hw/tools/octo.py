"""Octilinear router on pcbnew boards (KiCad's python; numpy and scipy come with it).

The grid routers before it (`maze.py`, `maze3.py`, `maze_w.py`) walk 0.05 mm cells and keep every step, so their
tracks are staircases. Here the search costs each bend (A* over cell and heading, 45-degree turns cheap, 90 dear,
sharper never) and the result is pulled tight: each run of corners is replaced by the shortest one- or two-segment
45-degree connection that clears everything, until nothing changes. Ends are exact (pad centres, vias, junctions).

Clearance comes from a 0.025 mm raster of each copper layer (net code per cell; NPTH and mounting-hole keep-outs are
-1, foreign to every net) and a distance transform: a centreline point is free when the nearest foreign copper is at
least clearance + half the width (+ `MARGIN` for the raster) away, it is `EDGE` + half the width inside the real
board outline, and outside the rule areas that forbid tracks. Zones are ignored (they refill).

`Router(board)` builds the rasters; `chains(net, layer)` lists the track runs between terminals (pads, vias,
junctions, width changes); `route(net, layer, width, a, b)` returns the points of a route or None; `replace(chain,
pts, layer)` swaps a chain's tracks for the new ones and updates the raster. Run DRC after (kicad-cli)."""

import heapq
import math
import sys

sys.path.insert(0, "/Applications/KiCad/KiCad.app/Contents/Frameworks/Python.framework/Versions/Current/lib/python3.9/site-packages")
import numpy as np
import pcbnew
from scipy.ndimage import distance_transform_edt

M = pcbnew.ToMM
F = lambda v: pcbnew.FromMM(float(v))
G = 0.025  # raster cell, mm
S = 2  # search step in cells (0.05 mm)
CLR, EDGE, MARGIN = 0.2, 0.5, 0.03
T45, T90 = 0.6, 3.0  # bend costs, mm of track
VIA_COST = 4.0
LAYERS = (pcbnew.F_Cu, pcbnew.In2_Cu, pcbnew.B_Cu)  # In1 is the ground plane: never routed
HOLE_KEEP = {pcbnew.F_Cu: 2.9, pcbnew.In2_Cu: 1.6, pcbnew.B_Cu: 2.5}  # mounting holes: standoff (F), hole, screw head (B)
POUR_LAYERS = (pcbnew.F_Cu, pcbnew.B_Cu)
SPOKE_ROOM = 0.8
BREATHE, BREATHE_COST = 0.5, 4.0  # around SMD GND pads on the poured layers: a step there costs 1 + 4 times as much
DIRS = [(1, 0), (1, 1), (0, 1), (-1, 1), (-1, 0), (-1, -1), (0, -1), (1, -1)]


def inpoly(px, py, poly):
    """Even-odd point-in-polygon for arrays of points; poly is an (n, 2) array."""
    inside = np.zeros(px.shape, bool)
    x0, y0 = poly[-1]
    for x1, y1 in poly:
        c = ((y1 > py) != (y0 > py)) & (px < (x0 - x1) * (py - y1) / ((y0 - y1) or 1e-12) + x1)
        inside ^= c
        x0, y0 = x1, y1
    return inside


def polyset_pts(ps):
    return [np.array([(M(o.CPoint(i).x), M(o.CPoint(i).y)) for i in range(o.PointCount())]) for o in (ps.Outline(k) for k in range(ps.OutlineCount()))]


class Router:
    def __init__(s, b, pad=1.0):
        s.b = b
        o = pcbnew.SHAPE_POLY_SET()
        b.GetBoardPolygonOutlines(o, True)
        s.outline = polyset_pts(o)
        xs = np.concatenate([p[:, 0] for p in s.outline]); ys = np.concatenate([p[:, 1] for p in s.outline])
        s.x0, s.y0 = xs.min() - pad, ys.min() - pad
        s.nx, s.ny = int((xs.max() + pad - s.x0) / G) + 1, int((ys.max() + pad - s.y0) / G) + 1
        X, Y = s.grid(0, 0, s.nx, s.ny)
        inside = np.zeros((s.nx, s.ny), bool)
        for p in s.outline:
            inside ^= inpoly(X, Y, p)
        s.edge_d = distance_transform_edt(inside) * G  # distance to the outline, mm
        s.code = {L: np.zeros((s.nx, s.ny), np.int32) for L in LAYERS}  # pads and holes (fixed)
        s.tcode = {L: np.zeros((s.nx, s.ny), np.int32) for L in LAYERS}  # tracks and vias
        s.keep = {L: np.zeros((s.nx, s.ny), bool) for L in LAYERS}  # rule areas without tracks
        s.ring = {L: np.zeros((s.nx, s.ny), bool) for L in LAYERS}  # mounting-hole keep-outs beyond the pad
        for fp in b.GetFootprints():
            for z in fp.Zones():
                if z.GetIsRuleArea() and z.GetDoNotAllowTracks():
                    for L in LAYERS:
                        if z.IsOnLayer(L):
                            for p in polyset_pts(z.Outline()):
                                s.fill(s.keep[L], p, True)
        for z in b.Zones():
            if z.GetIsRuleArea() and z.GetDoNotAllowTracks():
                for L in LAYERS:
                    if z.IsOnLayer(L):
                        for p in polyset_pts(z.Outline()):
                            s.fill(s.keep[L], p, True)
        for fp in b.GetFootprints():
            for p in fp.Pads():
                s.draw_pad(p)
            if fp.GetReference().startswith("H"):
                c = fp.GetPosition()
                inner = max([max(M(p.GetSize(L).x), M(p.GetSize(L).y)) / 2 for p in fp.Pads() for L in LAYERS] + [0.0]) + 0.05
                for L in LAYERS:
                    s.disc(s.code[L], M(c.x), M(c.y), HOLE_KEEP[L], -1)
                    s.disc(s.ring[L], M(c.x), M(c.y), HOLE_KEEP[L], True)  # the keep-out past the hole's pad
                    s.disc(s.ring[L], M(c.x), M(c.y), inner, False)
        for t in b.GetTracks():
            s.draw_track(t, t.GetNetCode())
        s.breathe = {L: np.zeros((s.nx, s.ny), bool) for L in LAYERS}  # round SMD GND pads that take the pour
        for fp in b.GetFootprints():
            for p in fp.Pads():
                if p.GetNetname() == "GND" and not p.HasHole():
                    for L in POUR_LAYERS:
                        if p.IsOnLayer(L):
                            ps = p.GetEffectivePolygon(L, pcbnew.ERROR_OUTSIDE)
                            ps = ps.CloneDropTriangulation() if hasattr(ps, "CloneDropTriangulation") else pcbnew.SHAPE_POLY_SET(ps)
                            ps.Inflate(F(BREATHE), pcbnew.CORNER_STRATEGY_ROUND_ALL_CORNERS, F(0.005))
                            for poly in polyset_pts(ps):
                                s.fill(s.breathe[L], poly, True)

    # --- raster ---
    def grid(s, i0, j0, i1, j1):
        return np.meshgrid(s.x0 + G * np.arange(i0, i1), s.y0 + G * np.arange(j0, j1), indexing="ij")

    def win(s, x0, y0, x1, y1):
        i0, j0 = max(0, int((x0 - s.x0) / G) - 1), max(0, int((y0 - s.y0) / G) - 1)
        i1, j1 = min(s.nx, int((x1 - s.x0) / G) + 2), min(s.ny, int((y1 - s.y0) / G) + 2)
        return i0, j0, i1, j1

    def fill(s, arr, poly, val):
        i0, j0, i1, j1 = s.win(poly[:, 0].min(), poly[:, 1].min(), poly[:, 0].max(), poly[:, 1].max())
        X, Y = s.grid(i0, j0, i1, j1)
        m = inpoly(X, Y, poly)
        arr[i0:i1, j0:j1][m] = val

    def disc(s, arr, x, y, r, val):
        i0, j0, i1, j1 = s.win(x - r, y - r, x + r, y + r)
        X, Y = s.grid(i0, j0, i1, j1)
        arr[i0:i1, j0:j1][np.hypot(X - x, Y - y) <= r] = val

    def capsule(s, arr, ax, ay, bx, by, r, val):
        i0, j0, i1, j1 = s.win(min(ax, bx) - r, min(ay, by) - r, max(ax, bx) + r, max(ay, by) + r)
        X, Y = s.grid(i0, j0, i1, j1)
        dx, dy = bx - ax, by - ay
        l2 = dx * dx + dy * dy
        t = 0 if l2 == 0 else np.clip(((X - ax) * dx + (Y - ay) * dy) / l2, 0, 1)
        arr[i0:i1, j0:j1][np.hypot(X - ax - t * dx, Y - ay - t * dy) <= r] = val

    def draw_pad(s, p, val=None):
        val = (p.GetNetCode() or -1) if val is None else val  # no net: foreign to every net
        lc = p.GetLocalClearance()
        extra = max(0.0, M(lc) - CLR) if lc is not None else 0.0  # e.g. the fiducials' 0.6
        for L in LAYERS:
            if p.IsOnLayer(L) and p.FlashLayer(L):
                ex = extra
                if p.GetNetname() == "GND" and p.HasHole() and L in POUR_LAYERS:
                    ex = max(ex, SPOKE_ROOM)  # room for the thermal spokes into the pour
                ps = p.GetEffectivePolygon(L, pcbnew.ERROR_OUTSIDE)
                if ex > 0:
                    ps = ps.CloneDropTriangulation() if hasattr(ps, "CloneDropTriangulation") else pcbnew.SHAPE_POLY_SET(ps)
                    ps.Inflate(F(ex), pcbnew.CORNER_STRATEGY_ROUND_ALL_CORNERS, F(0.005))
                for poly in polyset_pts(ps):
                    s.fill(s.code[L], poly, val)
        if p.GetAttribute() == pcbnew.PAD_ATTRIB_NPTH or (p.HasHole() and not p.IsOnCopperLayer()):
            c = p.GetPosition()
            for L in LAYERS:
                s.disc(s.code[L], M(c.x), M(c.y), M(p.GetDrillSize().x) / 2 + 0.05, -1)

    def draw_track(s, t, val):
        if t.Type() == pcbnew.PCB_VIA_T:
            c = t.GetPosition()
            for L in LAYERS:
                s.disc(s.tcode[L], M(c.x), M(c.y), M(t.GetWidth(pcbnew.F_Cu)) / 2, val)
        elif t.GetLayer() in LAYERS:
            a, e = t.GetStart(), t.GetEnd()
            s.capsule(s.tcode[t.GetLayer()], M(a.x), M(a.y), M(e.x), M(e.y), M(t.GetWidth()) / 2, val)

    def redraw_net(s, net):
        """Clear a net's cells and draw its pads, tracks and vias again (after tracks went or came)."""
        for L in LAYERS:
            s.code[L][s.code[L] == net] = 0
            s.tcode[L][s.tcode[L] == net] = 0
        for fp in s.b.GetFootprints():
            for p in fp.Pads():
                if p.GetNetCode() == net:
                    s.draw_pad(p)
        for t in s.b.GetTracks():
            if t.GetNetCode() == net:
                s.draw_track(t, net)

    def free(s, net, L, width, box, vias=False, ignore=(), rings=True):
        """Boolean free-centreline mask over the window `box` (i0, j0, i1, j1) for a track (or via) of `width`;
        tracks and vias of the nets in `ignore` do not count; with `rings` False, neither do the mounting-hole
        keep-outs beyond the hole's pad (standoff and screw head room, wider than the rules)."""
        i0, j0, i1, j1 = box
        need = CLR + width / 2 + MARGIN
        k = int(need / G) + 3
        a0, b0, a1, b1 = max(0, i0 - k), max(0, j0 - k), min(s.nx, i1 + k), min(s.ny, j1 + k)
        foreign = np.zeros((a1 - a0, b1 - b0), bool)
        for LL in (LAYERS if vias else (L,)):
            c = s.code[LL][a0:a1, b0:b1]
            f1 = (c != 0) & (c != net)
            if not rings:
                f1 &= ~s.ring[LL][a0:a1, b0:b1]
            foreign |= f1
            c = s.tcode[LL][a0:a1, b0:b1]
            f2 = (c != 0) & (c != net)
            if len(ignore):
                f2 &= ~np.isin(c, list(ignore))
            foreign |= f2
        d = distance_transform_edt(~foreign) * G
        ok = d[i0 - a0:i1 - a0, j0 - b0:j1 - b0] >= need
        ok &= s.edge_d[i0:i1, j0:j1] >= EDGE + width / 2 + MARGIN
        for LL in (LAYERS if vias else (L,)):
            kp = s.keep[LL][a0:a1, b0:b1]
            if kp.any():
                dk = distance_transform_edt(~kp) * G
                ok &= dk[i0 - a0:i1 - a0, j0 - b0:j1 - b0] > width / 2 + MARGIN
        return ok

    # --- chains ---
    def chains(s, net, L):
        """Track runs of `net` on `L` between terminals: [{'tracks', 'pts', 'width', 'pinned'}]."""
        tr = [t for t in s.b.GetTracks() if t.Type() == pcbnew.PCB_TRACE_T and t.GetNetCode() == net and t.GetLayer() == L]
        key = lambda v: (v.x, v.y)
        adj = {}
        for t in tr:
            for v in (t.GetStart(), t.GetEnd()):
                adj.setdefault(key(v), []).append(t)
        vias = {key(v.GetPosition()) for v in s.b.GetTracks() if v.Type() == pcbnew.PCB_VIA_T and v.GetNetCode() == net}
        pads = [p for fp in s.b.GetFootprints() for p in fp.Pads() if p.GetNetCode() == net and p.IsOnLayer(L)]
        allvias = [v for v in s.b.GetTracks() if v.Type() == pcbnew.PCB_VIA_T and v.GetNetCode() == net]

        def at_pad(k):
            v = pcbnew.VECTOR2I(*k)
            return any(p.HitTest(v) for p in pads)

        term = {k for k, ts in adj.items() if len(ts) != 2 or k in vias or at_pad(k) or len({t.GetWidth() for t in ts}) > 1}
        seen, out = set(), []
        for k0 in term:
            for t0 in adj[k0]:
                if id(t0) in seen:
                    continue
                run, pts, k, t = [], [k0], k0, t0
                while True:
                    seen.add(id(t))
                    run.append(t)
                    k = key(t.GetEnd()) if key(t.GetStart()) == k else key(t.GetStart())
                    pts.append(k)
                    if k in term:
                        break
                    t = [u for u in adj[k] if u is not t][0]
                ends = {pts[0], pts[-1]}
                pinned = False  # a pad, via or track end under the run's middle would lose its connection
                for u in run:
                    for p in pads:
                        if u.HitTest(p.GetPosition()) and not any(p.HitTest(pcbnew.VECTOR2I(*e)) for e in ends):
                            pinned = True
                    for v in allvias:
                        if key(v.GetPosition()) not in ends and u.HitTest(v.GetPosition()):
                            pinned = True
                    ua, ue = u.GetStart(), u.GetEnd()
                    for k2 in adj:  # another track's end on its centreline: a T that would come apart
                        if k2 not in ends and k2 not in pts and seg_dist(k2, (ua.x, ua.y), (ue.x, ue.y)) < F(0.002):
                            pinned = True
                out.append({"tracks": run, "pts": [(M(x), M(y)) for x, y in pts], "width": M(run[0].GetWidth()), "pinned": pinned, "net": net, "layer": L})
        return out

    # --- search ---
    def route(s, net, L, width, a, b, margin=4.0, mask=None):
        """A 45-degree route on one layer from a to b (mm), or None."""
        box = s.win(min(a[0], b[0]) - margin, min(a[1], b[1]) - margin, max(a[0], b[0]) + margin, max(a[1], b[1]) + margin)
        ok = s.free(net, L, width, box) if mask is None else mask
        i0, j0 = box[0], box[1]
        nx, ny = ok.shape
        cell = lambda p: (int(round((p[0] - s.x0) / G)) - i0, int(round((p[1] - s.y0) / G)) - j0)
        si, sj = cell(a)
        gi, gj = cell(b)
        # the search lattice: anchored on the start cell, step S
        on = lambda i, j: 0 <= i < nx and 0 <= j < ny and ok[i, j]
        if not (on(si, sj) and on(gi, gj)):
            return None
        goal_r = S
        h = lambda i, j: (lambda dx, dy: (max(dx, dy) + (math.sqrt(2) - 1) * min(dx, dy)) * G)(abs(i - gi), abs(j - gj))
        start = (si, sj, -1)
        dist = {start: 0.0}
        prev = {}
        pq = [(h(si, sj), 0.0, start)]
        end = None
        n = 0
        while pq:
            f, d, st = heapq.heappop(pq)
            if d > dist.get(st, 1e18) + 1e-12:
                continue
            i, j, hd = st
            n += 1
            if n > 600000:
                return None
            if abs(i - gi) <= goal_r and abs(j - gj) <= goal_r:
                end = st
                break
            for nd_, (di, dj) in enumerate(DIRS):
                if hd >= 0:
                    turn = min((nd_ - hd) % 8, (hd - nd_) % 8)
                    if turn > 2:
                        continue
                    tc = (0, T45, T90)[turn]
                else:
                    tc = 0
                ni, nj = i + di * S, j + dj * S
                if not (on(ni, nj) and on(i + di * S // 2, j + dj * S // 2)):
                    continue
                step = S * G * (math.sqrt(2) if di and dj else 1)
                nst = (ni, nj, nd_)
                ndist = d + step + tc
                if ndist < dist.get(nst, 1e18) - 1e-12:
                    dist[nst] = ndist
                    prev[nst] = st
                    heapq.heappush(pq, (ndist + h(ni, nj), ndist, nst))
        if end is None:
            return None
        path = [end]
        while path[-1] in prev:
            path.append(prev[path[-1]])
        xy = lambda i, j: (s.x0 + (i + i0) * G, s.y0 + (j + j0) * G)
        pts = [a] + [xy(i, j) for i, j, _ in path[::-1]] + [b]
        chk = lambda p, q: s.seg_ok(ok, box, p, q)
        return tighten(simplify(pts), chk)

    def seg_ok(s, ok, box, p, q):
        i0, j0 = box[0], box[1]
        l = math.hypot(q[0] - p[0], q[1] - p[1])
        n = max(2, int(l / (G / 2)) + 1)
        t = np.linspace(0, 1, n)
        ii = np.rint((p[0] + (q[0] - p[0]) * t - s.x0) / G).astype(int) - i0
        jj = np.rint((p[1] + (q[1] - p[1]) * t - s.y0) / G).astype(int) - j0
        if ii.min() < 0 or jj.min() < 0 or ii.max() >= ok.shape[0] or jj.max() >= ok.shape[1]:
            return False
        return bool(ok[ii, jj].all())

    # --- edit ---
    def add_tracks(s, pts, L, width, net):
        out = []
        for p, q in zip(pts, pts[1:]):
            if abs(p[0] - q[0]) < 1e-6 and abs(p[1] - q[1]) < 1e-6:
                continue
            t = pcbnew.PCB_TRACK(s.b)
            t.SetStart(pcbnew.VECTOR2I(F(p[0]), F(p[1])))
            t.SetEnd(pcbnew.VECTOR2I(F(q[0]), F(q[1])))
            t.SetWidth(F(width))
            t.SetLayer(L)
            t.SetNetCode(net)
            s.b.Add(t)
            out.append(t)
        return out

    def split_at(s, net, L, p, skip=()):
        """Split `net`'s track on `L` that passes through p (mm) strictly between its ends, so p becomes a vertex (a T
        joins there). Tracks in `skip` are left alone. Returns True if one was split."""
        v = pcbnew.VECTOR2I(F(p[0]), F(p[1]))
        for t in s.b.GetTracks():
            if t.Type() != pcbnew.PCB_TRACE_T or t.GetNetCode() != net or t.GetLayer() != L or t in skip:
                continue
            a, e = t.GetStart(), t.GetEnd()
            ax, ay, ex, ey = M(a.x), M(a.y), M(e.x), M(e.y)
            dx, dy = ex - ax, ey - ay
            l2 = dx * dx + dy * dy
            if l2 == 0:
                continue
            u = ((p[0] - ax) * dx + (p[1] - ay) * dy) / l2
            if not (1e-6 < u < 1 - 1e-6) or math.hypot(ax + u * dx - p[0], ay + u * dy - p[1]) > 0.002:
                continue
            if math.hypot(p[0] - ax, p[1] - ay) < 0.002 or math.hypot(p[0] - ex, p[1] - ey) < 0.002:
                continue
            t2 = pcbnew.PCB_TRACK(s.b)
            t2.SetStart(v)
            t2.SetEnd(e)
            t2.SetWidth(t.GetWidth())
            t2.SetLayer(L)
            t2.SetNetCode(net)
            t.SetEnd(v)
            s.b.Add(t2)
            return True
        return False

    def replace(s, chain, pts, L=None, width=None):
        L = chain["layer"] if L is None else L
        for t in chain["tracks"]:
            s.b.RemoveNative(t)
        new = s.add_tracks(pts, L, chain["width"] if width is None else width, chain["net"])
        s.redraw_net(chain["net"])
        return new


def simplify(pts):
    """Drop repeated and collinear points."""
    out = [pts[0]]
    for p in pts[1:]:
        if math.hypot(p[0] - out[-1][0], p[1] - out[-1][1]) < 1e-6:
            continue
        if len(out) >= 2:
            a, b = out[-2], out[-1]
            if abs((b[0] - a[0]) * (p[1] - b[1]) - (b[1] - a[1]) * (p[0] - b[0])) < 1e-9 and (b[0] - a[0]) * (p[0] - b[0]) + (b[1] - a[1]) * (p[1] - b[1]) > 0:
                out[-1] = p
                continue
        out.append(p)
    return out


def seg_dist(p, a, b):
    dx, dy = b[0] - a[0], b[1] - a[1]
    l2 = dx * dx + dy * dy
    t = 0 if l2 == 0 else max(0.0, min(1.0, ((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / l2))
    return math.hypot(p[0] - a[0] - t * dx, p[1] - a[1] - t * dy)


def octo(p, q, tol=1e-4):
    """Is p->q horizontal, vertical or diagonal?"""
    ax, ay = abs(q[0] - p[0]), abs(q[1] - p[1])
    return ax < tol or ay < tol or abs(ax - ay) < tol


def square_ends(pts, chk):
    """Make the end segments (at pads, whose centres are off the lattice) 45-degree: slide the first corner along the
    next segment until the end segment is octilinear, else put a short 45-degree jog next to the end."""
    for rev in (False, True):
        pts = list(reversed(pts)) if rev else list(pts)
        if len(pts) >= 2 and not octo(pts[0], pts[1]):
            p0, p1 = pts[0], pts[1]
            done = False
            if len(pts) >= 3:
                d = (pts[2][0] - p1[0], pts[2][1] - p1[1])
                best = None
                for u in ((1, 0), (0, 1), (1, 1), (1, -1)):
                    det = u[0] * (-d[1]) - u[1] * (-d[0])
                    if abs(det) < 1e-12:
                        continue
                    rx, ry = p1[0] - p0[0], p1[1] - p0[1]
                    t = (u[0] * ry - u[1] * rx) / det  # p0 + s*u = p1 + t*d
                    if -1.0 < t < 0.95 and abs(t) * math.hypot(*d) < 0.5:
                        q = (p1[0] + t * d[0], p1[1] + t * d[1])
                        if best is None or abs(t) < best[0]:
                            best = (abs(t), q)
                if best is not None and chk(p0, best[1]) and chk(best[1], pts[2]):
                    pts[1] = best[1]
                    done = True
            if not done:
                opts = sorted(octo_links(p0, p1), key=lambda m: math.hypot(m[0][0] - p0[0], m[0][1] - p0[1]) if m else 0)
                for m in opts:
                    if m and chk(p0, m[0]) and chk(m[0], p1):
                        pts[1:1] = m
                        break
        pts = simplify(list(reversed(pts)) if rev else pts)
    return pts


def octo_links(p, q):
    """The one- or two-segment 45-degree connections from p to q (the middle points)."""
    dx, dy = q[0] - p[0], q[1] - p[1]
    ax, ay = abs(dx), abs(dy)
    if ax < 1e-6 or ay < 1e-6 or abs(ax - ay) < 1e-6:
        return [[]]
    m = min(ax, ay)
    diag = (math.copysign(m, dx), math.copysign(m, dy))
    straight = (dx - diag[0], dy - diag[1])
    return [[(p[0] + diag[0], p[1] + diag[1])], [(p[0] + straight[0], p[1] + straight[1])]]


def bends(pts):
    n = 0.0
    for a, b, c in zip(pts, pts[1:], pts[2:]):
        u = (b[0] - a[0], b[1] - a[1]); v = (c[0] - b[0], c[1] - b[1])
        cosang = (u[0] * v[0] + u[1] * v[1]) / (math.hypot(*u) * math.hypot(*v) or 1)
        n += 1 if cosang > 0.5 else 2  # 45 degrees one, 90 or more two
    return n


def length(pts):
    return sum(math.hypot(q[0] - p[0], q[1] - p[1]) for p, q in zip(pts, pts[1:]))


def cost(pts):
    return length(pts) + T45 * bends(pts)


def tighten(pts, chk):
    """Replace runs of corners with the shortest clear 45-degree link, farthest first, until nothing changes."""
    pts = list(pts)
    changed = True
    while changed:
        changed = False
        i = 0
        while i < len(pts) - 2:
            for k in range(len(pts) - 1, i + 1, -1):
                best = None
                for mid in octo_links(pts[i], pts[k]):
                    cand = [pts[i]] + mid + [pts[k]]
                    if all(chk(p, q) for p, q in zip(cand, cand[1:])):
                        new = simplify(pts[:i] + cand + pts[k + 1:])
                        if cost(new) < cost(pts) - 1e-6 and (best is None or cost(new) < cost(best)):
                            best = new
                if best is not None:
                    pts = best
                    changed = True
                    break
            i += 1
    return simplify(pts)


# --- whole nets, three layers ---
LCOST = {pcbnew.F_Cu: 1.0, pcbnew.In2_Cu: 1.15, pcbnew.B_Cu: 1.8}  # per mm: the back faces up in the pan, keep it clear
VIA_D, VIA_DRILL = 0.6, 0.3


def net_items(b, net):
    tracks = [t for t in b.GetTracks() if t.GetNetCode() == net]
    return tracks


def snapshot(b, net):
    out = []
    for t in net_items(b, net):
        if t.Type() == pcbnew.PCB_VIA_T:
            out.append(("via", t.GetPosition().x, t.GetPosition().y, t.GetWidth(pcbnew.F_Cu), t.GetDrillValue()))
        elif t.Type() == pcbnew.PCB_TRACE_T:
            out.append(("trk", t.GetStart().x, t.GetStart().y, t.GetEnd().x, t.GetEnd().y, t.GetWidth(), t.GetLayer()))
        else:
            raise ValueError("arcs not handled")
    return out


def restore(b, net, snap):
    for it in snap:
        if it[0] == "via":
            v = pcbnew.PCB_VIA(b)
            v.SetPosition(pcbnew.VECTOR2I(it[1], it[2]))
            v.SetWidth(it[3])
            v.SetDrill(it[4])
            v.SetViaType(pcbnew.VIATYPE_THROUGH)
            v.SetNetCode(net)
            b.Add(v)
        else:
            t = pcbnew.PCB_TRACK(b)
            t.SetStart(pcbnew.VECTOR2I(it[1], it[2]))
            t.SetEnd(pcbnew.VECTOR2I(it[3], it[4]))
            t.SetWidth(it[5])
            t.SetLayer(it[6])
            t.SetNetCode(net)
            b.Add(t)


def snap_cost(snap):
    c = 0.0
    segs = {}
    for it in snap:
        if it[0] == "via":
            c += VIA_COST
        else:
            p, q = (M(it[1]), M(it[2])), (M(it[3]), M(it[4]))
            c += LCOST.get(it[6], 1.0) * math.hypot(q[0] - p[0], q[1] - p[1])
            for k in (p, q):
                segs.setdefault((it[6], k), []).append((p, q) if k == q else (q, p))
    for (L, k), ss in segs.items():  # bends where exactly two tracks meet
        if len(ss) == 2:
            u = (ss[0][0][0] - k[0], ss[0][0][1] - k[1]); v = (ss[1][0][0] - k[0], ss[1][0][1] - k[1])
            cs = (u[0] * v[0] + u[1] * v[1]) / ((math.hypot(*u) * math.hypot(*v)) or 1)
            if cs > -0.999:
                c += T45 * (1 if cs < -0.5 else 2)
    return c


def route_net(r, net, width, margin=5.0, max_nodes=400000):
    """Rip up `net`'s tracks and vias and connect its pads again; keep the new copper only if every pad joins and it
    costs less than the old. Returns (old cost, new cost or None)."""
    b = r.b
    pads = [p for fp in b.GetFootprints() for p in fp.Pads() if p.GetNetCode() == net]
    if len(pads) < 2:
        return None
    snap = snapshot(b, net)
    old = snap_cost(snap)
    xs = [M(p.GetPosition().x) for p in pads]; ys = [M(p.GetPosition().y) for p in pads]
    box = r.win(min(xs) - margin, min(ys) - margin, max(xs) + margin, max(ys) + margin)
    for t in net_items(b, net):
        b.RemoveNative(t)
    r.redraw_net(net)
    i0, j0, i1, j1 = box
    ok = {L: r.free(net, L, width, box) for L in LAYERS}
    okv = r.free(net, None, VIA_D, box, vias=True)
    for p in pads:  # no via in our own SMD pads
        if p.GetAttribute() == pcbnew.PAD_ATTRIB_SMD:
            bb = p.GetBoundingBox()
            a0, c0, a1, c1 = r.win(M(bb.GetX()) - 0.35, M(bb.GetY()) - 0.35, M(bb.GetRight()) + 0.35, M(bb.GetBottom()) + 0.35)
            okv[max(0, a0 - i0):max(0, a1 - i0), max(0, c0 - j0):max(0, c1 - j0)] = False
    nx, ny = i1 - i0, j1 - j0
    cellf = lambda p: ((p[0] - r.x0) / G - i0, (p[1] - r.y0) / G - j0)

    def lat(p):  # nearest search-lattice node (global lattice: indices multiple of S)
        fi, fj = cellf(p)
        gi = int(round((fi + i0) / S)) * S - i0
        gj = int(round((fj + j0) / S)) * S - j0
        return gi, gj

    def padlayers(p):
        return [L for L in LAYERS if p.IsOnLayer(L) and p.FlashLayer(L)]

    # groups of pads already joined (overlapping same-net pads): start with each pad alone
    comp = list(range(len(pads)))
    def find(i):
        while comp[i] != i:
            i = comp[i]
        return i
    tree_pts = []  # (x, y, L) exact source points of the tree
    first = min(range(len(pads)), key=lambda i: (xs[i] - sum(xs) / len(xs)) ** 2 + (ys[i] - sum(ys) / len(ys)) ** 2)
    joined = {first}
    for L in padlayers(pads[first]):
        tree_pts.append((xs[first], ys[first], L))
    new_items = []
    xy = lambda i, j: (r.x0 + (i + i0) * G, r.y0 + (j + j0) * G)
    while len(joined) < len(pads):
        goals = {}
        for k, p in enumerate(pads):
            if k in joined:
                continue
            for L in padlayers(p):
                gi, gj = lat((xs[k], ys[k]))
                goals[(gi, gj, L)] = k
        gl = {(g[0], g[1]) for g in goals}
        I, J = np.meshgrid(np.arange(nx), np.arange(ny), indexing="ij")
        harr = np.full((nx, ny), np.inf)
        for a, c in gl:
            dx, dy = np.abs(I - a), np.abs(J - c)
            harr = np.minimum(harr, (np.maximum(dx, dy) + 0.4142 * np.minimum(dx, dy)) * G)
        harr = harr.tolist()
        h = lambda i, j: harr[i][j]
        pq, dist, prev, src = [], {}, {}, {}
        for (x, y, L) in tree_pts:
            si, sj = lat((x, y))
            if not (0 <= si < nx and 0 <= sj < ny):
                continue
            st = (si, sj, L, -1)
            if st not in dist:
                dist[st] = 0.0
                src[st] = (x, y)
                heapq.heappush(pq, (h(si, sj), 0.0, st))
        end = None
        n = 0
        while pq:
            f, d, st = heapq.heappop(pq)
            if d > dist.get(st, 1e18) + 1e-12:
                continue
            i, j, L, hd = st
            if (i, j, L) in goals:
                end = st
                break
            n += 1
            if n > max_nodes:
                break
            for nd_, (di, dj) in enumerate(DIRS):
                if hd >= 0:
                    turn = min((nd_ - hd) % 8, (hd - nd_) % 8)
                    if turn > 2:
                        continue
                    tc = (0, T45, T90)[turn]
                else:
                    tc = 0
                ni, nj = i + di * S, j + dj * S
                if not (0 <= ni < nx and 0 <= nj < ny):
                    continue
                okL = ok[L]
                if (ni, nj, L) not in goals and not (okL[ni, nj] and okL[i + di * S // 2, j + dj * S // 2]):
                    continue
                step = S * G * (1.41421 if di and dj else 1) * LCOST[L]
                nst = (ni, nj, L, nd_)
                ndist = d + step + tc
                if ndist < dist.get(nst, 1e18) - 1e-12:
                    dist[nst] = ndist
                    prev[nst] = st
                    heapq.heappush(pq, (ndist + h(ni, nj), ndist, nst))
            if okv[i, j]:
                for L2 in LAYERS:
                    if L2 != L and ok[L2][i, j]:
                        nst = (i, j, L2, -1)
                        ndist = d + VIA_COST
                        if ndist < dist.get(nst, 1e18) - 1e-12:
                            dist[nst] = ndist
                            prev[nst] = st
                            heapq.heappush(pq, (ndist + h(i, j), ndist, nst))
        if end is None:
            for t in new_items:
                b.RemoveNative(t)
            restore(b, net, snap)
            r.redraw_net(net)
            return old, None
        k = goals[(end[0], end[1], end[2])]
        path = [end]
        while path[-1] in prev:
            path.append(prev[path[-1]])
        path = path[::-1]
        # split into layer runs; exact ends
        runs, cur = [], [path[0]]
        for a, c in zip(path, path[1:]):
            if a[2] != c[2]:
                runs.append(cur)
                cur = [c]
            else:
                cur.append(c)
        runs.append(cur)
        vias = [(xy(rr[-1][0], rr[-1][1])) for rr in runs[:-1]]
        for ri, rr in enumerate(runs):
            L = rr[0][2]
            pts = [xy(i, j) for i, j, _, _ in rr]
            if ri == 0:
                pts = [src[path[0]]] + pts
            if ri == len(runs) - 1:
                pts = pts + [(xs[k], ys[k])]
            chk = lambda p, q, L=L: r.seg_ok(ok[L], box, p, q) or math.hypot(q[0] - p[0], q[1] - p[1]) < 0.06
            pts = tighten(simplify(pts), chk)
            new = r.add_tracks(pts, L, width, net)
            new_items += new
            for p, q in zip(pts, pts[1:]):
                l = math.hypot(q[0] - p[0], q[1] - p[1])
                for t in np.linspace(0, 1, max(2, int(l / 0.3) + 1)):
                    tree_pts.append((p[0] + (q[0] - p[0]) * t, p[1] + (q[1] - p[1]) * t, L))
        for (x, y) in vias:
            v = pcbnew.PCB_VIA(b)
            v.SetPosition(pcbnew.VECTOR2I(F(x), F(y)))
            v.SetWidth(F(VIA_D))
            v.SetDrill(F(VIA_DRILL))
            v.SetViaType(pcbnew.VIATYPE_THROUGH)
            v.SetNetCode(net)
            b.Add(v)
            new_items.append(v)
            for L in LAYERS:
                tree_pts.append((x, y, L))
        joined.add(k)
        for L in padlayers(pads[k]):
            tree_pts.append((xs[k], ys[k], L))
        # pads overlapping the joined one are joined too
        for k2, p2 in enumerate(pads):
            if k2 not in joined and abs(xs[k2] - xs[k]) < 1e-3 and abs(ys[k2] - ys[k]) < 1e-3:
                joined.add(k2)
    new_snap = snapshot(b, net)
    new = snap_cost(new_snap)
    if new >= old - 0.2:
        for t in net_items(b, net):
            b.RemoveNative(t)
        restore(b, net, snap)
        r.redraw_net(net)
        return old, None
    r.redraw_net(net)
    return old, new
