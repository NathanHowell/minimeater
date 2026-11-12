"""`just mech::cfd::les-post <case dir>...` (`just mech::cfd::les-post`): what a finished (or running) LES case says about the flap.

Reads the function objects' time series (forces, moments, wall and shear-layer probes, the Lamb-vector slabs) from
`postProcessing/`, resamples them to a uniform 20 kHz (the run's time step adjusts to the Courant limit), drops the spin-up
(everything before `t_stats`), and writes next to the case:

  - `les.json`: the mean force and what it implies against the steady CFD, the force's rms and its spectral peaks, the shedding
    peaks of the probes, the run's Courant number and the sampling's frequency resolution;
  - `spectra.npz`: the PSDs (f, S_Fx, S_Fy, S_Fz, S_Mz, the probes' p and U) and the slab integrals, for `.2.19`;
  - `les.png`: the force and probe spectra against the duct's modes.

`--compare A B` checks two resolutions of one case: the shedding peak must not move (5 % or two frequency bins).
Forces are per unit density in the case; here they are in N.
"""

import argparse
import json
import math
import re
import sys
from pathlib import Path

import numpy as np
from scipy import signal

RHO = 1.204
FS = 20000.0  # Hz, the resampled rate
SHED_BAND = (50.0, 5000.0)  # Hz: where a shedding peak is looked for


def _rows(path: Path) -> np.ndarray:
    """The numbers of a function object's .dat file: the parentheses are separators, '#' lines are headers."""
    rows = []
    for line in path.read_text().splitlines():
        if line.startswith("#") or not line.strip():
            continue
        nums = [float(v) for v in re.sub(r"[()]", " ", line).split()]
        rows.append(nums)
    n = min(map(len, rows)) if rows else 0
    return np.array([r[:n] for r in rows])


def _dat(case: Path, fo: str, name: str | None = None) -> np.ndarray:
    d = sorted((case / "postProcessing" / fo).glob("*"))
    if not d:
        raise FileNotFoundError(f"{case}/postProcessing/{fo}")
    parts = [_rows(f) for dd in d for f in ([dd / name] if name else sorted(dd.glob("*.dat")))]
    a = np.concatenate([p for p in parts if p.size])
    _, keep = np.unique(a[:, 0], return_index=True)  # a restart rewrites the overlap
    return a[keep]


def uniform(a: np.ndarray, t0: float, t1: float) -> tuple[np.ndarray, np.ndarray]:
    """The columns of `a` (time first) interpolated onto a uniform grid over [t0, t1]."""
    t = np.arange(t0, t1, 1 / FS)
    return t, np.stack([np.interp(t, a[:, 0], a[:, k]) for k in range(1, a.shape[1])], axis=1)


