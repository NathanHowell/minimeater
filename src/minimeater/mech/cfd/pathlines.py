"""`just mech::cfd::pathlines`: the air's paths through the whole airway, from the RANS fields, for the
raytraced animation (docs/cfd.md, `minimeat-cp2.2`).

Two steady cases carry the air: the grille case (grille.py: the room, the rear wall's gills, the plenum, the fan with its rotor
in a rotating frame, its first 60 mm of pipe) and the duct case (run.py: from 5 mm inside the fan's outlet face, through the
cone, the vanes, the flap, to the intake tube), whose inlet is the grille case's own exit profile. Lines are traced through the
first and handed to the second at the plane the duct case starts on.

A fluid particle's path through the rotor is not a streamline of the absolute velocity (that runs into the blades, which are
moving). In the rotor's cell zone the line is a streamline of the relative velocity U - Omega x r, drawn in the frame of the mesh
where the blades stand still, and turned back to the room by the rotor's angle at that time. So `t` is the particle's own
clock from its seed, and the rotor stands at the meshed pose at t = 0 and turns at `omega` (rad/s about +x) from there.

What comes out (`write`): one polyline per particle in the mech frame, mm, with the time, the absolute speed and a stage
per vertex, as an npz (and a vtp to look at in ParaView).
"""

import argparse
import json
import math
from dataclasses import dataclass
from pathlib import Path

import numpy as np
import pyvista as pv
from scipy.spatial import cKDTree

from .. import geom as G

ZONE = (-121.0, -103.5, 28.2)  # the rotor's MRF cells (fan.py's topoSet): x0, x1 (mm) and radius, about the x axis
DEEPER = 4  # steps of 0.05 mm into the other part to seed
HANDOVER = 1.5e-3  # m: how far ahead of a line's end the other part's cell may be (a tracer stops short of the edge, and the two meshes' cells differ in size)
STAGES = ("room", "rotor", "duct")  # the `stage` of a vertex: outside the rotor zone in the grille case, in it, the duct case


def turn(p: np.ndarray, a: np.ndarray) -> np.ndarray:
    """Points p (n, 3) turned about +x by the angles a (n,)."""
    c, s = np.cos(a), np.sin(a)
    return np.c_[p[:, 0], c * p[:, 1] - s * p[:, 2], s * p[:, 1] + c * p[:, 2]]


@dataclass
class Segment:
    """One piece of one particle's path: points (m, mech frame, so already turned), time (s), absolute speed (m/s), stage."""
    line: int
    p: np.ndarray
    t: np.ndarray
    speed: np.ndarray
    stage: int


