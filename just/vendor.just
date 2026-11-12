# The vendor 3D models the repository does not redistribute (LICENSE): the board's EasyEDA models, Noctua's fan and
# Hitec's servo. Everything builds and tests without them; a few fit checks are skipped.
set working-directory := ".."

# Fetch the EasyEDA models and install hand-downloaded ones (`--from FILE`, a STEP or the vendor's zip), each checked by hash.
fetch *args:
    uv run --group models scripts/fetch_models.py {{ args }}

# Which models are present and match their hash.
check:
    uv run --group models scripts/fetch_models.py --check
