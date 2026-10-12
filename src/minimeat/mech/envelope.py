"""The board outline, mounting holes and connector positions the enclosure assumes (`.1.6`).

Board frame: origin at the board centre, +x toward the front (the flow direction), +y to the left of the
rear edge. The rear edge carries the USB-C receptacle and the four probe jacks and faces the tray's rear wall.
"""

import json

from .params import P


def envelope():
    p = P.pcb
    return {
        "outline": {"x": p.x_len, "y": p.y_len, "corner_r": p.corner_r, "thickness": p.t},
        "holes": [
            {"x": sx * (p.x_len / 2 - p.hole_inset), "y": sy * (p.y_len / 2 - p.hole_inset), "d": p.hole_d}
            for sx in (-1, 1)
            for sy in (-1, 1)
        ],
        "rear_edge_connectors": [{"kind": "usb-c", "y": p.usb_y, "w": p.usb_w, "h": p.usb_h}]
        + [{"kind": "jack-2.5mm", "y": y, "w": p.jack_w, "h": p.jack_h} for y in p.jack_ys],
        "height_above_board": p.top_h,
        "clearance_below_board": p.bottom_h,
        "rear_gap_to_wall": p.rear_gap,
        "note": "provisional until the ICD (.1.2) and the schematic (.3.x) fix the outline",
    }


def main():
    print(json.dumps(envelope(), indent=2))
