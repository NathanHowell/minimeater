# The enclosure (src/minimeater/mech, build123d): exports, renders and checks. `just mech::cfd::...` is its airflow CFD.
set working-directory := ".."

mod cfd "mech/cfd.just"

# STEP and STL of every printed and machined part in its print pose, with drawings and order zips (build/mech).
export *args:
    uv run mech-export {{ args }}

# The review renders (VTK): docs/mech/*.png by default.
render dir="docs/mech":
    uv run mech-render {{ dir }}

# The board outline, holes and connector positions the enclosure assumes (envelope.json).
envelope *args:
    uv run mech-envelope {{ args }}

# Every spot under 1.0 mm thick on the print STLs (docs/jlc3dp.md).
thinwall *args:
    uv run mech-thinwall {{ args }}

# The README's exploded-view loop, docs/mech/explode.webp (Blender Cycles; needs img2webp). `--encode` re-encodes the frames.
explode *args:
    uv run --group scene mech-explode {{ args }}

# build/scene/scene.blend; `--still N` PNGs, `--video` the mp4, `--preview` quick (640 x 360, 8 samples).
# Needs `just mech::cfd::pathlines` and `just mech::export` first.
# The air-molecule film (docs/mech.md, ADR 0007): the scene, stills and the video.
scene *args:
    uv run --group scene mech-scene {{ args }}
