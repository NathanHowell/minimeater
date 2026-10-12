"""The chassis builds, its parts fit with no interference through the flap's travel, and they print without support."""

import pytest

from minimeat.mech import analysis, assembly as A
from minimeat.mech import geom as G
from minimeat.mech.export import export
from minimeat.mech.params import P
from minimeat.mech.parts import PARTS
from minimeat.mech.printing import overhangs, print_pose


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
    """Every chassis part and bought part, with the flap, crank, horn, pins and coupler at `deg`."""
    allp = {**A.printed(deg), **A.bought(deg)}
    hits = [h for h in A.interferences(allp) if set(h[:2]) != {"duct", "oring"}]  # the ring is squeezed by design
    assert hits == []


def test_linkage_is_a_parallelogram():
    """Equal arms, the coupler as long as the shaft spacing, the horn level with the crank, under the roof."""
    L = P.linkage
    assert G.SERVO_X == G.g.flap_x
    assert abs(A.coupler_shift(90).position.Y + L.arm_r) < 1e-9 and abs(A.coupler_shift(90).position.X - L.arm_r) < 1e-9
    assert G.CRANK_Z0 > G.g.duct_r  # the linkage plane is over the duct's crown
    assert G.COUPLER_Z1 + L.pin_head_h < P.shell.roof_in
    assert G.SERVO_Z0 > G.PCB_Z1 + P.pcb.top_h + 2.0  # the servo clears the board's keep-out


def test_board_lies_under_the_straight_duct():
    """The 44 mm board sits beside the r = 18.2 straight section (one mm into the cone and the flare), with its
    keep-out one mm under the duct and the cone no steeper than 26 degrees."""
    assert G.g.straight_x0 <= G.g.board_x0 + P.layout.straight_over + 1e-9
    assert G.g.board_x1 <= G.g.socket_x0 - 4.0
    assert G.PCB_Z1 + P.pcb.top_h <= -G.g.duct_r - 1.0 + 1e-9
    assert analysis.cone_half_angle() <= 26.1


def test_oring_seals_the_joint():
    """The ring is pinched between the tube and the groove bottom: 10-30 % squeeze, under 5 % stretch."""
    squeeze, stretch = analysis.oring_squeeze()
    assert 0.10 <= squeeze <= 0.30
    assert 0 <= stretch <= 0.05
    ring = A.bought()["oring"]
    assert A.overlap(ring, A.printed()["duct"]) > 0
    assert A.overlap(ring, A.bought()["tube"]) < 0.5


def test_tube_seats_on_the_shoulder_without_touching_the_duct():
    assert A.overlap(A.bought()["tube"], A.printed()["duct"]) < 0.5


def test_closed_flap_leak_is_inside_the_budget():
    assert analysis.flap_leak_cm2() <= 0.3  # docs/modeling: closed leak <= 0.3 cm^2


def test_flap_fit_needs_the_bore_reamed():
    """The disc is machined, the duct's bore is printed (JLC: +-0.3 mm, +-0.15 radial): as printed the gap can close (the
    flap jams) or open to a leak over budget; reamed at the flap station (+-0.05) it stays inside both."""
    dm = P.damper
    lo, hi = analysis.flap_gap_range(dm.print_bore_tol)
    assert lo < 0 and analysis.flap_leak_worst_cm2(dm.print_bore_tol) > 0.3
    lo, hi = analysis.flap_gap_range(dm.ream_tol)
    assert lo > 0.03 and analysis.flap_leak_worst_cm2(dm.ream_tol) <= 0.3


def test_open_flap_does_not_choke_the_bore():
    assert analysis.flap_blockage_open() < 0.2  # the rib is the loss to shave in CFD (.2.13)


# Overhang inventory: faces steeper than 45 degrees off vertical, in the print pose. What is left must be
# short bridges and small holes. A new patch or a bigger total means a change that needs a printing look.
OVERHANG_AREA_MM2 = {"duct": 340}


@pytest.mark.parametrize("name", list(PARTS))
def test_prints_without_support(name):
    patches = overhangs(print_pose(name))
    total = sum(p["area"] for p in patches)
    assert total <= OVERHANG_AREA_MM2[name], patches
    assert all(p["area"] <= 230 for p in patches), patches  # the largest is the O-ring groove's 2 mm ceiling


def test_export_writes_step_and_stl(tmp_path):
    export(tmp_path)
    for n in list(PARTS) + ["hood", "pan"]:
        assert (tmp_path / f"{n}.step").stat().st_size > 1000
        assert (tmp_path / f"{n}.stl").stat().st_size > 1000
    assert (tmp_path / "assembly.step").exists()
    assert (tmp_path / "envelope.json").exists()


def test_made_parts_are_valid_and_the_bar_spans_the_pins():
    """The crank (a clamp hub with a radial set screw hole and a tapped pin hole) and the coupler are one valid
    solid each; the bar's holes are as far apart as the shaft and the servo's output axis."""
    from minimeat.mech.parts import MACHINED
    for build in MACHINED.values():
        s = build()
        assert s.is_valid and len(s.solids()) == 1
    bar = MACHINED["coupler"]()
    assert abs(bar.bounding_box().size.Y - (G.SERVO_Y + P.linkage.coupler_w)) < 1e-6  # centres SERVO_Y apart, round ends
