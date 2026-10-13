"""Copies the 3D model references from the library footprints onto the placed ones (the board lost them when it was
built). Usage: models.py <in.kicad_pcb> <out.kicad_pcb>   (KiCad's bundled python; run from the repo root)."""
import os
import sys

sys.path.insert(0, '/Applications/KiCad/KiCad.app/Contents/Frameworks/Python.framework/Versions/Current/lib/python3.9/site-packages')
import pcbnew

STOCK = '/Applications/KiCad/KiCad.app/Contents/SharedSupport/footprints'
PROJECT = {'minimeater': os.path.abspath('hw/lib/minimeater.pretty'), 'espressif': os.path.abspath('hw/lib/espressif.pretty')}
b = pcbnew.LoadBoard(sys.argv[1])
missing = []
for f in b.GetFootprints():
    nick, name = str(f.GetFPID().GetLibNickname()), str(f.GetFPID().GetLibItemName())
    lib = PROJECT.get(nick, os.path.join(STOCK, nick + '.pretty'))
    try:
        src = pcbnew.FootprintLoad(lib, name)
    except Exception:
        src = None
    if src is None or not len(list(src.Models())):
        missing.append(f.GetReference())
        continue
    for m in list(f.Models()):
        f.Models().pop_back()
    for m in src.Models():
        f.Add3DModel(m)
# the Espressif module's footprint carries no model line: the vendored STEP, unrotated at the footprint origin
u1 = b.FindFootprintByReference('U1')
if u1 is not None and not len(list(u1.Models())):
    m = pcbnew.FP_3DMODEL()
    m.m_Filename = '${KIPRJMOD}/lib/3d/ESP32-C6-WROOM-1.step'
    # the model's corner is at its origin: x 0..18, y 0..25.5 with the antenna at y = 25.5; the footprint's origin is the
    # pad block's centre (x +-9, antenna end at y = -15.75), and the model's y runs against the footprint's
    m.m_Offset = pcbnew.VECTOR3D(-9.0, -9.75, 0.0)
    u1.Add3DModel(m)
    missing.remove('U1')
print('no model for', missing)
pcbnew.SaveBoard(sys.argv[2], b)
