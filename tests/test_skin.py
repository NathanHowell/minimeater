"""The exterior shells build, clear the internals, keep the envelope and print without support."""

import pytest

from minimeat.mech import assembly as A, skin as K
from minimeat.mech.params import P
from minimeat.mech.printing import overhangs


@pytest.fixture(scope="module")
def shells():
    return {n: f() for n, f in K.SKIN.items()}


@pytest.mark.parametrize("name", list(K.SKIN))
def test_shell_is_one_valid_solid(shells, name):
    s = shells[name]
    assert s.is_valid
    assert len(s.solids()) == 1
    assert s.volume > 10000


def test_shells_meet_on_the_parting_plane_without_overlap(shells):
    assert A.overlap(shells["hood"], shells["pan"]) < 0.5
    assert abs(shells["hood"].bounding_box().min.Z - P.skin.z_part) < 0.01
    assert shells["pan"].bounding_box().max.Z <= P.skin.z_part + P.skin.lip_h + 0.01  # only the lip and columns rise


@pytest.mark.parametrize("deg", [0, 90])
def test_shells_clear_the_internals(shells, deg):
    """Every v0 part but the lid, the bought parts and the board's keep-out, with the flap open and closed."""
    hits = A.interferences({**K.internals(deg), **shells})
    assert [h for h in hits if h[0] in shells or h[1] in shells] == []


def test_envelope():
    skin, with_nose = K.envelope()
    assert skin == (103.0, 76.0, 96.8)  # x, y, z: the rear plate to the front face, the width, the belly to the top
    assert with_nose == (112.0, 76.0, 96.8)  # plus the exposed socket


def test_grille_breathes():
    assert K.grille_open_area() >= 1500  # mm^2 in the rear plate; the inner grille behind it has 1294


def test_rear_openings_line_up_with_the_tray():
    """The jack and USB-C openings in the rear plate are the tray's, 1 mm bigger, on the same axes."""
    tray = A.printed()["tray"]
    hood = K.hood()
    for jy in P.pcb.jack_ys:
        probe = K.xcyl(P.pcb.jack_hole_d / 2 - 0.1, K.S.x_rear - 1, K.X_SLAB + 3, jy, K.PCB_Z1 + P.pcb.jack_h / 2)
        assert A.overlap(probe, hood) < 0.5 and A.overlap(probe, tray) < 0.5


# Overhang inventory in the print poses (hood top down, pan on its rear plate): what is left is the ceilings of
# the USB-C opening and the four jack holes, the 0.5 mm label step, and inside the pan the front wall's fillets.
OVERHANG_AREA_MM2 = {"hood": 300, "pan": 1000}


@pytest.mark.parametrize("name", list(K.SKIN))
def test_prints_without_support(name):
    patches = overhangs(K.print_pose(name))
    total = sum(p["area"] for p in patches)
    assert total <= OVERHANG_AREA_MM2[name], patches
    assert all(p["area"] <= 200 for p in patches), patches  # the largest is the pan's front-bottom fillet, inside
