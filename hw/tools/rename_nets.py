"""Names the board's auto-named nets after the schematic labels: `rename_nets.py <in> <out>` (the labels were added in
the schematic; this is "Update PCB from Schematic" for just those names)."""

import sys

sys.path.insert(0, "/Applications/KiCad/KiCad.app/Contents/Frameworks/Python.framework/Versions/Current/lib/python3.9/site-packages")
import pcbnew

NAMES = {
    "Net-(D2-A)": "/LED_A",
    "Net-(FB2-Pad2)": "/3V3A_FILT",
    "Net-(U4-SW)": "/BUCK_SW",
    "Net-(U5-EN)": "/VBUS_LDO",
    "Net-(U6-ILIM)": "/SERVO_ILIM",
    "Net-(U7-ILIM)": "/FAN_ILIM",
    "Net-(U8-RESET)": "/ADC_RESET",
}
b = pcbnew.LoadBoard(sys.argv[1])
for old, new in NAMES.items():
    n = b.FindNet(old)
    assert n is not None, old
    n.SetNetname(new)
    print(old, "->", new)
pcbnew.SaveBoard(sys.argv[2], b)
