"""An OpenFOAM case for the duct: snappyHexMesh around the patch STLs (geometry.py), steady incompressible RANS
(simpleFoam, k-omega SST), the fan's flow into the inlet annulus, the outlet at 0 Pa.

The mesh is built in mm (the STLs' units) and scaled to metres before the solver runs. Kinematic pressure (p / rho)
throughout; run.py multiplies by the density. Measurements, every iteration: the flow-weighted static and total
pressure on the inlet and the outlet, the flow through the outlet, and the area-averaged static pressure on planes
across the tube (for the developed pressure line).
"""

from dataclasses import dataclass
from pathlib import Path

from .geometry import Stations, inside_point, stations

HEADER = """FoamFile {{ version 2.0; format ascii; class {cls}; object {obj}; }}
"""
NU = 1.5e-5  # m^2/s, air at 20 C
BASE = 2.0  # mm, the background cells
PATCHES = ("inlet", "outlet", "duct", "tube", "flap")
WALLS = '"(duct|tube|flap|background)"'  # every wall patch, as a dictionary regex


@dataclass(frozen=True)
class Case:
    flap_deg: float = 0.0
    cfm: float = 6.0
    rpm: float = 0.0  # swirl at the inlet: solid-body rotation at this speed (the fan's exit swirl; 0 = none)
    tube_d: float = 12.0  # tube length after the shoulder, in tube diameters
    iterations: int = 2000
    np: int = 8
    base: float = BASE  # mm, the background cells; the refinement levels halve it
    flap_level: int = 3  # the flap's surface cells: base / 2^level
    zone: tuple = ()  # (x0, x1, level) mm: a cylinder round the bore refined to this level (the LES's flap region); () = the one wake cylinder

    @property
    def q_m3s(self) -> float:
        return self.cfm * 0.0283168 / 60.0


def tube_planes(s: Stations, tube_d: float) -> list[float]:
    """x (mm) of the sampling planes across the tube: one per diameter, from the shoulder to a diameter short of the outlet."""
    d = 2 * s.r_tube
    return [s.x_tube + k * d for k in range(1, int(tube_d))]


def _f(cls: str, obj: str, body: str) -> str:
    return HEADER.format(cls=cls, obj=obj) + body


