"""`just mech::cfd::fan [--cfm Q] [--rpm N]`: the NF-A6x25 itself, steady RANS with its rotor in a
rotating frame (MRF), from Noctua's public model (`mock.fan_step`): its curve against the spec and its exit swirl.

The domain: a square plenum ahead of the fan (the background box's walls, a square inlet face 45 mm upstream), the
frame and the rotor as Noctua drew them, and a straight pipe the size of the frame's outlet bore for two diameters
behind it, out at 0 Pa. The rotor's cells (r < 28.2, x -121 .. -103.5: the whole rotor, none of the frame) turn at
-omega about +x (the rotor turns clockwise looking downstream, -theta). The pipe's wall is a solid sealed to the frame's outlet face (`seal`), as
the duct's flange is: the pocket round the frame's outside is a dead end.

Results: the fan's static rise (outlet static less the plenum's total), the swirl just past its outlet face (the duct
cases' `fan` plane, swirl.py) and the profile of r c_theta and c_x across the annulus there, which the duct cases can
take as their inlet (`profile.json`).
"""

import argparse
import json
import math
import shutil
import sys
import time
from dataclasses import dataclass
from pathlib import Path

import numpy as np
from build123d import Compound, Plane, Rectangle, export_stl

from .. import geom as G
from ..mock import fan_step
from ..parts import box, xcyl
from . import runtime, swirl
from .case import HEADER, NU
from .run import RHO, _tail

X_IN = G.FAN_X0 - 45.0  # the inlet face, in the plenum
R_OUT = 29.4  # the outlet pipe: the frame's exhaust bore (about 29.5 there)
X_OUT = G.g.fan_face_x + 60.0  # two diameters behind the outlet face
BOX = 33.0  # the plenum's half width: the frame (60 square) sits inside it
ZONE = (-121.0, -103.5, 28.2)  # the rotating cells: x0, x1, r
PATCHES = ("inlet", "outlet", "frame", "rotor", "pipe")
PLANE_X = G.g.fan_face_x + 2.0  # the duct cases' "fan" plane


@dataclass(frozen=True)
class FanCase:
    cfm: float = 4.2
    rpm: float = 3000.0
    iterations: int = 1500
    np: int = 8
    base: float = 2.0
    rotor_level: int = 3
    laminar: bool = False  # no turbulence model: the stopped fan's natural draft (Re ~200 in the gills, ~1300 in the tube)

    @property
    def q_m3s(self) -> float:
        return self.cfm * 0.0283168 / 60.0


def _f(cls, obj, body):
    return HEADER.format(cls=cls, obj=obj) + body


def seal(half: float = BOX):
    """The pipe's wall as a solid filling the box outside it, from 2 mm behind the fan's outlet face (inside the frame)
    to past the outlet: as the duct's flange seals on the face, no air goes round the frame's outside from the pipe
    back to the plenum (a bare pipe wall left a gap there, and a loop of about 1 CFM through it)."""
    x0 = G.g.fan_face_x - 2.0
    return box(x0, X_OUT + 1.0, -half - 1, half + 1, -half - 1, half + 1) - xcyl(R_OUT, x0 - 1, X_OUT + 2)


def write_stl(out: Path) -> dict[str, int]:
    out.mkdir(parents=True, exist_ok=True)
    s = fan_step().solids()
    pipe = seal().faces()
    groups = {
        "inlet": (Plane.YZ.offset(X_IN) * Rectangle(2 * BOX + 6, 2 * BOX + 6)).faces(),
        "outlet": [f for f in xcyl(R_OUT, G.g.fan_face_x - 0.5, X_OUT).faces() if f.geom_type.name == "PLANE" and f.center().X > X_OUT - 1e-6],
        "frame": s[0].faces(),
        "rotor": s[1].faces(),
        "pipe": pipe,
    }
    for name, faces in groups.items():
        export_stl(Compound(list(faces)), str(out / f"{name}.stl"), tolerance=0.03, angular_tolerance=0.1)
    return {k: len(v) for k, v in groups.items()}


