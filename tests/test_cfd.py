"""The CFD case builds without the solver: the air volume is one solid at every flap angle with each patch present,
and the case files carry the flow and the patches (the solve itself is `just mech::cfd::run <case>`)."""

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
    flat = np.diff(film.screen_time(ln, 10.0, alpha=0.0))
    assert flat[0] == pytest.approx(flat[1])  # alpha 0: outside the rotor's zone, equal steps along the line take equal screen time
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


def test_swarm_parcels_share_one_clock_and_the_stream_is_steady(tmp_path):
    import numpy as np

    from minimeater.mech import firebox, swarm

    x = np.linspace(-150, 100, 26)
    fast, slow = _line(x, np.where(x < -20, 4.0, 9.0), np.zeros(26)), _line(x + 0.0, np.full(26, 4.0), np.zeros(26))
    assert swarm.raw_clock(fast, 0.5)[-1] < swarm.raw_clock(slow, 0.5)[-1]  # one law: the same constant makes the fast line quicker on screen
    hero_end = np.array([100.0, 5.0, 0.0])
    p_in, v_in = firebox.approach(firebox.layout(16.0), hero_end, 4.0)
    k1, k2 = swarm.raw_clock(fast, 0.5)[-1] / 10.0, swarm.raw_clock({"p": p_in, "speed": v_in}, 0.3)[-1] / 5.0
    off = {**fast, "p": fast["p"] + [0.0, 3.0, 2.0]}  # a neighbour 3 mm over and 2 mm up
    (p, v, tau, _), (q, _, tau2, _) = swarm.chain(fast, p_in, v_in, hero_end, k1, k2, 0.5), swarm.chain(off, p_in, v_in, hero_end, k1, k2, 0.5)
    assert len(p) == len(v) == len(tau) and (np.diff(tau) >= 0).all() and tau[-1] == pytest.approx(15.0)
    assert np.allclose(p[len(x) - 1], hero_end) and np.allclose(q[len(x) - 1], hero_end + [0.0, 3.0, 2.0])  # the way on starts where the line ended
    assert np.linalg.norm(q[-1] - p[-1]) < np.linalg.norm([3.0, 2.0])  # and narrows through the grate's hole
    n, per = 240, 2
    track, period, _ = swarm.air([(p, v, tau, None), (q, v, tau2, None)], n, 24, per=per)
    nm = 2 * per * swarm.TAIL
    assert track.shape == (n, nm + 2 * per, 3) and period > tau[-1]
    heads = track[:, 0:nm:swarm.TAIL, 0] * 1e3  # the heads' x (mm)
    assert (np.diff(heads[:12, 0]) > 0).all() or (np.diff(heads[:12, 1]) > 0).all()  # a parcel goes down its line ...
    assert heads[0].min() < -100 and heads[0].max() > 0 and track[0, nm:, 0].min() > 0  # ... and the stream has parcels all along it at once; the carriers hold speeds
    cfd = heads[0] < 90  # (on the straight part of the way) a tail trails its head: its points are further back
    assert (track[0, 0:nm:swarm.TAIL, 0][cfd] >= track[0, 3:nm:swarm.TAIL, 0][cfd]).all()
    path = np.stack([np.zeros(200), np.zeros(200), np.linspace(0, 199, 200)], axis=1)
    rise = swarm.plume(path, np.full(200, 0.5), 24, 240, 24, 8.0, per_s=10.0)
    parcels = rise.shape[1] // (swarm.TAIL + 1)
    assert parcels == int(10.0 * (239 / 24 - 1)) and rise[:24, : parcels * swarm.TAIL, 2].max() < 0.001  # none has left before the start frame (all are at the path's start, in metres)
    assert rise[-1, 0 : parcels * swarm.TAIL : swarm.TAIL, 2].max() * 1e3 > 150  # the first ones get to the top, in metres
    swarm.write_pc2(tmp_path / "t.pc2", track)
    raw = (tmp_path / "t.pc2").read_bytes()
    assert raw[:12] == b"POINTCACHE2\0" and len(raw) == 32 + track.size * 4


def test_swarm_parcels_meet_the_blades_where_the_cfd_had_them():
    import math

    import numpy as np

    from minimeater.mech import swarm

    x = np.linspace(-150, 100, 126)
    stage = np.where(x < -120, 0, np.where(x < -100, 1, 2))
    line = {**_line(x, np.full(126, 5.0), stage), "t": np.cumsum(np.r_[0.0, np.diff(x) * 1e-3 / 5.0])}
    # in the zone screen time is real time over a fixed ratio, whatever the speed
    tau = swarm.raw_clock({**line, "speed": np.where(stage == 1, 11.0, 5.0)}, 0.5)
    zone = np.flatnonzero(stage == 1)
    dz = np.diff(tau)[zone[0] : zone[-1]] / np.diff(line["t"])[zone[0] : zone[-1]]
    assert np.allclose(dz, dz[0]) and dz[0] == pytest.approx(1e3 * swarm.V_REF**0.5)
    k = tau[-1] / 20.0
    omega = -314.0
    omega_f = swarm.rotor_rate(omega, k, 0.5)
    assert 0 > omega_f > omega  # slower on screen, the same way round
    # the molecule's own entry fixes the rotor's angle at screen time 0; the other lines' parcels then enter when it is where they expect it
    hero = swarm.entry(line, tau / k)
    theta0 = omega * hero[1] - omega_f * hero[0]
    other = {**line, "t": line["t"] + 0.0137}  # a neighbour that reached the zone later: it expects the rotor elsewhere
    pts = (other["p"].astype(float), other["speed"], tau / k, swarm.entry(other, tau / k))
    track, period, per = swarm.air([pts], 4000, 24, per=2, rotor=(omega, omega_f, theta0))
    turn = 2 * math.pi / abs(omega_f)
    assert period / turn == pytest.approx(round(period / turn)) and per == 2
    tin, t_in = pts[3]
    heads = track[:, 0 : per * swarm.TAIL : swarm.TAIL, 0] * 1e3
    arrive = np.flatnonzero((heads[1:, 0] >= x[stage == 1][0]) & (heads[:-1, 0] < x[stage == 1][0]))
    assert len(arrive) >= 2
    for f in arrive:  # at each arrival the film's rotor is where this line's zone points assume it (mod a turn)
        angle = theta0 + omega_f * f / 24
        assert np.angle(np.exp(1j * (angle - omega * t_in))) == pytest.approx(0.0, abs=abs(omega_f) * 2 / 24 * 1.5)


