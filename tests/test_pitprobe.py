import numpy as np
import pytest

from minimeater.model.control import AirflowMap, FeedForward
from minimeater.model.fans import FANS
from minimeater.model.params import DEFAULT_EGG, DEFAULT_INTAKE, Ambient
from minimeater.model.pitprobe import AutoPit, PitFinder, PitMismatch
from minimeater.model.sim import Obs

AMB_F = 68.0


def run(finder, readings, sp=225.0, dt=10.0):
    """Feed (n_steps, jacks) readings, NaN for an empty jack; the step it named a jack, or None."""
    for i, row in enumerate(readings):
        jacks = tuple(None if np.isnan(r) else float(r) for r in row)
        if finder.step(i * dt, AMB_F, sp, jacks) is not None:
            return i * dt
    return None


def ramp(n, start, end, over):
    return np.minimum(start + (end - start) * np.arange(n) * 10.0 / over, end)


def test_pit_among_meat_is_named_in_any_jack():
    for jack in range(4):
        n = 180
        r = np.full((n, 4), np.nan)
        for k in range(4):
            r[:, k] = ramp(n, 38.0, 45.0, 3600.0)  # meat from the fridge barely moves
        r[:, jack] = ramp(n, AMB_F, 220.0, 1200.0)
        f = PitFinder()
        assert run(f, r) is not None and f.locked == jack


def test_lone_pit_probe_is_named_once_hot_and_rising():
    r = ramp(180, AMB_F, 220.0, 1200.0)[:, None]
    f = PitFinder()
    t = run(f, r)
    assert f.locked == 0
    assert t is not None and r[int(t / 10), 0] >= AMB_F + f.hot_f


def test_a_cold_probe_is_not_named():
    r = ramp(180, 38.0, 60.0, 1800.0)[:, None]
    assert run(PitFinder(), r) is None


def test_two_hot_probes_are_ambiguous():
    n = 180
    r = np.stack([ramp(n, AMB_F, 220.0, 1200.0), ramp(n, AMB_F, 210.0, 1200.0)], axis=1)
    assert run(PitFinder(), r) is None


def test_a_flat_hot_probe_is_not_named_unless_near_setpoint():
    flat = np.full((60, 2), 150.0)
    flat[:, 1] = np.nan
    assert run(PitFinder(), flat, sp=225.0) is None
    assert run(PitFinder(), flat, sp=160.0) is not None


def test_the_choice_is_locked_and_a_lost_probe_never_moves_it():
    n = 60
    r = np.full((n, 2), np.nan)
    r[:, 0] = ramp(n, AMB_F, 220.0, 300.0)
    f = PitFinder()
    run(f, r)
    assert f.locked == 0
    assert f.step(1000.0, AMB_F, 225.0, (None, 300.0)) == 0


def test_unassigned_controller_runs_on_the_hottest_probe_and_feeds_the_fire_with_none():
    amap = AirflowMap(DEFAULT_EGG, DEFAULT_INTAKE, FANS[1])
    seen = []

    class Spy:
        def step(self, obs, dt):
            seen.append(obs.pit_f)
            return 1.0, 0.5

    ctl = AutoPit(Spy(), amap, FeedForward(DEFAULT_EGG))
    amb = Ambient()
    assert ctl.step(Obs(0.0, 70.0, 225.0, False, amb, (38.0, 70.0, None, None)), 10.0) == (1.0, 0.5)
    assert seen == [70.0]  # the hottest, not jack 0
    out = ctl.step(Obs(10.0, 70.0, 225.0, False, amb, (None, None, None, None)), 10.0)
    assert out != (0.0, 0.0) and seen == [70.0]  # no probe: the flap is not shut, the model's airflow


def watch(m, readings, sp=225.0, fan=True, dt=10.0):
    """The time `m` tripped feeding (n_steps,) readings, or None."""
    for i, r in enumerate(readings):
        if m.step(i * dt, float(r), sp, fan):
            return i * dt
    return None


def test_a_pit_probe_lighting_off_slowly_does_not_trip():
    # the slowest band corner reaches setpoint - 40 F at 39 min
    r = ramp(720, AMB_F, 320.0, 2700.0)
    assert watch(PitMismatch(), r, sp=325.0) is None


def test_meat_on_full_air_trips_slow_and_the_policy_is_the_callers():
    r = ramp(720, 38.0, 150.0, 7200.0)  # 2 h of a thick roast, never within 40 F of 225
    m = PitMismatch()
    assert watch(m, r) == 3600.0 and m.tripped == "slow"


def test_no_fan_no_slow_trip():
    assert watch(PitMismatch(), np.full(720, 100.0), fan=False) is None


def test_a_probe_far_over_setpoint_trips_hot_but_a_blip_does_not():
    m = PitMismatch()
    assert watch(m, np.full(30, 290.0)) == 120.0 and m.tripped == "hot"
    blip = np.full(30, 230.0)
    blip[5:8] = 300.0
    assert watch(PitMismatch(), blip) is None


def test_off_never_trips_and_reset_clears():
    assert watch(PitMismatch(policy="off"), np.full(720, 100.0)) is None
    m = PitMismatch()
    watch(m, np.full(30, 290.0))
    m.reset()
    assert m.tripped is None


def test_stop_shuts_the_actuator_and_warn_keeps_driving():
    amap = AirflowMap(DEFAULT_EGG, DEFAULT_INTAKE, FANS[1])
    amb = Ambient()

    class Full:
        def step(self, obs, dt):
            return 1.0, 1.0

    def run_hot(policy):
        ctl = AutoPit(Full(), amap, FeedForward(DEFAULT_EGG), finder=PitFinder(locked=0), mismatch=PitMismatch(policy=policy))
        out = None
        for i in range(30):
            out = ctl.step(Obs(i * 10.0, 290.0, 225.0, False, amb, (290.0, None, None, None)), 10.0)
        return ctl, out

    ctl, out = run_hot("stop")
    assert ctl.mismatch.tripped == "hot" and out == (0.0, 0.0)
    ctl, out = run_hot("warn")
    assert ctl.mismatch.tripped == "hot" and out == (1.0, 1.0)
