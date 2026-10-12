"""Stand-ins for the parts that are bought, for fit and interference checks (mm, assembly frame)."""

from functools import cache
from pathlib import Path

from build123d import Axis, Pos, Rot, Torus, import_step

from . import geom as G
from .geom import g
from .parts import _arm, box, d_bore, xcyl, ycyl, zcyl
from .params import P


def fan():
    """The fan frame and hub (blades omitted): 60 x 60 x 25 with an aperture and four corner holes, and the
    heads of its four screws on the rear face."""
    f = P.fan
    s = box(G.FAN_X0, g.fan_face_x, -f.size / 2, f.size / 2, -f.size / 2, f.size / 2)
    s = s.fillet(f.corner_r, s.edges().filter_by(Axis.X))
    s -= xcyl(f.aperture_d / 2, G.FAN_X0 - 0.1, g.fan_face_x + 0.1)
    s += xcyl(f.hub_d / 2, G.FAN_X0, g.fan_face_x)
    for sy in (-1, 1):
        for sz in (-1, 1):
            s -= xcyl(f.hole_d / 2, G.FAN_X0 - 0.1, g.fan_face_x + 0.1, sy * f.hole_pitch / 2, sz * f.hole_pitch / 2)
            s += xcyl(3.0, G.FAN_X0 - 3.0, G.FAN_X0 + 0.2, sy * f.hole_pitch / 2, sz * f.hole_pitch / 2)  # screw heads
    return s


FAN_STEP = Path(__file__).parent / "data" / "NF-A6x25.stp"


def fan_step():
    """Noctua's public model of the NF-A6x25 for the exported assembly. Its frame: Y along the axis, 25 deep with the
    rotor toward y = 25 (the intake, label side) and the motor mount plate at y = 0 (the exhaust); X, Z across, centred.
    Turned so the air leaves along +x through the outlet face, like `fan()`. The frame does not tessellate in the
    renderer and the interference checks use the box version."""
    s = Rot(Z=90) * import_step(FAN_STEP)  # (X, Y, Z) -> (-Y, X, Z)
    return Pos(g.fan_face_x, 0, 0) * s


SERVO_STEP = Path(__file__).parent / "data" / "HS-5065MG.stp"
# the vendor model's frame: X along the long axis (output axis at x = 5.7, short end toward +x), Y up with the back face at
# y = -14.0, Z across the case; the output axis at z = 0
_STEP_AXIS_X, _STEP_BACK_Y = 5.7, -14.0


@cache
def _servo_step():
    return import_step(SERVO_STEP)


def servo():
    """Hitec's own model of the HS-5065MG (SERVO_STEP), turned to stand beside the duct with the output up and
    its axis at the output axis; `servo_box` is the simple stand-in."""
    s = _servo_step()
    s = Pos(-_STEP_AXIS_X, -_STEP_BACK_Y, 0) * s  # axis to the origin, back face to y = 0
    s = Rot(X=90) * s  # Y up -> Z up (Z -> -Y)
    return Pos(G.SERVO_X, G.SERVO_Y, G.SERVO_Z0) * s


def servo_box():
    """Body, tabs, output boss and spline as boxes: the stand-in before the vendor model."""
    sv = P.servo
    z1 = G.SERVO_FACE_Z
    xc, yc = G.SERVO_X_CENTRE, G.SERVO_Y
    body = box(xc - sv.body_l / 2, xc + sv.body_l / 2, yc - sv.body_w / 2, yc + sv.body_w / 2, G.SERVO_Z0, z1)
    tabs = box(xc - sv.tab_l / 2, xc + sv.tab_l / 2, yc - sv.body_w / 2, yc + sv.body_w / 2, G.TAB_Z0, G.TAB_Z1)
    for x in G.TAB_HOLE_XS:
        tabs -= zcyl(sv.tab_hole_d / 2, G.TAB_Z0 - 0.1, G.TAB_Z1 + 0.1, x, yc)
    boss = zcyl(sv.boss_d / 2, z1 - 0.1, z1 + sv.boss_h, G.SERVO_X, yc)
    spline = zcyl(sv.spline_d / 2, z1 + sv.boss_h - 0.1, z1 + sv.boss_h + sv.spline_h, G.SERVO_X, yc)
    return body + tabs + boss + spline


