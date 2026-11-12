"""`just mech::cfd::les`: the flap under incompressible LES (pimpleFoam, WALE) in the OpenFOAM container (`.2.18`).

The same air volume and mesher as the steady case (geometry.py, case.py), the flap region refined, then an adjustable time step run
long enough to settle and to average over. What it writes for the post-processing (les_post.py) and for `.2.19`:

  - forces and moments on the flap and rod every step (kinematic: the post multiplies by the density); the moment is about
    the shaft axis, so Mz is the servo's torque ripple;
  - p and U at probes: the bore wall round the flap (the wall pressure the duct's acoustic field also sees), the axis and
    the flap's two shear layers downstream;
  - Howe's source: the Lamb vector omega x u integrated over 2 mm slabs along the axis every few steps (the post projects
    it on a duct mode's particle velocity);
  - UMean / UPrime2Mean / pMean over the averaging window, and vorticity and Q at the checkpoints.

Resolution is the one thing that must not move the answer: `--res coarse|medium|fine` scales the background cell by 1.25 each
and the post compares the shedding peaks (`minimeater-les-post --compare`). `smoke` is for checking the pipeline.
"""

import argparse
import json
import math
import re
import shutil
import sys
import time
from dataclasses import asdict, dataclass, replace
from pathlib import Path

from build123d import Vector

from . import geometry, runtime
from . import case as steady
from .case import PATCHES, Case, _f, stations
from .variants import damper, leak_cm2

RHO = 1.204  # kg/m^3, air at 20 C
RES = {  # base cell (mm), flap surface level, wake zone level: cell = base / 2^level
    "smoke": (4.0, 3, 2),
    "coarse": (2.5, 4, 2),
    "medium": (2.0, 4, 2),
    "fine": (1.6, 4, 2),
}
# (flap deg from open, CFM). What the damper-first map visits (docs/cfd.md, minimeater-operating): the low-and-slow holds on natural
# draft with the flap 25-36 deg from open at 0.5-1 CFM, a hold with the flap nearly open, and the fan through an open flap (a
# hold at 2 CFM, light-up at 4.1). Then the two points off the map where the acoustics chart crosses the modes, in case the
# map ever blends fan and flap: 30 deg at 6 CFM, and the steady CFD's 45 deg at 6 CFM as the anchor.
SWEEP = ((33, 0.7), (25, 0.95), (8, 1.36), (0, 2.0), (0, 4.1), (30, 6.0), (45, 6.0))
DT_MAX = 5e-5  # s: the forces and probes are sampled at least at 20 kHz (the duct's modes are below 6 kHz)
SLAB = 2.0  # mm, the Howe slabs (a slab must hold cell centres: the mesh is 0.5-1 mm round the flap, so not thinner)
SLAB_FROM, SLAB_TO = -10.0, 40.0  # mm from the shaft axis


@dataclass(frozen=True)
class LesCase(Case):
    variant: str = "round"
    res: str = "medium"
    t_end: float = 0.0  # s; make() sizes both from the flow unless given
    t_stats: float = 0.0  # s: the averaging and spectra start here (three flow-throughs of the flap region before it are spin-up)
    co: float = 0.8  # the largest Courant number the adjustable time step allows
    intensity: float = 0.05  # the inlet's velocity fluctuations (the fan's blade wakes), share of the annulus speed
    howe: bool = True  # the Lamb-vector slab integrals (.2.19)
    force_hz: float = 0.0  # a forced run (.2.19): the inflow oscillates at this frequency, a duct mode's (0: the turbulent inlet)
    force_amp: float = 0.0  # its amplitude, a share of the inflow's mean speed: a compact flap sees the mode's acoustic velocity
    zone_from: float = -8.0  # mm from the shaft axis: the refined cylinder round the flap
    zone_to: float = 30.0

    @property
    def u_ref(self) -> float:
        """The fastest speed to expect, m/s: the bulk speed over the share of the bore the tilted disc leaves open."""
        s = stations(self.tube_d)
        v = self.q_m3s / (math.pi * (s.r_bore / 1000) ** 2)
        open_ = max(1 - math.sin(math.radians(self.flap_deg)), 0.04)
        return min(2.0 * v / open_, 60.0)  # the 30 deg smoke case ran at Courant 1.1-1.6 on a 1.5 factor

    @property
    def dt(self) -> float:
        """The time step to expect, s (the run adjusts it to the Courant limit; the post resamples to a uniform 20 kHz): Courant `co`
        at the flap's surface cells and `u_ref`, no more than DT_MAX, to two figures, rounded down."""
        h = self.base / 2**self.flap_level / 1000
        dt = min(self.co * h / self.u_ref, DT_MAX)
        e = 10 ** math.floor(math.log10(dt))
        return math.floor(dt / e * 10) / 10 * e

    @property
    def steps(self) -> int:
        return round(self.t_end / self.dt)


