import sys
sys.path.insert(0,'/Applications/KiCad/KiCad.app/Contents/Frameworks/Python.framework/Versions/Current/lib/python3.9/site-packages')
import pcbnew
mm=pcbnew.FromMM
b=pcbnew.LoadBoard(sys.argv[1])
z=pcbnew.ZONE(b); z.SetLayer(pcbnew.F_Cu); z.SetNet(b.FindNet('GND'))
z.SetZoneName('GND_F'); z.SetPadConnection(pcbnew.ZONE_CONNECTION_THERMAL)
z.SetMinThickness(mm(0.2)); z.SetLocalClearance(mm(0.2)); z.SetAssignedPriority(0)
z.SetIslandRemovalMode(pcbnew.ISLAND_REMOVAL_MODE_ALWAYS)
ol=z.Outline(); ol.NewOutline()
for x,y in ((0.3,0.3),(45.7,0.3),(45.7,53.7),(0.3,53.7)): ol.Append(mm(x),mm(y))
b.Add(z)
for zz in b.Zones():
    if zz.GetNetname()=='GND': zz.SetIslandRemovalMode(pcbnew.ISLAND_REMOVAL_MODE_ALWAYS)
# small GND SMD pads: full connection (thermal spokes starve at this density)
for fp in b.GetFootprints():
    for p in fp.Pads():
        if p.GetNetname()=='GND' and p.GetAttribute()==pcbnew.PAD_ATTRIB_SMD and fp.GetReference() not in ('U1','J1'):
            p.SetLocalZoneConnection(pcbnew.ZONE_CONNECTION_FULL)
for fp in b.GetFootprints():
    r=fp.Reference(); ref=fp.GetReference()
    if ref[0] in 'CRF' and not ref.startswith('FID') or ref.startswith('TP') or ref.startswith('H'):
        r.SetLayer(pcbnew.F_Fab); r.SetTextSize(pcbnew.VECTOR2I(mm(0.6),mm(0.6)))
    else:
        r.SetTextSize(pcbnew.VECTOR2I(mm(0.8),mm(0.8))); r.SetTextThickness(mm(0.12))
    fp.Value().SetVisible(False)
    for p in fp.Pads():
        if p.GetNetname()=='GND' and p.GetAttribute()==pcbnew.PAD_ATTRIB_PTH and fp.GetReference() in ('J7','J6'):
            p.SetLocalZoneConnection(pcbnew.ZONE_CONNECTION_FULL)
        if fp.GetReference()=='J1' and p.GetNetname()=='GND':
            p.SetLocalZoneConnection(pcbnew.ZONE_CONNECTION_FULL)
f=pcbnew.ZONE_FILLER(b); f.Fill(b.Zones())
pcbnew.SaveBoard(sys.argv[2],b)
