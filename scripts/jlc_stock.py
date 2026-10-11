"""Check JLCPCB assembly stock for part keywords or C-numbers.  Run: uv run scripts/jlc_stock.py ADS1220IPWR C2089026"""

import json
import subprocess
import sys

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


def main() -> None:
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
