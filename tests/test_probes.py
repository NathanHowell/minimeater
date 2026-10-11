import pytest

from minimeat.model.probes import ADCS, PROBES, FrontEnd, detect_thresholds, worst_f
from minimeat.model.units import f_to_k


@pytest.mark.parametrize("probe", PROBES, ids=lambda p: p.name)
def test_steinhart_hart_roundtrip(probe):
    for t_f in (32.0, 225.0, 500.0):
        assert probe.t_k(probe.r(f_to_k(t_f))) == pytest.approx(f_to_k(t_f), abs=1e-6)


def test_ntc_resistance_falls_with_temperature():
    for p in PROBES:
        assert p.r(f_to_k(100.0)) > p.r(f_to_k(300.0)) > 0
        assert p.dlnr_dt(f_to_k(225.0)) < 0


def test_chosen_front_end_meets_budget():
    fe = FrontEnd()
    mainstream = [p for p in PROBES if "Maverick" not in p.name]
    assert worst_f(fe, mainstream) < 0.75
    # pit range 150-325F for every family, including the 1M-class Maverick
    assert max(fe.total_k(p, f_to_k(t), False) * 1.8 for p in PROBES for t in (150.0, 225.0, 325.0)) < 0.6


def test_ratiometric_beats_mcu_adc():
    assert worst_f(FrontEnd(adc=ADCS[0])) < worst_f(FrontEnd(adc=ADCS[2])) / 5


def test_detect_thresholds_bracket_valid_band():
    d = detect_thresholds(FrontEnd())
    assert 0 < d.short_below < d.hottest_valid < d.coldest_valid < d.open_above < 1
