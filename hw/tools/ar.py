"""Route a set of nets from scratch with rip-up and retry, on `octo`'s rasters (KiCad's python).

`route_all(r, nets, widths)`: every net in `nets` (codes) loses its tracks and vias; then each net's pads are joined
into a tree (`route_tree`), 45-degree runs over F.Cu, In2 and B.Cu with a cost per bend, per via and per layer (`LCOST`:
the back is dear). Negotiated congestion (PathFinder): pads, fixed copper, the edge and keep-outs are hard, but the
nets being routed may overlap one another at a price, each step costing more for every other net whose clearance
room it is in (`Nego.occ`, scaled by a present factor that grows each pass) and for the overlaps that place has seen
before (`Nego.hist`). Each pass reroutes the nets still in conflict and those they conflict with, until none are.
Returns the codes of the nets left in conflict or unrouted (empty on success)."""

import math
import time

import numpy as np
import pcbnew
from scipy.ndimage import distance_transform_edt

import octo
from octo import DIRS, G, LAYERS, M, S, F

LCOST = (1.0, 1.1, 3.0)  # F.Cu, In2, B.Cu per mm
VIA_COST = 3.0
VIA_D, VIA_DRILL = 0.6, 0.3
SQ2 = math.sqrt(2)
HW = 1.0  # heuristic weight (above 1: faster, a little less than shortest)
STALL, STALL_CELLS = 4, 10  # passes with the same few conflicting cells before they are left to DRC


def pads_of(b, net):
    return [p for fp in b.GetFootprints() for p in fp.Pads() if p.GetNetCode() == net]


def padlayers(p):
    return [li for li, L in enumerate(LAYERS) if p.IsOnLayer(L) and p.FlashLayer(L)]


def padnodes(lat, p, li, pos):
    """Lattice nodes for pad `p` on layer index `li`: its centre's, and every free node on its copper (a pad centre
    off the lattice or in a tight spot still has a way out). Keyed (li, a, b)."""
    a0, b0 = lat.ab(pos)
    out = [(li, a0, b0)]
    bb = p.GetBoundingBox()
    a1, b1 = lat.ab((M(bb.GetX()), M(bb.GetY())))
    a2, b2 = lat.ab((M(bb.GetRight()), M(bb.GetBottom())))
    for a in range(max(0, a1), min(lat.na, a2 + 1)):
        for b in range(max(0, b1), min(lat.nb, b2 + 1)):
            if lat.node[li][a, b]:
                x, y = lat.xy(a, b)
                if p.HitTest(pcbnew.VECTOR2I(F(x), F(y))):
                    out.append((li, a, b))
    return out


