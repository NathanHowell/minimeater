import pytest

from minimeat.model.probes import PROBES
from minimeat.sim import frontend as F, ngspice

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
