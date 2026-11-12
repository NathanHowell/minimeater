"""Check JLCPCB assembly stock for part keywords or C-numbers.  Run: just hw::stock ADS1220IPWR C2089026

`--bom [boards]` checks every assembled line of hw/minimeater.kicad_sch instead (its LCSC field, through kicad-cli's
BOM export) against the quantity for that many boards (default 5): OK at 3x or more in stock, LOW at 1-3x, SHORT
below, MISSING when the C-number is not listed, and flags assembled lines with no MPN. Exits 1 on SHORT or MISSING."""

import csv
import json
import os
import shutil
import subprocess
import sys
from pathlib import Path

URL = "https://jlcpcb.com/api/overseas-pcb-order/v1/shoppingCart/smtGood/selectSmtComponentList"


def search(keyword: str, size: int = 30) -> list[dict]:
    body = json.dumps({"keyword": keyword, "currentPage": 1, "pageSize": size})
    out = subprocess.run(
        ["curl", "-s", "-m", "30", "-X", "POST", URL, "-H", "Content-Type: application/json", "-d", body],
        capture_output=True, text=True, check=True,
    ).stdout
    items = json.loads(out)["data"]["componentPageInfo"]["list"] or []
    items = [i for i in items if i.get("componentBrandEn") != "JLCPCB Assembly"]  # placeholder listings
    return sorted(items, key=lambda i: -(i.get("stockCount") or 0))


def kicad_cli(root: Path, *args: str) -> None:
    """kicad-cli from the PATH, else from the kicad container under podman (MINIMEATER_KICAD_IMAGE); paths are relative
    to the repository root."""
    if shutil.which("kicad-cli"):
        subprocess.run(["kicad-cli", *args], check=True, capture_output=True, cwd=root)
        return
    image = os.environ.get("MINIMEATER_KICAD_IMAGE", "docker.io/kicad/kicad:10.0")
    subprocess.run(["podman", "run", "--rm", "--userns=keep-id", "-v", f"{root}:/work:Z", "-w", "/work", "-e", "HOME=/tmp",
                    image, "kicad-cli", *args], check=True, capture_output=True)


def check_bom(boards: int) -> int:
    root = Path(__file__).resolve().parent.parent
    out = root / "build" / "hw" / "bom.csv"
    out.parent.mkdir(parents=True, exist_ok=True)
    kicad_cli(root, "sch", "export", "bom", "--fields", "Reference,Value,LCSC,MPN", "--labels", "Refs,Value,LCSC,MPN",
              "--group-by", "Value,LCSC", "--exclude-dnp", "--ref-range-delimiter", "", "-o", str(out.relative_to(root)),
              "hw/minimeater.kicad_sch")
    rows = [r for r in csv.DictReader(out.open()) if r["LCSC"].strip().startswith("C")]
    bad = 0
    for r in rows:
        code, need = r["LCSC"].strip(), len(r["Refs"].split(",")) * boards
        hit = [i for i in search(code, 5) if i["componentCode"] == code]
        if not hit:
            print(f"  {code:>10} MISSING {r['Refs']}")
            bad += 1
            continue
        stock = hit[0].get("stockCount") or 0
        flag = "OK" if stock >= 3 * need else "LOW" if stock >= need else "SHORT"
        bad += flag == "SHORT"
        mpn = "" if r["MPN"].strip() else "  (no MPN)"
        print(f"  {code:>10} {flag:5} stock={stock:<8} need={need:<4} {hit[0]['componentLibraryType']:7} "
              f"{hit[0]['componentModelEn'][:26]:26} {r['Refs'][:40]}{mpn}")
    return 1 if bad else 0


def main() -> None:
    if sys.argv[1:2] == ["--bom"]:
        sys.exit(check_bom(int(sys.argv[2]) if len(sys.argv) > 2 else 5))
    for kw in sys.argv[1:]:
        print(f"== {kw}")
        for i in search(kw)[:8]:
            price = (i.get("componentPrices") or [{}])[0].get("productPrice")
            print(
                f"  {i['componentCode']:>10} {i['componentModelEn'][:28]:28} {(i.get('componentBrandEn') or '')[:14]:14} "
                f"{(i.get('componentSpecificationEn') or '')[:14]:14} stock={i.get('stockCount'):<7} "
                f"{i.get('componentLibraryType')} ${price}"
            )


if __name__ == "__main__":
    main()
