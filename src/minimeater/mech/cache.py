"""A disk cache for the slow parts: a built shape is saved as BREP under build/cache, keyed by the parameters (`P`), the
geometry's source and the call's arguments, so a new process (a render, a test run, a CFD export) skips the booleans.

Any change to `params.py` or to a geometry module gives a new key; `MINIMEATER_NO_CACHE=1` turns it off. The cache is
safe to delete at any time.
"""

import functools
import hashlib
import os
from pathlib import Path

from build123d import Compound, Part, Solid, export_brep, import_brep

from .params import P

ROOT = Path(__file__).resolve().parents[3] / "build" / "cache"
# the modules a cached shape can depend on; render, views, printing, analysis and the CFD only read shapes
SOURCES = ("params.py", "geom.py", "parts.py", "mock.py", "shell.py", "assembly.py", "cache.py")
TYPES = {"Part": Part, "Solid": Solid, "Compound": Compound}


@functools.cache
def _code() -> str:
    h = hashlib.sha256()
    for name in SOURCES:
        f = Path(__file__).parent / name
        if f.exists():
            h.update(f.read_bytes())
    return h.hexdigest()


def disk_cache(fn):
    """Cache fn's shape on disk (and in the process). The result comes back as the type fn returned."""

    @functools.cache
    @functools.wraps(fn)
    def wrap(*args, **kw):
        if os.environ.get("MINIMEATER_NO_CACHE"):
            return fn(*args, **kw)
        key = hashlib.sha256(f"{fn.__module__}.{fn.__qualname__}|{args!r}|{sorted(kw.items())!r}|{P!r}|{_code()}".encode()).hexdigest()[:20]
        for kind, cls in TYPES.items():
            f = ROOT / f"{fn.__name__}-{key}.{kind}.brep"
            if f.exists():
                try:
                    return cls(import_brep(str(f)).wrapped)
                except Exception:  # a torn write: rebuild below
                    f.unlink(missing_ok=True)
        out = fn(*args, **kw)
        kind = type(out).__name__ if type(out).__name__ in TYPES else "Compound"
        ROOT.mkdir(parents=True, exist_ok=True)
        tmp = ROOT / f".{fn.__name__}-{key}.{os.getpid()}.tmp.brep"
        export_brep(out, str(tmp))
        tmp.replace(ROOT / f"{fn.__name__}-{key}.{kind}.brep")  # atomic: a parallel process never reads half a file
        return out

    return wrap