def test_swarm_sparks_leave_the_lump_rise_and_go_out():
    import numpy as np

    from minimeater.mech import swarm

    centre, r, fps = np.array([300.0, 0.0, 140.0]), 21.0, 24
    xyz = swarm.sparks(centre, r, 48, 240, 120, 360, fps)
    m = xyz.shape[1] // (swarm.TAIL + 1)
    nm = m * swarm.TAIL
    heads, heat = xyz[:, 0:nm:swarm.TAIL] * 1e3, xyz[:, nm:, 0]
    assert m > 100 and heat.min() >= 0 and heat.max() <= 1
    live = heat > 0
    assert np.allclose(heads[~live], centre, atol=1e-3)  # unborn and burnt-out sparks wait at the lump's centre (the fade hides them)
    born = np.array([np.argmax(live[:, j]) for j in range(m)])
    first = np.linalg.norm(heads[born, np.arange(m)] - centre, axis=1)[live.any(axis=0)]
    assert (first > r - 6).all()  # a live one starts at the lump's surface (a frame's travel at most)
    assert (born[live.any(axis=0)] >= 48 - 1).all() and (live[:47].sum() == 0) and (live[250:].sum() < live.sum() * 0.2)  # only in its window
    rate = np.bincount(born[live.any(axis=0)], minlength=360)
    assert rate[100:140].sum() > 1.5 * rate[48:88].sum() and heads[live][:, 2].mean() > centre[2]  # busiest near the flare, and they rise


def test_whistle_reads_a_forced_runs_impedance(tmp_path):
    """A forced run whose flap force is a known resistance and inertance on the oscillating flow gives them back, and a negative
    resistance larger than the rest of the loop's is a whistle."""
    import json
    import math

    import numpy as np

    from minimeater.mech.cfd import acoustics as ac
    from minimeater.mech.cfd import whistle

    hz, amp, cfm = 846.0, 0.05, 0.7
    a_bore = math.pi * (ac.g.bore_r * 1e-3) ** 2
    q0 = cfm * ac.CFM
    t = np.arange(0.0, 0.06, 2.5e-5)
    q_ac = q0 * amp * np.sin(2 * math.pi * hz * t)
    dq = q0 * amp * 2 * math.pi * hz * np.cos(2 * math.pi * hz * t)

    def run(r, l):
        d = tmp_path / f"r{r}"
        (d / "postProcessing" / "forces" / "0").mkdir(parents=True)
        fx = (r * q_ac + l * dq) * a_bore / ac.RHO  # the case holds forces per unit density
        rows = "\n".join(f"{ti:.7f} ({x:.9e} 0 0) ({x:.9e} 0 0) (0 0 0)" for ti, x in zip(t, fx))
        (d / "postProcessing" / "forces" / "0" / "force.dat").write_text("# Time total pressure viscous\n" + rows + "\n")
        (d / "case.json").write_text(json.dumps({"force_hz": hz, "force_amp": amp, "t_stats": 0.01, "cfm": cfm, "flap_deg": 33.0}))
        return whistle.case(d)

    ok = run(2.0e4, 50.0)
    assert ok["z_flap"][0] == pytest.approx(2.0e4, rel=0.02)
    assert ok["z_flap"][1] == pytest.approx(50.0 * 2 * math.pi * hz, rel=0.02)
    assert not ok["whistles"] and ok["margin"] > 1
    bad = run(-10 * ok["r_rest"], 0.0)
    assert bad["whistles"] and bad["margin"] < 0


def test_whistle_radiated_level_follows_the_force():
    """Ten times the force is 20 dB more at the listener; the A-weighting takes off more at 100 Hz than at 1 kHz."""
    import numpy as np

    from minimeater.mech.cfd import acoustics as ac
    from minimeater.mech.cfd import whistle

    f = np.arange(20.0, 6000.0, 5.0)
    s = np.where(abs(f - 1000) < 50, 1e-8, 1e-14)
    a, b = (whistle.radiated(f, k * s, 33, 0.7 * ac.CFM) for k in (1, 100))
    assert b["room_db"] - a["room_db"] == pytest.approx(20.0, abs=0.01)
    assert whistle.a_weight(np.array([100.0]))[0] < -15 < whistle.a_weight(np.array([1000.0]))[0] + 1