def files(c: Case) -> dict[str, str]:
    s = stations(c.tube_d)
    q = c.q_m3s
    loc = inside_point(c.tube_d)
    pad = 2.7  # the background box's faces stay off the inlet and outlet planes
    r = s.r_fan + 2.0
    x0, x1 = s.x_in - pad, s.x_out + pad
    nx, ny = round((x1 - x0) / c.base), round(2 * r / c.base)
    out = {}
    out["system/blockMeshDict"] = _f("dictionary", "blockMeshDict", f"""
vertices ( ({x0} {-r} {-r}) ({x1} {-r} {-r}) ({x1} {r} {-r}) ({x0} {r} {-r})
           ({x0} {-r} {r}) ({x1} {-r} {r}) ({x1} {r} {r}) ({x0} {r} {r}) );
blocks ( hex (0 1 2 3 4 5 6 7) ({nx} {ny} {ny}) simpleGrading (1 1 1) );
boundary ( background {{ type wall; faces ( (0 3 2 1) (4 5 6 7) (0 4 7 3) (1 2 6 5) (0 1 5 4) (3 7 6 2) ); }} );
""")
    out["system/surfaceFeatureExtractDict"] = _f("dictionary", "surfaceFeatureExtractDict", "".join(
        f"{p}.stl {{ extractionMethod extractFromSurface; includedAngle 150; subsetFeatures {{ nonManifoldEdges no; openEdges yes; }} writeObj no; }}\n"
        for p in PATCHES))
    levels = {"inlet": 2, "outlet": 1, "duct": 2, "tube": 1, "flap": c.flap_level}
    geo = "".join(f"    {p} {{ type triSurfaceMesh; file \"{p}.stl\"; }}\n" for p in PATCHES)
    wake = f"    wake {{ type searchableCylinder; point1 ({s.x_bore} 0 0); point2 ({s.x_tube + 2 * s.r_tube} 0 0); radius {s.r_bore + 0.5}; }}\n"
    regions = "wake { mode inside; levels ((1E15 1)); }"
    if c.zone:
        z0, z1, zl = c.zone
        wake += f"    zone {{ type searchableCylinder; point1 ({z0} 0 0); point2 ({z1} 0 0); radius {s.r_bore + 0.5}; }}\n"
        regions += f" zone {{ mode inside; levels ((1E15 {zl})); }}"
    feats = "".join(f"        {{ file \"{p}.eMesh\"; level {levels[p]}; }}\n" for p in PATCHES)
    surfs = "".join(f"        {p} {{ level ({levels[p]} {levels[p]}); patchInfo {{ type {'patch' if p in ('inlet', 'outlet') else 'wall'}; }} }}\n"
                    for p in PATCHES)
    out["system/snappyHexMeshDict"] = _f("dictionary", "snappyHexMeshDict", f"""
castellatedMesh true; snap true; addLayers true;
geometry
{{
{geo}{wake}}}
castellatedMeshControls
{{
    maxLocalCells 4000000; maxGlobalCells 20000000; minRefinementCells 0; maxLoadUnbalance 0.10;
    nCellsBetweenLevels 3; resolveFeatureAngle 30; allowFreeStandingZoneFaces true;
    features
    (
{feats}    );
    refinementSurfaces
    {{
{surfs}    }}
    refinementRegions {{ {regions} }}
    locationInMesh ({loc.X} {loc.Y} {loc.Z});
}}
snapControls {{ nSmoothPatch 3; tolerance 2.0; nSolveIter 50; nRelaxIter 5; nFeatureSnapIter 10;
    implicitFeatureSnap false; explicitFeatureSnap true; multiRegionFeatureSnap false; }}
addLayersControls
{{
    relativeSizes true;
    layers {{ "(duct|tube)" {{ nSurfaceLayers 3; }} flap {{ nSurfaceLayers 2; }} }}
    expansionRatio 1.2; finalLayerThickness 0.4; minThickness 0.1; nGrow 0; featureAngle 130;
    slipFeatureAngle 30; nRelaxIter 3; nSmoothSurfaceNormals 1; nSmoothNormals 3; nSmoothThickness 10;
    maxFaceThicknessRatio 0.5; maxThicknessToMedialRatio 0.3; minMedialAxisAngle 90; nBufferCellsNoExtrude 0;
    nLayerIter 50;
}}
meshQualityControls
{{
    maxNonOrtho 65; maxBoundarySkewness 20; maxInternalSkewness 4; maxConcave 80; minVol 1e-13;
    minTetQuality 1e-15; minArea -1; minTwist 0.02; minDeterminant 0.001; minFaceWeight 0.05; minVolRatio 0.01;
    minTriangleTwist -1; nSmoothScale 4; errorReduction 0.75;
    relaxed {{ maxNonOrtho 75; }}
}}
mergeTolerance 1e-6;
""")
    out["system/decomposeParDict"] = _f("dictionary", "decomposeParDict", f"numberOfSubdomains {c.np}; method scotch;\n")
    planes = tube_planes(s, c.tube_d)
    fos = []
    for p in ("inlet", "outlet"):
        fos.append(f"""    {p} {{ type surfaceFieldValue; libs (fieldFunctionObjects); regionType patch; name {p};
        operation weightedAreaAverage; weightField phi; fields (p total(p)); writeFields false; log false; }}
""")
    fos.append("""    flow { type surfaceFieldValue; libs (fieldFunctionObjects); regionType patch; name outlet;
        operation sum; fields (phi); writeFields false; log false; }
""")
    for i, x in enumerate(planes):
        fos.append(f"""    plane{i} {{ type surfaceFieldValue; libs (fieldFunctionObjects); regionType sampledSurface; name plane{i};
        sampledSurfaceDict {{ type cuttingPlane; planeType pointAndNormal; pointAndNormalDict {{ point ({x / 1000} 0 0); normal (1 0 0); }}
            interpolate false; }}
        operation areaAverage; fields (p); writeFields false; log false; }}
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
    total {{ type pressure; libs (fieldFunctionObjects); mode total; p p; U U; rho rhoInf; rhoInf 1; pRef 0; }}
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
SIMPLE
{
    nNonOrthogonalCorrectors 1; consistent yes;
    residualControl { p 2e-5; U 1e-6; "(k|omega)" 1e-6; }
}
relaxationFactors { equations { U 0.7; "(k|omega)" 0.6; } fields { p 0.9; } }
""")
    out["constant/transportProperties"] = _f("dictionary", "transportProperties", f"transportModel Newtonian; nu {NU};\n")
    out["constant/turbulenceProperties"] = _f("dictionary", "turbulenceProperties",
                                              "simulationType RAS; RAS { RASModel kOmegaSST; turbulence on; printCoeffs off; }\n")
    # the inlet: the fan's flow into the annulus, 10 % intensity, the blade passage as the length scale
    u_in = q / (3.14159265 * ((s.r_fan / 1000) ** 2 - (s.r_hub / 1000) ** 2))
    k_in = 1.5 * (0.1 * u_in) ** 2
    bf = {
        "U": ("volVectorField", "[0 1 -1 0 0 0 0]", "uniform (0 0 0)", {
            "inlet": f"type swirlFlowRateInletVelocity; flowRate constant {q}; rpm constant {c.rpm}; origin (0 0 0); axis (1 0 0); value uniform (0 0 0);",
            "outlet": "type inletOutlet; inletValue uniform (0 0 0); value uniform (0 0 0);",
            WALLS: "type noSlip;"}),
        "p": ("volScalarField", "[0 2 -2 0 0 0 0]", "uniform 0", {
            "inlet": "type zeroGradient;", "outlet": "type fixedValue; value uniform 0;",
            WALLS: "type zeroGradient;"}),
        "k": ("volScalarField", "[0 2 -2 0 0 0 0]", f"uniform {k_in}", {
            "inlet": f"type turbulentIntensityKineticEnergyInlet; intensity 0.1; value uniform {k_in};",
            "outlet": f"type inletOutlet; inletValue uniform {k_in}; value uniform {k_in};",
            WALLS: f"type kqRWallFunction; value uniform {k_in};"}),
        "omega": ("volScalarField", "[0 0 -1 0 0 0 0]", "uniform 100", {
            "inlet": "type turbulentMixingLengthFrequencyInlet; mixingLength 0.003; value uniform 100;",
            "outlet": "type inletOutlet; inletValue uniform 100; value uniform 100;",
            WALLS: "type omegaWallFunction; value uniform 100;"}),
        "nut": ("volScalarField", "[0 2 -1 0 0 0 0]", "uniform 0", {
            "inlet": "type calculated; value uniform 0;", "outlet": "type calculated; value uniform 0;",
            WALLS: "type nutkWallFunction; value uniform 0;"}),
    }
    for name, (cls, dim, internal, patches) in bf.items():
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
run checkMesh
cp -r 0.orig 0
run decomposePar
prun simpleFoam
echo "== done"
"""
    return out


def write(case_dir: Path, c: Case) -> None:
    for rel, text in files(c).items():
        p = case_dir / rel
        p.parent.mkdir(parents=True, exist_ok=True)
        p.write_text(text)
    (case_dir / "Allrun").chmod(0o755)