def files(c: FanCase, x_in: float = X_IN, half: float = BOX, extra: dict[str, int] | None = None, slip: bool = False,
          planes: dict[str, float] | None = None) -> dict[str, str]:
    """The case's files. For a bigger domain round the same fan (grille.py): the inlet face at x_in, the box's half width,
    `extra` wall STLs by name with their surface levels, the box's own walls slip (`slip`) and more sampling planes."""
    extra = extra or {}
    patches = PATCHES + tuple(extra)
    x0, x1, r = x_in - 2.0, X_OUT + 2.0, half
    nx, ny = round((x1 - x0) / c.base), round(2 * r / c.base)
    levels = {"inlet": 1, "outlet": 1, "frame": 2, "rotor": c.rotor_level, "pipe": 1, **extra}
    out = {}
    out["system/blockMeshDict"] = _f("dictionary", "blockMeshDict", f"""
vertices ( ({x0} {-r} {-r}) ({x1} {-r} {-r}) ({x1} {r} {-r}) ({x0} {r} {-r})
           ({x0} {-r} {r}) ({x1} {-r} {r}) ({x1} {r} {r}) ({x0} {r} {r}) );
blocks ( hex (0 1 2 3 4 5 6 7) ({nx} {ny} {ny}) simpleGrading (1 1 1) );
boundary ( background {{ type wall; faces ( (0 3 2 1) (4 5 6 7) (0 4 7 3) (1 2 6 5) (0 1 5 4) (3 7 6 2) ); }} );
""")
    out["system/surfaceFeatureExtractDict"] = _f("dictionary", "surfaceFeatureExtractDict", "".join(
        f"{p}.stl {{ extractionMethod extractFromSurface; includedAngle 150; subsetFeatures {{ nonManifoldEdges no; openEdges yes; }} writeObj no; }}\n"
        for p in patches))
    geo = "".join(f"    {p} {{ type triSurfaceMesh; file \"{p}.stl\"; }}\n" for p in patches)
    zx0, zx1, zr = ZONE
    geo += f"    zone {{ type searchableCylinder; point1 ({zx0 - 1} 0 0); point2 ({zx1 + 4} 0 0); radius {zr + 1.5}; }}\n"
    feats = "".join(f"        {{ file \"{p}.eMesh\"; level {levels[p]}; }}\n" for p in patches)
    surfs = "".join(f"        {p} {{ level ({levels[p]} {levels[p]}); patchInfo {{ type {'patch' if p in ('inlet', 'outlet') else 'wall'}; }} }}\n"
                    for p in patches)
    loc = (G.g.fan_face_x + 20.0, 3.113, 17.071)  # in the outlet pipe, off every cell face
    out["system/snappyHexMeshDict"] = _f("dictionary", "snappyHexMeshDict", f"""
castellatedMesh true; snap true; addLayers false;
geometry
{{
{geo}}}
castellatedMeshControls
{{
    maxLocalCells 6000000; maxGlobalCells 30000000; minRefinementCells 0; maxLoadUnbalance 0.10;
    nCellsBetweenLevels 3; resolveFeatureAngle 30; allowFreeStandingZoneFaces true;
    features
    (
{feats}    );
    refinementSurfaces
    {{
{surfs}    }}
    refinementRegions {{ zone {{ mode inside; levels ((1E15 2)); }} }}
    locationInMesh ({loc[0]} {loc[1]} {loc[2]});
}}
snapControls {{ nSmoothPatch 3; tolerance 2.0; nSolveIter 50; nRelaxIter 5; nFeatureSnapIter 10;
    implicitFeatureSnap false; explicitFeatureSnap true; multiRegionFeatureSnap false; }}
addLayersControls {{ relativeSizes true; layers {{ }}; expansionRatio 1.2; finalLayerThickness 0.4; minThickness 0.1;
    nGrow 0; featureAngle 130; nRelaxIter 3; nSmoothSurfaceNormals 1; nSmoothNormals 3; nSmoothThickness 10;
    maxFaceThicknessRatio 0.5; maxThicknessToMedialRatio 0.3; minMedialAxisAngle 90; nBufferCellsNoExtrude 0; nLayerIter 50; }}
meshQualityControls
{{
    maxNonOrtho 65; maxBoundarySkewness 20; maxInternalSkewness 4; maxConcave 80; minVol 1e-13;
    minTetQuality 1e-15; minArea -1; minTwist 0.02; minDeterminant 0.001; minFaceWeight 0.05; minVolRatio 0.01;
    minTriangleTwist -1; nSmoothScale 4; errorReduction 0.75;
    relaxed {{ maxNonOrtho 75; }}
}}
mergeTolerance 1e-6;
""")
    out["system/topoSetDict"] = _f("dictionary", "topoSetDict", f"""
actions
(
    {{ name rotorCells; type cellSet; action new; source cylinderToCell; p1 ({zx0 / 1000} 0 0); p2 ({zx1 / 1000} 0 0); radius {zr / 1000}; }}
    {{ name rotorZone; type cellZoneSet; action new; source setToCellZone; set rotorCells; }}
);
""")
    omega = -c.rpm * 2 * math.pi / 60  # clockwise looking downstream: -theta about +x
    out["constant/MRFProperties"] = _f("dictionary", "MRFProperties", f"""
rotor {{ cellZone rotorZone; active {"yes" if c.rpm else "no"}; nonRotatingPatches (frame pipe inlet outlet background {" ".join(extra)}); origin (0 0 0); axis (1 0 0); omega {omega}; }}
""")
    out["system/decomposeParDict"] = _f("dictionary", "decomposeParDict", f"numberOfSubdomains {c.np}; method scotch;\n")
    fos = []
    for p in ("inlet", "outlet"):
        fos.append(f"""    {p} {{ type surfaceFieldValue; libs (fieldFunctionObjects); regionType patch; name {p};
        operation weightedAreaAverage; weightField phi; fields (p total(p)); writeFields false; log false; }}
""")
    fos.append("""    flow { type surfaceFieldValue; libs (fieldFunctionObjects); regionType patch; name outlet;
        operation sum; fields (phi); writeFields false; log false; }
    torque { type forces; libs (forces); patches (rotor); rho rhoInf; rhoInf 1.204; CofR (0 0 0); log false; }
""")
    fos.append(f"""    swirl {{ type surfaces; libs (sampling); writeControl onEnd; surfaceFormat raw; fields (U p);
        surfaces {{
{"".join(f"            {n} {{{{ type cuttingPlane; planeType pointAndNormal; pointAndNormalDict {{{{ point ({x / 1000} 0 0); normal (1 0 0); }}}} interpolate false; }}}}\n".replace("{{", "{").replace("}}", "}") for n, x in {"fan": PLANE_X, **(planes or {})}.items())}        }} }}
""")
    out["system/controlDict"] = _f("dictionary", "controlDict", f"""
application simpleFoam; startFrom startTime; startTime 0; stopAt endTime; endTime {c.iterations}; deltaT 1;
writeControl timeStep; writeInterval {c.iterations}; purgeWrite 1; writeFormat binary; writePrecision 8;
writeCompression off; timeFormat general; timePrecision 6; runTimeModifiable false;
functions
{{
    converged {{ type runTimeControl; libs (utilityFunctionObjects); satisfiedAction end;
        conditions {{ pt {{ type average; functionObject inlet; fields (weightedAreaAverage(total(p))); tolerance 1e-4;
            window 200; windowType exact; }} }} }}
    total {{ type pressure; libs (fieldFunctionObjects); mode total; p p; U U; rho rhoInf; rhoInf 1; pRef 0; writeControl writeTime; }}
{"".join(fos)}}}
""")
    out["system/fvSchemes"] = _f("dictionary", "fvSchemes", """
ddtSchemes { default steadyState; }
gradSchemes { default Gauss linear; grad(U) cellLimited Gauss linear 1; grad(k) cellLimited Gauss linear 1; grad(omega) cellLimited Gauss linear 1; }
divSchemes
{
    default none;
    div(phi,U) bounded Gauss linearUpwind grad(U);
    div(phi,k) bounded Gauss upwind;
    div(phi,omega) bounded Gauss upwind;
    div((nuEff*dev2(T(grad(U))))) Gauss linear;
}
laplacianSchemes { default Gauss linear limited corrected 0.5; }
interpolationSchemes { default linear; }
snGradSchemes { default limited corrected 0.5; }
wallDist { method meshWave; }
""")
    out["system/fvSolution"] = _f("dictionary", "fvSolution", """
solvers
{
    p { solver GAMG; smoother GaussSeidel; tolerance 1e-7; relTol 0.05; }
    "(U|k|omega)" { solver smoothSolver; smoother symGaussSeidel; tolerance 1e-8; relTol 0.1; }
}
SIMPLE { nNonOrthogonalCorrectors 1; consistent yes; residualControl { p 1e-5; U 1e-6; "(k|omega)" 1e-6; } }
relaxationFactors { equations { U 0.5; "(k|omega)" 0.5; } fields { p 0.7; } }
""")
    out["constant/transportProperties"] = _f("dictionary", "transportProperties", f"transportModel Newtonian; nu {NU};\n")
    out["constant/turbulenceProperties"] = _f("dictionary", "turbulenceProperties", "simulationType laminar;\n" if c.laminar else
                                              "simulationType RAS; RAS { RASModel kOmegaSST; turbulence on; printCoeffs off; }\n")
    q = c.q_m3s
    u_in = q / ((2 * half / 1000) ** 2)
    k_in = 1.5 * (0.05 * max(u_in, 0.1)) ** 2
    walls = f'"({"|".join(("frame", "rotor", "pipe") + tuple(extra) + (() if slip else ("background",)))})"'
    bf = {
        "U": ("volVectorField", "[0 1 -1 0 0 0 0]", "uniform (0 0 0)", {
            "inlet": f"type flowRateInletVelocity; volumetricFlowRate constant {q}; value uniform (0 0 0);",
            "outlet": "type inletOutlet; inletValue uniform (0 0 0); value uniform (0 0 0);",
            walls: "type noSlip;"}),
        "p": ("volScalarField", "[0 2 -2 0 0 0 0]", "uniform 0", {
            "inlet": "type zeroGradient;", "outlet": "type fixedValue; value uniform 0;", walls: "type zeroGradient;"}),
        "k": ("volScalarField", "[0 2 -2 0 0 0 0]", f"uniform {k_in}", {
            "inlet": f"type fixedValue; value uniform {k_in};",
            "outlet": f"type inletOutlet; inletValue uniform {k_in}; value uniform {k_in};",
            walls: f"type kqRWallFunction; value uniform {k_in};"}),
        "omega": ("volScalarField", "[0 0 -1 0 0 0 0]", "uniform 100", {
            "inlet": "type turbulentMixingLengthFrequencyInlet; mixingLength 0.005; value uniform 100;",
            "outlet": "type inletOutlet; inletValue uniform 100; value uniform 100;",
            walls: "type omegaWallFunction; value uniform 100;"}),
        "nut": ("volScalarField", "[0 2 -1 0 0 0 0]", "uniform 0", {
            "inlet": "type calculated; value uniform 0;", "outlet": "type calculated; value uniform 0;",
            walls: "type nutkWallFunction; value uniform 0;"}),
    }
    for name, (cls, dim, internal, patches) in bf.items():
        if slip:
            patches["background"] = "type slip;"
        body = "".join(f"    {p} {{ {v} }}\n" for p, v in patches.items())
        out[f"0.orig/{name}"] = _f(cls, name, f"dimensions {dim};\ninternalField {internal};\nboundaryField\n{{\n{body}}}\n")
    out["Allrun"] = f"""#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")"
export OMPI_ALLOW_RUN_AS_ROOT=1 OMPI_ALLOW_RUN_AS_ROOT_CONFIRM=1
run() {{ echo "== $1"; "$@" > "log.$1" 2>&1 || {{ tail -40 "log.$1"; exit 1; }}; }}
prun() {{ echo "== $1 (x{c.np})"; mpirun --oversubscribe -np {c.np} "$1" -parallel > "log.$1" 2>&1 || {{ tail -40 "log.$1"; exit 1; }}; }}
rm -rf 0 processor* postProcessing constant/polyMesh constant/extendedFeatureEdgeMesh log.*
run blockMesh
run surfaceFeatureExtract
run snappyHexMesh -overwrite
run transformPoints -scale 0.001
run topoSet
run checkMesh
cp -r 0.orig 0
run decomposePar
prun simpleFoam
echo "== done"
"""
    return out


