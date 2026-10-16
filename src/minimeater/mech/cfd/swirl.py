"""The swirl on the planes a RANS case samples at its end (case.swirl_planes): the cutting planes' faces are written raw
(centres and values, no areas), so each plane is resampled by nearest face onto an even polar grid before integrating.

Per plane: the air's mean angle from the axis (flux-weighted |c_theta| / c_x), the swirl's share of the kinetic energy
flux, and the swirl's dynamic head (its kinetic energy flux over the flow, Pa): what the tube dissipates if nothing
takes it out. c_theta is about +x, positive from +y toward +z; the NF-A6x25 leaves it negative."""

import math
from pathlib import Path

import numpy as np
from scipy.spatial import cKDTree

RHO = 1.204


def _read(f: Path) -> np.ndarray:
    return np.loadtxt(f, comments="#")


def plane(f: Path, n_r: int = 60, n_t: int = 180) -> dict:
    a = _read(f)
    y, z, u = a[:, 1], a[:, 2], a[:, 3:6]
    r_max = float(np.hypot(y, z).max())
    rs = (np.arange(n_r) + 0.5) / n_r * r_max
    ts = (np.arange(n_t) + 0.5) / n_t * 2 * math.pi
    rr, tt = np.meshgrid(rs, ts, indexing="ij")
    gy, gz = rr * np.cos(tt), rr * np.sin(tt)
    d, i = cKDTree(np.c_[y, z]).query(np.c_[gy.ravel(), gz.ravel()])
    spacing = r_max / n_r
    ok = d < 3 * spacing  # grid points over the air (not inside the cone or a strut)
    ux, uy, uz = (u[i, k] for k in range(3))
    ut = -uy * np.sin(tt.ravel()) + uz * np.cos(tt.ravel())
    ur = uy * np.cos(tt.ravel()) + uz * np.sin(tt.ravel())
    da = (rr * (r_max / n_r) * (2 * math.pi / n_t)).ravel()
    w = ok * da
    q = np.sum(ux * w)
    ke = 0.5 * (ux**2 + ut**2 + ur**2)
    return {
        "x_mm": float(a[0, 0] * 1000),
        "flow_m3s": float(q),
        "angle_deg": float(math.degrees(math.atan2(np.sum(np.abs(ut) * ux * w), np.sum(ux * ux * w)))),
        "ct_mean_m_s": float(np.sum(ut * ux * w) / q),
        "swirl_ke_share": float(np.sum(0.5 * ut**2 * ux * w) / np.sum(ke * ux * w)),
        "swirl_head_pa": float(RHO * np.sum(0.5 * ut**2 * ux * w) / q),
        "angular_momentum_m2s": float(np.sum(ut * rr.ravel() * ux * w) / q),  # r c_theta, flux mean
    }


def planes(case_dir: Path) -> dict[str, dict]:
    d = next((case_dir / "postProcessing" / "swirl").glob("*"))
    return {f.stem[2:]: plane(f) for f in sorted(d.glob("U_*.raw"), key=lambda f: _read(f)[0, 0])}


if __name__ == "__main__":
    import sys

    for name, v in planes(Path(sys.argv[1])).items():
        print(f"{name:8s} x {v['x_mm']:7.1f}  angle {v['angle_deg']:5.1f} deg  c_theta {v['ct_mean_m_s']:6.2f} m/s  "
              f"r c_theta {v['angular_momentum_m2s'] * 1e4:6.1f} cm2/s  swirl head {v['swirl_head_pa']:5.2f} Pa  "
              f"({v['swirl_ke_share'] * 100:4.1f} % of the KE)")
