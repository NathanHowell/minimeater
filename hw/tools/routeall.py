"""Signal nets from scratch: `routeall.py <in> <out> [nets|-] [passes]` (KiCad's python, from `hw/tools`).

All 0.2 mm signal nets (not GND, not the USB pairs, not the wide power nets, which stay as they are and are
obstacles) are ripped up and routed again with `ar.route_all`. If nets are left in conflict, the board goes to
`<out>.conflict` instead, for a look."""

import subprocess
import sys

import ar
import octo
import pcbnew

SKIP = {"GND", "USB_DP", "USB_DN", "/USB_DP_J", "/USB_DN_J"}
b = pcbnew.LoadBoard(sys.argv[1])
only = set(sys.argv[3].split(",")) if len(sys.argv) > 3 and sys.argv[3] != "-" else None
r = octo.Router(b)
widths, names = {}, {}
for t in b.GetTracks():
    if t.Type() == pcbnew.PCB_TRACE_T:
        n = t.GetNetCode()
        widths[n] = max(widths.get(n, 0), pcbnew.ToMM(t.GetWidth()))
        names[n] = t.GetNetname()
nets = [n for n, w in widths.items() if w <= 0.25 and names[n] not in SKIP and (not only or names[n] in only)]
print(f"{len(nets)} nets", flush=True)
passes = int(sys.argv[4]) if len(sys.argv) > 4 else 40
left = ar.route_all(r, nets, {n: 0.2 for n in nets}, passes=passes, log=lambda m: print(m, flush=True))
if left:
    print("in conflict or unrouted:", ", ".join(names[n] for n in left), "- written for inspection only, as", sys.argv[2] + ".conflict", flush=True)
    pcbnew.SaveBoard(sys.argv[2] + ".conflict", b)
    sys.exit(1)
pcbnew.SaveBoard(sys.argv[2], b)
print("saved", flush=True)