def profile(case_dir: Path, n: int = 24) -> dict:
    """c_x and r c_theta across the annulus on the `fan` plane, in n radial bins (flux-weighted means)."""
    f = next((case_dir / "postProcessing" / "swirl").glob("*/U_fan.raw"))
    a = np.loadtxt(f, comments="#")
    y, z, u = a[:, 1], a[:, 2], a[:, 3:6]
    r = np.hypot(y, z)
    th = np.arctan2(z, y)
    ut = -u[:, 1] * np.sin(th) + u[:, 2] * np.cos(th)
    edges = np.linspace(0, R_OUT / 1000, n + 1)
    rows = []
    for a0, a1 in zip(edges, edges[1:]):
        m = (r >= a0) & (r < a1)
        if m.sum() < 5:
            continue
        rows.append({"r_m": float((a0 + a1) / 2), "cx_m_s": float(u[m, 0].mean()), "rct_m2_s": float((r[m] * ut[m]).mean())})
    return {"plane_x_mm": PLANE_X, "bins": rows}


def run(c: FanCase, out: Path, mem: str) -> dict:
    name = f"fan-{c.cfm:g}cfm-{c.rpm:g}rpm"
    case_dir = (out / name).resolve()
    if case_dir.exists():
        shutil.rmtree(case_dir)
    t0 = time.time()
    counts = write_stl(case_dir / "constant" / "triSurface")
    for rel, text in files(c).items():
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
    res = {
        "case": c.__dict__,
        "flow_cfm": q_out * 60 / 0.0283168,
        "dp_static_pa": RHO * (p_out - pt_in),  # the fan's static rise: outlet static less the plenum's total
        "dp_total_pa": RHO * (pt_out - pt_in),
        "fan_plane": swirl.plane(next((case_dir / "postProcessing" / "swirl").glob("*/U_fan.raw"))),
        "wall_s": round(time.time() - t0),
    }
    (case_dir / "results.json").write_text(json.dumps(res, indent=2))
    (case_dir / "profile.json").write_text(json.dumps(profile(case_dir), indent=2))
    return res


def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--cfm", type=float, default=4.2)
    ap.add_argument("--rpm", type=float, default=3000.0)
    ap.add_argument("--iterations", type=int, default=1500)
    ap.add_argument("--np", type=int, default=8)
    ap.add_argument("--mem", default="24G")
    ap.add_argument("--level", type=int, default=3, help="the rotor's surface refinement")
    ap.add_argument("--out", type=Path, default=Path("build/cfd"))
    a = ap.parse_args()
    c = FanCase(cfm=a.cfm, rpm=a.rpm, iterations=a.iterations, np=a.np, rotor_level=a.level)
    print(json.dumps(run(c, a.out, a.mem), indent=2))


if __name__ == "__main__":
    main()