class Field:
    """One case's fluid mesh and its velocity (m, m/s). With `omega` it has a rotor zone: the cells inside it hold the absolute
    velocity of a frozen rotor, and are traced apart from the rest."""

    def __init__(self, vtu: Path, omega: float = 0.0, x_stop: float | None = None):
        m = pv.read(vtu)
        m.set_active_vectors("U", preference="point")
        self.omega = omega
        self.mesh = m
        c = m.cell_centers().points
        self.zone = np.zeros(m.n_cells, bool)
        if omega:
            x0, x1, r = (v / 1000 for v in ZONE)
            self.zone = (c[:, 0] > x0) & (c[:, 0] < x1) & (np.hypot(c[:, 1], c[:, 2]) < r)
        keep = np.ones(m.n_cells, bool) if x_stop is None else c[:, 0] < x_stop + 0.002
        self.outside = self._part(~self.zone & keep, relative=False)
        self.inside = self._part(self.zone, relative=True) if self.zone.any() else None
        self.centres = {False: cKDTree(c[~self.zone & keep]), True: cKDTree(c[self.zone]) if self.zone.any() else None}

    def _part(self, mask, relative):
        part = self.mesh.extract_cells(np.flatnonzero(mask))
        u = np.asarray(part.point_data["U"], float)
        if relative:  # U - Omega x r, Omega = (omega, 0, 0): (0, -omega z, omega y) is its turning velocity
            p = np.asarray(part.points)
            u = u - np.c_[np.zeros(len(p)), -self.omega * p[:, 2], self.omega * p[:, 1]]
        part.point_data["V"] = u
        part.set_active_vectors("V", preference="point")
        return part

    def hand_over(self, end: np.ndarray, d: np.ndarray, zone: bool):
        """Where a line that ended at `end` heading along `d` goes on in the zone's cells (zone True) or the others': the first point
        along `d` inside that part within HANDOVER (a tracer stops a step short of the edge); failing that the centre of its nearest
        cell ahead of the end, within the same distance. None if there is neither."""
        ahead = end + np.arange(0.05e-3, HANDOVER, 0.05e-3)[:, None] * d
        inside = np.flatnonzero(self.contains(ahead, zone))
        if len(inside):  # a little way in: the tracer will not take a seed on a cell's edge
            return ahead[inside[min(DEEPER, len(inside) - 1)]]
        tree = self.centres[zone]
        if tree is None:
            return None
        dist, i = tree.query(end, k=6, distance_upper_bound=HANDOVER)
        c = tree.data[i[np.isfinite(dist)]]
        c = c[(c - end) @ d >= 0] if len(c) else c
        return c[np.argmin(np.linalg.norm(c - end, axis=1))] if len(c) else None

    def contains(self, p: np.ndarray, zone: bool) -> np.ndarray:
        part = self.inside if zone else self.outside
        if part is None or not len(p):
            return np.zeros(len(p), bool)
        return part.find_containing_cell(p) >= 0

    def trace(self, p, zone, max_length, step=0.2):
        """Streamlines of V from points p (n, 3, m) through the zone's cells (zone True) or the others'. Returns one
        (seed index, points, time, |U|) per seed that gave a line."""
        part = self.inside if zone else self.outside
        seeds = pv.PolyData(np.asarray(p, float))
        seeds.point_data["sid"] = np.arange(len(p))
        sl = part.streamlines_from_source(seeds, vectors="V", integration_direction="forward", max_length=max_length,
                                          initial_step_length=step, min_step_length=0.001, max_step_length=0.25, step_unit="cl",
                                          max_steps=200000, terminal_speed=1e-3, max_error=1e-5, interpolator_type="cell")
        out = []
        lines, i = sl.lines, 0
        pts = np.asarray(sl.points)
        tm = np.asarray(sl.point_data["IntegrationTime"])
        speed = np.linalg.norm(np.asarray(sl.point_data["U"]), axis=1)
        ids = np.asarray(sl.cell_data["SeedIds"]) if "SeedIds" in sl.cell_data else None
        for k in range(sl.n_cells):
            n = lines[i]
            sel = lines[i + 1 : i + 1 + n]
            i += n + 1
            out.append((int(ids[k]), pts[sel], tm[sel], speed[sel]))
        return out


def follow(field: Field, p0: np.ndarray, max_length: float = 1.0, rounds: int = 12) -> list[list[Segment]]:
    """The particles from `p0` (n, 3, m) at t = 0 through `field`: each as a list of segments, a new one each time it crosses the
    rotor zone's edge. A particle whose seed is in no cell gives an empty list; one goes on only if its line ends where the other
    part of the mesh begins (a wall, the outlet or the end of the mesh stops it)."""
    n = len(p0)
    paths: list[list[Segment]] = [[] for _ in range(n)]
    pos, t = np.asarray(p0, float).copy(), np.zeros(n)
    alive = np.ones(n, bool)
    for _ in range(rounds):
        inside = field.contains(pos, True)
        alive &= inside | field.contains(pos, False)
        moved = np.zeros(n, bool)
        for zone in (False, True) if field.inside is not None else (False,):
            idx = np.flatnonzero(alive & (inside == zone))
            if not len(idx):
                continue
            phi = field.omega * t[idx]
            q = turn(pos[idx], -phi) if zone else pos[idx]  # the zone's mesh is in the rotor's frame
            for sid, pts, tm, sp in field.trace(q, zone, max_length):
                i = idx[sid]
                tt = t[i] + tm
                p = turn(pts, field.omega * tt) if zone else pts
                paths[i].append(Segment(i, p, tt, sp, 1 if zone else 0))
                pos[i] = p[-1]
                t[i] = tt[-1]
                moved[i] = True
        alive = np.zeros(n, bool)
        for i in np.flatnonzero(moved):  # on, if the other part of the mesh begins just ahead
            s = paths[i][-1]
            d = s.p[-1] - s.p[-2] if len(s.p) > 1 else np.zeros(3)
            nxt = field.hand_over(s.p[-1], d / (np.linalg.norm(d) + 1e-30), s.stage == 0)
            if nxt is not None:
                pos[i], alive[i] = nxt, True
        if not alive.any():
            break
    return paths