def make(flap_deg: float, cfm: float, variant: str = "round", res: str = "medium", np: int = 8, **kw) -> LesCase:
    base, flap_level, zone_level = RES[res]
    c = LesCase(flap_deg=flap_deg, cfm=cfm, variant=variant, res=res, np=np, tube_d=6.0, base=base, flap_level=flap_level, **kw)
    c = replace(c, zone=(g_flap_x() + c.zone_from, g_flap_x() + c.zone_to, zone_level), iterations=0)
    t_stats = c.t_stats or 0.9 / c.u_ref  # 3 flow-throughs of 150 mm at half the fastest speed
    window = max(0.08, 40 / (100 * c.u_ref))  # s: 40 periods of the edge's shedding (St 0.2 on the speed past a 2 mm edge, 100 u Hz), 0.08 s at least
    return replace(c, t_stats=t_stats, t_end=c.t_end or round(t_stats + window, 4))


def g_flap_x() -> float:
    from ..geom import g

    return g.flap_x


def name_of(c: LesCase) -> str:
    return f"{c.variant}-{c.flap_deg:g}deg-{c.cfm:g}cfm-{c.res}" + (f"-forced{c.force_hz:g}hz-{c.force_amp:g}" if c.force_hz else "") + (
        f"-ti{c.intensity:g}" if c.intensity != LesCase.intensity else "") + (f"-rpm{c.rpm:g}" if c.rpm else "") + (
        "-fanprofile" if c.profile else "")


def _inlet(c: LesCase, s, u_in: float) -> str:
    """The inlet's U: the fan's measured exit (`profile`, steady), solid-body swirl at `rpm` in the rotor's sense (its flow
    rate forced or steady), an axial flow forced at `force_hz`, or the turbulent inlet."""
    q = c.q_m3s
    if c.profile:
        if c.force_hz:
            raise SystemExit("--profile is a steady inlet: force with --rpm instead")
        return steady._profile_inlet(Path(c.profile), s, q, u_in)
    if c.rpm:
        # the NF-A6x25 turns clockwise looking downstream; a positive rpm here turns the other way (axis ^ r)
        rate = (f"{{ type sine; frequency {c.force_hz}; amplitude constant {q * c.force_amp}; scale constant 1; level constant {q}; }}"
                if c.force_hz else f"constant {q}")
        return (f"type swirlFlowRateInletVelocity; flowRate {rate}; rpm constant {-c.rpm}; origin (0 0 0); axis (1 0 0); "
                f"value uniform ({u_in} 0 0);")
    if c.force_hz:
        return (f"type uniformFixedValue; uniformValue {{ type sine; frequency {c.force_hz}; amplitude constant {u_in * c.force_amp}; "
                f"scale constant (1 0 0); level constant ({u_in} 0 0); }} value uniform ({u_in} 0 0);")
    return (f"type turbulentInlet; referenceField uniform ({u_in} 0 0); fluctuationScale ({u_in * c.intensity} {u_in * c.intensity} "
            f"{u_in * c.intensity}); alpha 0.1; value uniform ({u_in} 0 0);")


def probes(c: LesCase) -> list[tuple[str, tuple[float, float, float]]]:
    """Named probe points (mm) that lie in the air: the bore wall round the flap, the axis downstream, the shear layers
    off the disc's two edges. The flap turns about z, its downstream edge going to +y."""
    s = stations(c.tube_d)
    from ..geom import g

    xf, r, th = g.flap_x, s.r_bore, math.radians(c.flap_deg)
    rd = r - 0.15  # the disc's radius
    wall = 0.96 * r
    pts = []
    for d in (-4.0, 4.0, 8.0, 16.0, 32.0):
        for tag, (y, z) in {"+y": (wall, 0.0), "-y": (-wall, 0.0), "+z": (0.0, wall), "-z": (0.0, -wall)}.items():
            pts.append((f"wall{tag}@{d:+g}", (xf + d, y, z)))
    for d in (3.0, 6.0, 12.0, 24.0):
        pts.append((f"axis@{d:g}", (xf + d, 0.0, 0.0)))
    for tag, sx in (("down", 1.0), ("up", -1.0)):  # off each edge of the disc, outside it and a little downstream
        x, y = xf + sx * rd * math.cos(th), sx * rd * math.sin(th)
        for k, off in enumerate((0.8, 2.0)):
            pts.append((f"edge-{tag}{k}", (x + 1.0, y + sx * off, 0.0)))
    air = geometry.fluid(c.flap_deg, c.tube_d, damper(c.variant))
    return [(n, p) for n, p in pts if air.is_inside(Vector(*p))]


