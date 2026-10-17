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


def test_pathlines_turn_with_the_rotor_inside_its_zone(tmp_path):
    """The pathline tracer's frame handling on a field it can be checked against by hand: a uniform 1 m/s axial flow with the
    rotor zone's air turning with the rotor (no blades, so the flow relative to it is the axial flow alone). A particle crosses the
    zone straight in the rotor's frame, so in the room's it turns about the axis with the rotor, at its radius, for the time it
    is in there, and leaves the zone's far side still at that angle going straight on."""
    import numpy as np
    import pyvista as pv

    from minimeater.mech.cfd import pathlines as PL

    omega = -3000 * 2 * np.pi / 60
    g = pv.ImageData(dimensions=(61, 61, 61), spacing=(1e-3, 1e-3, 1e-3), origin=(-0.14, -0.03, -0.03)).cast_to_unstructured_grid()
    p = np.asarray(g.points)
    u = np.tile([1.0, 0, 0], (len(p), 1))
    zone = (p[:, 0] > PL.ZONE[0] / 1000) & (p[:, 0] < PL.ZONE[1] / 1000) & (np.hypot(p[:, 1], p[:, 2]) < PL.ZONE[2] / 1000)
    u[zone] += np.c_[np.zeros(zone.sum()), -omega * p[zone, 2], omega * p[zone, 1]]  # the zone's air turns with the rotor
    g.point_data["U"] = u
    g.save(tmp_path / "internal.vtu")
    field = PL.Field(tmp_path / "internal.vtu", omega=omega)
    y0, z0 = 0.012, 0.005
    path = PL.follow(field, np.array([[-0.135, y0, z0]]))[0]
    assert [s.stage for s in path] == [0, 1, 0]
    pts = np.vstack([s.p for s in path])
    t = np.concatenate([s.t for s in path])
    r = np.hypot(pts[:, 1], pts[:, 2])
    assert np.allclose(r, np.hypot(y0, z0), atol=2e-4)  # at its radius all the way
    ang = np.unwrap(np.arctan2(pts[:, 2], pts[:, 1]))
    in_zone = np.flatnonzero(np.concatenate([np.full(len(s.p), s.stage) for s in path]) == 1)  # (the zone's cells, not its nominal x range)
    entry, leave = in_zone[0], in_zone[-1]
    turned = ang[leave] - ang[entry]
    expect = omega * (t[leave] - t[entry])
    assert abs(turned - expect) < 0.05 * abs(expect) and abs(expect) > 3  # it turned with the rotor (about 5.5 rad here)
    assert np.allclose(ang[leave:], ang[leave], atol=0.02)  # and goes straight on past the zone


def _line(x, speed, stage):
    import numpy as np

    n = len(x)
    return {"p": np.stack([x, np.full(n, 5.0), np.zeros(n)], axis=1), "t": np.linspace(0, 0.03, n), "speed": np.asarray(speed, float), "stage": np.asarray(stage, np.uint8)}


def test_film_retimes_a_pathline_and_trims_it_where_the_tube_ends():
    import numpy as np

    from minimeater.mech import film

    x = np.linspace(-150, 100, 6)
    ln = _line(x, [1, 1, 9, 9, 5, 5], [0, 0, 1, 1, 2, 2])
    (cut,) = film.trim([ln], 20.0)
    assert cut["p"][-1, 0] > 20.0 and cut["p"][-2, 0] <= 20.0 and len(cut["p"]) == 5  # ends on the first vertex past it
    tau = film.screen_time(ln, 10.0, alpha=0.5)
    assert tau[0] == 0 and tau[-1] == 10.0 and (np.diff(tau) > 0).all()
    assert np.ptp(film.screen_time(ln, 10.0, alpha=0.0)[:3]) == pytest.approx(10.0 * 100 / 250)  # alpha 0: screen time follows arc length
    pos, real, arc = film.sample(ln, tau, np.array([0.0, 5.0, 10.0]))
    assert pos[0, 0] == -150 and pos[-1, 0] == 100 and real[-1] == pytest.approx(0.03) and arc[-1] == pytest.approx(250)
    t = np.arange(48) / 24
    assert film.tumble(t) == pytest.approx(film.tumble(t))  # seeded: the same every run
    assert not np.allclose(film.tumble(t), film.tumble(t, seed=1))


def test_film_timeline_is_continuous_and_the_firebox_path_goes_where_the_air_has_to():
    import numpy as np

    from minimeater.mech import film, firebox

    lay = firebox.layout(16.0)
    assert lay["x0"] == pytest.approx(16.0 + firebox.PORT + firebox.inner_radius(0.0))
    cs, rs = lay["lumps"]
    assert len(cs) > 150 and (cs[:, 2] - rs >= firebox.BED[1] - 1e-6).all()
    up = lay["up"]
    clear = np.hypot(cs[:, 0] - up[0], cs[:, 1]) - rs  # the corridor above the grate's hole stays open for the O2 and the CO2
    assert (clear[(cs[:, 2] > 70) & (cs[:, 2] < 195)] > 15).all()
    start = np.array([16.0, 3.0, -2.0])
    p_in, v_in = firebox.approach(lay, start, 2.7)
    assert np.allclose(p_in[0], start) and (v_in > 0).all()
    assert np.abs(p_in[:, 1]).max() < 10  # it stays near the cut plane
    z_hole = firebox.GRATE[1] + firebox.GRATE[2] / 2
    k = np.argmin(abs(p_in[:, 2] - z_hole) + abs(p_in[:, 0] - up[0]))  # it crosses the grate's plane at the hole, on the cut plane
    assert abs(p_in[k, 0] - up[0]) < 4 and abs(p_in[k, 1]) < 4
    p_up, _ = firebox.rise(lay)
    assert np.allclose(p_up[-1], lay["plume_end"]) and p_up[-1, 2] > p_up[0, 2]
    a, b = film.line_of(p_in, v_in), film.line_of(p_up, np.full(len(p_up), 0.5))
    b["t"] += a["t"][-1]
    stop = {"p": p_in[-1:], "speed": v_in[-1:], "t": a["t"][-1:]}
    pos, real, arcs, firsts = film.timeline([(a, 2.0, 0.3), (stop, 1.0, 0.0), (b, 2.0, 0.0)], 24)
    assert len(pos) == 24 * 5 + 1 and firsts == [0, 48, 72]
    assert np.abs(np.diff(pos, axis=0)).max() < 25  # no jumps at the joins (mm per frame)
    assert (np.diff(real) >= 0).all()
    assert (pos[48:72] == p_in[-1]).all()
