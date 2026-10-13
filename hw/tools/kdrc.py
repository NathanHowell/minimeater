"""KiCad DRC from the scripts (KiCad's python): `violations(board, beside)` saves the in-memory board beside the
project files of the directory holding `beside` (so its rules apply), runs `kicad-cli pcb drc` and returns the JSON
report's violations."""

import json
import os
import shutil
import subprocess
import sys
import tempfile

import pcbnew


def violations(b, beside, severity="error"):
    d = os.path.dirname(os.path.abspath(beside))
    with tempfile.TemporaryDirectory(dir=d) as tmp:
        for f in os.listdir(d):
            if f.endswith((".kicad_pro", ".kicad_dru")):
                shutil.copy(os.path.join(d, f), tmp)
        pro = [f for f in os.listdir(tmp) if f.endswith(".kicad_pro")]
        brd = os.path.join(tmp, (pro[0][:-3] if pro else "x.kicad_") + "pcb")
        pcbnew.SaveBoard(brd, b)
        rep = os.path.join(tmp, "drc.json")
        cli = shutil.which("kicad-cli") or "/Applications/KiCad/KiCad.app/Contents/MacOS/kicad-cli"
        p = subprocess.run([cli, "pcb", "drc", "--format", "json", "--severity-" + severity, "-o", rep, brd], capture_output=True, text=True)
        if not os.path.exists(rep):
            sys.exit("kicad-cli drc failed: " + p.stdout + p.stderr)
        return json.load(open(rep))["violations"]


def flagged(b, beside):
    """uuids of the tracks and vias in DRC errors with other copper (not the pours, whose fill may be stale)."""
    out = set()
    for v in violations(b, beside):
        if any(i["description"].startswith("Zone") for i in v["items"]):
            continue
        out |= {i["uuid"] for i in v["items"] if i["description"].startswith(("Track", "Via"))}
    return out