def cut(path: list[Segment], x_mm: float):
    """The path up to where it reaches x = x_mm (mm, interpolated), as (points m, time, speed, stage) arrays; None if it never
    does. The stage is the segment's: 0 room, 1 rotor."""
    if not path:
        return None
    p = np.vstack([s.p for s in path])
    t = np.concatenate([s.t for s in path])
    sp = np.concatenate([s.speed for s in path])
    st = np.concatenate([np.full(len(s.p), s.stage) for s in path])
    over = np.flatnonzero(p[:, 0] * 1000 >= x_mm)
    if not len(over) or over[0] == 0:
        return None
    k = over[0]
    w = (x_mm / 1000 - p[k - 1, 0]) / (p[k, 0] - p[k - 1, 0])
    cross = lambda a: a[k - 1] + w * (a[k] - a[k - 1])  # noqa: E731
    return (np.vstack([p[:k], cross(p)]), np.append(t[:k], cross(t)), np.append(sp[:k], cross(sp)), np.append(st[:k], st[k - 1]))


X_CUT = G.g.fan_face_x - 5.0 + 0.05  # mm: where the duct case's mesh begins (geometry.FAN_STUB ahead of the fan's outlet face), a hair inside
KEEP_EVERY = 0.35  # mm of path between the vertices that are kept


def ensure_vtu(case_dir: Path) -> Path:
    """The case's last time as VTK (U and p on the fluid mesh, metres), written in the container if it is not there yet."""
    found = sorted((case_dir / "VTK").glob("case_*/internal.vtu"), key=lambda f: int(f.parent.name.split("_")[1]))
    if found:
        return found[-1]
    from . import runtime

    script = "cd /case && reconstructPar -latestTime > log.reconstructPar 2>&1 && foamToVTK -latestTime -fields '(U p)' > log.foamToVTK 2>&1"
    if runtime.run(case_dir.resolve(), script, 2, "8G"):
        raise SystemExit(f"{case_dir}: reconstructPar/foamToVTK failed (log.reconstructPar, log.foamToVTK)")
    return ensure_vtu(case_dir)


def lattice(x_mm: float, half_mm: float, spacing_mm: float) -> np.ndarray:
    """A square lattice of seeds on the plane x = x_mm (m): one per `spacing` square, so each carries the same flux on a uniform inflow."""
    c = (np.arange(-half_mm, half_mm, spacing_mm) + spacing_mm / 2)
    y, z = np.meshgrid(c, c)
    return np.c_[np.full(y.size, x_mm), y.ravel(), z.ravel()] / 1000


