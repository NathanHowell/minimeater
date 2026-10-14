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
| flap 60° | 57.4 | 56.4 | 454 Pa | — | sheds: ±5.6 Pa; far past the fan's 21 Pa, so 6 CFM never happens here (the k still holds) |
| open, swirl 2000 rpm | 8.3 | 8.7 | 70.9 Pa | — | solid-body rotation at the rotor's speed: an upper bound, not the fan |

The swirl case is a warning, not a number: if the fan's exit swirl were anything like solid-body rotation the cone
(which spins it up as it narrows) and the bore would cost several times the straight-flow loss. The real exit swirl of
the NF-A6x25 at these flows is the input that is missing (bake-off rig, `.2.11`, or a fan model); stator vanes in the
cone are the remedy if it is large (`.2.13`).

The model has carried k_box 2.0 + k_entry 0.5 = 2.5 for this path; the open duct is 1.26 without the grille. The duct
does not need a change for its loss. The 45° figure is inside the range published for butterfly valves (about 10-18). With the flap partly closed the
tube's friction reads high (0.043-0.050): the wake has not died out by the fitted planes, so those k carry some of the
tube's recovery; a longer tube (`tube_d`) is the fix when the numbers matter to better than ~10 %.

## Acoustics: `uv run minimeater-acoustics`

`acoustics.py`: below the bore's first cross mode (6.3 kHz) only plane waves travel, so the path is a chain of transfer
matrices: the open end behind the fan, the fan's annulus, the cone, the bore, the flap as a lumped flow resistance and
inertance, the bore, 100 mm of tube, the open end in the firebox, with Kirchhoff wall losses. A fluctuating force on the
flap is a pressure jump across it; the peaks of 1 / |Z_loop| are the modes it can drive, their Q what a feedback loop
must overcome. The flap's loss law (`open_area_fraction`, `flap_k`) is corrected to the CFD at 30°, 45° and 60°.

![modes and shedding](cfd/acoustics.png)

- Modes at 846, 1455, 2140, 2916, 3656, 5018 Hz with the flap open (Q 25, 12, 14, 15, 14, 13). The first is the
  path's half wave. A closing flap damps them: by 65° the first mode's Q is under 8.
- The 2 mm edge's shedding (St 0.2 on the gap speed) sweeps through the whole ladder as the flap turns and the flow
  changes; the inclined plate's wake (St 0.15 on D sin θ) mostly stays below the first mode. Frequency coincidence alone
  cannot rule a whistle out: there is always a mode near the edge's band somewhere across all angles and flows. The most
  exposed is the first mode (Q ~25) at 15-40° and 4-8 CFM, and at 55-65° at 1-2 CFM: points the firmware does not
  visit (next section).
- Whether the shedding actually feeds a mode is the LES (`.2.18`, on a Linux box) and Howe's energy balance with these
  mode shapes (`.2.19`); the flap's geometry is frozen for the combined JLCCNC + MJF order only after that.

## Where the firmware runs it: `uv run minimeater-operating`

![operating path](cfd/operating.png)

`operating.py` walks the firmware's airflow map (`control.AirflowMap`, ported to `minimeater-core::model::airflow`) and
marks the hold point `FeedForward` gives for each pit temperature and ambient (NF-A6x25, top vent 1/4). The map is
damper first: the flap opens with the fan stopped, and the fan only runs once the flap is fully open (below 600 rpm it
cycles between off and 600). So the fan never blows through a partly closed flap, and light-up (the black line: a cold
pit with no draft) is the fan alone with the flap open, up to about 4.1 CFM.

| pit F | ambient F | hold CFM | flap from closed | fan | flap sees (from open, CFM) | shedding near a mode |
|---|---|---|---|---|---|---|
| 225 | 40 | 0.70 | 57 | off | 33 deg 0.70 | none |
| 275 | 40 | 0.95 | 65 | off | 25 deg 0.95 | none |
| 350 | 40 | 1.36 | 82 | off | 8 deg 1.36 | none |
| 450 | 40 | 2.00 | 90 | 1006 rpm | 0 deg 2.00 | none |
| 225 | 70 | 0.62 | 56 | off | 34 deg 0.62 | none |
| 275 | 70 | 0.88 | 65 | off | 25 deg 0.88 | none |
| 350 | 70 | 1.30 | 82 | off | 8 deg 1.30 | none |
| 450 | 70 | 1.96 | 90 | 984 rpm | 0 deg 1.96 | none |
| 225 | 95 | 0.55 | 54 | off | 36 deg 0.55 | none |
| 275 | 95 | 0.81 | 64 | off | 26 deg 0.81 | none |
| 350 | 95 | 1.25 | 83 | off | 7 deg 1.25 | none |
| 450 | 95 | 1.92 | 90 | 948 rpm | 0 deg 1.92 | none |

- Low-and-slow holds on natural draft alone with the flap 25-36° from open at 0.5-1 CFM: gap speeds of 1-2 m/s.
- With the flap open the edge's shedding is about 76 Hz per CFM (300 Hz at the fan's 4.1 CFM), under the first mode
  (846 Hz); reaching it would take about 11 CFM, past what the fan can push through this path.
- Along every path in the chart, light-up included, no shedding estimate comes within 15 % of a mode. The partly
  closed, high-flow points that crossed the modes in the acoustics chart do not occur under this map.
- That rests on the damper-first map: blending the flap and the fan (the fan running through a partly closed flap)
  would put the operating points back where the chart above shows crossings. Keep it, or re-run this check.
- Not covered: the fan's own tones. Its blade-passing frequency (rpm / 60 × the blade count) and harmonics can drive the
  duct's modes directly, flap or no flap; the 846 Hz mode is the one to watch. That is the fan's noise, not the flap's,
  and belongs with the bake-off (`.2.11`).

## Not yet in it

- The grille and the cavity behind the fan (a hood change if they cost much).
- The fan's real exit: blades, struts, swirl (`open-swirl` is solid-body rotation, a first look).
- The flap's torque on the servo (the flap patch's force and moment are on the mesh).
