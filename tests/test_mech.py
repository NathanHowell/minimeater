"""The enclosure builds, its parts fit with no interference, and they print without support."""

import pytest

from minimeat.mech import analysis, assembly as A
from minimeat.mech.export import export
from minimeat.mech.parts import PARTS
from minimeat.mech.printing import POSES, overhangs, print_pose


@pytest.fixture(scope="module")
def printed():
    return {n: f() for n, f in PARTS.items()}


@pytest.mark.parametrize("name", list(PARTS))
def test_part_is_one_valid_solid(printed, name):
    s = printed[name]
    assert s.is_valid
    assert len(s.solids()) == 1
    assert s.volume > 100


@pytest.mark.parametrize("deg", [0, 30, 60, 90])
def test_no_interference_at_any_flap_angle(deg):
    """Every printed part and bought part, with the flap (and the link and rod on its axis) at `deg`."""
    allp = {**A.printed(deg), **A.bought(deg)}
    hits = [h for h in A.interferences(allp) if set(h[:2]) != {"duct", "oring"}]  # the ring is squeezed by design
    assert hits == []


def test_oring_seals_the_joint():
    """The ring is pinched between the tube and the groove bottom: 10-30 % squeeze, under 5 % stretch."""
    squeeze, stretch = analysis.oring_squeeze()
    assert 0.10 <= squeeze <= 0.30
    assert 0 <= stretch <= 0.05
    # the modelled ring lying on the tube overlaps the groove wall by that squeeze, and sits clear of the tube
    ring = A.bought()["oring"]
    assert A.overlap(ring, A.printed()["duct"]) > 0
    assert A.overlap(ring, A.bought()["tube"]) < 0.5


def test_tube_seats_on_the_shoulder_without_touching_the_duct():
    assert A.overlap(A.bought()["tube"], A.printed()["duct"]) < 0.5


def test_closed_flap_leak_is_inside_the_budget():
    assert analysis.flap_leak_cm2() <= 0.3  # docs/modeling: closed leak <= 0.3 cm^2


def test_open_flap_does_not_choke_the_bore():
    assert analysis.flap_blockage_open() < 0.2  # the rib is the loss to shave in CFD (.2.13)


def test_box_fits_a_hand_sized_envelope():
    sizes = analysis.sizes()
    assert max(s[0] for s in sizes.values()) < 115  # length along the tube
    assert max(s[1] for s in sizes.values()) < 70  # width


# Overhang inventory: faces steeper than 45 degrees off vertical, in the print pose. What is left must be
# short bridges and small holes. A new patch or a bigger total means a change that needs a printing look.
OVERHANG_AREA_MM2 = {"duct": 450, "flap": 90, "link": 0, "grille": 0, "tray": 160, "lid": 0}


@pytest.mark.parametrize("name", list(POSES))
def test_prints_without_support(name):
    patches = overhangs(print_pose(name))
    total = sum(p["area"] for p in patches)
    assert total <= OVERHANG_AREA_MM2[name], patches
    assert all(p["area"] <= 250 for p in patches), patches  # the largest is the O-ring groove's 2 mm ceiling


def test_export_writes_step_and_stl(tmp_path):
    export(tmp_path)
    for n in PARTS:
        assert (tmp_path / f"{n}.step").stat().st_size > 1000
        assert (tmp_path / f"{n}.stl").stat().st_size > 1000
    assert (tmp_path / "assembly.step").exists()
