import numpy as np
import pytest

from minimeater.model.control import (
    AirflowMap,
    FeedForward,
    Fopdt,
    LidRecovery,
    Mpc,
    RefShaper,
    Schedule,
    identify,
)
from minimeater.model.fans import FANS
from minimeater.model.params import DEFAULT_EGG, DEFAULT_INTAKE, TOP_VENT, Ambient
from minimeater.model.sim import Obs, air_flow, simulate
from minimeater.model.units import CFM, f_to_k

FAN = FANS[1]


def obs(t=0.0, pit=225.0, sp=225.0, lid=False):
    return Obs(t, pit, sp, lid, Ambient())


def test_airflow_map_inverts_plant_model():
    amap = AirflowMap(DEFAULT_EGG, DEFAULT_INTAKE, FAN)
    q_min, q_max = amap.limits(Ambient(), 250.0)
    for q in np.linspace(q_min + 0.05, q_max - 0.05, 7):
        damper, s = amap.actuate(Ambient(), 250.0, q)
        got = air_flow(DEFAULT_EGG, DEFAULT_INTAKE, FAN, Ambient(), s, damper, f_to_k(250.0), amap.top_area_m2) / CFM
        assert got == pytest.approx(q, rel=0.05, abs=0.03)


def test_feedforward_monotonic_in_setpoint():
    ff = FeedForward(DEFAULT_EGG)
    qs = [ff.q_cfm(Ambient(), t) for t in (150, 225, 275, 325)]
    assert qs == sorted(qs) and qs[0] > 0


def test_ref_shaper_tethers_on_light_off_then_disarms():
    sh = RefShaper()
    r = sh.step(225.0, 70.0, 10.0)
    assert r <= 70.0 + sh.tether_f
    sh.step(225.0, 223.0, 10.0)  # within band -> disarmed, reference is the setpoint
    assert not sh.armed
    assert sh.step(225.0, 110.0, 10.0) == 225.0  # a lid dip does not re-tether
    sh.step(275.0, 225.0, 10.0)  # setpoint increase re-arms
    assert sh.armed and sh.r < 275.0


def test_lid_recovery_replays_boosted_pre_lid_airflow():
    lr = LidRecovery(boost=1.5, band_f=20.0)
    assert lr.update(obs(lid=True), 10.0, q_last=1.0) == 0.0
    assert lr.update(obs(pit=150.0), 10.0, q_last=0.0) == pytest.approx(1.5)
    assert lr.update(obs(pit=215.0), 10.0, q_last=0.0) is None


def test_identify_recovers_plausible_fopdt():
    amap = AirflowMap(DEFAULT_EGG, DEFAULT_INTAKE, FAN)
    m = identify(DEFAULT_EGG, DEFAULT_INTAKE, FAN, Ambient(), TOP_VENT["slider 1/4"], amap, 225.0)
    assert 50 < m.k_f_per_cfm < 300
    assert 120 < m.tau_s < 1800
    assert 0 < m.theta_s < 300


def test_mpc_holds_setpoint_with_mismatched_top_vent():
    sched = Schedule([Fopdt(116.0, 505.0, 107.0, 225.0)])
    ctrl = Mpc(AirflowMap(DEFAULT_EGG, DEFAULT_INTAKE, FAN), FeedForward(DEFAULT_EGG), sched)
    r = simulate(
        DEFAULT_EGG, DEFAULT_INTAKE, FAN, Ambient(), TOP_VENT["petals open"], [], 2 * 3600, ctrl,
        setpoint_f=225.0, init_f=225.0, noise_f=0.3,
    )
    hold = r.t_s > 3600
    assert abs(r.pit_f[hold].mean() - 225.0) < 1.5
    assert r.pit_f.max() < 235.0
