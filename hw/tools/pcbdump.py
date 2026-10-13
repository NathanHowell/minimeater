"""Board geometry to JSON for render.py: `pcbdump.py <board> <out.json>` (KiCad's python).

Pads (their copper polygon per layer), tracks, arcs, vias, filled zones and zone outlines, footprint courtyards,
references and values, the board outline. Coordinates are mm, KiCad's axes (y down)."""

import json
import sys

import pcbnew

MM = 1e-6
LAYERS = {pcbnew.F_Cu: "F.Cu", pcbnew.In1_Cu: "In1.Cu", pcbnew.In2_Cu: "In2.Cu", pcbnew.B_Cu: "B.Cu"}


def pt(v):
    return [round(v.x * MM, 4), round(v.y * MM, 4)]


def polys(sps):
    out = []
    for i in range(sps.OutlineCount()):
        o = sps.Outline(i)
        holes = [[pt(sps.Hole(i, h).CPoint(k)) for k in range(sps.Hole(i, h).PointCount())] for h in range(sps.HoleCount(i))]
        out.append({"outline": [pt(o.CPoint(k)) for k in range(o.PointCount())], "holes": holes})
    return out


def main(src, dst):
    b = pcbnew.LoadBoard(src)
    d = {"pads": [], "tracks": [], "vias": [], "zones": [], "footprints": [], "edges": []}
    for fp in b.GetFootprints():
        cy = fp.GetCourtyard(pcbnew.F_CrtYd)
        d["footprints"].append({
            "ref": fp.GetReference(), "value": fp.GetValue(), "pos": pt(fp.GetPosition()),
            "rot": fp.GetOrientationDegrees(), "locked": fp.IsLocked(),
            "layer": "B.Cu" if fp.GetLayer() == pcbnew.B_Cu else "F.Cu",
            "courtyard": polys(cy) if cy.OutlineCount() else [],
        })
        for p in fp.Pads():
            for lid, lname in LAYERS.items():
                if not p.IsOnLayer(lid) or not p.FlashLayer(lid):
                    continue
                d["pads"].append({
                    "ref": fp.GetReference(), "num": p.GetNumber(), "net": p.GetNetname(), "layer": lname,
                    "pos": pt(p.GetPosition()), "poly": polys(p.GetEffectivePolygon(lid, pcbnew.ERROR_INSIDE)),
                    "drill": round(p.GetDrillSize().x * MM, 4) if p.GetDrillSize().x else 0,
                })
    for t in b.GetTracks():
        if t.GetClass() == "PCB_VIA":
            d["vias"].append({"pos": pt(t.GetPosition()), "d": t.GetWidth(pcbnew.F_Cu) * MM,
                              "drill": t.GetDrillValue() * MM, "net": t.GetNetname()})
        elif t.GetClass() == "PCB_ARC":
            d["tracks"].append({"arc": [pt(t.GetStart()), pt(t.GetMid()), pt(t.GetEnd())], "w": t.GetWidth() * MM,
                                "layer": t.GetLayerName(), "net": t.GetNetname()})
        else:
            d["tracks"].append({"a": pt(t.GetStart()), "b": pt(t.GetEnd()), "w": t.GetWidth() * MM,
                                "layer": t.GetLayerName(), "net": t.GetNetname()})
    for z in b.Zones():
        for lid, lname in LAYERS.items():
            if not z.IsOnLayer(lid):
                continue
            fill = z.GetFilledPolysList(lid) if z.HasFilledPolysForLayer(lid) else None
            d["zones"].append({"net": z.GetNetname(), "layer": lname, "keepout": z.GetIsRuleArea(),
                               "name": z.GetZoneName(), "outline": polys(z.Outline()),
                               "fill": polys(fill) if fill else []})
    for g in b.GetDrawings():
        if g.GetLayer() == pcbnew.Edge_Cuts:
            d["edges"].append([pt(p) for p in g.GetCorners()] if g.GetShape() == pcbnew.SHAPE_T_RECT
                              else [pt(g.GetStart()), pt(g.GetEnd())])
    json.dump(d, open(dst, "w"))


if __name__ == "__main__":
    main(*sys.argv[1:3])
