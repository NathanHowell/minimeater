"""Whole-net reroute: `reroute.py <in> <out> [passes] [nets]` (KiCad's python, from `hw/tools` for `octo.py`).

Each signal net (0.2 mm; not GND, not the USB pairs, not the wide power nets) is ripped up and routed again with
`octo.route_net` over F.Cu, In2 and B.Cu: 45-degree runs, a cost per bend and per via, the back (B.Cu, up in the pan)
dearer than the other two. The new copper stays only if every pad joins and it is cheaper; else the old goes back.
Ugliest first; a later pass gets the room earlier ones freed. Pours refill in a second process."""

import subprocess
import sys
import time

import octo
import pcbnew

SKIP = {"GND", "USB_DP", "USB_DN", "/USB_DP_J", "/USB_DN_J"}
b = pcbnew.LoadBoard(sys.argv[1])
passes = int(sys.argv[3]) if len(sys.argv) > 3 else 2
only = set(sys.argv[4].split(",")) if len(sys.argv) > 4 else None
r = octo.Router(b)
nets = {}
for t in b.GetTracks():
    nets.setdefault(t.GetNetCode(), []).append(t)
cands = []
for net, ts in nets.items():
    name = b.FindNet(net).GetNetname() if hasattr(b, "FindNet") else ""
    name = ts[0].GetNetname()
    if name in SKIP or (only and name not in only):
        continue
    if max(pcbnew.ToMM(t.GetWidth(pcbnew.F_Cu) if t.Type() == pcbnew.PCB_VIA_T else t.GetWidth()) for t in ts if t.Type() != pcbnew.PCB_VIA_T) > 0.25:
        continue
    cands.append((net, name))
total_old = total_new = 0.0
for ps in range(passes):
    order = sorted(cands, key=lambda c: -octo.snap_cost(octo.snapshot(b, c[0])))
    better = 0
    for k, (net, name) in enumerate(order):
        t0 = time.time()
        res = octo.route_net(r, net, 0.2)
        if res and res[1] is not None:
            better += 1
        what = "-" if not res else (f"{res[0]:.1f} -> {res[1]:.1f}" if res[1] is not None else f"{res[0]:.1f} kept")
        print(f"  pass {ps + 1} [{k + 1}/{len(order)}] {name}: {what} ({time.time() - t0:.1f} s)", flush=True)
    print(f"pass {ps + 1}: {better} of {len(order)} nets rerouted", flush=True)
    if not better:
        break
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
