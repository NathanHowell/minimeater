"""The board outline, mounting holes and connector positions the enclosure assumes (`.1.6`).

Board frame: origin at the board centre, +x toward the front (the flow direction), +y to the left looking
along the flow. The board lies flat, parts up, under the duct; its -y long edge carries the USB-C receptacle
and the four probe jacks, facing the pan's port wall.
"""

import json

from .params import P


def envelope():
    p = P.pcb
    return {
        "outline": {"x": p.x_len, "y": p.y_len - p.edge_trim, "corner_r": p.corner_r, "thickness": p.t,
                    "note": "the -y edge corners are square; the sheet frame is the untrimmed 54 mm (edge_trim is cut from the -y edge)"},
        "holes": [{"x": x, "y": y, "d": p.hole_d} for x, y in p.holes],
        "edge_connectors": {
            "edge": "-y",
            "usb-c": {"x": p.usb_x, "w": p.usb_w, "h": p.usb_h, "wall_opening": [p.usb_open_w, p.usb_open_h], "mouth_past_edge": p.edge_trim - p.usb_face},
            "jacks-2.5mm": [{"x": x, "w": p.jack_w, "h": p.jack_h, "wall_hole_d": p.jack_hole_d, "wall_opening_w": p.jack_open_w, "front_past_edge": p.edge_trim - p.jack_face} for x in p.jack_xs],
            "face_to_wall_inner": p.edge_gap,
            "wall_thickness": P.shell.bay_wall_t,
        },
        "indicators": {
            "edge": "+y",
            "led": {"x": p.led_x, "z_above_board": p.led_z, "part": "LTST-S220KGKT side-view, facing +y"},
            "button": {"x": p.btn_x, "z_above_board": p.btn_z, "part": "SKRPACE010 top-push, inside the case (open it to press)"},
        },
        "ambient_sensor": {"side": "top", "x": p.sensor[0], "y": p.sensor[1], "note": "top side at the rear +y corner, no copper under its thermal pad, at the flank louvres"},
        "height_above_board": p.top_h,
        "clearance_below_board": p.bottom_h,
        "note": "provisional until the ICD (.1.2) and the schematic (.3.x) fix the outline",
    }


def main():
    print(json.dumps(envelope(), indent=2))