def stitch(room: Field, duct: Field, seeds: np.ndarray, omega: float) -> list[dict]:
    """The particles from `seeds` through the grille case to the plane the duct case begins on, and on through the duct case to its
    outlet. Each is a dict of p (mm, mech frame), t (s), speed (m/s) and stage (0 room, 1 rotor, 2 duct) per vertex; particles that
    never reach the duct case's plane are left out."""
    a = follow(room, seeds)
    cuts = [(i, cut(path, X_CUT)) for i, path in enumerate(a)]
    cuts = [(i, c) for i, c in cuts if c is not None]
    start = np.array([c[0][-1] for _, c in cuts])
    ok = duct.contains(start, False)
    cuts = [x for x, o in zip(cuts, ok) if o]
    b = follow(duct, np.array([c[0][-1] for _, c in cuts]))
    out = []
    for (i, (p, t, sp, st)), path in zip(cuts, b):
        if not path:
            continue
        pb = np.vstack([s.p for s in path])
        tb = np.concatenate([s.t for s in path]) + t[-1]
        sb = np.concatenate([s.speed for s in path])
        out.append({"seed": seeds[i] * 1000, "p": np.vstack([p, pb[1:]]) * 1000, "t": np.concatenate([t, tb[1:]]),
                    "speed": np.concatenate([sp, sb[1:]]), "stage": np.concatenate([st, np.full(len(pb) - 1, 2)]).astype(np.uint8)})
    return out


def thin(line: dict, every: float = KEEP_EVERY) -> dict:
    """The line with its vertices at least `every` mm of path apart (the first, the last and every change of stage kept)."""
    p = line["p"]
    seg = np.r_[0.0, np.cumsum(np.linalg.norm(np.diff(p, axis=0), axis=1))]
    keep = [0]
    for k in range(1, len(p) - 1):
        if seg[k] - seg[keep[-1]] >= every or line["stage"][k] != line["stage"][k - 1]:
            keep.append(k)
    keep.append(len(p) - 1)
    return {**line, **{k: line[k][keep] for k in ("p", "t", "speed", "stage")}}


def write(path: Path, lines: list[dict], meta: dict):
    """`lines` as an npz at `path` (p, t, speed, stage over all vertices, and `offsets` into them; seeds; `meta` as JSON) and a vtp beside it."""
    lines = [thin(x) for x in lines]
    off = np.r_[0, np.cumsum([len(x["p"]) for x in lines])]
    cat = lambda k, dt: np.concatenate([x[k] for x in lines]).astype(dt)  # noqa: E731
    path.parent.mkdir(parents=True, exist_ok=True)
    np.savez_compressed(path, p=cat("p", np.float32), t=cat("t", np.float32), speed=cat("speed", np.float32), stage=cat("stage", np.uint8),
                        offsets=off, seeds=np.array([x["seed"] for x in lines], np.float32), meta=json.dumps(meta))
    pd = pv.PolyData(cat("p", float))
    pd.lines = np.concatenate([np.r_[off[k + 1] - off[k], np.arange(off[k], off[k + 1])] for k in range(len(lines))])
    pd.point_data["t_ms"], pd.point_data["speed"], pd.point_data["stage"] = cat("t", float) * 1000, cat("speed", float), cat("stage", np.uint8)
    pd.save(path.with_suffix(".vtp"))


def load(path: Path) -> tuple[list[dict], dict]:
    """The lines of a `write` (p mm, t s, speed m/s, stage per vertex) and its meta."""
    z = np.load(path)
    off = z["offsets"]
    lines = [{k: z[k][off[i] : off[i + 1]] for k in ("p", "t", "speed", "stage")} | {"seed": z["seeds"][i]} for i in range(len(off) - 1)]
    return lines, json.loads(str(z["meta"]))


def summary(lines: list[dict]) -> str:
    t = np.array([x["t"][-1] for x in lines]) * 1000
    inzone = np.array([np.ptp(x["t"][x["stage"] == 1]) if (x["stage"] == 1).any() else 0 for x in lines]) * 1000
    return (f"{len(lines)} lines; seed to the duct's outlet {np.percentile(t, 10):.0f}-{np.percentile(t, 90):.0f} ms (median {np.median(t):.0f}); "
            f"{(inzone > 0).mean():.0%} cross the rotor zone, {np.median(inzone[inzone > 0]) if (inzone > 0).any() else 0:.1f} ms there (median)")


