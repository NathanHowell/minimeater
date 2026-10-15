import pytest

from minimeater.model.probes import PROBES
from minimeater.sim import frontend as F, ngspice

pytestmark = pytest.mark.skipif(not ngspice.available(), reason="ngspice not installed")


@pytest.mark.parametrize("probe", PROBES)
def test_bias_at_tip_reads_true_in_the_pit_range(probe):
    for t in (150, 225, 325):
        assert abs(F.dc_error_f("bias_at_tip", probe, t)) < 0.2


def test_series_resistor_inside_the_bias_node_is_wrong():
    """The schematic once had the 1k between the probe and the bias node: it reads tens of degF low at the hot end."""
    assert F.dc_error_f("as_drawn", PROBES[0], 325) < -20


def test_plug_in_settles_within_a_conversion():
    for p in PROBES:
        assert F.settle_f("bias_at_tip", p, 225)[0] < 0.05


def test_servo_inrush_keeps_the_rail_up():
    from minimeater.sim import power as W

    assert W.inrush(110e-6, 0.6, 0.5, 0.3)["vin_min"] > 4.0


def test_budget_closed_form_matches_ngspice():
    from minimeater.sim import budget as Q

    for p in PROBES:
        for t in (32, 225, 500):
            assert abs(Q.dc_ratio(p, t, 10e-9, 5e-9) - Q.dc_ratio_spice(p, t, 10e-9, 5e-9)) < 1e-7


def test_probe_budget_meets_the_acceptance():
    """Worst case over every source's datasheet limit: under 1 F in a 150-300 F pit, under 0.5 F at food 130-205 F."""
    from minimeater.sim import budget as Q

    acc = Q.accept()
    assert acc["pit"] < 1.0 and acc["food"] < 0.5


def test_kickback_is_its_average_current():
    from minimeater.sim import budget as Q

    r = Q.kickback(PROBES[2], 225, 5e-8 / (Q.F_MOD * Q.VCC))
    assert abs(r["err_f"] - r["dc_equiv_f"]) < 0.05 * abs(r["dc_equiv_f"]) and abs(r["err_f"]) < 0.02


@pytest.mark.parametrize("c_mutual", [8e-12, 30e-12])
def test_tach_reads_clean_through_the_pcnt_filter(c_mutual):
    from minimeater.sim import fan as FN

    r = FN.tach_report(FN.Fan(c_mutual=c_mutual))
    assert r["v_low"] < FN.VIL
    assert r["glitch_width"] < FN.PCNT_FILTER_S / 10  # the PWM's crosstalk kicks are far shorter than the filter


def test_fan_pwm_low_meets_the_fan_spec_at_the_strongest_pullup():
    from minimeater.sim import fan as FN

    assert FN.pwm_report(FN.Fan(pwm_pullup=1e3))["v_low"] < FN.FAN_VIL


def test_fan_load_switch_runs_cool():
    from minimeater.sim import fan as FN

    assert FN.load_switch()["rise_run"] < 2.0


def test_efuse_holds_vbus_f_under_the_6v_parts_and_limits_attach_charge():
    """U12 instead of the PTC: no cable ringing reaches VBUS_F, and a plug-in draws a few uC at attach (USB's guideline
    is 50 uC) instead of the 100 uF's 0.3-0.6 mC."""
    from minimeater.sim import power as W

    for l, r in ((0.25e-6, 0.05), (1e-6, 0.05), (2e-6, 0.05), (2e-6, 0.1)):
        for caps in ("nominal", "derated"):
            h = W.hotplug(W.HotPlug(l, r, caps))
            assert h["vbus_f_peak"] < 5.5
            assert h["vbus_raw_peak"] < 10.0  # the eFuse's input is rated 20 V; D1 clamps it near 7
            assert h["charge_100us"] < 10e-6
            assert h["i_peak_after_100us"] < 1.0  # the soft start's charging current


def test_hot_plug_leaves_the_tvs_far_under_its_rating():
    from minimeater.sim import power as W

    for caps in ("nominal", "derated"):
        h = W.hotplug(W.HotPlug(2e-6, 0.05, caps))
        assert h["tvs_peak"] < 21.7  # SMF5.0A pulse current
        assert h["tvs_energy"] < 1e-3  # J


def test_a_half_amp_port_boots_the_board_with_the_loads_gated():
    from minimeater.sim import rails as R

    r = R.Rails(tx=(5e-3, 0.5e-3, 2), v_src=4.75, r_cable=0.3, i_limit=0.5, l_cable=0, v3=R.V3_LOW, t_stop=8e-3)
    m = R.metrics(r, R.run(r))
    assert not m["brownout"] and m["t_3v3"] < 3e-3 and m["v3_min"] > R.V_MCU_MIN


def test_the_servo_on_a_half_amp_port_browns_the_board_out():
    """ADR 0004's 1.5 A rule: the servo's 0.6 A limit and the fan and radio are 1.1 A against 0.5 A."""
    from minimeater.sim import rails as R

    r = R.Rails(servo="stall", t_servo=6e-3, v_src=4.75, r_cable=0.3, i_limit=0.5, l_cable=0, t_stop=10e-3)
    m = R.metrics(r, R.run(r))
    assert m["brownout"] and m["t_end"] < r.t_servo + 1e-3


def test_worst_chain_on_a_1p5_a_source_keeps_the_rails_up():
    from minimeater.sim import rails as R

    r = R.Rails(v_src=4.75, r_cable=0.3, i_limit=1.5, l_cable=0, v3=R.V3_LOW, **R.WORST)
    m = R.metrics(r, R.run(r))
    assert not m["brownout"]
    assert m["v3_min"] > R.V_MCU_MIN + 0.1
    assert m["vbus_f_min"] > 4.0
