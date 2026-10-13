"""Stand-ins for the parts that are bought, for fit and interference checks (mm, assembly frame)."""

from functools import cache
from pathlib import Path

from build123d import Axis, Pos, Rot, Torus, import_step

from . import geom as G
from .geom import g
from .parts import _arm, box, d_bore, teardrop_z, xcyl, ycyl, zcyl
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
    for x, y in G.TAB_HOLES:
        tabs -= zcyl(sv.tab_hole_d / 2, G.TAB_Z0 - 0.1, G.TAB_Z1 + 0.1, x, y)
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
    h -= zcyl(L.thread_d / 2 + 0.1, z1 - sv.horn_arm_t - 0.1, z1 + 0.1, G.SERVO_X - L.arm_r, G.SERVO_Y)  # M2 clearance
    return h


def pin(x, y):
    """An M2 shoulder screw: the shank through the bar's hole from the arm's top, the head on the bar."""
    L = P.linkage
    return zcyl(L.pin_d / 2, G.ARM_Z1, G.COUPLER_Z1, x, y) + zcyl(L.pin_head_d / 2, G.COUPLER_Z1, G.COUPLER_Z1 + L.pin_head_h, x, y)


def pins():
    """(crank pin, horn pin) at the open position."""
    x = g.flap_x - P.linkage.arm_r
    return pin(x, 0.0), pin(x, G.SERVO_Y)


def rod():
    """Flap shaft: 3 mm rod with a flat, from the blind pocket in the bottom wall to 0.3 mm above the crank's top, to grip."""
    return d_bore(g.flap_x, 0.0, -g.duct_r + 1.3, G.ARM_Z1 + 0.3)  # P.damper.rod_len long (a test checks it)


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
    """The board, flat in the pan, turned over, plus a keep-out for what hangs under it (the right-angle headers are the
    tallest) and for the tails above it. The connectors on the -y edge hang past the board's trimmed edge into the port
    wall (their housings are `connectors()`)."""
    p = P.pcb
    b = box(G.g.board_x0, G.g.board_x1, G.PCB_EDGE_Y, p.y_len / 2, G.PCB_Z0, G.PCB_Z1)
    top = box(G.g.board_x0, G.g.board_x1, G.PCB_EDGE_Y, p.y_len / 2, G.PCB_Z1, G.PCB_Z1 + p.over_h)
    ye, he = p.edge_band
    xf = G.g.board_x1 - 1.5  # the pan's belly rises under the board's front edge: parts there are low
    under = box(G.g.board_x0, xf, -ye, ye, G.PCB_Z0 - p.under_h, G.PCB_Z0)
    under += box(G.g.board_x0, G.g.board_x1, G.PCB_EDGE_Y, -ye, G.PCB_Z0 - he, G.PCB_Z0)
    under += box(G.g.board_x0, G.g.board_x1, ye, p.y_len / 2, G.PCB_Z0 - he, G.PCB_Z0)
    under += box(xf, G.g.board_x1, -ye, ye, G.PCB_Z0 - he, G.PCB_Z0)
    xc = (G.g.board_x0 + G.g.board_x1) / 2
    for hx, hy in p.holes:  # the standoffs stand there
        under -= teardrop_z(xc + hx, hy, p.standoff_d / 2 + 0.3, G.PCB_Z0 - p.under_h - 1.0, G.PCB_Z0 + 0.1, toward=-1)
    top += under
    return b, top


def connectors():
    """The jack and USB-C housings past the trimmed edge, from their faces to the board's edge (the rest of them is on
    the board)."""
    p = P.pcb
    xc = (G.g.board_x0 + G.g.board_x1) / 2
    y0 = G.PCB_Y0
    out = None
    for jx in p.jack_xs:
        j = box(xc + jx - p.jack_w / 2, xc + jx + p.jack_w / 2, y0 + p.jack_face, G.PCB_EDGE_Y, G.PCB_Z0 - p.jack_h, G.PCB_Z0)
        out = j if out is None else out + j
    out += box(xc + p.usb_x - p.usb_w / 2, xc + p.usb_x + p.usb_w / 2, y0 + p.usb_face, G.PCB_EDGE_Y, G.PCB_Z0 - p.usb_h, G.PCB_Z0 - 0.3)
    return out


def plugs():
    """The probe plugs and the USB-C plug in their ports: overmolds outside the port wall, barrels through it to the
    connectors' faces (not into them); and the fan and servo plugs on the rear headers."""
    p = P.pcb
    xc = (G.g.board_x0 + G.g.board_x1) / 2
    out = None
    y_face = G.PORT_WALL_Y - P.shell.bay_wall_t
    for jx in p.jack_xs:
        barrel = ycyl(1.25, y_face - 0.5, G.PCB_Y0 + p.jack_face, xc + jx, G.PCB_Z0 - p.jack_h / 2)
        mold = ycyl(p.plug_d / 2, y_face - 18.0, y_face, xc + jx, G.PCB_Z0 - p.jack_h / 2)
        out = barrel + mold if out is None else out + barrel + mold
    usb = box(xc + p.usb_x - 4.2, xc + p.usb_x + 4.2, y_face - 0.1, G.PCB_Y0 + p.usb_face, G.PCB_Z0 - p.usb_h, G.PCB_Z0 - 0.3)
    usb += box(xc + p.usb_x - 6.0, xc + p.usb_x + 6.0, y_face - 18.0, y_face, G.PCB_Z0 - p.usb_h - 1.5, G.PCB_Z0 + 1.5)
    out += usb
    xf = G.g.board_x0 - 0.19  # the header housings' front faces, a little past the board's rear edge
    for yc, w in p.kk:  # the fan and servo plugs lie under the board, entering toward the rear
        out += box(xf - p.kk_plug_len, xf, yc - w / 2, yc + w / 2, G.PCB_Z0 - p.under_h, G.PCB_Z0 - 0.1)
    return out
