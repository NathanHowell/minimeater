"""Stand-ins for the parts that are bought, for fit and interference checks (mm, assembly frame)."""

from build123d import Axis, Box, Cylinder, Pos, Rot, Torus

from . import geom as G
from .geom import g
from .parts import PCB_X0, PCB_Z0, PCB_Z1, box, d_bore, xcyl, zcyl
from .params import P


def fan():
    """The fan frame and hub (blades omitted): 60 x 60 x 25 with an aperture and four corner holes."""
    f = P.fan
    s = box(G.FAN_X0, g.fan_face_x, -f.size / 2, f.size / 2, -f.size / 2, f.size / 2)
    s = s.fillet(f.corner_r, s.edges().filter_by(Axis.X))
    s -= xcyl(f.aperture_d / 2, G.FAN_X0 - 0.1, g.fan_face_x + 0.1)
    s += xcyl(f.hub_d / 2, G.FAN_X0, g.fan_face_x)
    for sy in (-1, 1):
        for sz in (-1, 1):
            s -= xcyl(f.hole_d / 2, G.FAN_X0 - 0.1, g.fan_face_x + 0.1, sy * f.hole_pitch / 2, sz * f.hole_pitch / 2)
    return s


def servo():
    sv = P.servo
    xf = g.flap_x
    z0 = G.SERVO_FACE_Z
    xc = G.SERVO_X_CENTRE
    body = box(xc - sv.body_l / 2, xc + sv.body_l / 2, -sv.body_w / 2, sv.body_w / 2, z0, z0 + sv.body_h)
    tab_z0 = z0 + sv.tab_to_face
    tabs = box(xc - sv.tab_l / 2, xc + sv.tab_l / 2, -sv.body_w / 2, sv.body_w / 2, tab_z0, tab_z0 + sv.tab_t)
    for s in (-1, 1):
        tabs -= zcyl(sv.tab_hole_d / 2, tab_z0 - 0.1, tab_z0 + sv.tab_t + 0.1, xc + s * sv.tab_hole_pitch / 2, 0.0)
    boss = zcyl(sv.boss_d / 2, z0 - sv.boss_h, z0 + 0.1, xf, 0.0)
    spline = zcyl(sv.spline_d / 2, z0 - sv.boss_h - sv.spline_h, z0 - sv.boss_h + 0.1, xf, 0.0)
    return body + tabs + boss + spline


def horn():
    """Round horn on the spline, conservatively a solid disc."""
    return zcyl(P.servo.horn_d / 2, G.HORN_Z0, G.HORN_Z1, g.flap_x, 0.0) - zcyl(
        P.servo.spline_d / 2, G.HORN_Z0 - 0.1, G.HORN_Z1 + 0.1, g.flap_x, 0.0
    )


def rod():
    """Flap shaft: 3 mm rod with a flat, from the blind pocket in the bottom wall to the link hub."""
    xf = g.flap_x
    return d_bore(xf, 0.0, -g.duct_r + 0.7, G.LINK_Z0 + 3.6)


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
    """The board plus a keep-out box for the tallest parts above it and the lead clearance below."""
    p = P.pcb
    b = box(PCB_X0, PCB_X0 + p.x_len, -p.y_len / 2, p.y_len / 2, PCB_Z0, PCB_Z1)
    top = box(PCB_X0, PCB_X0 + p.x_len, -p.y_len / 2, p.y_len / 2, PCB_Z1, PCB_Z1 + p.top_h)
    return b, top