def files(c: LesCase, pts: list) -> dict[str, str]:
    """The steady case's meshing files, and the LES's own solver, schemes, controls and fields."""
    out = {k: v for k, v in steady.files(c).items() if k.startswith(("system/blockMesh", "system/snappy", "system/surfaceFeature", "system/decompose"))}
    s = stations(c.tube_d)
    xf = g_flap_x()
    q = c.q_m3s
    u_in = q / (math.pi * ((s.r_fan / 1000) ** 2 - (s.r_hub / 1000) ** 2))
    fos = [f"""    forces {{ type forces; libs (forces); patches (flap); rho rhoInf; rhoInf 1; CofR ({xf / 1000} 0 0);
        writeControl timeStep; writeInterval 1; log no; }}
    probes {{ type probes; libs (sampling); fields (p U); writeControl timeStep; writeInterval 1;
        probeLocations ({' '.join(f'({x / 1000} {y / 1000} {z / 1000})' for _, (x, y, z) in pts)}); }}
    avg {{ type fieldAverage; libs (fieldFunctionObjects); timeStart {c.t_stats}; writeControl writeTime;
        fields ( U {{ mean on; prime2Mean on; base time; }} p {{ mean on; prime2Mean on; base time; }} ); }}
    vorticity {{ type vorticity; libs (fieldFunctionObjects); writeControl writeTime; }}
    Q {{ type Q; libs (fieldFunctionObjects); writeControl writeTime; }}
"""]
    n_slab = 0
    if c.howe:
        n_slab = round((SLAB_TO - SLAB_FROM) / SLAB)
        every = DT_MAX  # s, runTime
        fos.append(f"""    lamb {{ type LambVector; libs (fieldFunctionObjects); field U; executeControl runTime; executeInterval {every}; writeControl none; }}
""")
        for i in range(n_slab):
            fos.append(f"""    slab{i} {{ type volFieldValue; libs (fieldFunctionObjects); regionType cellZone; name slab{i}; operation volIntegrate;
        fields (LambVector); writeFields false; log false; writeControl runTime; writeInterval {every}; }}
""")
        r = s.r_bore + 1
        acts = "".join(
            f"    {{ name slab{i}; type cellZoneSet; action new; source boxToCell; "
            f"box ({(xf + SLAB_FROM + i * SLAB) / 1000} {-r / 1000} {-r / 1000}) ({(xf + SLAB_FROM + (i + 1) * SLAB) / 1000} {r / 1000} {r / 1000}); }}\n"
            for i in range(n_slab))
        out["system/topoSetDict"] = _f("dictionary", "topoSetDict", f"actions\n(\n{acts});\n")
    chk = max(c.t_end / 6, c.dt)
    out["system/controlDict"] = _f("dictionary", "controlDict", f"""
application pimpleFoam; startFrom latestTime; startTime 0; stopAt endTime; endTime {c.t_end}; deltaT {c.dt / 4}; adjustTimeStep yes; maxCo {c.co}; maxDeltaT {DT_MAX};
writeControl adjustableRunTime; writeInterval {chk}; purgeWrite 3; writeFormat binary; writePrecision 8; writeCompression off;
timeFormat general; timePrecision 8; runTimeModifiable false;
functions
{{
{''.join(fos)}}}
""")
    out["system/fvSchemes"] = _f("dictionary", "fvSchemes", """
ddtSchemes { default backward; }
gradSchemes { default Gauss linear; }
divSchemes { default none; div(phi,U) Gauss LUST grad(U); div((nuEff*dev2(T(grad(U))))) Gauss linear; }
laplacianSchemes { default Gauss linear limited corrected 0.5; }
interpolationSchemes { default linear; }
snGradSchemes { default limited corrected 0.5; }
wallDist { method meshWave; }
""")
    out["system/fvSolution"] = _f("dictionary", "fvSolution", """
solvers
{
    p { solver GAMG; smoother GaussSeidel; tolerance 1e-7; relTol 0.02; }
    pFinal { $p; relTol 0; tolerance 1e-8; }
    "U.*" { solver smoothSolver; smoother symGaussSeidel; tolerance 1e-8; relTol 0; }
}
PIMPLE { nOuterCorrectors 1; nCorrectors 3; nNonOrthogonalCorrectors 1; }
""")
    out["constant/transportProperties"] = _f("dictionary", "transportProperties", f"transportModel Newtonian; nu {steady.NU};\n")
    out["constant/turbulenceProperties"] = _f("dictionary", "turbulenceProperties", """
simulationType LES;
LES { LESModel WALE; turbulence on; printCoeffs on; delta cubeRootVol; cubeRootVolCoeffs { deltaCoeff 1; } }
""")
    bf = {
        "U": ("volVectorField", "[0 1 -1 0 0 0 0]", {
            "inlet": _inlet(c, s, u_in),
            "outlet": "type inletOutlet; inletValue uniform (0 0 0); value uniform (0 0 0);",
            steady.WALLS: "type noSlip;"}),
        "p": ("volScalarField", "[0 2 -2 0 0 0 0]", {
            "inlet": "type zeroGradient;", "outlet": "type fixedValue; value uniform 0;", steady.WALLS: "type zeroGradient;"}),
        "nut": ("volScalarField", "[0 2 -1 0 0 0 0]", {
            "inlet": "type calculated; value uniform 0;", "outlet": "type calculated; value uniform 0;",
            steady.WALLS: "type nutUSpaldingWallFunction; value uniform 0;"}),
    }
    for nm, (cls, dim, patches) in bf.items():
        body = "".join(f"    {p} {{ {v} }}\n" for p, v in patches.items())
        init = f"uniform ({u_in} 0 0)" if nm == "U" else "uniform 0"
        out[f"0.orig/{nm}"] = _f(cls, nm, f"dimensions {dim};\ninternalField {init};\nboundaryField\n{{\n{body}}}\n")
    out["Allrun"] = f"""#!/bin/bash
# ./Allrun: mesh and run; ./Allrun mesh: the mesh only (the cell count); ./Allrun resume: carry on from the latest checkpoint (the logs append)
set -euo pipefail
cd "$(dirname "$0")"
export OMPI_ALLOW_RUN_AS_ROOT=1 OMPI_ALLOW_RUN_AS_ROOT_CONFIRM=1
run() {{ echo "== $1"; "$@" > "log.$1" 2>&1 || {{ tail -40 "log.$1"; exit 1; }}; }}
prun() {{ echo "== $1 (x{c.np})"; mpirun --oversubscribe -np {c.np} "$@" -parallel >> "log.$1" 2>&1 || {{ tail -40 "log.$1"; exit 1; }}; }}
if [ "${{1:-}}" != resume ]; then
    rm -rf 0 processor* postProcessing constant/polyMesh constant/extendedFeatureEdgeMesh log.*
    run blockMesh
    run surfaceFeatureExtract
    run snappyHexMesh -overwrite
    run transformPoints -scale 0.001
    run checkMesh
{'    run topoSet' if c.howe else ''}
    cp -r 0.orig 0
    run decomposePar
fi
if [ "${{1:-}}" == mesh ]; then exit 0; fi
prun pimpleFoam
echo "== reconstruct"
reconstructPar -latestTime > log.reconstructPar 2>&1 || true
echo "== done"
"""
    return out


