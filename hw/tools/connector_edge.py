"""The connectors reach into the enclosure's port wall: `connector_edge.py <in> <out>`.

The jacks' fronts are 0.1 mm inside the board's connector edge (y = 0) and the USB-C's mouth 0.95 mm, with a 2.4 mm wall
to pass, and the strip in front of the pads is empty (nearest copper: the jacks' pads 2.25 mm from the edge, the USB-C's
shield tabs 2.305 mm). So the connector edge is trimmed back to y = 1.75, 0.5 mm (the board's edge-clearance rule) from
the nearest copper (once the jacks' pads are the datasheet's 1.8 x 1.5, not 1.8 x 1.8: 2.4 mm from the edge): the jacks hang 1.65 mm past it and the USB-C 0.8 mm, into the port wall, whose inner face moves in with
the edge (`Pcb.edge_trim` in the mechanical model). The two corners on that edge are square: a rounded one would come
within 0.15 mm of the USB-C's tab or the last jack's pad. The pours refill.
"""

import sys

sys.path.insert(0, "/Applications/KiCad/KiCad.app/Contents/Frameworks/Python.framework/Versions/Current/lib/python3.9/site-packages")
import pcbnew

F = pcbnew.FromMM
EDGE = 1.75
W, H, R = 46.0, 54.0, 2.0  # the board, and the radius of its other two corners
b = pcbnew.LoadBoard(sys.argv[1])

# the PJ-242's pads are 1.8 x 1.5 (SHOU HAN drawing, PCB layout)
for j in ("J2", "J3", "J4", "J5"):
    for p in b.FindFootprintByReference(j).Pads():
        if p.GetAttribute() == pcbnew.PAD_ATTRIB_SMD:
            p.SetSize(pcbnew.VECTOR2I(F(1.8), F(1.5)))

for d in list(b.GetDrawings()):
    if d.GetLayerName() == "Edge.Cuts":
        b.Remove(d)
P = lambda x, y: pcbnew.VECTOR2I(F(x), F(y))


def line(a, c):
    s = pcbnew.PCB_SHAPE(b)
    s.SetShape(pcbnew.SHAPE_T_SEGMENT)
    s.SetLayer(pcbnew.Edge_Cuts)
    s.SetWidth(F(0.05))
    s.SetStart(P(*a))
    s.SetEnd(P(*c))
    b.Add(s)


def arc(a, mid, c):
    s = pcbnew.PCB_SHAPE(b)
    s.SetShape(pcbnew.SHAPE_T_ARC)
    s.SetLayer(pcbnew.Edge_Cuts)
    s.SetWidth(F(0.05))
    s.SetArcGeometry(P(*a), P(*mid), P(*c))
    b.Add(s)


k = R * (1 - 0.7071067811865476)  # the arc midpoint's offset from the corner
line((0, EDGE), (W, EDGE))
line((W, EDGE), (W, H - R))
arc((W, H - R), (W - k, H - k), (W - R, H))
line((W - R, H), (R, H))
arc((R, H), (k, H - k), (0, H - R))
line((0, H - R), (0, EDGE))

pcbnew.ZONE_FILLER(b).Fill(b.Zones())
pcbnew.SaveBoard(sys.argv[2], b)
