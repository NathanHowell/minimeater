"""J6 and J7 become right-angle KK headers: `kk_swap.py <in> <out>` (KiCad's python, from the repo root).

The new footprints (`kk_ra.py`) go on the old ones' pin 1, unrotated, so the pads are where they were (x 4.22, down the
sheet) and nothing is rerouted; the plug enters toward the board's rear edge. Nets, fields, the schematic link and the
reference come across."""
import os
import sys

sys.path.insert(0, "/Applications/KiCad/KiCad.app/Contents/Frameworks/Python.framework/Versions/Current/lib/python3.9/site-packages")
import pcbnew

LIB = os.path.abspath("hw/lib/minimeater.pretty")
NEW = {"J6": ("KK254-RA-04", "22-05-3041"), "J7": ("KK254-RA-03", "22-05-3031")}
b = pcbnew.LoadBoard(sys.argv[1])
for ref, (name, value) in NEW.items():
    old = b.FindFootprintByReference(ref)
    pin1 = old.FindPadByNumber("1").GetPosition()
    new = pcbnew.FootprintLoad(LIB, name)
    new.SetFPID(pcbnew.LIB_ID("minimeater", name))
    new.SetPosition(pin1)
    new.SetOrientationDegrees(0)
    new.SetReference(ref)
    new.SetValue(value)
    new.SetPath(old.GetPath())
    for f in old.GetFields():
        n = f.GetName()
        if n in ("Reference", "Value", "Footprint"):
            continue
        text = f"N/A hand-solder (Mouser/Digi-Key {value})" if n == "LCSC" else f.GetText()
        if n == "Datasheet" or n == "Description":
            new.GetField(n).SetText(text)
            continue
        new.SetField(n, text)
        new.GetField(n).SetVisible(False)
    for p in old.Pads():
        q = new.FindPadByNumber(p.GetNumber())
        q.SetNet(p.GetNet())
    new.Reference().SetLayer(pcbnew.F_Fab)
    new.Reference().SetVisible(False)
    b.Remove(old)
    b.Add(new)
    print(ref, "->", name, [(p.GetNumber(), p.GetNetname()) for p in new.Pads()])
pcbnew.SaveBoard(sys.argv[2], b)
sys.stdout.flush()
import subprocess

subprocess.run([sys.executable, "-c", f"""
import sys
sys.path.insert(0, {sys.path[0]!r})
import pcbnew
b = pcbnew.LoadBoard({sys.argv[2]!r})
pcbnew.ZONE_FILLER(b).Fill(b.Zones())
pcbnew.SaveBoard({sys.argv[2]!r}, b)
"""], check=False)
