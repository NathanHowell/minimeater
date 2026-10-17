# ADR 0007: Scene toolchain (Blender `bpy`, Cycles)

- Status: accepted (2026-10-17)
- Beads: `minimeat-cp2.1`; the choice for the raytraced animation epic `minimeat-cp2`

## Context

The animation follows one air molecule through the assembly (grille, fan, duct, flap, socket) with its path and speed taken
from the OpenFOAM runs. It needs a path tracer, glTF import (`assembly.glb` from `mech-export`), curves and keyframes for the
motion, and video out, all from one `uv run` command.

## Decision

**Blender as the `bpy` pip module (5.2.2), Cycles for rendering**, in a uv dependency group:

```bash
uv add --group scene bpy          # pyproject.toml: [dependency-groups] scene
uv run --group scene mech-scene   # run anything that imports bpy
```

- `bpy` 5.2 ships `cp313` wheels (`==3.13.*`) for macOS arm64 and Linux x86_64, the project's own Python pin, so no separate
  Blender install or interpreter is needed. It resolves beside build123d, numpy 2.5 and PyVista without changes to them.
- It is a group, not a default dependency: the wheel is 233 MB (383 MB on Linux) and only the scene scripts import it.
  `uv run` without `--group scene` leaves it out, as it does for any other command.
- Checked here (M1 Max): `import bpy` works under `python -I`, Cycles can be selected, the Metal device is found (CPU is the
  fallback and the only option on the Linux box), the glTF importer is present and a render writes a PNG. ffmpeg video output
  is built in (`media_type = VIDEO`).
- PyVista/VTK stays what it is for the CFD side: sampling fields and integrating pathlines. The scene gets the pathlines as
  plain arrays and builds Blender curves from them.

## Alternatives

- **Mitsuba 3**: physically based and pure Python, but no animation, camera path or glTF tooling; we would build all three.
- **PyVista/VTK (OSPRay)**: already a dependency, but weak on materials, lighting and camera work.
- **Manim**: 2D first and not a ray tracer.
- **`blender --background --python`** from a Homebrew install: works (5.2.2 here, Python 3.13), but pins the contributor's
  machine, not the lockfile. Kept as the fallback if a `bpy` wheel stops matching the project's Python.

## Consequences

- The Python pin and `bpy`'s are coupled: a `bpy` release for another Python minor would hold the project's `requires-python`
  or force the fallback.
- The first Metal render compiles kernels (over a minute); later renders reuse the cache.
- Windows is not a target of the project, so its wheels do not matter.
