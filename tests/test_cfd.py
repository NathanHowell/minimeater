"""The CFD case builds without the solver: the air volume is one solid at every flap angle with each patch present,
and the case files carry the flow and the patches (the solve itself is `just cfd <case>`)."""

import pytest

from minimeater.mech.cfd import geometry
from minimeater.mech.cfd.case import PATCHES, Case, files


@pytest.mark.parametrize("deg", [0, 45, 70])
def test_air_volume_has_every_patch(deg):
    air = geometry.fluid(deg, 4)
    assert len(air.solids()) == 1
    groups = geometry.patches(air, 4)
    assert set(groups) == set(PATCHES)
    assert all(groups.values())
    assert len(groups["inlet"]) == 1 and len(groups["outlet"]) == 1


def test_inside_point_is_in_the_air():
    assert geometry.fluid(0, 4).is_inside(geometry.inside_point(4))


def test_case_files():
    c = Case(cfm=3.0)
    f = files(c)
    assert f"flowRate constant {c.q_m3s};" in f["0.orig/U"]
    for p in PATCHES:
        assert f'"{p}.stl"' in f["system/snappyHexMeshDict"]
    assert "transformPoints -scale 0.001" in f["Allrun"]


def test_duct_modes():
    """The first mode is the open-open pipe's half wave over the path with its end corrections; a closing flap damps
    every mode (its flow resistance)."""
    from minimeater.mech.cfd import acoustics as A

    q = 4 * A.CFM
    open_ = A.modes(0, q)
    assert 750 < open_[0].f < 950
    assert open_[0].q > 10
    shut = A.modes(65, q)
    assert shut[0].q < open_[0].q / 2


def test_flap_loss_law_matches_the_cfd():
    from minimeater.mech.cfd import acoustics as A

    assert abs(A.flap_k(45) - 11.2) < 0.5  # build/cfd/flap-45, k_total
    assert A.flap_k(0) < 0.2
    for a, k in A.CFD_K.items():
        assert abs(A.flap_k(a) - k) < 0.01 * k
    assert all(A.flap_k(a) < A.flap_k(a + 5) for a in range(0, 80, 5))
