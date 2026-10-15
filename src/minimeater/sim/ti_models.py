"""TI's unencrypted PSpice transient model of the TLV62569 (SLVMBW3), made runnable in ngspice.

TI publishes it as plain text (the product page's "PSpice Unencrypted Transient Model"). Its licence ("an aid for customers",
as is) is not read here, so the file is not in the repo: `tlv62569()` downloads the zip once into the cache
(`$MINIMEATER_CACHE` or `~/.cache/minimeater`), converts it and returns the path, or None when it cannot (offline, or the
page moved). `MINIMEATER_TLV62569_LIB` points at a library already on disk instead.

ngspice 44 runs it as it is with two changes, both made by `to_ngspice`: PSpice's `VSWITCH` models become ngspice `SW` (the
switch controls are 0/1 logic levels, so Von 0.8 / Voff 0.2 becomes a threshold at 0.5), and the one `.IC` inside the
subcircuit goes. The circuit must be run with `ngbehavior=psa` (`ngspice.run(..., ps_compat=True)`), for the IF() in the
behavioural sources. What the model is, from running it against the datasheet (`docs/sim/rails.md`): a switching model of the
controller (adaptive off time, peak current, PSM), good for ripple, start-up and load steps; not for losses (97-99 % efficient
where the datasheet's curve says 95 %).
"""

import io
import os
import re
import urllib.request
import zipfile
from pathlib import Path

URL = "https://www.ti.com/lit/zip/slvmbw3"
SUBCKT = "TLV62569_TRANS"  # pins: EN FB SW VIN GND


def cache_dir() -> Path:
    return Path(os.environ.get("MINIMEATER_CACHE") or Path.home() / ".cache" / "minimeater") / "ti"


def to_ngspice(text: str) -> str:
    text = text.replace("\r", "")
    text = re.sub(r"VSWITCH\s+Roff=(\S+)\s+Ron=(\S+)\s+Voff=0\.2\s+Von=0\.8", r"SW vt=0.5 vh=0 roff=\1 ron=\2", text, flags=re.I)
    return re.sub(r"^\.IC\s+V\(PAUSE\s*\)=0.*$", "", text, flags=re.I | re.M)


def tlv62569() -> Path | None:
    """The converted library, fetched on first use; None when it is not available."""
    env = os.environ.get("MINIMEATER_TLV62569_LIB")
    if env:
        p = Path(env)
        return p if p.exists() else None
    lib = cache_dir() / "tlv62569_ng.lib"
    if lib.exists():
        return lib
    try:
        req = urllib.request.Request(URL, headers={"User-Agent": "Mozilla/5.0", "Referer": "https://www.ti.com/product/TLV62569"})
        with urllib.request.urlopen(req, timeout=60) as r:
            z = zipfile.ZipFile(io.BytesIO(r.read()))
        name = next(n for n in z.namelist() if n.lower().endswith("tlv62569_trans.lib"))
        text = to_ngspice(z.read(name).decode("latin-1"))
        if f".subckt {SUBCKT}".lower() not in text.lower():
            return None
        lib.parent.mkdir(parents=True, exist_ok=True)
        lib.write_text(text)
        return lib
    except Exception:  # offline, blocked, or the zip changed: the callers fall back to the behavioural buck
        return None
