"""`just mech::cfd::inlet [out.json]`: the duct's inlet as two curves, the contraction's wall
r_w(x) and the tail cone r_h(x), each a degree-7 Bernstein polynomial in x / L. Ends fixed: wall 29 -> 16, flat at both ends; hub 13.25 flat at the plate -> 0 at its tip angle.
Objective: the mean axial speed Q / A rises as evenly as possible (least squares of dU/dx about its mean), plus
curvature penalties on both curves; the annulus may never widen. Prints the metrics for today's straight cone and four
weightings, and writes their coefficients (Params.contraction and HubCone.profile take the tip-18 pair)."""
import json
import sys
from math import comb
from pathlib import Path

import numpy as np
from scipy.optimize import minimize

RF, RB, R0, L = 29.0, 16.0, 13.25, 27.0
N = 7
x = np.linspace(0, L, 541); t = x / L
B = np.array([comb(N, i) * t**i * (1 - t)**(N - i) for i in range(N + 1)])


def curves(p, tip_deg, lh):
    w = np.r_[RF, RF, p[:N - 3], RB, RB]
    th = np.clip(x / lh, 0, 1)
    Bh = np.array([comb(N, i) * th**i * (1 - th)**(N - i) for i in range(N + 1)])
    h = np.r_[R0, R0, p[N - 3:], np.tan(np.radians(tip_deg)) * lh / N, 0.0]
    return w @ B, np.where(x <= lh, h @ Bh, 0.0)


def metrics(rw, rh):
    A = np.pi * (rw**2 - rh**2)
    U = A[0] / A
    dU = np.gradient(U, x)
    kw = np.gradient(np.gradient(rw, x), x) / (1 + np.gradient(rw, x)**2)**1.5
    kh = np.gradient(np.gradient(rh, x), x) / (1 + np.gradient(rh, x)**2)**1.5
    return A, U, dU, kw, kh


def cost(p, tip, lh, lw, lk):
    rw, rh = curves(p, tip, lh)
    A, U, dU, kw, kh = metrics(rw, rh)
    pen = 1e4 * (np.clip(np.diff(A), 0, None)**2).sum() + 1e4 * (np.clip(np.diff(rw), 0, None)**2).sum() \
        + 1e4 * (np.clip(np.diff(rh), 0, None)**2).sum() + 1e4 * (np.clip(-rh, 0, None)**2).sum()
    return ((dU - dU.mean())**2).mean() * L**2 + lw * (kw**2).mean() * L**2 + lk * (kh**2).mean() * L**2 + pen


def solve(tip, lh, lw, lk):
    p0 = np.r_[np.linspace(RF, RB, N + 1)[2:N - 1], np.linspace(R0, 0, N + 1)[2:N - 1]]
    r = minimize(cost, p0, args=(tip, lh, lw, lk), method="Nelder-Mead", options={"maxiter": 40000, "maxfev": 40000, "xatol": 1e-4, "fatol": 1e-10})
    r = minimize(cost, r.x, args=(tip, lh, lw, lk), method="Powell")
    return r.x


def report(name, rw, rh):
    A, U, dU, kw, kh = metrics(rw, rh)
    q = np.cumsum(np.clip(dU, 0, None)); q /= q[-1]
    xs = [x[np.searchsorted(q, f)] for f in (0.25, 0.5, 0.75)]
    ws = np.degrees(np.arctan(-np.gradient(rw, x))).max()
    hs = np.degrees(np.arctan(-np.gradient(rh, x)))
    print(f"{name:28s} speed-up 25/50/75% at {xs[0]:4.1f}/{xs[1]:4.1f}/{xs[2]:4.1f} mm, peak dU/dx {dU.max():.3f}/mm (even {(U[-1]-1)/L:.3f}),"
          f" wall max {ws:4.1f} deg, min R {1/np.abs(kw).max():5.1f} mm, hub max {hs.max():4.1f} deg, min R {1/np.abs(kh[x < x[np.argmax(rh <= 0.01)] - 1]).max() if (rh <= 0.01).any() else 1/np.abs(kh).max():5.1f} mm, A monotone {bool((np.diff(A) <= 1e-6).all())}")


if __name__ == "__main__":
    rw = RF + (RB - RF) * t; rh = R0 * np.clip(1 - t**2, 0, None)
    report("now (cone, 1-t^2)", rw, rh)
    out = {}
    for tip, lh, lw, lk in [(12, 27, 0.05, 0.05), (12, 27, 0.5, 0.5), (18, 27, 0.2, 0.2), (12, 24, 0.2, 0.2)]:
        p = solve(tip, lh, lw, lk)
        rw, rh = curves(p, tip, lh)
        name = f"tip {tip} L {lh} k {lw}"
        report(name, rw, rh)
        out[name] = {"wall": np.r_[RF, RF, p[:N - 3], RB, RB].tolist(), "hub": np.r_[R0, R0, p[N - 3:], np.tan(np.radians(tip)) * lh / N, 0.0].tolist(), "hub_len": lh,
                     "x": x[::20].tolist(), "rw": rw[::20].tolist(), "rh": rh[::20].tolist()}
    Path(sys.argv[1] if len(sys.argv) > 1 else "build/inlet.json").write_text(json.dumps(out, indent=1))
