"""`uv run python -m minimeater.mech.cfd.grille [--grille current|s-louvre] [--gap MM]`: the fan's intake, steady RANS
with the rotor in a rotating frame (fan.py's case with the rear wall in front of it): outside air, the rear wall's
louvres, the gap to the fan, the fan and its exhaust pipe.

The domain: a room behind the rear face (the background box, slip walls, a uniform inflow face 25 mm out), the rear
wall with its grille as a solid plate across the room, the enclosure's cavity round the fan (its section at the
fan, carried back to the wall) and fan.py's frame, rotor and pipe. `gap` moves the wall and the room back by that
much (the box made longer): the louvres' trailing edges further from the rotor.

Results, as fan.py's: the fan's static rise against the room's total pressure, at the same flow and speed as the
free-air fan case, so the difference is what the grille and the gap cost the fan (its loss plus what the uneven
inflow takes from the rotor); and the inflow just in front of the rotor (`intake` plane): its non-uniformity and
angle.
"""

import argparse
import math
import importlib.util
import json
import shutil
import sys
import time
from dataclasses import dataclass
from pathlib import Path

import numpy as np
from build123d import Axis, Compound, Edge, Face, Plane, Pos, Rectangle, Rot, Wire, export_stl, extrude, revolve

from .. import geom as G
from .. import shell
from ..mock import fan_step
from ..params import P
from ..parts import box, xcyl
from . import fan, runtime, swirl
from .run import RHO, _tail

HALF = 45.0  # the room's half width: the rear face is 72 x 70
ROOM = 25.0  # the room's depth behind the rear face
GRILLES = ("current", "s-louvre")


@dataclass(frozen=True)
class GrilleCase:
    grille: str = "current"
    gap: float = 0.0  # mm added between the rear wall and the fan (the nominal is P.layout.rear_gap)
    cfm: float = 4.2
    rpm: float = 3000.0
    iterations: int = 1500
    np: int = 8
    base: float = 2.0
    rotor_level: int = 3
    grille_level: int = 2
    concept: str = ""  # the S-louvre script (s_louvre)
    plenum: str = ""  # flow shaping in the plenum: "ribs" (anti-swirl ribs at the blade tips), "dome" (a stationary nose over the rotor hub), "ring" (a separator ring at the tips, the ribs outside it), "bell" (a bellmouth down to the fan's bore)

    @property
    def q_m3s(self) -> float:
        return self.cfm * 0.0283168 / 60.0

    @property
    def x_in(self) -> float:
        return shell.X_REAR - self.gap - ROOM


def s_louvre(path: Path):
    """The S-louvre concept (minimeat-seq.2.26), a script outside the package until it is adopted: its `build()` insert
    (round field, blades, hub cover, collar in a plate) and its field's radius `R_F`."""
    spec = importlib.util.spec_from_file_location("s_louvre", path)
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod.build(), mod.R_F


def wall(c: GrilleCase):
    """The rear wall with its grille across the room, and the cavity's walls from it to the fan's outlet face: one
    solid, in the assembly frame with the wall moved back by c.gap."""
    x0, x1 = shell.X_REAR, G.REAR_IN_X
    slab = box(x0, x1, -HALF - 1, HALF + 1, -HALF - 1, HALF + 1)
    if c.grille == "current":
        plate = slab - shell.grille_cut()
    else:
        insert, r_f = s_louvre(Path(c.concept))
        plate = (slab - xcyl(r_f, x0 - 1, x1 + 1)) + insert
    plate = Pos(-c.gap, 0, 0) * plate
    # the cavity: its section at the fan's intake face, carried from the wall to past the outlet face
    xs = G.FAN_X0 + 2.0
    sec = (shell.cavity() & box(xs - 0.05, xs + 0.05, -60, 60, -60, 60)).faces().filter_by(lambda f: abs(f.center().X - xs - 0.05) < 1e-3)[0]
    x2 = G.g.fan_face_x - 1.0  # into fan.seal, which carries the walls on as the pipe's
    hole = extrude(Pos(x1 - c.gap - 0.3 - sec.center().X, 0, 0) * sec, x2 + 1 - (x1 - c.gap - 0.3), dir=(1, 0, 0))
    sleeve = box(x1 - c.gap - 0.3, x2, -HALF - 1, HALF + 1, -HALF - 1, HALF + 1) - hole
    out = plate + sleeve
    if c.gap == 0:  # what stands in the plenum as the shells draw it: the fan's shim blocks and its four rest pads
        out = out + shell.fan_shims() + shell.rear_bumps()
    if c.plenum:
        out = out + plenum_shaping(c)
    return out


