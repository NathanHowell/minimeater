"""Fab outputs for JLCPCB into hw/fab/: gerbers+drill (zip), BOM (JLC columns) and component placement (CPL), for the
single board and for a one-up assembly panel with JLC's tooling holes.
Run from the repo root: uv run python hw/tools/fab.py   (needs kicad-cli on PATH)

Exports come from a copy of the board with its zones refilled (kicad-cli's gerber export uses the fills as last saved).
The panel adds 5 mm rails on the left and right (the connector edge at the top has parts past it), mouse-bite tabs,
three 1.152 mm NPTH tooling holes and three fiducials on the rails (JLC's tooling-hole size), made by KiKit. KiKit runs
on KiCad's own python; install it once into hw/.kikit (gitignored):
  /Applications/KiCad/KiCad.app/Contents/Frameworks/Python.framework/Versions/Current/bin/python3 \\
      -m venv --system-site-packages hw/.kikit && hw/.kikit/bin/pip install kikit
or point KIKIT at a kikit executable. Without it the panel is skipped."""
import csv
import math
import os
import shutil
import subprocess
import zipfile
from pathlib import Path

hw = Path("hw")
out = hw / "fab"
out.mkdir(exist_ok=True)
pcb, sch = hw / "minimeater.kicad_pcb", hw / "minimeater.kicad_sch"
build = out / "build"  # generated boards (refilled copy, panel); gitignored
shutil.rmtree(build, ignore_errors=True)
build.mkdir()
layers = "F.Cu,In1.Cu,In2.Cu,B.Cu,F.Paste,F.Silkscreen,B.Silkscreen,F.Mask,B.Mask,Edge.Cuts"


KICAD_PY = os.environ.get("KICAD_PY") or "/Applications/KiCad/KiCad.app/Contents/Frameworks/Python.framework/Versions/Current/bin/python3"
ORIGIN = """
import sys, pcbnew
b = pcbnew.LoadBoard(sys.argv[1])
r = b.GetBoardEdgesBoundingBox()
b.GetDesignSettings().SetAuxOrigin(pcbnew.VECTOR2I(r.GetX(), r.GetBottom()))
pcbnew.SaveBoard(sys.argv[1], b)
"""


def refilled(src: Path, name: str) -> Path:
    """A copy of `src` (with its project and rules beside it) whose zones kicad-cli has refilled and saved."""
    dst = build / f"{name}.kicad_pcb"
    shutil.copy(src, dst)
    for ext in ("kicad_pro", "kicad_dru"):
        if src.with_suffix(f".{ext}").exists():
            shutil.copy(src.with_suffix(f".{ext}"), dst.with_suffix(f".{ext}"))
    subprocess.run(["kicad-cli", "pcb", "drc", "--refill-zones", "--save-board", "-o", str(build / f"{name}-drc.rpt"), str(dst)],
                   check=True, capture_output=True)
    # every output is measured from the board outline's bottom-left corner (JLC's convention): the gerbers, drill and
    # CPL all positive and in one frame, so JLC's assembly upload has nothing to align
    subprocess.run([KICAD_PY, "-c", ORIGIN, str(dst)], check=True, capture_output=True)
    return dst


def gerbers(board: Path, name: str) -> None:
    g = out / f"{name}-gerber"
    shutil.rmtree(g, ignore_errors=True)  # generated; stale files would ride along in the zip
    g.mkdir()
    subprocess.run(["kicad-cli", "pcb", "export", "gerbers", "--layers", layers, "--subtract-soldermask", "--no-x2", "--use-drill-file-origin", "-o", str(g), str(board)], check=True)
    subprocess.run(["kicad-cli", "pcb", "export", "drill", "--format", "excellon", "--drill-origin", "plot", "--excellon-separate-th", "-o", str(g) + "/", str(board)], check=True)
    with zipfile.ZipFile(out / f"{name}-gerber.zip", "w", zipfile.ZIP_DEFLATED) as z:
        for f in sorted(g.iterdir()):
            z.write(f, f.name)
    shutil.rmtree(g)


# JLC's parts library orients some packages differently from KiCad's footprints, so the CPL rotation needs an offset
# (checked against JLC's assembly preview, 2026-10-14: its pin-1 dots sat 180 deg from ours on the SOT-23-5/6 parts
# and 90 deg on the TSSOP and SOIC, and the SOT-363 (U11, 2026-10-20: JLC drew its pins in rows against our columns, like the SOIC); these are the offsets JLC's community rotation tables list for those packages).
# Matched on the footprint name; first match wins. Re-check every polarised part in the preview after a change.
JLC_ROTATION = [("SOT-363", 270), ("SOT-23", 180), ("TSSOP", 270), ("SOIC", 270)]

