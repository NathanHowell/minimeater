"""After "Update PCB from Schematic" brought U1's antenna keepout onto the board: `antenna_keepout.py <in> <out>`.

1. The I2C_SDA via at (14.3953, 50.5021) reached 0.1 mm into the keepout (a rule area over the antenna end of the
   module: no tracks, vias, pads, pours or footprints). It moves 0.6 mm toward the board centre, y 50.2, between the pad row and the keepout; the In2 run and
   the B.Cu riser follow.
2. U1 gets the module's 3D model (the library footprint had none; it is added there too).
3. The four PJ-242 jacks (J2-J5) get the library footprint's model: its origin is the bore axis, 1.6 mm above the
   housing's underside, so the model sits 1.6 mm up (the library footprint has that offset now); without it the housing
   was half inside the board and the pegs stood out of the back.
4. "Update PCB from Schematic" resets the pads' local zone connections: the full connection for the small GND SMD pads
   (not U1, J1), J1's GND pads and the PTH ground pads of J6 and J7 goes back (as `finish.py` set it), or the thermal
   spokes starve.
5. The pours refill.
"""

import os
import sys

sys.path.insert(0, "/Applications/KiCad/KiCad.app/Contents/Frameworks/Python.framework/Versions/Current/lib/python3.9/site-packages")
import pcbnew

mm, F = pcbnew.ToMM, pcbnew.FromMM
LIB = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "lib", "minimeat.pretty")
X, Y_OLD, Y_NEW = 14.3953, 50.5021, 50.2
b = pcbnew.LoadBoard(sys.argv[1])


def at(p, x, y):
    return abs(mm(p.x) - x) < 0.001 and abs(mm(p.y) - y) < 0.001


via = next((t for t in b.GetTracks() if t.GetClass() == "PCB_VIA" and at(t.GetPosition(), X, Y_OLD)), None)
if via is None:
    print("via already moved or absent")
else:
    assert via.GetNetname() == "I2C_SDA"
    for t in list(b.GetTracks()):
        if t.GetClass() == "PCB_VIA" or t.GetNetname() != "I2C_SDA" or t.GetLayerName() != "B.Cu":
            continue
        if at(t.GetStart(), X, Y_OLD):
            t.SetStart(pcbnew.VECTOR2I(F(X), F(Y_NEW)))
        elif at(t.GetEnd(), X, Y_OLD):
            t.SetEnd(pcbnew.VECTOR2I(F(X), F(Y_NEW)))
    stub = pcbnew.PCB_TRACK(b)
    stub.SetStart(pcbnew.VECTOR2I(F(X), F(Y_OLD)))
    stub.SetEnd(pcbnew.VECTOR2I(F(X), F(Y_NEW)))
    stub.SetWidth(F(0.2))
    stub.SetLayer(b.GetLayerID("In2.Cu"))
    stub.SetNet(via.GetNet())
    b.Add(stub)
    via.SetPosition(pcbnew.VECTOR2I(F(X), F(Y_NEW)))
    print("via moved to", X, Y_NEW)

u1 = b.FindFootprintByReference("U1")
if not len(list(u1.Models())):
    m = pcbnew.FP_3DMODEL()
    m.m_Filename = "${KIPRJMOD}/lib/3d/ESP32-C6-WROOM-1.step"
    m.m_Offset = pcbnew.VECTOR3D(-9, -9.75, 0)
    u1.Models().push_back(m)
    print("U1 model added")
for j in ("J2", "J3", "J4", "J5"):
    f = b.FindFootprintByReference(j)
    src = pcbnew.FootprintLoad(LIB, "PJ-242")
    for _ in list(f.Models()):
        f.Models().pop_back()
    for m in src.Models():
        f.Add3DModel(m)
for fp in b.GetFootprints():
    ref = fp.GetReference()
    for p in fp.Pads():
        if p.GetNetname() != "GND":
            continue
        smd = p.GetAttribute() == pcbnew.PAD_ATTRIB_SMD
        if (smd and ref not in ("U1", "J1")) or ref == "J1" or (ref in ("J6", "J7") and p.GetAttribute() == pcbnew.PAD_ATTRIB_PTH):
            p.SetLocalZoneConnection(pcbnew.ZONE_CONNECTION_FULL)
pcbnew.ZONE_FILLER(b).Fill(b.Zones())
pcbnew.SaveBoard(sys.argv[2], b)
