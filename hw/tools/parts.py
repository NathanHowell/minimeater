"""Reference -> footprint -> courtyard bbox (mm, footprint frame) from the schematic and the libraries."""
import re, json, glob, os, sys
STOCK = "/Applications/KiCad/KiCad.app/Contents/SharedSupport/footprints"
HW = "/Users/nhowell/source/NathanHowell/minimeat/hw"
LIBS = {"minimeat": f"{HW}/lib/minimeat.pretty", "espressif": f"{HW}/lib/espressif.pretty"}

def fp_path(fid):
    lib, name = fid.split(":")
    d = LIBS.get(lib) or f"{STOCK}/{lib}.pretty"
    return f"{d}/{name}.kicad_mod"

def schematic_parts():
    t = open(f"{HW}/minimeat.kicad_sch").read()
    out = {}
    # instance blocks: each (symbol (lib_id ...) ... ) at depth 1 under the file; find by property pairs
    for m in re.finditer(r'\(property "Reference" "([^"]+)"', t):
        ref = m.group(1)
        if ref.startswith("#"): continue
        seg = t[m.start(): m.start() + 3000]
        f = re.search(r'\(property "Footprint" "([^"]*)"', seg)
        v = re.search(r'\(property "Value" "([^"]*)"', seg)
        if f and f.group(1) and ref not in out:
            out[ref] = (f.group(1), v.group(1) if v else "")
    return out

def courtyard(fid):
    t = open(fp_path(fid)).read()
    xs, ys = [], []
    for m in re.finditer(r'\(fp_(?:line|rect)\s*\(start ([-\d.]+) ([-\d.]+)\)\s*\(end ([-\d.]+) ([-\d.]+)\)(?:(?!\(fp_).)*?\(layer "F\.CrtYd"\)', t, re.S):
        a = list(map(float, m.groups())); xs += [a[0], a[2]]; ys += [a[1], a[3]]
    for m in re.finditer(r'\(fp_poly\s*\(pts(.*?)\)\s*(?:\(stroke|\(width|\(layer)[^\n]*?\n?(?:(?!\(fp_).)*?\(layer "F\.CrtYd"\)', t, re.S):
        for p in re.finditer(r'\(xy ([-\d.]+) ([-\d.]+)\)', m.group(1)):
            xs.append(float(p.group(1))); ys.append(float(p.group(2)))
    if not xs:  # fall back to pads
        for m in re.finditer(r'\(pad "[^"]*" \w+ \w+\s*\(at ([-\d.]+) ([-\d.]+)[^)]*\)\s*\(size ([\d.]+) ([\d.]+)\)', t):
            x, y, w, h = map(float, m.groups()); xs += [x - w/2 - .25, x + w/2 + .25]; ys += [y - h/2 - .25, y + h/2 + .25]
    return (min(xs), min(ys), max(xs), max(ys)) if xs else None

if __name__ == "__main__":
    P = schematic_parts()
    res = {}
    for ref, (fid, val) in sorted(P.items(), key=lambda kv: (re.sub(r'\d+', '', kv[0]), int(re.sub(r'\D', '', kv[0]) or 0))):
        c = courtyard(fid)
        res[ref] = {"fp": fid, "val": val, "crt": c}
    json.dump(res, open(sys.argv[1], "w"), indent=1)
    print(len(res), "parts;", [r for r, v in res.items() if not v["crt"]])
