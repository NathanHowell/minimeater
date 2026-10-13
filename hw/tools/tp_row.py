"""Debug test points in one row: `tp_row.py <in> <out>` (KiCad's python, from `hw/tools`).

TP5 (EN), TP8 (GND), TP6 (UART TX) and TP7 (UART RX) go to one 2.54 mm row at y = 42 mm, so a pogo clip or a 4-pin
header reaches them; R7 (the LOAD_FAULT pull-up) moves beside R11 to clear the band above the row, where the labels
EN / GND / TX / RX now stand over their pads (the old free-standing labels go). Copper is stretched as
`shift_group.shift` does; route after with `routeall.py --repair`."""

import sys

from shift_group import F, M, move_to  # first: it puts KiCad's pcbnew on the path
import pcbnew

ROW_Y, X0, PITCH = 42.0, 36.38, 2.54
ROW = [("TP5", "EN"), ("TP8", "GND"), ("TP6", "TX"), ("TP7", "RX")]
OLD = {"EN": "TP5", "GND": "TP8", "UART_TX": "TP6", "UART_RX": "TP7"}

b = pcbnew.LoadBoard(sys.argv[1])
for d in list(b.GetDrawings()):  # the old labels: the free-standing text nearest each test point
    if d.Type() == pcbnew.PCB_TEXT_T and d.GetText() in OLD:
        tp = b.FindFootprintByReference(OLD[d.GetText()]).GetPosition()
        if abs(M(d.GetPosition().x) - M(tp.x)) < 4.5 and abs(M(d.GetPosition().y) - M(tp.y)) < 4.5:
            b.RemoveNative(d)
move_to(b, "R7", 40.2, 38.67)
for k, (ref, label) in enumerate(ROW):
    x = X0 + PITCH * k
    move_to(b, ref, x, ROW_Y)
    t = pcbnew.PCB_TEXT(b)
    t.SetText(label)
    t.SetLayer(pcbnew.F_SilkS)
    t.SetTextSize(pcbnew.VECTOR2I(F(0.8), F(0.8)))
    t.SetTextThickness(F(0.12))
    t.SetPosition(pcbnew.VECTOR2I(F(x), F(ROW_Y - 1.95)))
    b.Add(t)
pcbnew.SaveBoard(sys.argv[2], b)
print("row placed")