def write(case_dir: Path, c: LesCase) -> list:
    """The STLs and the case files; returns the probe points."""
    geometry.write_stl(case_dir / "constant" / "triSurface", c.flap_deg, c.tube_d, damper(c.variant))
    pts = probes(c)
    for rel, text in files(c, pts).items():
        p = case_dir / rel
        p.parent.mkdir(parents=True, exist_ok=True)
        p.write_text(text)
    (case_dir / "Allrun").chmod(0o755)
    (case_dir / "probes.json").write_text(json.dumps([{"name": n, "xyz_mm": p} for n, p in pts], indent=1))
    (case_dir / "case.json").write_text(json.dumps({**asdict(c), "dt": c.dt, "steps": c.steps, "u_ref": c.u_ref,
                                                  "leak_cm2": leak_cm2(c.variant)}, indent=1))
    return pts


def cells(case_dir: Path) -> int | None:
    log = case_dir / "log.checkMesh"
    m = re.search(r"^\s*cells:\s+(\d+)", log.read_text(), re.M) if log.exists() else None
    return int(m.group(1)) if m else None


def courant(case_dir: Path) -> float | None:
    log = case_dir / "log.pimpleFoam"
    if not log.exists():
        return None
    mx = re.findall(r"Courant Number mean: \S+ max: (\S+)", log.read_text())
    return max(map(float, mx)) if mx else None