class Lattice:
    """The search lattice for one net over a box: node and move masks per layer, via mask; optional soft masks."""

    def __init__(s, r, net, width, box, soft=(), cost=None):
        i0, j0, i1, j1 = box
        i0 -= i0 % S
        j0 -= j0 % S
        s.box = (i0, j0, i1, j1)
        s.r, s.i0, s.j0 = r, i0, j0
        hard = [r.free(net, L, width, s.box, ignore=soft) for L in LAYERS]
        for h, L in zip(hard, LAYERS):  # inside its own pad (shrunk by half the width) a track adds no copper
            own = r.code[L][i0:i1, j0:j1] == net
            if own.any():
                h |= distance_transform_edt(own) * G >= width / 2 + G / 2
        hv = r.free(net, None, VIA_D, s.box, vias=True, ignore=soft)
        s.ok_r = hard  # raster masks, for tightening
        s.na, s.nb = (hard[0].shape[0] - 1) // S, (hard[0].shape[1] - 1) // S
        na, nb = s.na, s.nb
        s.node = [h[: na * S : S, : nb * S : S].copy() for h in hard]
        s.breathe = [r.breathe[L][i0: i0 + na * S: S, j0: j0 + nb * S: S] for L in LAYERS]
        s.cost = cost(s) if cost else None  # ([per-layer node factor], via node factor), arrays (na, nb)
        # moves: source node (a, b) to (a + da, b + db) needs the target node and the raster cell between free
        s.move = []
        for li in range(3):
            h, nd = hard[li], s.node[li]
            mv = []
            for da, db in DIRS:
                m = np.zeros((na, nb), bool)
                a0, a1 = max(0, -da), na - max(0, da)
                b0, b1 = max(0, -db), nb - max(0, db)
                tgt = nd[a0 + da:a1 + da, b0 + db:b1 + db]
                mid = h[(a0 * S + da):(a1 * S + da):S, (b0 * S + db):(b1 * S + db):S][: a1 - a0, : b1 - b0]
                m[a0:a1, b0:b1] = tgt & mid  # the source may be a pad centre in a tight spot
                mv.append(m.ravel().tolist())
            s.move.append(mv)
        s.via = hv[: na * S : S, : nb * S : S].ravel().tolist()
        s.node_l = [n.ravel().tolist() for n in s.node]

    def ab(s, p):
        return int(round(((p[0] - s.r.x0) / G - s.i0) / S)), int(round(((p[1] - s.r.y0) / G - s.j0) / S))

    def xy(s, a, b):
        return s.r.x0 + (s.i0 + a * S) * G, s.r.y0 + (s.j0 + b * S) * G

    def graph(s):
        """The lattice as a sparse graph (built once): moves on each layer, vias between layers."""
        if getattr(s, "_g", None) is not None:
            return s._g
        na, nb = s.na, s.nb
        plane = na * nb
        rows, cols, w = [], [], []
        idx = np.arange(plane).reshape(na, nb)
        for li in range(3):
            for d, (da, db) in enumerate(DIRS):
                m = np.array(s.move[li][d], bool).reshape(na, nb)
                src = idx[m]
                dst = src + da * nb + db
                cost = np.full(src.shape, S * G * (SQ2 if da and db else 1.0) * LCOST[li])
                if s.cost is not None:
                    cost *= s.cost[0][li].ravel()[dst]
                cost *= 1 + octo.BREATHE_COST * s.breathe[li].ravel()[dst]  # leave the pour a way to SMD GND pads
                rows.append(src + li * plane); cols.append(dst + li * plane); w.append(cost)
        v = np.array(s.via, bool) & np.array(s.node_l[0], bool)
        for l1 in range(3):
            for l2 in range(3):
                if l1 != l2:
                    ok = v & np.array(s.node_l[l1], bool) & np.array(s.node_l[l2], bool)
                    src = np.nonzero(ok)[0]
                    vc = np.full(src.shape, VIA_COST) if s.cost is None else VIA_COST * s.cost[1].ravel()[src]
                    rows.append(src + l1 * plane); cols.append(src + l2 * plane); w.append(vc)
        s._g = (np.concatenate(rows), np.concatenate(cols), np.concatenate(w))
        return s._g

    def search(s, sources, goals, max_nodes=None):
        """Dijkstra from source nodes {(li, a, b): exact xy} to the nearest goal node {(li, a, b): key}.
        Returns (path of (li, a, b), key) or None."""
        from scipy.sparse import coo_matrix
        from scipy.sparse.csgraph import dijkstra
        na, nb = s.na, s.nb
        plane = na * nb
        N = 3 * plane + 1  # the last node: a super-source joined to every source
        rows, cols, w = s.graph()
        er, ec, ew = [], [], []
        for (li, a, b) in goals:  # into a goal from any neighbour, even where the goal node itself is tight
            if not (0 <= a < na and 0 <= b < nb):
                continue
            g = li * plane + a * nb + b
            for da, db in DIRS:
                a2, b2 = a - da, b - db
                if 0 <= a2 < na and 0 <= b2 < nb and s.node_l[li][a2 * nb + b2]:
                    er.append(li * plane + a2 * nb + b2); ec.append(g); ew.append(S * G * (SQ2 if da and db else 1.0) * LCOST[li])
        for (li, a, b) in sources:
            if 0 <= a < na and 0 <= b < nb:
                er.append(3 * plane); ec.append(li * plane + a * nb + b); ew.append(1e-9)
        R = np.concatenate([rows, np.array(er, int)]); C = np.concatenate([cols, np.array(ec, int)]); W = np.concatenate([w, np.array(ew)])
        A = coo_matrix((W, (R, C)), shape=(N, N)).tocsr()
        dist, pred = dijkstra(A, directed=True, indices=3 * plane, return_predecessors=True)
        best, bk = None, None
        for (li, a, b), k in goals.items():
            if 0 <= a < na and 0 <= b < nb:
                g = li * plane + a * nb + b
                if np.isfinite(dist[g]) and (best is None or dist[g] < dist[best]):
                    best, bk = g, k
        if best is None:
            return None
        path = [best]
        while pred[path[-1]] != 3 * plane:
            path.append(pred[path[-1]])
        out = []
        for c in reversed(path):
            li, rem = divmod(int(c), plane)
            out.append((li, rem // nb, rem % nb))
        return out, bk


def route_tree(r, net, width, margin=4.0, nego=None, tight=True):
    """Join `net`'s pads (its tracks already gone) and add the copper. Returns the items added, or None (nothing added)
    when there is no route. With `nego`, the other routed nets are passable at its price (see `Nego`)."""
    b = r.b
    pads = pads_of(b, net)
    if len(pads) < 2:
        return []
    pos = [(M(p.GetPosition().x), M(p.GetPosition().y)) for p in pads]
    xs, ys = [p[0] for p in pos], [p[1] for p in pos]
    box = r.win(min(xs) - margin, min(ys) - margin, max(xs) + margin, max(ys) + margin)
    soft = nego.routed - {net} if nego else ()
    lat = Lattice(r, net, width, box, soft, (lambda l: nego.cost(l, net)) if nego else None)
    ok_r = lat.ok_r
    if nego:  # tightening may not cut through anyone's room
        i0, j0, i1, j1 = lat.box
        ok_r = [h & ((nego.occ[L][i0:i1, j0:j1] == 0) | (r.code[L][i0:i1, j0:j1] == net)) for h, L in zip(ok_r, LAYERS)]
    cx, cy = sum(xs) / len(xs), sum(ys) / len(ys)
    first = min(range(len(pads)), key=lambda k: (xs[k] - cx) ** 2 + (ys[k] - cy) ** 2)
    joined = {first}
    for k in range(len(pads)):  # pads on the same spot
        if abs(xs[k] - xs[first]) < 1e-3 and abs(ys[k] - ys[first]) < 1e-3:
            joined.add(k)
    sources = {}
    for k in joined:
        for li in padlayers(pads[k]):
            for nd in padnodes(lat, pads[k], li, pos[k]):
                sources[nd] = pos[k]
    plan = []  # ("trk", li, pts) / ("via", xy)
    while len(joined) < len(pads):
        goals = {}
        for k in range(len(pads)):
            if k not in joined:
                for li in padlayers(pads[k]):
                    for nd in padnodes(lat, pads[k], li, pos[k]):
                        goals[nd] = k
        res = lat.search(sources, goals)
        if res is None:
            return None
        path, k = res
        runs, cur = [], [path[0]]
        for p, q in zip(path, path[1:]):
            if p[0] != q[0]:
                runs.append(cur)
                cur = [q]
            else:
                cur.append(q)
        runs.append(cur)
        for ri, rr in enumerate(runs):
            li = rr[0][0]
            pts = [lat.xy(a, bb) for _, a, bb in rr]
            if ri == 0:
                pts = [sources.get(path[0], pts[0])] + pts
            if ri == len(runs) - 1:
                pts = pts + [pos[k]]
            okr = ok_r[li]
            chk = lambda p, q, okr=okr: r.seg_ok(okr, lat.box, p, q) or math.hypot(q[0] - p[0], q[1] - p[1]) < 0.06
            pts = octo.tighten(octo.simplify(pts), chk) if tight else octo.simplify(pts)
            plan.append(("trk", li, pts))
            for p, q in zip(pts, pts[1:]):
                l = math.hypot(q[0] - p[0], q[1] - p[1])
                for t in np.linspace(0, 1, max(2, int(l / (S * G)) + 1)):
                    x, y = p[0] + (q[0] - p[0]) * t, p[1] + (q[1] - p[1]) * t
                    sources[(li,) + lat.ab((x, y))] = (x, y)
            if ri < len(runs) - 1:
                v = lat.xy(rr[-1][1], rr[-1][2])
                plan.append(("via", v))
                for l2 in range(3):
                    sources[(l2,) + lat.ab(v)] = v
        joined.add(k)
        for k2 in range(len(pads)):
            if abs(xs[k2] - xs[k]) < 1e-3 and abs(ys[k2] - ys[k]) < 1e-3:
                joined.add(k2)
        for k2 in joined:
            for li in padlayers(pads[k2]):
                for nd in padnodes(lat, pads[k2], li, pos[k2]):
                    sources.setdefault(nd, pos[k2])
    items = []
    for it in plan:
        if it[0] == "trk":
            new = r.add_tracks(it[2], LAYERS[it[1]], width, net)
            for p in (it[2][0], it[2][-1]):  # a branch joining a run partway: make the joint a vertex
                r.split_at(net, LAYERS[it[1]], p, skip=new)
            items += new
        else:
            v = pcbnew.PCB_VIA(b)
            v.SetPosition(pcbnew.VECTOR2I(F(it[1][0]), F(it[1][1])))
            v.SetWidth(F(VIA_D))
            v.SetDrill(F(VIA_DRILL))
            v.SetViaType(pcbnew.VIATYPE_THROUGH)
            v.SetNetCode(net)
            b.Add(v)
            items.append(v)
    r.redraw_net(net)
    return items


def rip(r, net):
    for t in [t for t in r.b.GetTracks() if t.GetNetCode() == net]:
        r.b.RemoveNative(t)
    r.redraw_net(net)


class Nego:
    """Shared state of negotiated routing, on the router's raster. For each routed net, its rooms: the cells where a
    track centreline (`occ`, per layer) or a via centre (`vocc`) would be too close to its copper. `occ`/`vocc` count
    the rooms over each cell; `hist`/`vhist` remember where conflicts were. All routed nets share one `width`."""

    def __init__(s, r, width):
        s.r, s.width = r, width
        s.need_t = octo.CLR + width + octo.MARGIN  # centre of a `width` track to the centre line of another
        s.need_v = octo.CLR + width / 2 + VIA_D / 2 + octo.MARGIN
        shape = (r.nx, r.ny)
        s.occ = {L: np.zeros(shape, np.int16) for L in LAYERS}
        s.vocc = np.zeros(shape, np.int16)
        s.hist = {L: np.zeros(shape, np.float32) for L in LAYERS}
        s.vhist = np.zeros(shape, np.float32)
        s.rooms = {}  # net: (window, {L: mask}, via mask)
        s.routed = set()
        s.pres = 0.5

    def add(s, net):
        r = s.r
        items = [t for t in r.b.GetTracks() if t.GetNetCode() == net]
        if not items:
            return
        bb = None
        for t in items:
            if t.Type() == pcbnew.PCB_VIA_T:
                c = t.GetPosition(); h = M(t.GetWidth(pcbnew.F_Cu)) / 2
                e = (M(c.x) - h, M(c.y) - h, M(c.x) + h, M(c.y) + h)
            else:
                a, z = t.GetStart(), t.GetEnd(); h = M(t.GetWidth()) / 2
                e = (min(M(a.x), M(z.x)) - h, min(M(a.y), M(z.y)) - h, max(M(a.x), M(z.x)) + h, max(M(a.y), M(z.y)) + h)
            bb = e if bb is None else (min(bb[0], e[0]), min(bb[1], e[1]), max(bb[2], e[2]), max(bb[3], e[3]))
        k = max(s.need_t, s.need_v) + 0.1
        w = r.win(bb[0] - k, bb[1] - k, bb[2] + k, bb[3] + k)
        i0, j0, i1, j1 = w
        cop = {L: np.zeros((r.nx, r.ny), bool) for L in LAYERS}
        for t in items:
            if t.Type() == pcbnew.PCB_VIA_T:
                c = t.GetPosition()
                for L in LAYERS:
                    r.disc(cop[L], M(c.x), M(c.y), M(t.GetWidth(pcbnew.F_Cu)) / 2, True)
            elif t.GetLayer() in LAYERS:
                a, z = t.GetStart(), t.GetEnd()
                r.capsule(cop[t.GetLayer()], M(a.x), M(a.y), M(z.x), M(z.y), M(t.GetWidth()) / 2, True)
        rooms, anyc = {}, np.zeros((i1 - i0, j1 - j0), bool)
        for L in LAYERS:
            c = cop[L][i0:i1, j0:j1]
            anyc |= c
            rooms[L] = (distance_transform_edt(~c) * G < s.need_t) if c.any() else np.zeros(c.shape, bool)
            s.occ[L][i0:i1, j0:j1] += rooms[L]
        vroom = distance_transform_edt(~anyc) * G < s.need_v
        s.vocc[i0:i1, j0:j1] += vroom
        s.rooms[net] = (w, rooms, vroom)
        s.routed.add(net)

    def remove(s, net):
        if net not in s.rooms:
            s.routed.discard(net)
            return
        (i0, j0, i1, j1), rooms, vroom = s.rooms.pop(net)
        for L in LAYERS:
            s.occ[L][i0:i1, j0:j1] -= rooms[L]
        s.vocc[i0:i1, j0:j1] -= vroom
        s.routed.discard(net)

    def cost(s, lat, net):
        """Node cost factors over a lattice's box (the net itself already removed). Inside its own pads a track adds
        no copper, so crowding there costs nothing."""
        i0, j0 = lat.i0, lat.j0
        sl = lambda a: a[i0: i0 + lat.na * S: S, j0: j0 + lat.nb * S: S]
        node = [(1 + sl(s.hist[L])) * (1 + s.pres * sl(s.occ[L]) * (sl(s.r.code[L]) != net)) for L in LAYERS]
        via = (1 + sl(s.vhist)) * (1 + s.pres * sl(s.vocc))
        return node, via

    def conflicts(s, net):
        """Cells where `net`'s centrelines or vias are in another net's room: [(L or None, i, j)]."""
        r = s.r
        out = []
        for t in r.b.GetTracks():
            if t.GetNetCode() != net:
                continue
            if t.Type() == pcbnew.PCB_VIA_T:
                c = t.GetPosition()
                i, j = int(round((M(c.x) - r.x0) / G)), int(round((M(c.y) - r.y0) / G))
                if s.vocc[i, j] > 1:
                    out.append((None, i, j))
            elif t.GetLayer() in LAYERS:
                a, z = t.GetStart(), t.GetEnd()
                p, q = (M(a.x), M(a.y)), (M(z.x), M(z.y))
                n = max(2, int(math.hypot(q[0] - p[0], q[1] - p[1]) / G) + 1)
                tt = np.linspace(0, 1, n)
                ii = np.rint((p[0] + (q[0] - p[0]) * tt - r.x0) / G).astype(int)
                jj = np.rint((p[1] + (q[1] - p[1]) * tt - r.y0) / G).astype(int)
                bad = (s.occ[t.GetLayer()][ii, jj] > 1) & (r.code[t.GetLayer()][ii, jj] != net)
                out += [(t.GetLayer(), int(i), int(j)) for i, j in zip(ii[bad], jj[bad])]
        return out

    def who(s, L, i, j, net):
        """The other nets whose room holds cell (i, j)."""
        out = set()
        for m, ((i0, j0, i1, j1), rooms, vroom) in s.rooms.items():
            if m != net and i0 <= i < i1 and j0 <= j < j1 and (vroom if L is None else rooms[L])[i - i0, j - j0]:
                out.add(m)
        return out

    def blame(s, cells, amount):
        """Raise the history cost around conflict cells (a small disc: centrelines are off the lattice)."""
        for L, i, j in cells:
            h = s.vhist if L is None else s.hist[L]
            h[max(0, i - 3): i + 4, max(0, j - 3): j + 4] += amount


def route_all(r, nets, widths, passes=40, log=print):
    b = r.b
    width = max(widths[n] for n in nets)
    nego = Nego(r, width)
    names = {p.GetNetCode(): p.GetNetname() for fp in b.GetFootprints() for p in fp.Pads()}
    for n in nets:
        rip(r, n)

    def span(n):
        ps = pads_of(b, n)
        xs = [M(p.GetPosition().x) for p in ps]; ys = [M(p.GetPosition().y) for p in ps]
        return (max(xs) - min(xs) + max(ys) - min(ys)) if ps else 0

    todo = sorted(nets, key=span)
    unrouted = set()
    seen = []  # (nets in conflict, cells) per pass, to notice a stall
    for ps in range(passes):
        t0 = time.time()
        for n in todo:
            rip(r, n)
            nego.remove(n)
            res = None
            for margin in (4.0, 10.0):
                res = route_tree(r, n, widths[n], margin=margin, nego=nego, tight=False)
                if res is not None:
                    break
            if res is None:
                unrouted.add(n)
                continue
            unrouted.discard(n)
            nego.add(n)
        bad, cells = set(), []
        for n in nets:
            c = nego.conflicts(n)
            if c:
                bad.add(n)
                cells += c
                for L, i, j in c[:: max(1, len(c) // 4)]:
                    bad |= nego.who(L, i, j, n)
        nego.blame(cells, 0.3)
        log(f"pass {ps + 1}: {len(todo)} routed, {len(bad)} nets in conflict ({len(cells)} cells), pres {nego.pres:.2f}, "
            f"{len(unrouted)} unrouted{' (' + ', '.join(names.get(n, str(n)) for n in unrouted) + ')' if unrouted else ''} ({time.time() - t0:.1f} s)" + (f": {', '.join(sorted(names.get(n, str(n)) for n in bad))}" if len(bad) <= 12 else ""))
        seen.append((frozenset(bad), len(cells)))
        stalled = len(seen) > STALL and not unrouted and len(cells) <= STALL_CELLS and len({b for b, _ in seen[-STALL:]}) == 1
        if stalled:  # a few cells the raster calls too close and no route avoids: DRC, with the exact geometry, judges
            log(f"stalled on {len(cells)} cells: leaving them to DRC")
        if (not bad or stalled) and not unrouted:
            pull_tight(r, nets, widths, log)
            return set()
        nego.pres *= 1.6
        todo = sorted(bad | unrouted, key=span)
    for n in sorted(bad, key=lambda n: names.get(n, "")):
        for L, i, j in nego.conflicts(n)[:3]:
            x, y = r.x0 + i * G, r.y0 + j * G
            log(f"  conflict {names.get(n, n)} {'via' if L is None else pcbnew.LayerName(L) if hasattr(pcbnew, 'LayerName') else L} "
                f"({x:.2f}, {y:.2f}) with {', '.join(names.get(m, str(m)) for m in nego.who(L, i, j, n))}")
    return bad | unrouted


def pull_tight(r, nets, widths, log=print):
    """After negotiation (no overlaps left): redraw the raster cleanly, then pull every chain tight."""
    for n in nets:
        r.redraw_net(n)
    t0, k = time.time(), 0
    for n in nets:
        for L in LAYERS:
            for c in r.chains(n, L):
                if c["pinned"] or len(c["pts"]) < 2:
                    continue
                xs, ys = [p[0] for p in c["pts"]], [p[1] for p in c["pts"]]
                box = r.win(min(xs) - 1.5, min(ys) - 1.5, max(xs) + 1.5, max(ys) + 1.5)
                ok = r.free(n, L, c["width"], box)
                chk = lambda p, q, ok=ok, box=box: r.seg_ok(ok, box, p, q) or math.hypot(q[0] - p[0], q[1] - p[1]) < 0.06
                pts = octo.square_ends(octo.tighten(octo.simplify(c["pts"]), chk), chk)
                if pts != c["pts"] and octo.cost(pts) < octo.cost(c["pts"]) + 0.5:
                    r.replace(c, pts)
                    k += 1
    log(f"pulled tight: {k} chains ({time.time() - t0:.1f} s)")


def repair_fixed(r, nets, flagged, log=print):
    """After a placement move: each chain of the fixed (not rerouted) nets in `nets` holding a track in `flagged`
    (uuids DRC reports, with the signal nets already ripped up) is routed again on its layer between its ends, at its
    width or narrower down to 0.2 mm; a flagged GND via is dropped (the pours and `islandvias.py`/`gndfan.py` stitch
    again). Returns the chains it could not repair."""
    b = r.b
    gnd = b.FindNet("GND").GetNetCode()
    dropped = []
    for v in [t for t in b.GetTracks() if t.Type() == pcbnew.PCB_VIA_T and t.GetNetCode() == gnd]:
        if v.m_Uuid.AsString() in flagged:
            dropped.append((M(v.GetPosition().x), M(v.GetPosition().y)))
            log(f"  GND via at ({dropped[-1][0]:.2f}, {dropped[-1][1]:.2f}) dropped")
            b.RemoveNative(v)
    for L in LAYERS:  # the stub that ran to a dropped via goes too (its pad has the pour)
        for c in r.chains(gnd, L):
            if any(math.hypot(e[0] - d[0], e[1] - d[1]) < 1e-3 for e in (c["pts"][0], c["pts"][-1]) for d in dropped):
                for t in c["tracks"]:
                    b.RemoveNative(t)
                log(f"  GND stub to a dropped via removed")
    r.redraw_net(gnd)
    bad = []
    for n in nets:
        for L in LAYERS:
            for c in r.chains(n, L):
                if not any(t.m_Uuid.AsString() in flagged for t in c["tracks"]):
                    continue
                name = c["tracks"][0].GetNetname()
                for w in [c["width"]] + [w for w in (0.5, 0.4, 0.3, 0.2) if w < c["width"]]:
                    p0, p1 = c["pts"][0], c["pts"][-1]
                    pts = r.route(n, L, w, p0, p1, margin=4.0)
                    if pts is None:  # an end in a mounting hole's standoff ring (plastic, over mask): as the rules allow
                        box = r.win(min(p0[0], p1[0]) - 4.0, min(p0[1], p1[1]) - 4.0, max(p0[0], p1[0]) + 4.0, max(p0[1], p1[1]) + 4.0)
                        mask = r.free(n, L, w, box, rings=False)
                        X, Y = r.grid(*box)
                        for p in (p0, p1):  # and the chain's own ends (copper already there; DRC judges)
                            mask |= np.hypot(X - p[0], Y - p[1]) < 0.3
                        pts = r.route(n, L, w, p0, p1, margin=4.0, mask=mask)
                    if pts is not None:
                        r.replace(c, pts, width=w)
                        log(f"  {name} on {b.GetLayerName(L)}: rerouted at {w} mm (was {c['width']})")
                        break
                else:
                    bad.append((name, b.GetLayerName(L), c["pts"][0], c["pts"][-1]))
                    log(f"  {name} on {b.GetLayerName(L)}: no repair from {c['pts'][0]} to {c['pts'][-1]}")
    return bad
