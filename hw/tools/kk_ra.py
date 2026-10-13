"""Right-angle Molex KK 254 footprints for the fan (22-05-3041, 4 pins) and servo (22-05-3031, 3 pins) headers:
`kk_ra.py` (KiCad's python, from the repo root) writes `hw/lib/minimeater.pretty/KK254-RA-04.kicad_mod` and `-03`.

Origin at pin 1, pins down the sheet (-y) at 2.54 mm, the plug entering toward -x (the board's rear edge): the
housing is 6.35 mm tall on F.Cu (the board is inverted in the enclosure: it hangs under the board), the tails
go 2 mm through the board. The STEP models are Molex's, turned into this frame by `kk_ra_step.py`.
"""
import os
import sys

sys.path.insert(0, "/Applications/KiCad/KiCad.app/Contents/Frameworks/Python.framework/Versions/Current/lib/python3.9/site-packages")
import pcbnew

F = pcbnew.FromMM
P = lambda x, y: pcbnew.VECTOR2I(F(x), F(y))
LIB = os.path.abspath("hw/lib/minimeater.pretty")
PITCH = 2.54


def seg(fp, layer, a, b, w):
    s = pcbnew.PCB_SHAPE(fp)
    s.SetShape(pcbnew.SHAPE_T_SEGMENT)
    s.SetLayer(layer)
    s.SetWidth(F(w))
    s.SetStart(P(*a))
    s.SetEnd(P(*b))
    fp.Add(s)


def rect(fp, layer, x0, y0, x1, y1, w):
    for a, b in (((x0, y0), (x1, y0)), ((x1, y0), (x1, y1)), ((x1, y1), (x0, y1)), ((x0, y1), (x0, y0))):
        seg(fp, layer, a, b, w)


def make(n, part):
    fp = pcbnew.FOOTPRINT(None)
    name = f"KK254-RA-{n:02d}"
    fp.SetFPID(pcbnew.LIB_ID("minimeater", name))
    fp.SetReference("REF**")
    fp.SetValue(part)
    fp.SetLibDescription(f"Molex KK 254 {part} right-angle header, {n} circuits, plug toward -x; pin 1 at the origin, pins toward -y")
    fp.Reference().SetLayer(pcbnew.F_Fab)
    fp.Value().SetLayer(pcbnew.F_Fab)
    fp.Reference().SetPosition(P(-4.3, -(n - 1) * PITCH / 2))
    fp.Value().SetPosition(P(-4.3, -(n - 1) * PITCH / 2 + 1.6))
    for i in range(n):
        pad = pcbnew.PAD(fp)
        pad.SetNumber(str(i + 1))
        pad.SetAttribute(pcbnew.PAD_ATTRIB_PTH)
        pad.SetShape(pcbnew.PAD_SHAPE_RECT if i == 0 else pcbnew.PAD_SHAPE_OVAL)
        pad.SetSize(pcbnew.VECTOR2I(F(2.19), F(1.74)))  # across the row x along it, as the vertical footprint's
        pad.SetDrillSize(pcbnew.VECTOR2I(F(1.19), F(1.19)))
        pad.SetLayerSet(pad.PTHMask())
        pad.SetPosition(P(0, -i * PITCH))
        fp.Add(pad)
    top, bot = -(n - 1) * PITCH - 1.27, 1.27  # the row's extent, a half pitch past the end pads
    rect(fp, pcbnew.F_Fab, -5.91, top - 0.0, -2.73, bot, 0.1)  # the housing
    rect(fp, pcbnew.F_CrtYd, -6.16, top - 0.25, 1.0, bot + 0.25, 0.05)
    t = pcbnew.PCB_TEXT(fp)
    t.SetText("1")
    t.SetLayer(pcbnew.F_Fab)
    t.SetTextSize(pcbnew.VECTOR2I(F(1.0), F(1.0)))
    t.SetPosition(P(-4.3, 1.9))
    fp.Add(t)
    m = pcbnew.FP_3DMODEL()
    m.m_Filename = f"${{KIPRJMOD}}/lib/3d/{name}.step"
    fp.Add3DModel(m)
    pcbnew.PCB_IO_KICAD_SEXPR().FootprintSave(LIB, fp)
    print("wrote", name)


make(4, "22-05-3041")
make(3, "22-05-3031")
