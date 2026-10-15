"""The real board (KiCad's STEP of hw/minimeater.kicad_pcb, every part on it) against the shells and the chassis: no part
of it intersects any of them, with the flap open and closed. Skips without KiCad (the Mac app, or the kicad container
under podman; see `mech/board3d.py`). Slow: about four minutes, nearly all of it the boolean intersections with the shells
and the duct, which the bounding boxes cannot rule out."""

from pathlib import Path

import pytest
from build123d import Pos

from minimeater.mech import assembly as A, board3d, shell as K

pytestmark = pytest.mark.skipif(not board3d.available(), reason="needs KiCad (Mac app, or the kicad container under podman)")

MOCKS = {"pcb", "pcb_keepout", "connectors", "plugs"}  # stand-ins for the board that the real one replaces


@pytest.fixture(scope="module")
def board():
    return board3d.placed(board3d.export_board(Path("build/mech")))


@pytest.fixture(scope="module")
def shells():
    return {n: f() for n, f in K.SHELLS.items()}


def test_the_real_board_clears_the_shells_and_the_chassis(board, shells):
    closed = {n: s for n, s in K.internals(0).items() if n not in MOCKS}
    hits = A.touching(board, {**closed, **shells})
    assert hits == []
    # the linkage moves; the rest of the chassis does not, and only these can reach the board's side of the box
    moving = ("horn", "flap", "crank", "coupler", "pin_crank", "pin_horn", "rod")
    open_ = {n: s for n, s in K.internals(90).items() if n in moving}
    assert A.touching(board, open_) == []


def test_the_check_sees_a_board_that_does_not_fit(board, shells):
    """Moved 6 mm into the pan, the board's parts hit it: the test above has teeth."""
    assert A.touching(Pos(0, 0, -6) * board, {"pan": shells["pan"]}, limit=1)
