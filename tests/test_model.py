import dataclasses
import math

import pytest

from minimeat.model import flow, thermal
from minimeat.model.fans import FANS, max_flow, required_speed
from minimeat.model.params import DEFAULT_EGG, DEFAULT_INTAKE, TOP_VENT, Ambient
from minimeat.model.control import PercentPid
from minimeat.model.sim import Event, air_flow, output_map, simulate
from minimeat.model.units import CFM, MMH2O, c_to_k, f_to_k, k_to_f


def test_unit_roundtrip():
    assert k_to_f(f_to_k(225.0)) == pytest.approx(225.0)
    assert MMH2O == pytest.approx(9.80665)


@pytest.mark.parametrize("t_f", [225.0, 350.0, 600.0])
def test_steady_state_energy_closes(t_f):
    amb = Ambient()
    ss = thermal.steady_state(DEFAULT_EGG, amb, f_to_k(t_f))
    assert ss.fire_w == pytest.approx(ss.shell_loss_w + ss.flue_loss_w, rel=1e-6)
    assert amb.t_k < ss.t_wall_out_k < ss.t_wall_in_k


def test_required_air_increases_with_temp_and_cold():
    amb = Ambient()
    cold = Ambient(c_to_k(-10.0), 8.0)
    m = [thermal.steady_state(DEFAULT_EGG, amb, f_to_k(t)).m_air_kg_s for t in (225, 350, 450)]
    assert m == sorted(m)
    assert thermal.steady_state(DEFAULT_EGG, cold, f_to_k(225)).m_air_kg_s > m[0]


def test_ergun_scales_with_flow():
    lo = flow.ergun_drop(DEFAULT_EGG, 1e-3)
    hi = flow.ergun_drop(DEFAULT_EGG, 2e-3)
    assert 2.0 <= hi / lo <= 4.0  # between purely viscous and purely inertial


def test_stack_draft_positive_when_hot():
    assert flow.stack_draft(DEFAULT_EGG, c_to_k(20), f_to_k(225)) > 0
    assert flow.stack_draft(DEFAULT_EGG, c_to_k(20), c_to_k(20)) == pytest.approx(0.0)


def test_fan_curve_endpoints():
    fan = FANS[1]
    assert fan.pressure(0.0, 1.0) == pytest.approx(fan.pmax_pa)
    assert fan.pressure(fan.qmax_m3_s, 1.0) == pytest.approx(0.0, abs=1e-9)
    assert fan.pressure(0.0, 0.5) == pytest.approx(fan.pmax_pa * 0.25)


def test_required_speed_matches_curve():
    fan = FANS[1]
    op = required_speed(fan, 3 * CFM, 10.0)
    assert op.feasible
    assert fan.pressure(3 * CFM, op.s) == pytest.approx(10.0, rel=1e-6)
    assert not required_speed(fan, 3 * CFM, 1e4).feasible


def test_max_flow_on_free_air():
    fan = FANS[1]
    assert max_flow(fan, 1.0, lambda q: 0.0) == pytest.approx(fan.qmax_m3_s)


def test_output_map_damper_first():
    assert output_map(0.0) == (0.0, 0.0)
    assert output_map(0.25) == (0.5, 0.0)
    assert output_map(0.75) == (1.0, 0.5)
    assert output_map(1.0) == (1.0, 1.0)


def test_closed_damper_flow_is_leak_only():
    amb = Ambient()
    top = TOP_VENT["slider 1/4"]
    closed = air_flow(DEFAULT_EGG, DEFAULT_INTAKE, FANS[1], amb, 0.0, 0.0, f_to_k(225), top)
    open_ = air_flow(DEFAULT_EGG, DEFAULT_INTAKE, FANS[1], amb, 0.0, 1.0, f_to_k(225), top)
    tighter = dataclasses.replace(DEFAULT_EGG, leak_area_m2=DEFAULT_EGG.leak_area_m2 / 3)
    assert 0 < closed < open_
    assert air_flow(tighter, DEFAULT_INTAKE, FANS[1], amb, 0.0, 0.0, f_to_k(225), top) < closed


def test_slow_fan_never_blocks_more_than_stopped():
    amb = Ambient()
    top = TOP_VENT["slider 1/4"]
    for fan in FANS:
        stopped = air_flow(DEFAULT_EGG, DEFAULT_INTAKE, fan, amb, 0.0, 1.0, f_to_k(225), top)
        slow = air_flow(DEFAULT_EGG, DEFAULT_INTAKE, fan, amb, 0.01, 1.0, f_to_k(225), top)
        assert slow >= stopped * (1 - 1e-9)


def test_closed_loop_holds_setpoint():
    r = simulate(DEFAULT_EGG, DEFAULT_INTAKE, FANS[1], Ambient(), TOP_VENT["slider 1/4"], [Event(0.0)], 3 * 3600, PercentPid())
    hold = r.t_s > 2 * 3600
    assert abs(r.pit_f[hold].mean() - 225.0) < 2.0
    assert r.pit_f[hold].std() < 2.0
    assert not math.isnan(r.pit_f[-1])
