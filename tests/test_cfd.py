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


def test_flap_variants_are_one_solid_and_leave_the_design_alone():
    from dataclasses import replace

    from minimeater.mech.cfd import variants
    from minimeater.mech.params import P
    from minimeater.mech.parts import flap

    design = flap().volume
    assert flap(variants.damper("round")).volume == pytest.approx(design)
    for v in variants.VARIANTS:
        f = flap(variants.damper(v))
        assert len(f.solids()) == 1, v
    assert flap(variants.damper("serrated")).volume < design  # the notches took material
    assert variants.leak_cm2("serrated") > 0.3 > variants.leak_cm2("round")  # and they leak when shut
    assert P.damper == replace(P.damper)  # the shared parameters were not touched


def test_les_case_files():
    from minimeater.mech.cfd import les

    c = les.make(30, 6.0, res="smoke", np=4)
    pts = les.probes(c)
    assert any(n.startswith("edge") for n, _ in pts) and any(n.startswith("wall") for n, _ in pts)
    f = les.files(c, pts)
    assert "LESModel WALE" in f["constant/turbulenceProperties"]
    assert "patches (flap)" in f["system/controlDict"] and "adjustTimeStep yes" in f["system/controlDict"]
    assert f["system/controlDict"].count("volFieldValue") == f["system/topoSetDict"].count("cellZoneSet")
    assert "zone { type searchableCylinder" in f["system/snappyHexMeshDict"]
    assert "mpirun --oversubscribe -np 4" in f["Allrun"]
    assert "run topoSet" not in les.files(les.make(30, 6.0, res="smoke", howe=False), pts)["Allrun"]


def test_les_sizes_itself_from_the_flow():
    """A slow, mostly open flap needs a long spin-up and averaging window but a big step; a fast jet the reverse."""
    from minimeater.mech.cfd import les

    slow, fast = les.make(33, 0.7), les.make(45, 6.0)
    assert slow.t_end > 2 * fast.t_end and slow.dt > 5 * fast.dt
    assert all(c.t_end > c.t_stats > 0 for c in (slow, fast))
    assert all(5_000 < c.steps < 60_000 for c in (les.make(f, q) for f, q in les.SWEEP))


def test_les_post_finds_a_tone():
    import numpy as np

    from minimeater.mech.cfd import les_post as L

    t = np.arange(0, 0.2, 1 / L.FS)
    x = (np.sin(2 * np.pi * 812 * t) + 0.3 * np.random.default_rng(1).standard_normal(len(t)))[:, None]
    f, s = L.psd(x)
    p = L.peaks(f, s[:, 0])
    assert abs(p[0]["f_hz"] - 812) < 2 * (f[1] - f[0])
    a = np.array([[0.0, 1.0], [0.001, 3.0]])  # non-uniform samples are put on the 20 kHz grid
    _, y = L.uniform(a, 0.0, 0.001)
    assert y[10, 0] == pytest.approx(2.0)
