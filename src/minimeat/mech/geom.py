"""Derived geometry shared by the parts, the mock components and the checks (mm, assembly frame)."""

from dataclasses import dataclass

from .params import P


@dataclass(frozen=True)
class G:
    bore_r: float = P.tube.id / 2
    wall: float = P.print.wall
    duct_r: float = P.tube.id / 2 + P.print.wall  # outer radius of the straight duct
    # fan stack, x from the coupling mouth backwards
    socket_x0: float = -P.coupling.depth
    flap_x: float = P.damper.flap_x
    straight_x0: float = P.damper.flap_x - P.tube.id / 2 - P.damper.swing_margin  # flap swing clears the cone
    flange_t: float = 8.0
    fan_face_x: float = P.damper.flap_x - P.tube.id / 2 - P.damper.swing_margin - P.cone_len  # fan outlet face
    flange_w: float = P.fan.size + 2.0
    grille_t: float = 3.0


g = G()

# servo stack above the duct, bottom up (z from the tube axis). The output face points at the duct
# and the output axis is the flap axis (+Z).
LINK_Z0 = g.duct_r + 0.8  # bottom of the link hub
LINK_FLANGE_Z0 = LINK_Z0 + 4.0
LINK_FLANGE_Z1 = LINK_FLANGE_Z0 + P.damper.head_t
HORN_Z0 = LINK_FLANGE_Z1
HORN_Z1 = HORN_Z0 + P.servo.horn_t
SERVO_FACE_Z = HORN_Z1 + P.servo.boss_h + 0.3  # output face of the servo body
PLATE_TOP_Z = SERVO_FACE_Z + P.servo.tab_to_face
PLATE_T = 4.5
SERVO_TOP_Z = SERVO_FACE_Z + P.servo.body_h
# the servo's long axis lies along the flow: its body centre is this far from the output axis, toward +x
SERVO_X_CENTRE = P.damper.flap_x + P.servo.body_l / 2 - P.servo.shaft_off

# the fan and the electronics tray
FAN_X0 = g.fan_face_x - P.fan.depth
GRILLE_X0 = FAN_X0 - g.grille_t
TRAY_Z0 = g.flange_w / 2  # floor underside sits on the flange top
TRAY_X0 = GRILLE_X0
TRAY_W = g.flange_w + 2 * P.print.wall
TRAY_STANDOFF = 4.0
TRAY_PCB_CLEAR = 1.0
TRAY_INNER_H = TRAY_STANDOFF + P.pcb.t + P.pcb.top_h + TRAY_PCB_CLEAR
