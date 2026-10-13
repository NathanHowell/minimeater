"""Signal nets from scratch: `routeall.py <in> <out> [--nets a,b] [--passes N] [--usb] [--repair]` (KiCad's python,
from `hw/tools`).

All 0.2 mm signal nets (not GND, not the USB pair unless `--usb`, not the wide power nets, which stay as they are and
are obstacles) are ripped up and routed again with `ar.route_all`. With `--repair` (after a placement move), the
fixed nets' stretched tracks are first routed again where they break clearance (`ar.repair_fixed`). If nets are left
in conflict, the board goes to `<out>.conflict` instead, for a look."""

import argparse
import sys

import ar
import kdrc
import octo
import pcbnew

SKIP = {"GND", "USB_DP", "USB_DN", "/USB_DP_J", "/USB_DN_J"}
ap = argparse.ArgumentParser()
ap.add_argument("inp")
ap.add_argument("out")
ap.add_argument("--nets")
ap.add_argument("--passes", type=int, default=40)
ap.add_argument("--usb", action="store_true")
ap.add_argument("--repair", action="store_true")
a = ap.parse_args()
if a.usb:  # full-speed USB: the pair need not be routed as a pair
    SKIP -= {"USB_DP", "USB_DN", "/USB_DP_J", "/USB_DN_J"}
b = pcbnew.LoadBoard(a.inp)
only = set(a.nets.split(",")) if a.nets else None
r = octo.Router(b)
widths, names = {}, {}
for t in b.GetTracks():
    if t.Type() == pcbnew.PCB_TRACE_T:
        n = t.GetNetCode()
        widths[n] = max(widths.get(n, 0), pcbnew.ToMM(t.GetWidth()))
        names[n] = t.GetNetname()
nets = [n for n, w in widths.items() if w <= 0.25 and names[n] not in SKIP and (not only or names[n] in only)]
print(f"{len(nets)} nets", flush=True)
log = lambda m: print(m, flush=True)
if a.repair:
    for n in nets:
        ar.rip(r, n)
    fixed = [n for n in widths if n not in nets]
    if ar.repair_fixed(r, fixed, kdrc.flagged(b, a.out), log):
        pcbnew.SaveBoard(a.out + ".conflict", b)
        sys.exit("fixed nets left in conflict - written for inspection only, as " + a.out + ".conflict")
left = ar.route_all(r, nets, {n: 0.2 for n in nets}, passes=a.passes, log=log)
if left:
    print("in conflict or unrouted:", ", ".join(names[n] for n in left), "- written for inspection only, as", a.out + ".conflict", flush=True)
    pcbnew.SaveBoard(a.out + ".conflict", b)
    sys.exit(1)
pcbnew.SaveBoard(a.out, b)
print("saved", flush=True)
