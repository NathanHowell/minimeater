"""The project and its library renamed minimeat -> minimeater: `rename_lib.py <in> <out>` (KiCad's python).

Footprints from the `minimeat` library point at `minimeater`, and every footprint's sheet file is the renamed
schematic (KiCad would otherwise carry `minimeat.kicad_sch` until the next Update PCB from Schematic)."""
import sys

sys.path.insert(0, "/Applications/KiCad/KiCad.app/Contents/Frameworks/Python.framework/Versions/Current/lib/python3.9/site-packages")
import pcbnew

b = pcbnew.LoadBoard(sys.argv[1])
n = m = 0
for f in b.GetFootprints():
    fid = f.GetFPID()
    if fid.GetLibNickname() == "minimeat":
        f.SetFPID(pcbnew.LIB_ID("minimeater", fid.GetLibItemName().wx_str()))
        n += 1
    if f.GetSheetfile() == "minimeat.kicad_sch":
        f.SetSheetfile("minimeater.kicad_sch")
        m += 1
print(f"{n} library links, {m} sheet files")
pcbnew.SaveBoard(sys.argv[2], b)
