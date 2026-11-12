"""Plots the twin scenario suite's CSVs (`just fw::twin::scenarios`): one PNG per scenario next to its CSV.

Usage: `just fw::twin::plot [DIR]` (default fw/target/tmp/scenarios); `just fw::twin::scenarios` runs it after the suite.
"""

import csv
import sys
from pathlib import Path

import matplotlib

matplotlib.use("Agg")
import matplotlib.pyplot as plt  # noqa: E402


def column(rows, name):
    return [float(r[name]) if r[name] not in ("", "None") else float("nan") for r in rows]


def plot(path: Path) -> Path:
    with path.open() as f:
        rows = list(csv.DictReader(f))
    t = [x / 3600.0 for x in column(rows, "t_s")]
    fig, (top, mid) = plt.subplots(2, 1, figsize=(11, 6), sharex=True, height_ratios=[3, 1])
    top.plot(t, column(rows, "pit_true"), label="pit (true)", lw=1.0)
    top.plot(t, column(rows, "sp"), label="setpoint", lw=1.0, ls="--", color="k")
    core = column(rows, "core_true")
    if any(c == c for c in core):
        top.plot(t, core, label="food core (true)", lw=1.0, color="tab:red")
    top.set_ylabel("F")
    top.grid(alpha=0.3)
    top.legend(loc="lower right")
    top.set_title(path.stem)
    mid.plot(t, column(rows, "flap"), label="flap %", lw=0.8)
    mid.plot(t, column(rows, "fan"), label="fan %", lw=0.8)
    mid.set_ylabel("%")
    mid.set_xlabel("hours")
    mid.grid(alpha=0.3)
    mid.legend(loc="upper right")
    out = path.with_suffix(".png")
    fig.tight_layout()
    fig.savefig(out, dpi=100)
    plt.close(fig)
    return out


def main() -> None:
    root = Path(sys.argv[1]) if len(sys.argv) > 1 else Path("fw/target/tmp/scenarios")
    files = sorted(root.glob("*.csv"))
    if not files:
        sys.exit(f"no CSVs in {root}: run `just fw::twin::scenarios` first")
    for p in files:
        print(plot(p))


if __name__ == "__main__":
    main()
