import pytest

from minimeater.sim import dissipation as D


@pytest.mark.parametrize("s", D.SCENARIOS, ids=lambda s: s.name.split(" (")[0])
def test_every_part_stays_under_its_junction_limit_at_the_board_ceiling(s):
    r = D.solve(s)
    for part in D.THETA:
        assert D.tj(part, r[part]) < D.TJ_MAX


def test_power_balance_closes():
    """The source's power is the loads' plus the board's own: rebuild the loads from the same currents."""
    for s in D.SCENARIOS:
        r = D.solve(s)
        p3 = D.V_3V3 * s.i_3v3
        p_ana = D.V_3V3 * D.I_ANALOG
        p_sw = (s.i_fan + s.i_servo) * D.V_BUS - (s.i_fan**2 + s.i_servo**2) * D.SW_RON_MAX
        delivered = p3 + p_ana + p_sw
        # the switches' own supply current and the TVS leakage are in the board's share
        assert r["p_in"] == pytest.approx(delivered + r["p_board"], rel=0.02)


def test_budget_case_fits_the_1p5_a_source():
    assert D.solve(D.SCENARIOS[2])["i_in"] < 1.5


def test_buck_beats_a_linear_regulator_at_the_tx_burst():
    r = D.solve(D.SCENARIOS[1])
    assert r["U4"] < 0.2 * (D.V_BUS - D.V_3V3) * D.SCENARIOS[1].i_3v3


def test_buck_efficiency_follows_the_curve():
    assert D.buck_eta(0.35) == pytest.approx(0.93, abs=0.005)
    assert D.buck_eta(2.0) < D.buck_eta(0.35)  # falls off at the top of the range
    assert D.buck_eta(0.001) < D.buck_eta(0.1)


def test_limits_hold_the_loads_the_firmware_gates():
    f = D.faults()
    # a stall is limited, not a short: U6's drop is the supply less what the rotor takes at 0.6 A
    stall = f["U6, servo stalled at 1.2 A (hard)"]
    assert stall["drop"] == pytest.approx(D.V_BUS - D.I_SERVO_LIMIT * D.V_BUS / 1.2)
    assert all(v["p"] > 1.0 for v in f.values())  # every fault case is a thermal-shutdown case


def test_soft_start_energy_is_the_capacitors():
    e = D.efuse_start()
    assert e["energy"] == pytest.approx(0.5 * e["c"] * D.V_BUS**2)
    assert e["t"] == pytest.approx(D.V_BUS / 4200.0)