def horn():
    """A single-arm horn on the spline, pointing at -x when open; its hole at the linkage radius."""
    sv, L = P.servo, P.linkage
    z1 = G.ARM_Z1
    h = zcyl(sv.horn_hub_d / 2, z1 - sv.horn_hub_h, z1, G.SERVO_X, G.SERVO_Y)
    h += _arm(G.SERVO_X, G.SERVO_Y, z1 - sv.horn_arm_t, z1, sv.horn_len, sv.horn_arm_w)
    h -= zcyl(sv.spline_d / 2, z1 - sv.horn_hub_h - 0.1, z1 + 0.1, G.SERVO_X, G.SERVO_Y)
    h -= zcyl(L.pin_d / 2 + 0.1, z1 - sv.horn_arm_t - 0.1, z1 + 0.1, G.SERVO_X - L.arm_r, G.SERVO_Y)  # M2 clearance
    return h


def barrel(x, y):
    """An E-Z connector on an arm: a swivel barrel on an M2 screw through the arm's hole, a cross hole for the
    wire; its screw and set screw are left out. Its pivot is the arm's hole."""
    L = P.linkage
    b = zcyl(L.barrel_d / 2, G.ARM_Z1, G.BARREL_Z1, x, y)
    b -= ycyl(L.wire_d / 2 + 0.05, y - L.barrel_d, y + L.barrel_d, x, G.WIRE_Z)
    return b


def barrels():
    """(crank's, horn's) at the open position."""
    x = g.flap_x - P.linkage.arm_r
    return barrel(x, 0.0), barrel(x, G.SERVO_Y)


def wire():
    """The Ø2 stainless pushrod, between the barrels' axes plus a little through each (held by the set screws)."""
    L = P.linkage
    x = g.flap_x - L.arm_r
    return ycyl(L.wire_d / 2, -1.0, G.SERVO_Y + 1.0, x, G.WIRE_Z)


def rod():
    """Flap shaft: 3 mm rod with a flat, from the blind pocket in the bottom wall to the crank hub's top."""
    return d_bore(g.flap_x, 0.0, -g.duct_r + 0.7, G.ARM_Z1 - 0.3)


def tube():
    """The smoker's intake tube, seated against the socket's shoulder, 40 mm of it."""
    t = P.tube
    return xcyl(t.od / 2, g.socket_x0, g.socket_x0 + t.free_len) - xcyl(t.id / 2, g.socket_x0 - 1, g.socket_x0 + t.free_len + 1)


def oring():
    """The ring as installed: stretched over the tube, lying on it, so the groove wall squeezes it."""
    c = P.coupling
    maj = (P.tube.od + c.oring_cs) / 2
    return Pos(c.oring_x, 0, 0) * Rot(0, 90, 0) * Torus(maj, c.oring_cs / 2)


def pcb():
    """The board, flat in the pan, plus a keep-out box for the tallest parts above it; the connectors on the
    -y edge are inside the keep-out."""
    p = P.pcb
    b = box(G.g.board_x0, G.g.board_x1, -p.y_len / 2, p.y_len / 2, G.PCB_Z0, G.PCB_Z1)
    top = box(G.g.board_x0, G.g.board_x1, -p.y_len / 2, p.y_len / 2, G.PCB_Z1, G.PCB_Z1 + p.top_h)
    return b, top


def plugs():
    """The probe plugs and the USB-C plug in their ports: overmolds outside the port wall, barrels through it."""
    p = P.pcb
    xc = (G.g.board_x0 + G.g.board_x1) / 2
    out = None
    y_face = G.PORT_WALL_Y - P.shell.bay_wall_t
    for jx in p.jack_xs:
        barrel = ycyl(1.25, y_face - 0.5, G.PORT_WALL_Y, xc + jx, G.PCB_Z1 + p.jack_h / 2)
        mold = ycyl(p.plug_d / 2, y_face - 18.0, y_face, xc + jx, G.PCB_Z1 + p.jack_h / 2)
        out = barrel + mold if out is None else out + barrel + mold
    usb = box(xc + p.usb_x - 4.2, xc + p.usb_x + 4.2, y_face - 0.1, G.PORT_WALL_Y, G.PCB_Z1 + 0.3, G.PCB_Z1 + p.usb_h)
    usb += box(xc + p.usb_x - 6.0, xc + p.usb_x + 6.0, y_face - 18.0, y_face, G.PCB_Z1 - 1.5, G.PCB_Z1 + p.usb_h + 1.5)
    return out + usb