RIBS = (12, 1.2, 25.0, 34.0, 2.0)  # count, thickness, r0, r1, gap to the fan's frame
DOME = (12.5, 8.0, 1.0)  # radius, depth upstream, gap to the rotor's hub
RING = (29.5, 1.2, 6.0, 0.5)  # inner radius, wall, length upstream, gap to the frame
BELL = 6.0  # the bellmouth's quarter-round lip, down to the fan's bore


def plenum_shaping(c: GrilleCase):
    """`ribs`: radial ribs at the blade tips from the rear wall to just off the frame, 15 deg off the axes (none on
    the seam): an anti-swirl ring for the tips' recirculation, a stall stabiliser in industrial axial fans. `dome`: a
    half-ellipsoid over the rotor's hub, floating (the control: the hub carries almost no flow)."""
    x0, x1 = G.REAR_IN_X - c.gap - 0.2, G.FAN_X0 - RIBS[4]
    if c.plenum == "ribs":
        n, t, r0, r1, _ = RIBS
        one = box(x0, x1, -t / 2, t / 2, r0, r1)
        out = None
        for i in range(n):
            rib = Rot(15.0 + 360.0 * i / n, 0, 0) * one
            out = rib if out is None else out + rib
        return out
    if c.plenum == "dome":
        r, d, gap = DOME
        xe = G.FAN_X0 - gap
        return _dome(xe, r, d)
    if c.plenum == "ring":
        # a separator ring just outside the blade tips; the tips' reverse flow leaves outward between it and the wall,
        # where the ribs take its swirl out, and rejoins upstream of the ring's lip (an axial fan's stall stabiliser)
        ri, t, ln, gap = RING
        xr1 = G.FAN_X0 - gap
        ring = xcyl(ri + t, xr1 - ln, xr1) - xcyl(ri, xr1 - ln - 1, xr1 + 1)
        n = RIBS[0]
        one = box(xr1 - ln, xr1, -RIBS[1] / 2, RIBS[1] / 2, ri + t / 2, 45.0)
        out = ring
        for i in range(n):
            out = out + Rot(15.0 + 360.0 * i / n, 0, 0) * one
        return out & box(x0, xr1, -45, 45, -45, 45)
    if c.plenum == "bell":
        # the plenum filled outside a bellmouth: a quarter round from the bore's r 29 at the frame to r 29 + BELL upstream
        r0, R, xe = P.fan.aperture_d / 2, BELL, G.FAN_X0 - 0.3
        pts = [(xe - R * math.sin(math.pi / 2 * i / 16), 0, r0 + R * (1 - math.cos(math.pi / 2 * i / 16))) for i in range(17)]
        prof = [Edge.make_spline(pts), Edge.make_line(pts[-1], (pts[-1][0], 0, 0)), Edge.make_line((pts[-1][0], 0, 0), (xe, 0, 0)), Edge.make_line((xe, 0, 0), pts[0])]
        air = revolve(Face(Wire(prof)), Axis.X, 360)
        return box(xe - R, xe, -45, 45, -45, 45) - air
    raise ValueError(f"unknown plenum shaping {c.plenum}")


def _dome(xe, r, d):
    """A half-ellipsoid, flat face at xe toward the rotor, reaching d upstream."""
    from build123d import Axis, Edge, Face, Wire, revolve
    pts = [(xe - d * math.sin(math.pi / 2 * i / 16), 0, r * math.cos(math.pi / 2 * i / 16)) for i in range(17)]
    pts[-1] = (xe - d, 0, 0)  # the arc ends on the axis
    prof = [Edge.make_spline(pts), Edge.make_line(pts[-1], (xe, 0, 0)), Edge.make_line((xe, 0, 0), pts[0])]
    return revolve(Face(Wire(prof)), Axis.X, 360)


PATCHES = ("inlet", "outlet", "frame", "rotor", "pipe", "walls")


def write_stl(c: GrilleCase, out: Path) -> dict[str, int]:
    out.mkdir(parents=True, exist_ok=True)
    s = fan_step().solids()
    tube = xcyl(fan.R_OUT, G.g.fan_face_x - 0.5, fan.X_OUT)
    groups = {
        "inlet": (Plane.YZ.offset(c.x_in) * Rectangle(2 * HALF + 6, 2 * HALF + 6)).faces(),
        "outlet": [f for f in tube.faces() if f.geom_type.name == "PLANE" and f.center().X > fan.X_OUT - 1e-6],
        "frame": s[0].faces(),
        "rotor": s[1].faces(),
        "pipe": fan.seal(HALF).faces(),
        "walls": wall(c).faces(),
    }
    for name, faces in groups.items():
        export_stl(Compound(list(faces)), str(out / f"{name}.stl"), tolerance=0.03, angular_tolerance=0.1)
    return {k: len(v) for k, v in groups.items()}


