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


def test_hot_plug_stays_inside_the_6v_limit_with_a_normal_cable():
    from minimeater.sim import power as W

    assert W.hotplug(W.HotPlug(1e-6, 0.2, "derated"))["vbus_f_peak"] < 5.5
    assert W.hotplug(W.HotPlug(1e-6, 0.1, "derated"))["vbus_f_peak"] < 6.0  # 1 m cable, 0.1 ohm round trip; at 0.05 ohm and 2 uH it rings to 7 V (docs/sim/power.md)


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


def test_snubber_footprint_damps_the_hot_plug():
    from minimeater.sim import power as W

    for l, r in ((2e-6, 0.05), (2e-6, 0.1), (1e-6, 0.05)):
        bare = W.hotplug(W.HotPlug(l, r, "derated"))["vbus_f_peak"]
        damped = W.hotplug(W.HotPlug(l, r, "derated", snubber=W.SNUBBER))["vbus_f_peak"]
        assert damped < bare - 0.5
    assert W.hotplug(W.HotPlug(1e-6, 0.05, "derated", snubber=W.SNUBBER))["vbus_f_peak"] < 6.0