# Parts whose JLC footprint differs from the package rule or has its origin elsewhere, by LCSC number: the rotation
# offset, and where JLC's origin sits in KiCad's footprint frame (mm, y down, before rotation). Read from JLC's own
# footprints (EasyEDA) and checked in the preview.
JLC_PART = {
    "C5184243": (0, (0.0, -1.3025)),  # USB4105: JLC's origin is midway between the SMD row and the front shell legs
}


def jlc_rotation(package: str, rot: float, lcsc: str = "") -> float:
    if lcsc in JLC_PART:
        return (rot + JLC_PART[lcsc][0] + 180) % 360 - 180
    for key, offset in JLC_ROTATION:
        if key in package:
            return (rot + offset + 180) % 360 - 180
    return rot


def jlc_origin(lcsc: str, rot: float) -> tuple[float, float]:
    """The shift (CPL x, CPL y; y up) from KiCad's footprint origin to JLC's."""
    dx, dy = JLC_PART.get(lcsc, (0, (0.0, 0.0)))[1]
    t = math.radians(rot)
    bx, by = dx * math.cos(t) + dy * math.sin(t), -dx * math.sin(t) + dy * math.cos(t)  # board frame, y down
    return bx, -by


def cpl(board: Path, name: str, placed: dict[str, str]) -> None:
    pos = build / f"{name}-pos.csv"
    subprocess.run(["kicad-cli", "pcb", "export", "pos", "--format", "csv", "--units", "mm", "--side", "front", "--exclude-dnp", "--use-drill-file-origin", "-o", str(pos), str(board)], check=True)
    with pos.open() as f, (out / f"{name}-cpl.csv").open("w", newline="") as o:
        w = csv.writer(o)
        w.writerow(["Designator", "Mid X", "Mid Y", "Layer", "Rotation"])
        for r in csv.DictReader(f):
            if r["Ref"] not in placed:
                continue
            lcsc, rot = placed[r["Ref"]], float(r["Rot"])
            dx, dy = jlc_origin(lcsc, rot)
            w.writerow([r["Ref"], f"{float(r['PosX']) + dx:.4f}mm", f"{float(r['PosY']) + dy:.4f}mm", "Top",
                        f"{jlc_rotation(r['Package'], rot, lcsc):.6f}"])


raw = build / "bom-raw.csv"
subprocess.run(["kicad-cli", "sch", "export", "bom", "--fields", "Reference,Value,Footprint,LCSC,DNP", "--labels", "Refs,Value,Footprint,LCSC,DNP",
                "--group-by", "Value,Footprint,LCSC", "--exclude-dnp", "--ref-range-delimiter", "", "-o", str(raw), str(sch)], check=True)
rows = list(csv.DictReader(raw.open()))
placed = {}
with (out / "minimeater-bom.csv").open("w", newline="") as f:
    w = csv.writer(f)
    w.writerow(["Comment", "Designator", "Footprint", "LCSC Part #"])
    for r in rows:
        if r["LCSC"].strip().startswith("C"):
            placed.update(dict.fromkeys(r["Refs"].replace('"', "").split(","), r["LCSC"].strip()))
            w.writerow([r["Value"], r["Refs"].replace(",", " ").replace('"', ""), r["Footprint"].split(":")[-1], r["LCSC"]])
        else:
            print("not assembled by JLC:", r["Refs"], r["Value"])

board = refilled(pcb, "minimeater")
gerbers(board, "minimeater")
cpl(board, "minimeater", placed)

kikit = os.environ.get("KIKIT") or str(hw / ".kikit" / "bin" / "kikit")
if Path(kikit).exists() or shutil.which(kikit):
    panel = build / "minimeater-panel-raw.kicad_pcb"
    subprocess.run([kikit, "panelize",
                    "--layout", "grid; rows: 1; cols: 1",
                    "--tabs", "fixed; width: 3mm; vcount: 2; hcount: 0",
                    "--cuts", "mousebites; drill: 0.5mm; spacing: 0.8mm; offset: 0.2mm; prolong: 0.5mm",
                    "--framing", "railslr; width: 5mm; space: 2mm",
                    "--tooling", "3hole; hoffset: 2.5mm; voffset: 2.5mm; size: 1.152mm",
                    "--fiducials", "3fid; hoffset: 2.5mm; voffset: 7mm; coppersize: 1mm; opening: 2mm",
                    "--post", "millradius: 1mm",
                    str(board), str(panel)], check=True)
    panel = refilled(panel, "minimeater-panel")
    gerbers(panel, "minimeater-panel")
    cpl(panel, "minimeater-panel", placed)
else:
    print("KiKit not found: panel skipped (see the docstring to install it)")
print("wrote", out)