def intake(f: Path) -> dict:
    """The inflow on a plane just in front of the rotor, over the blade annulus: its axial non-uniformity (the
    flux-weighted spread of c_x over its mean) and its mean angle off the axis."""
    a = np.loadtxt(f, comments="#")
    y, z, u = a[:, 1] * 1000, a[:, 2] * 1000, a[:, 3:6]
    r = np.hypot(y, z)
    m = (r > G.P.fan.hub_d / 2) & (r < G.P.fan.aperture_d / 2)
    cx = u[m, 0]
    cross = np.hypot(u[m, 1], u[m, 2])
    return {"cx_mean_m_s": float(cx.mean()), "cx_spread": float(cx.std() / cx.mean()),
            "angle_deg": float(np.degrees(np.arctan2(cross, np.maximum(cx, 1e-6))).mean()), "backflow_share": float((cx < 0).mean())}


def run(c: GrilleCase, out: Path, mem: str) -> dict:
    name = f"grille-{c.grille}-gap{c.gap:g}-{c.cfm:g}cfm" + (f"-{c.plenum}" if c.plenum else "")
    case_dir = (out / name).resolve()
    if case_dir.exists():
        shutil.rmtree(case_dir)
    t0 = time.time()
    counts = write_stl(c, case_dir / "constant" / "triSurface")
    fc = fan.FanCase(cfm=c.cfm, rpm=c.rpm, iterations=c.iterations, np=c.np, base=c.base, rotor_level=c.rotor_level)
    x_intake = G.FAN_X0 - 0.5
    for rel, text in fan.files(fc, x_in=c.x_in, half=HALF, extra={"walls": c.grille_level}, slip=True,
                               planes={"intake": x_intake}).items():
        p = case_dir / rel
        p.parent.mkdir(parents=True, exist_ok=True)
        p.write_text(text)
    (case_dir / "Allrun").chmod(0o755)
    print(f"{name}: geometry {counts}, {time.time() - t0:.0f} s", flush=True)
    if runtime.run(case_dir, "/case/Allrun", c.np, mem):
        sys.exit(f"{name}: OpenFOAM failed (logs in {case_dir})")
    pt_in, _ = _tail(case_dir, "inlet", "weightedAreaAverage(total(p))")
    p_out, _ = _tail(case_dir, "outlet", "weightedAreaAverage(p)")
    pt_out, _ = _tail(case_dir, "outlet", "weightedAreaAverage(total(p))")
    q_out, _ = _tail(case_dir, "flow", "sum(phi)")
    sw = next((case_dir / "postProcessing" / "swirl").glob("*"))
    res = {
        "case": c.__dict__,
        "flow_cfm": q_out * 60 / 0.0283168,
        "dp_static_pa": RHO * (p_out - pt_in),  # what the fan delivers past the grille: outlet static less the room's total
        "dp_total_pa": RHO * (pt_out - pt_in),
        "intake": intake(sw / "U_intake.raw"),
        "fan_plane": swirl.plane(sw / "U_fan.raw"),
        "wall_s": round(time.time() - t0),
    }
    (case_dir / "results.json").write_text(json.dumps(res, indent=2))
    return res


def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--grille", choices=GRILLES, default="current")
    ap.add_argument("--concept", default="", help="the S-louvre script, for --grille s-louvre")
    ap.add_argument("--gap", type=float, default=0.0, help="mm added between the rear wall and the fan")
    ap.add_argument("--cfm", type=float, default=4.2)
    ap.add_argument("--rpm", type=float, default=3000.0)
    ap.add_argument("--iterations", type=int, default=1500)
    ap.add_argument("--np", type=int, default=8)
    ap.add_argument("--mem", default="32G")
    ap.add_argument("--out", type=Path, default=Path("build/cfd"))
    ap.add_argument("--plenum", default="", choices=("", "ribs", "dome", "ring", "bell"), help="flow shaping in the plenum")
    ap.add_argument("--dry", action="store_true", help="write the case and stop")
    a = ap.parse_args()
    c = GrilleCase(grille=a.grille, concept=a.concept, gap=a.gap, cfm=a.cfm, rpm=a.rpm, iterations=a.iterations, np=a.np, plenum=a.plenum)
    if a.dry:
        d = a.out / f"grille-{c.grille}-gap{c.gap:g}-dry"
        print(write_stl(c, d / "constant" / "triSurface"))
        for rel, text in fan.files(fan.FanCase(np=c.np), x_in=c.x_in, half=HALF, extra={"walls": c.grille_level}, slip=True,
                                   planes={"intake": G.FAN_X0 - 0.5}).items():
            (d / rel).parent.mkdir(parents=True, exist_ok=True)
            (d / rel).write_text(text)
        return
    print(json.dumps(run(c, a.out, a.mem), indent=2))


if __name__ == "__main__":
    main()
