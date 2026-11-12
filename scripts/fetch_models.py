"""Fetch the vendor 3D models the repository does not redistribute (`just vendor::fetch`).

The board's EasyEDA models come from LCSC by part number through `easyeda2kicad`; Noctua's fan and Hitec's servo
have no stable download link and are fetched by hand (the script says where), then checked and put in place with
`--from FILE` (a STEP file, or the vendor's zip). Every file is checked against the SHA-256 the design was built
with: a model that has changed upstream is not installed (its footprint offsets were set against the old one).

Without the models everything still builds and tests: KiCad shows those parts without bodies, `mech` uses its box
stand-ins (`mock.servo_box`, `mock.fan`) and the checks that need the real geometry are skipped.
"""

import argparse
import hashlib
import shutil
import subprocess
import sys
import tempfile
import zipfile
from dataclasses import dataclass
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent


@dataclass(frozen=True)
class Model:
    path: str  # where the design expects it, from the repository root
    sha256: str
    lcsc: str | None = None  # fetched with easyeda2kicad when set
    manual: str | None = None  # otherwise: where to download it by hand


MODELS = (
    Model("hw/lib/3d/PJ-242.step", "3e93b09a146858a1fc20362e125174cb27b8f353020feab6531b437817c83520", lcsc="C431536"),
    Model("hw/lib/3d/LQH3NPN2R2MMEL.step", "321b30d08aee81de09b6e7859f45bcad72d354eba50bd245288558ac5bd0d426", lcsc="C668451"),
    Model("hw/lib/3d/SKRPACE010.step", "fa106d37b6433d3a87facf796fa6a530d08df4a52a529cb1c7a294793d7d04d5", lcsc="C139797"),
    Model("hw/lib/3d/TPD4E05U06DQA.step", "99ac7c9288b630b945a8a1bd96e999bb848feb97bb49138f0359a1fcd93c92e2", lcsc="C138714"),
    Model("hw/lib/3d/TMP117AIDRV_WSON-6_noEP.step", "6ecb7c0d9e2bf26099d55ccee55650efa5907cb2b0ad51fc1b7b2ff8c71d0dd0", lcsc="C699536"),
    Model(
        "src/minimeater/mech/data/NF-A6x25.stp",
        "b9870504582b03fc890b45a137fff0725623f733e21457b66372b12169deed65",
        manual="Noctua's download section (https://www.noctua.at/en/support/downloads), NF-A6x25: NF-A6x25_Public-CAD.zip",
    ),
    Model(
        "src/minimeater/mech/data/HS-5065MG.stp",
        "710f551314eb2e1c9ed898c8f880de53bf8ec5ae1483f42d71edb876d4f95e7a",
        manual="Hitec's CAD download for the HS-5065MG (www.hiteccs.com): HS-5065MG-1642816206.stp",
    ),
)


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def install(m: Model, data: bytes) -> None:
    dest = ROOT / m.path
    dest.parent.mkdir(parents=True, exist_ok=True)
    tmp = dest.with_name(dest.name + ".part")
    tmp.write_bytes(data)
    tmp.replace(dest)  # atomic: never a half-written model
    print(f"  installed {m.path}")


def fetch_lcsc(m: Model) -> bytes | None:
    """The STEP easyeda2kicad writes for `m.lcsc`, fetched into a fresh temporary directory."""
    with tempfile.TemporaryDirectory(prefix=f"easyeda-{m.lcsc}-") as d:
        r = subprocess.run(
            [sys.executable, "-m", "easyeda2kicad", f"--lcsc_id={m.lcsc}", "--3d", "--output", str(Path(d) / "lib")],
            capture_output=True, text=True,
        )
        steps = list(Path(d).rglob("*.step"))
        if r.returncode != 0 or len(steps) != 1:
            print(f"  easyeda2kicad failed for {m.lcsc}:\n{r.stdout}{r.stderr}", file=sys.stderr)
            return None
        return steps[0].read_bytes()


def from_file(m: Model, src: Path) -> bytes | None:
    """`src` itself, or the member of a vendor zip whose hash is the one expected."""
    if zipfile.is_zipfile(src):
        with zipfile.ZipFile(src) as z:
            for info in z.infolist():
                if not info.is_dir() and info.file_size < 64 << 20:
                    data = z.read(info)
                    if sha256(data) == m.sha256:
                        return data
        return None
    return src.read_bytes()


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--from", dest="src", type=Path, action="append", default=[],
                    help="a downloaded STEP or zip for a hand-fetched model (repeatable)")
    ap.add_argument("--check", action="store_true", help="only report which models are present and correct")
    args = ap.parse_args()

    missing = 0
    for m in MODELS:
        dest = ROOT / m.path
        if dest.exists() and sha256(dest.read_bytes()) == m.sha256:
            print(f"ok      {m.path}")
            continue
        state = "changed" if dest.exists() else "missing"
        print(f"{state} {m.path}")
        if args.check:
            missing += 1
            continue
        data = None
        if m.lcsc:
            data = fetch_lcsc(m)
        else:
            for src in args.src:
                cand = from_file(m, src)
                if cand is not None and sha256(cand) == m.sha256:
                    data = cand
                    break
            if data is None:
                print(f"  download by hand from {m.manual}\n  then: just vendor::fetch --from <file>")
                missing += 1
                continue
        if data is None:
            missing += 1
        elif sha256(data) != m.sha256:
            print(f"  not installed: the upstream file has changed (sha256 {sha256(data)}); check it against the footprint and"
                  " update MODELS if it is right", file=sys.stderr)
            missing += 1
        else:
            install(m, data)
    return 1 if missing else 0


if __name__ == "__main__":
    sys.exit(main())