def run(name: str, c: LesCase, out: Path, mem: str, resume: bool, dry: bool, mesh_only: bool = False) -> Path:
    case_dir = (out / name).resolve()
    if not resume:
        if case_dir.exists():
            shutil.rmtree(case_dir)
        t0 = time.time()
        write(case_dir, c)
        print(f"{name}: dt {c.dt:.2g} s, {c.steps} steps to {c.t_end} s (stats from {c.t_stats} s), u_ref {c.u_ref:.1f} m/s, "
              f"geometry + case {time.time() - t0:.0f} s", flush=True)
    if dry:
        print(f"dry run: {case_dir}")
        return case_dir
    t0 = time.time()
    if runtime.run(case_dir, "/case/Allrun" + (" resume" if resume else " mesh" if mesh_only else ""), c.np, mem):
        sys.exit(f"{name}: OpenFOAM failed (logs in {case_dir})")
    info = {"wall_s": round(time.time() - t0), "cells": cells(case_dir), "co_max": courant(case_dir)}
    (case_dir / "run.json").write_text(json.dumps(info, indent=1))
    print(name, info)
    return case_dir


def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--flap", type=float, default=45.0, help="flap angle, deg (0 open)")
    ap.add_argument("--cfm", type=float, default=6.0, help="flow")
    ap.add_argument("--variant", default="round", help="flap edge/rib variant (variants.py)")
    ap.add_argument("--res", default="medium", choices=sorted(RES))
    ap.add_argument("--sweep", action="store_true", help="the SWEEP list of angles and flows, one after another")
    ap.add_argument("--t-end", type=float, help="s of physical time")
    ap.add_argument("--t-stats", type=float, help="s: where the averaging starts")
    ap.add_argument("--co", type=float, help="the Courant number to size the time step for")
    ap.add_argument("--no-howe", action="store_true", help="skip the Lamb-vector slab integrals")
    ap.add_argument("--force-hz", type=float, help="a forced run: the inflow oscillates at this frequency (a duct mode's; .2.19)")
    ap.add_argument("--force-amp", type=float, default=0.05, help="the forcing's amplitude, a share of the mean inflow")
    ap.add_argument("--intensity", type=float, help="the turbulent inlet's fluctuations, a share of the inflow (0: a steady inlet)")
    ap.add_argument("--rpm", type=float, help="solid-body swirl at the inlet at this speed, in the rotor's sense (.2.20)")
    ap.add_argument("--profile", help="the fan's measured exit (cfd/fan.py's profile.json) as a steady inlet (.2.20)")
    ap.add_argument("--np", type=int, default=12, help="MPI ranks (and container CPUs)")
    ap.add_argument("--mem", default="48G", help="container memory")
    ap.add_argument("--out", type=Path, default=Path("build/les"))
    ap.add_argument("--resume", action="store_true", help="carry on a stopped run from its checkpoint (needs the same arguments)")
    ap.add_argument("--mesh-only", action="store_true", help="mesh it and report the cell count, do not solve")
    ap.add_argument("--dry", action="store_true", help="write the case and print the time step, do not run it")
    a = ap.parse_args()
    kw = {k: v for k, v in (("t_end", a.t_end), ("t_stats", a.t_stats), ("co", a.co)) if v is not None}
    if a.no_howe:
        kw["howe"] = False
    if a.intensity is not None:
        kw["intensity"] = a.intensity
    if a.rpm:
        kw["rpm"] = a.rpm
    if a.profile:
        kw["profile"] = a.profile
    if a.force_hz:
        kw |= {"force_hz": a.force_hz, "force_amp": a.force_amp}
    todo = SWEEP if a.sweep else ((a.flap, a.cfm),)
    for flap, cfm in todo:
        c = make(flap, cfm, a.variant, a.res, a.np, **kw)
        run(name_of(c), c, a.out, a.mem, a.resume, a.dry, a.mesh_only)


if __name__ == "__main__":
    main()