def trace(grille: Path, duct: Path, out: Path, rpm: float, spacing: float, flap: float | None = None, cfm: float | None = None) -> list[dict]:
    """Pathlines through a finished grille case and a finished duct case, written to `out`."""
    omega = -rpm * 2 * math.pi / 60  # the rotor turns clockwise looking downstream (fan.py)
    room = Field(ensure_vtu(grille), omega=omega, x_stop=X_CUT / 1000)
    x0 = room.mesh.bounds[0] * 1000 + 1.0
    lines = stitch(room, Field(ensure_vtu(duct)), lattice(x0, 44.0, spacing), omega)
    meta = {"rpm": rpm, "omega_rad_s": omega, "flap_deg": flap, "cfm": cfm, "grille": str(grille), "duct": str(duct), "x_cut_mm": X_CUT,
            "seed_plane_mm": x0, "seed_spacing_mm": spacing, "stages": STAGES, "zone_mm": ZONE,
            "note": "t is each particle's own clock from its seed; the rotor stands at the meshed pose at t = 0 and turns at omega about +x"}
    write(out, lines, meta)
    return lines


def run_stages(out: Path, flaps: list[float], cfm: float, rpm: float, np_: int, mem: str, tube_d: float, spacing: float):
    """The CFD and then the lines: the grille case (once), the fan's exit profile from it, a duct case on that profile at each flap
    angle (at the same flow), and `trace` through each pair. Cases already finished in `out` are not run again."""
    from . import fan, grille, run as duct

    a = out / f"grille-current-gap0-{cfm:g}cfm"
    if not (a / "results.json").exists():
        grille.run(grille.GrilleCase(cfm=cfm, rpm=rpm, np=np_), out, mem)
    prof = a / "profile.json"
    prof.write_text(json.dumps(fan.profile(a), indent=2))
    for f in flaps:
        d = out / f"duct-flap{f:g}"
        if not (d / "results.json").exists():
            duct.run(d.name, duct.Case(flap_deg=f, cfm=cfm, profile=str(prof), tube_d=tube_d, np=np_), out, np_, "8G")
        lines = trace(a, d, out / f"pathlines-flap{f:g}.npz", rpm, spacing, f, cfm)
        print(f"flap {f:g}: {summary(lines)}")


def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    sub = ap.add_subparsers(dest="cmd", required=True)
    r = sub.add_parser("run", help="run the cases (grille once, a duct case per flap angle) and trace through each pair")
    r.add_argument("--flap", type=float, nargs="+", default=[0.0], help="flap angles, deg (0 open)")
    r.add_argument("--cfm", type=float, default=4.2)
    r.add_argument("--np", type=int, default=8)
    r.add_argument("--mem", default="32G", help="container memory for the grille case")
    r.add_argument("--tube-d", type=float, default=4.0, help="tube length after the socket's shoulder, in diameters")
    t = sub.add_parser("trace", help="trace through finished cases")
    t.add_argument("--grille", type=Path, required=True, help="the finished grille case (build/pathlines/grille-current-gap0-4.2cfm)")
    t.add_argument("--duct", type=Path, required=True, help="the finished duct case, run on the grille case's profile.json")
    t.add_argument("--out", type=Path, default=Path("build/pathlines/pathlines.npz"))
    for q in (r, t):
        q.add_argument("--rpm", type=float, default=3000.0)
        q.add_argument("--spacing", type=float, default=2.0, help="mm between seeds on the room's inflow face")
    r.add_argument("--out", type=Path, default=Path("build/pathlines"))
    a = ap.parse_args()
    if a.cmd == "run":
        run_stages(a.out, a.flap, a.cfm, a.rpm, a.np, a.mem, a.tube_d, a.spacing)
    else:
        print(summary(trace(a.grille, a.duct, a.out, a.rpm, a.spacing)), "->", a.out)


if __name__ == "__main__":
    main()
