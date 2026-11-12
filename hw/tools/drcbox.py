"""DRC items inside a box: `just hw::drcbox <drc.json> x0,y0,x1,y1`.

Reads a kicad-cli (or Konnect run_drc) JSON report and prints every violation with an item inside the box, so a
block can be checked while the rest of the board is unrouted. Library-drift and missing-field warnings are noise
here; blockcheck.sh filters them."""

import collections
import json
import sys

d = json.load(open(sys.argv[1]))
x0, y0, x1, y1 = map(float, sys.argv[2].split(","))
inbox = lambda p: x0 <= p.get("x", -1e9) <= x1 and y0 <= p.get("y", -1e9) <= y1
seen = collections.Counter()
for key, vs in d.items():
    if not isinstance(vs, list):
        continue
    for v in vs:
        if not isinstance(v, dict):
            continue
        items = v.get("items", [])
        if any(inbox(i.get("pos", {})) for i in items):
            seen[v.get("description")] += 1
            print("-", v.get("severity", ""), v.get("description"), "|",
                  " ; ".join(f"{i['description']} @({i['pos']['x']:.2f},{i['pos']['y']:.2f})" for i in items))
print(dict(seen))
