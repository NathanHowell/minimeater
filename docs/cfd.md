# Duct CFD and acoustics

What the air does between the fan and the tube: the pressure the duct costs the fan (`.2.12`, `.2.13`), and whether the
flap can whistle (`.2.17`-`.2.19`). The code is `src/minimeater/mech/cfd/`; the geometry comes from the same build123d
parts as the print (`mech/parts.py`), so a change to the duct or the flap is a change to the case.

## Steady RANS: `just cfd <case>`

`uv run minimeater-cfd <case> [--flap DEG] [--cfm Q] [--rpm N]` builds the air volume, meshes it and solves it in
OpenFOAM v2606 (`opencfd/openfoam-run:2606`, arm64, in Apple's `container`; no build), and writes
`build/cfd/<case>/results.json`. Named cases: `open`, `open-swirl`, `flap-30`, `flap-45`, `flap-60`.

- **Geometry** (`geometry.py`): the fan's outlet annulus (5 mm of the frame's Ø58 bore around the Ø26.5 hub), the
  27 mm cone, the Ø32 bore with the flap and its rod at the case's angle, the 0.2 mm step into the tube at the socket's
  shoulder, and 12 diameters of the Ø31.6 tube. Patches: inlet, outlet, duct, tube, flap. The fan's blades and struts
  are not modelled; the inlet can swirl (solid-body rotation at `--rpm`).
- **Mesh**: snappyHexMesh, 2 mm background, 0.5 mm at the duct's walls, 0.25 mm on the flap, 1 mm through the bore and
  the first two tube diameters; 3 boundary layers (2 on the flap). About 0.75 M cells.
- **Solver**: simpleFoam (SIMPLEC), k-omega SST with wall functions; the fan's flow fixed into the annulus at 10 %
  turbulence; the outlet at 0 Pa. Up to 2000 iterations, 8 ranks: about 20 minutes on the Mac.
- **Loss** (`run.py`): static pressure on planes across the tube, one per diameter; a line through the last six (the
  developed flow) extrapolated back to the shoulder. Referenced to the tube's velocity head q:
  - `k_static` = (inlet static - line at the shoulder) / q: what the fan's (static) curve must supply for the duct,
    including the acceleration from the annulus into the tube. It replaces `Intake.k_box + k_entry` less the grille.
  - `k_total` = the total-pressure loss: what is dissipated.
  - The tube's own friction is in neither; the fitted line's slope is checked against Blasius.

### Results, 6 CFM (3.6 m/s in the tube)

| case | k_static | k_total | duct Δp static | tube f (Blasius 0.034) | note |
|---|---|---|---|---|---|
| open | 1.26 | 0.16 | 12.8 Pa | 0.031 | steady |
| flap 30° | 3.63 | 2.56 | 33.0 Pa | 0.050 | sheds: ±0.8 Pa |
| flap 45° | 12.2 | 11.2 | 99.8 Pa | 0.043 | the wake sheds: ±2.4 Pa over the last 500 iterations; the tube has not recovered by the outlet |

The model has carried k_box 2.0 + k_entry 0.5 = 2.5 for this path; the open duct is 1.26 without the grille. The duct
does not need a change for its loss. The 45° figure is inside the range published for butterfly valves (about 10-18). With the flap partly closed the
tube's friction reads high (0.043-0.050): the wake has not died out by the fitted planes, so those k carry some of the
tube's recovery; a longer tube (`tube_d`) is the fix when the numbers matter to better than ~10 %.

## Acoustics: `uv run minimeater-acoustics`

`acoustics.py`: below the bore's first cross mode (6.3 kHz) only plane waves travel, so the path is a chain of transfer
matrices: the open end behind the fan, the fan's annulus, the cone, the bore, the flap as a lumped flow resistance and
inertance, the bore, 100 mm of tube, the open end in the firebox, with Kirchhoff wall losses. A fluctuating force on the
flap is a pressure jump across it; the peaks of 1 / |Z_loop| are the modes it can drive, their Q what a feedback loop
must overcome. The flap's open area and loss law (`open_area_fraction`, `flap_k`) are fitted to the CFD at 45°.

![modes and shedding](cfd/acoustics.png)

- Modes at 846, 1455, 2140, 2916, 3656, 5018 Hz with the flap open (Q 25, 12, 14, 15, 14, 13). The first is the
  path's half wave. A closing flap damps them: by 65° the first mode's Q is under 8.
- The 2 mm edge's shedding (St 0.2 on the gap speed) sweeps through the whole ladder as the flap turns and the flow
  changes; the inclined plate's wake (St 0.15 on D sin θ) mostly stays below the first mode. Frequency coincidence alone
  cannot rule a whistle out: there is always a mode near the edge's band somewhere in the operating range. The most
  exposed is the first mode (Q ~25) at 15-40° and 4-8 CFM, and at 55-65° at 1-2 CFM.
- Whether the shedding actually feeds a mode is the LES (`.2.18`, on a Linux box) and Howe's energy balance with these
  mode shapes (`.2.19`); the flap's geometry is frozen for the combined JLCCNC + MJF order only after that.

## Not yet in it

- The grille and the cavity behind the fan (a hood change if they cost much).
- The fan's real exit: blades, struts, swirl (`open-swirl` is solid-body rotation, a first look).
- Partial angles' loss beyond 45° and the flap's torque on the servo (the flap patch's force and moment are on the mesh).
