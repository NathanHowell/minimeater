"""Fab outputs for JLCPCB into hw/fab/: gerbers+drill (zip), BOM (JLC columns) and component placement (CPL).
Run from the repo root: uv run python hw/tools/fab.py   (needs kicad-cli on PATH)"""
import csv
import subprocess
import zipfile
from pathlib import Path

hw = Path("hw")
out = hw / "fab"
out.mkdir(exist_ok=True)
pcb, sch = hw / "minimeat.kicad_pcb", hw / "minimeat.kicad_sch"
g = out / "gerber"
g.mkdir(exist_ok=True)
layers = "F.Cu,In1.Cu,In2.Cu,B.Cu,F.Paste,F.Silkscreen,B.Silkscreen,F.Mask,B.Mask,Edge.Cuts"
subprocess.run(["kicad-cli", "pcb", "export", "gerbers", "--layers", layers, "--subtract-soldermask", "--no-x2", "-o", str(g), str(pcb)], check=True)
subprocess.run(["kicad-cli", "pcb", "export", "drill", "--format", "excellon", "--drill-origin", "absolute", "--excellon-separate-th", "-o", str(g) + "/", str(pcb)], check=True)
with zipfile.ZipFile(out / "minimeat-gerber.zip", "w", zipfile.ZIP_DEFLATED) as z:
    for f in sorted(g.iterdir()):
        z.write(f, f.name)

raw = out / "bom-raw.csv"
subprocess.run(["kicad-cli", "sch", "export", "bom", "--fields", "Reference,Value,Footprint,LCSC,DNP", "--labels", "Refs,Value,Footprint,LCSC,DNP",
                "--group-by", "Value,Footprint,LCSC", "--exclude-dnp", "--ref-range-delimiter", "", "-o", str(raw), str(sch)], check=True)
rows = list(csv.DictReader(raw.open()))
placed = set()
with (out / "minimeat-bom.csv").open("w", newline="") as f:
    w = csv.writer(f)
    w.writerow(["Comment", "Designator", "Footprint", "LCSC Part #"])
    for r in rows:
        if r["LCSC"].strip().startswith("C"):
            placed.update(r["Refs"].replace('"', "").split(","))
            w.writerow([r["Value"], r["Refs"].replace(",", " ").replace('"', ""), r["Footprint"].split(":")[-1], r["LCSC"]])
        else:
            print("not assembled by JLC:", r["Refs"], r["Value"])
raw.unlink()

pos = out / "pos-raw.csv"
subprocess.run(["kicad-cli", "pcb", "export", "pos", "--format", "csv", "--units", "mm", "--side", "front", "--exclude-dnp", "-o", str(pos), str(pcb)], check=True)
with pos.open() as f, (out / "minimeat-cpl.csv").open("w", newline="") as o:
    w = csv.writer(o)
    w.writerow(["Designator", "Mid X", "Mid Y", "Layer", "Rotation"])
    for r in csv.DictReader(f):
        if r["Ref"] not in placed:
            continue
        w.writerow([r["Ref"], f"{float(r['PosX']):.4f}mm", f"{float(r['PosY']):.4f}mm", "Top", r["Rot"]])
pos.unlink()
print("wrote", out)