def psd(x: np.ndarray, nseg: int | None = None) -> tuple[np.ndarray, np.ndarray]:
    """One-sided Welch PSD of each column (unit^2 / Hz): Hann segments, 50 % overlap, `nseg` samples (default a fifth of the record,
    so five independent-ish averages), the mean removed."""
    n = len(x)
    nseg = nseg or max(256, n // 3)
    nseg = min(nseg, n)
    return signal.welch(x - x.mean(axis=0), fs=FS, window="hann", nperseg=nseg, noverlap=nseg // 2, axis=0)


def peaks(f: np.ndarray, s: np.ndarray, n: int = 3) -> list[dict]:
    """The strongest `n` peaks of a PSD inside SHED_BAND: frequency and the share of the band's power."""
    band = (f >= SHED_BAND[0]) & (f <= SHED_BAND[1])
    fb, sb = f[band], s[band]
    if len(sb) < 3:
        return []
    idx, _ = signal.find_peaks(sb, prominence=sb.max() * 0.05)
    idx = idx[np.argsort(sb[idx])[::-1]][:n]
    tot = sb.sum()
    return [{"f_hz": float(fb[i]), "share": float(sb[i - 1 : i + 2].sum() / tot)} for i in idx]


def analyse(case: Path, min_window: float = 0.02) -> dict:
    meta = json.loads((case / "case.json").read_text())
    probes = json.loads((case / "probes.json").read_text())
    t_stats = meta["t_stats"]
    force = _dat(case, "forces", "force.dat")
    moment = _dat(case, "forces", "moment.dat")
    t_end = force[-1, 0]
    if t_end - t_stats < min_window:
        raise SystemExit(f"{case}: only {max(t_end - t_stats, 0):.3f} s past t_stats {t_stats} s: not enough to analyse")
    t0 = max(t_stats, force[0, 0])
    _, F = uniform(force, t0, t_end)  # total x y z, pressure x y z, viscous x y z
    _, M = uniform(moment, t0, t_end)
    F *= RHO
    M *= RHO
    f, S_F = psd(F[:, :3])
    _, S_M = psd(M[:, 2:3])
    out = {"case": meta, "window_s": [float(t0), float(t_end)], "bin_hz": float(f[1] - f[0]), "samples": int(len(F))}
    area = math.pi * 0.016**2  # the bore, m^2
    out["force"] = {
        "mean_n": F[:, :3].mean(axis=0).tolist(),
        "mean_axial_dp_pa": float(F[:, 0].mean() / area),  # the flap's axial force over the bore: about the duct's loss across it
        "rms_n": F[:, :3].std(axis=0).tolist(),
        "mean_torque_nmm": float(M[:, 2].mean() * 1000),
        "rms_torque_nmm": float(M[:, 2].std() * 1000),
        "peaks": {c: peaks(f, S_F[:, i]) for i, c in enumerate(("Fx", "Fy", "Fz"))} | {"Mz": peaks(f, S_M[:, 0])},
    }
    spectra = {"f": f, "S_Fx": S_F[:, 0], "S_Fy": S_F[:, 1], "S_Fz": S_F[:, 2], "S_Mz": S_M[:, 0]}
    pr = {}
    pp, uu = _dat(case, "probes", "p"), _dat(case, "probes", "U")
    _, P = uniform(pp, t0, t_end)
    _, U = uniform(uu, t0, t_end)
    P *= RHO
    U = U.reshape(len(U), -1, 3)
    fp, S_P = psd(P)
    fu, S_U = psd(U[:, :, 1])  # the cross-stream velocity: the shear layers' roll-up
    n_all = P.shape[1]
    # the probes file holds the points that were in the air, in order, as probes.json lists them
    for i, pt in enumerate(probes[:n_all]):
        pr[pt["name"]] = {"p_rms_pa": float(P[:, i].std()), "u_mean_m_s": float(np.linalg.norm(U[:, i].mean(axis=0))),
                          "p_peaks": peaks(fp, S_P[:, i], 2), "uy_peaks": peaks(fu, S_U[:, i], 2)}
        spectra[f"S_p[{pt['name']}]"] = S_P[:, i]
        spectra[f"S_uy[{pt['name']}]"] = S_U[:, i]
    out["probes"] = pr
    wall = [i for i, pt in enumerate(probes[:n_all]) if pt["name"].startswith("wall")]
    if wall:
        out["wall_p_peaks"] = peaks(fp, S_P[:, wall].mean(axis=1))  # all the bore-wall probes' mean PSD
    shed = [i for i, pt in enumerate(probes[:n_all]) if pt["name"].startswith("edge")]
    if shed:
        out["shedding_peaks"] = peaks(fu, S_U[:, shed].mean(axis=1))
    slabs = sorted((case / "postProcessing").glob("slab*"), key=lambda p: int(p.name[4:]))
    if slabs:
        xs, ser = [], []
        for k, d in enumerate(slabs):
            try:
                a = _dat(case, d.name)
            except (FileNotFoundError, ValueError):
                continue
            if a.size and a.shape[1] >= 4:
                _, v = uniform(a, t0, t_end)
                xs.append(k)
                ser.append(v[:, 0] * RHO)  # x component of the Lamb-vector integral (the case holds it per unit density)
        spectra["lamb_x_slab_index"] = np.array(xs)
        spectra["lamb_x"] = np.array(ser)
    run = case / "run.json"
    if run.exists():
        out["run"] = json.loads(run.read_text())
    np.savez(case / "spectra.npz", **spectra)
    (case / "les.json").write_text(json.dumps(out, indent=1))
    plot(case, out, spectra)
    return out


def plot(case: Path, out: dict, sp: dict) -> None:
    import matplotlib

    matplotlib.use("Agg")
    import matplotlib.pyplot as plt

    fig, ax = plt.subplots(1, 2, figsize=(13, 4.8), dpi=120)
    f = sp["f"]
    for k in ("S_Fx", "S_Fy", "S_Fz"):
        ax[0].loglog(f[1:], sp[k][1:], label=k[2:], lw=1)
    ax[0].set(xlabel="Hz", ylabel="force PSD, N$^2$/Hz", title=f"{case.name}: flap force")
    walls = [k for k in sp if k.startswith("S_p[wall")]
    if walls:
        ax[1].loglog(f[1:], np.mean([sp[k] for k in walls], axis=0)[1:], label="bore-wall p (mean of probes)", lw=1)
    sheds = [k for k in sp if k.startswith("S_uy[edge")]
    if sheds:
        ax[1].loglog(f[1:], np.mean([sp[k] for k in sheds], axis=0)[1:], label="shear-layer u$_y$ (m$^2$/s$^2$/Hz)", lw=1)
    ax[1].set(xlabel="Hz", title="wall pressure and the shear layers")
    try:
        from . import acoustics as A

        c = out["case"]
        for m in A.modes(c["flap_deg"], c["cfm"] * A.CFM)[:6]:
            for a in ax:
                a.axvline(m.f, color="grey", ls=":", lw=0.8)
    except Exception:
        pass
    for a in ax:
        a.set_xlim(SHED_BAND[0] / 2, 8000)
        a.legend(fontsize=8)
        a.grid(alpha=0.3, which="both")
    fig.tight_layout()
    fig.savefig(case / "les.png")
    plt.close(fig)


def compare(a: Path, b: Path) -> bool:
    """The main shedding peak of two resolutions of one case: it must not move more than 5 % (or two bins)."""
    ra, rb = (json.loads((p / "les.json").read_text()) for p in (a, b))
    ok = True
    for key, get in (("force Fx", lambda r: r["force"]["peaks"]["Fx"]), ("force Fy", lambda r: r["force"]["peaks"]["Fy"]),
                     ("shear layers", lambda r: r.get("shedding_peaks", [])), ("wall p", lambda r: r.get("wall_p_peaks", []))):
        pa, pb = get(ra), get(rb)
        if not pa or not pb:
            print(f"{key:13s} no peak in one of them")
            continue
        fa, fb = pa[0]["f_hz"], pb[0]["f_hz"]
        tol = max(0.05 * fa, 2 * max(ra["bin_hz"], rb["bin_hz"]))
        good = abs(fa - fb) <= tol
        ok &= good
        print(f"{key:13s} {fa:7.0f} Hz ({ra['case']['res']}) vs {fb:7.0f} Hz ({rb['case']['res']}): {'ok' if good else 'MOVED'} (tol {tol:.0f} Hz)")
    for r in (ra, rb):
        print(f"  {r['case']['res']}: {r.get('run', {}).get('cells')} cells, Co max {r.get('run', {}).get('co_max')}, "
              f"mean Fx {r['force']['mean_n'][0]:.4f} N")
    return ok


def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("cases", nargs="+", type=Path)
    ap.add_argument("--min-window", type=float, default=0.02, help="s past t_stats needed to analyse (smaller only to test)")
    ap.add_argument("--compare", action="store_true", help="two resolutions of one case: the shedding peak must not move")
    a = ap.parse_args()
    for c in a.cases:
        res = analyse(c, a.min_window)
        f = res["force"]
        print(f"{c.name}: window {res['window_s'][0]:.3f}-{res['window_s'][1]:.3f} s, bin {res['bin_hz']:.1f} Hz; mean Fx {f['mean_n'][0]:.4f} N "
              f"({f['mean_axial_dp_pa']:.1f} Pa over the bore), rms {f['rms_n'][0]:.4f} N; Fx peaks "
              f"{', '.join(f'{p['f_hz']:.0f} Hz' for p in f['peaks']['Fx']) or 'none'}")
    if a.compare:
        if len(a.cases) != 2:
            sys.exit("--compare takes two cases")
        sys.exit(0 if compare(*a.cases) else 1)


if __name__ == "__main__":
    main()
