"""Outer-layer tidy: `tidy.py <in> <out> [layers]` (KiCad's python, from `hw/tools` for `octo.py`).

Every track chain on the given layers (default F.Cu,B.Cu) is routed again between its two ends with `octo` at its own
width; the new route replaces the old when it is cheaper (length plus a bend cost) by more than 0.1 mm. Chains with a
pad, via or T under their middle are left. Then every chain's end segments (at pads, off the lattice) are made
45-degree (`octo.square_ends`). Pours refill in a second process."""

import math
import subprocess
import sys

import octo
import pcbnew

b = pcbnew.LoadBoard(sys.argv[1])
layers = [b.GetLayerID(n) for n in (sys.argv[3] if len(sys.argv) > 3 else "F.Cu,B.Cu").split(",")]
r = octo.Router(b)
done = kept = pinned = failed = 0
for _ in range(2):  # a second pass: routes freed by the first
    for net in sorted({t.GetNetCode() for t in b.GetTracks()}):
        for L in layers:
            for c in r.chains(net, L):
                if c["pinned"]:
                    pinned += 1
                    continue
                pts = r.route(net, L, c["width"], c["pts"][0], c["pts"][-1])
                if pts is None:
                    failed += 1
                elif octo.cost(pts) < octo.cost(c["pts"]) - 0.1:
                    r.replace(c, pts)
                    done += 1
                else:
                    kept += 1
squared = 0  # end segments at pads made 45-degree
for net in sorted({t.GetNetCode() for t in b.GetTracks()}):
    for L in layers:
        for c in r.chains(net, L):
            if c["pinned"] or "USB_D" in b.FindNet(net).GetNetname():  # the USB pair keeps its symmetry
                continue
            xs, ys = [p[0] for p in c["pts"]], [p[1] for p in c["pts"]]
            box = r.win(min(xs) - 1.5, min(ys) - 1.5, max(xs) + 1.5, max(ys) + 1.5)
            ok = r.free(net, L, c["width"], box)
            chk = lambda p, q, ok=ok, box=box: r.seg_ok(ok, box, p, q) or math.hypot(q[0] - p[0], q[1] - p[1]) < 0.06
            pts = octo.square_ends(c["pts"], chk)
            if pts != c["pts"]:
                r.replace(c, pts)
                squared += 1
print(f"rerouted {done}, kept {kept}, pinned {pinned}, no route {failed}; {squared} ends squared")
pcbnew.SaveBoard(sys.argv[2], b)
sys.stdout.flush()
subprocess.run([sys.executable, "-c", f"""
import sys
sys.path.insert(0, {sys.path[0]!r})
import pcbnew
b = pcbnew.LoadBoard({sys.argv[2]!r})
pcbnew.ZONE_FILLER(b).Fill(b.Zones())
pcbnew.SaveBoard({sys.argv[2]!r}, b)
"""], check=False)
