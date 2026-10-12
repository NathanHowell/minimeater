"""The printed chassis parts, in the assembly frame (mm). `PARTS` maps a name to a builder.

The chassis is the duct with the fan on its flange, the servo on its shelf and the flap linkage: a working
unit on its own. The shells (shell.py) go around it.
"""

import math

from build123d import (
    Align,
    Axis,
    Box,
    Cone,
    Cylinder,
    Plane,
    Sphere,
    Polyline,
    Pos,
    Rot,
    extrude,
    make_face,
)

from . import geom as G
from .geom import g
from .params import P

C, MIN = Align.CENTER, Align.MIN


def box(x0, x1, y0, y1, z0, z1):
    """An axis-aligned box between the two corners, in either order."""
    return Pos((x0 + x1) / 2, (y0 + y1) / 2, (z0 + z1) / 2) * Box(abs(x1 - x0), abs(y1 - y0), abs(z1 - z0))


def xcyl(r, x0, x1, y=0.0, z=0.0):
    return Pos(x0, y, z) * Rot(0, 90, 0) * Cylinder(r, x1 - x0, align=(C, C, MIN))


def ycyl(r, y0, y1, x=0.0, z=0.0):
    return Pos(x, y0, z) * Rot(-90, 0, 0) * Cylinder(r, y1 - y0, align=(C, C, MIN))


def xcone(r0, r1, x0, x1):
    """Cone along +X, radius r0 at x0 and r1 at x1."""
    return Pos(x0, 0, 0) * Rot(0, 90, 0) * Cone(r0, r1, x1 - x0, align=(C, C, MIN))


def zcyl(r, z0, z1, x=0.0, y=0.0):
    return Pos(x, y, z0) * Cylinder(r, z1 - z0, align=(C, C, MIN))


def prism_xy(pts, z0, z1):
    """The (x, y) polygon extruded over z0..z1."""
    return Pos(0, 0, z0) * extrude(make_face(Polyline(*pts, close=True)), amount=z1 - z0)


def prism_xz(pts, y0, y1):
    """The (x, z) polygon extruded over y0..y1."""
    # Plane.XZ's normal is -Y: the face sits at y = y1 and is extruded toward -Y down to y0
    face = make_face(Plane.XZ.offset(-y1) * Polyline(*pts, close=True))
    return extrude(face, amount=y1 - y0, dir=(0, -1, 0))


def teardrop_z(x, y, r, z0, z1, toward=-1):
    """A Z cylinder with a 44 degree point toward `toward` x. Point it up (the print direction) for a hole,
    down for a column: the face under the round then prints."""
    k = r * math.sqrt(0.5)  # the tangent points at 45 degrees, the apex beyond them
    tri = make_face(Polyline((x + toward * k, y - k), (x + toward * k * (1 + P.layout.slope), y), (x + toward * k, y + k), close=True))
    return zcyl(r, z0, z1, x, y) + Pos(0, 0, z0) * extrude(tri, amount=z1 - z0)


def sock_r():
    return (P.tube.od + 0.3) / 2


def groove_r():
    return sock_r() + P.coupling.groove_depth


def d_bore(x, y, z0, z1, clear=0.0):
    """A D-shaped bore along Z: the rod's round with its flat."""
    d, flat = P.damper.shaft_d, P.damper.shaft_flat
    r = d / 2 + clear / 2
    # the flat is `flat` across from the opposite side of the round: material is kept below y_flat
    y_flat = y + flat - d / 2 + clear / 2
    return zcyl(r, z0, z1, x, y) - box(x - r - 1, x + r + 1, y_flat, y + r + 1, z0 - 1, z1 + 1)


# --- duct ---------------------------------------------------------------------------------------------------

SHELF_Y0 = 14.0  # the servo shelf starts inside the duct wall
SHELF_Y1 = G.SERVO_Y0 + P.servo.body_w + 0.2
SHELF_X0 = G.TAB_HOLE_XS[0] - 5.0
SHELF_X1 = G.TAB_HOLE_XS[1] + 2.3  # short of the shells' front cone
BOSS_Y0 = G.g.duct_r + 1.8  # the thumbscrew boss: from inside the socket wall out past it
BOSS_Y1 = P.coupling.od / 2 + 3.0


def servo_shelf():
    """A shelf under the servo's tabs, beside the duct on +y, with a cutout the body drops through and a
    boss under each tab screw. Its front end merges with the socket's step."""
    sv = P.servo
    z0, z1 = G.TAB_Z0 - G.PAD_T, G.TAB_Z0
    run = (G.PAD_T + 0.2) * P.layout.slope + 0.02
    s = box(SHELF_X0, SHELF_X1, SHELF_Y0, SHELF_Y1, z0, z1)
    s = s.fillet(3.0, s.edges().filter_by(Axis.Z).filter_by(lambda e: e.center().X > SHELF_X1 - 1))
    # faces toward -x hang in the print pose (flange down): the rear end and the cutout's front face lean 44 deg
    s -= prism_xz([(SHELF_X0 - 0.1, z0 - 0.1), (SHELF_X0 - 0.1 + run, z0 - 0.1), (SHELF_X0 - 0.1, z1 + 0.1)], SHELF_Y0 - 1, SHELF_Y1 + 1)
    cut_x0 = G.SERVO_X_CENTRE - sv.body_l / 2 - 0.2
    cut_x1 = G.SERVO_X_CENTRE + sv.body_l / 2 + 0.2
    s -= box(cut_x0, cut_x1, G.SERVO_Y0 - 0.2, SHELF_Y1 + 1, z0 - 1, z1 + 1)
    s -= prism_xz([(cut_x1 - 0.01, z0 - 0.1), (cut_x1 - 0.01 + run, z1 + 0.1), (cut_x1 - 0.01, z1 + 0.1)],
                  G.SERVO_Y0 - 0.2, SHELF_Y1 + 1)
    for x in G.TAB_HOLE_XS:
        s += teardrop_z(x, G.SERVO_Y, 2.6, z0 - 4.0, z0 + 0.1)
    s -= box(cut_x0, cut_x1, G.SERVO_Y0 - 0.2, SHELF_Y1 + 1, z0 - 5, z0 + 0.1)  # the bosses stop at the body
    for x in G.TAB_HOLE_XS:
        s -= zcyl(0.85, z0 - 3.5, z1 + 0.1, x, G.SERVO_Y)  # M2 pilot, 6 mm
    return s


def thumbscrew_boss():
    """A block on the socket's +y side, flush with the mouth (the print bed), rounded on its -x edges; it takes
    an M4 heat-set insert for the thumbscrew."""
    c = P.coupling
    hw = c.boss_w / 2
    b = box(c.boss_x - hw, 0.0, BOSS_Y0, BOSS_Y1, -hw, hw)
    b = b.fillet(hw - 0.8, b.edges().filter_by(Axis.Y).filter_by(lambda e: e.center().X < c.boss_x))
    return b


def duct():
    """Fan flange, contraction cone, the straight bore with the flap station, the servo shelf and the socket
    onto the intake tube, with its drip collar and the thumbscrew boss."""
    c = P.coupling
    br, dr, w = g.bore_r, g.duct_r, g.flange_w
    xf = g.flap_x
    flange = box(g.fan_face_x, g.fan_face_x + g.flange_t, -w / 2, w / 2, -w / 2, w / 2)
    flange = flange.fillet(g.flange_r, flange.edges().filter_by(Axis.X))
    outer = flange
    outer += xcone(w / 2, dr, g.fan_face_x + g.flange_t, g.straight_x0)
    k = P.layout.slope
    flare = (c.od / 2 - dr) * k  # the socket's step, at 44 degrees: it prints flange down
    outer += xcyl(dr, g.straight_x0, g.socket_x0 - flare + 0.01)
    outer += xcone(dr, c.od / 2, g.socket_x0 - flare, g.socket_x0)
    outer += xcyl(c.od / 2, g.socket_x0, 0.0)
    # drip collar behind the hood's front ring: its front face seats on the shells' front cone, its back is
    # chamfered for the print
    dc = c.collar_r - c.od / 2
    outer += xcone(c.od / 2, c.collar_r, c.collar_x - 1.5 - 1.6 - dc * k, c.collar_x - 1.5 - 1.6)
    outer += xcyl(c.collar_r, c.collar_x - 1.5 - 1.6, c.collar_x - 1.5)
    outer += xcone(c.collar_r, c.od / 2, c.collar_x - 1.5, c.collar_x)
    outer += thumbscrew_boss()
    body = outer + servo_shelf()

    # the airway and the features cut into the walls
    body -= xcone(P.fan.aperture_d / 2, br, g.fan_face_x - 0.1, g.straight_x0)
    body -= xcyl(br, g.straight_x0 - 0.01, g.socket_x0 + 0.01)
    body -= xcyl(sock_r(), g.socket_x0, 0.5)
    body -= xcone(br, sock_r() + 0.01, g.socket_x0 - (sock_r() - br) * k, g.socket_x0 + 0.01)  # the shoulder, 44 deg
    body -= xcone(sock_r(), sock_r() + c.lead_in, -c.lead_in, 0.0)  # lead-in chamfer at the mouth
    body -= xcyl(groove_r(), c.oring_x - 1.6, c.oring_x + 1.6)
    body -= zcyl(1.7, -dr + 0.6, dr + 1.0, xf, 0.0)  # shaft bore, blind at the bottom
    body -= ycyl(2.8, BOSS_Y0 + 2.0, BOSS_Y1 + 0.1, c.boss_x, 0.0)  # M4 heat-set insert for the thumbscrew
    body -= ycyl(2.1, br - 0.5, BOSS_Y0 + 2.1, c.boss_x, 0.0)  # the screw reaches the tube
    for sy in (-1, 1):
        for sz in (-1, 1):
            body -= xcyl(1.3, g.fan_face_x - 0.1, g.fan_face_x + 9.0, sy * P.fan.hole_pitch / 2, sz * P.fan.hole_pitch / 2)
    return body


# --- flap and its linkage ----------------------------------------------------------------------------------


def flap():
    """Butterfly flap: a disc with a square rib along the shaft axis (Z), open = parallel to the flow.

    The disc sits on one side of the rib (flush with its bottom face) so the flap prints flat with no support.
    """
    dm = P.damper
    off = (dm.rib_t - dm.flap_t) / 2  # the disc centre is this far from the shaft axis
    r = math.sqrt((g.bore_r - dm.flap_gap) ** 2 - off**2)  # its edge stays within the gap at every angle
    xf = g.flap_x
    disc = Pos(xf, -off, 0) * Rot(90, 0, 0) * Cylinder(r, dm.flap_t, align=(C, C, C))
    rib = box(xf - dm.rib_w / 2, xf + dm.rib_w / 2, -dm.rib_t / 2, dm.rib_t / 2, -r, r) & (Pos(xf, 0, 0) * Sphere(r))
    f = disc + rib
    f -= d_bore(xf, 0.0, -r - 1, r + 1, clear=P.print.press)
    return f


def _arm(x, y, z0, z1, r, w, toward=-1.0):
    """A flat arm from (x, y) out to radius r along `toward` x, rounded ends, between z0 and z1."""
    tip = x + toward * r
    bar = box(min(x, tip), max(x, tip), y - w / 2, y + w / 2, z0, z1)
    return bar + zcyl(w / 2, z0, z1, x, y) + zcyl(w / 2, z0, z1, tip, y)


def crank():
    """Machined aluminium, on the rod's top end: a hub clamped on the rod's flat by an M2 set screw, and an arm out
    to the tapped M2 hole for the E-Z connector's screw. Open (0 deg) points at -x."""
    L = P.linkage
    xf = g.flap_x
    k = zcyl(L.hub_d / 2, G.CRANK_Z0, G.ARM_Z1, xf, 0.0)
    k += _arm(xf, 0.0, G.ARM_Z0, G.ARM_Z1, L.arm_r, L.arm_w)
    k -= d_bore(xf, 0.0, G.CRANK_Z0 - 0.5, G.ARM_Z1 + 0.5, clear=L.slip)
    k -= zcyl(L.set_screw_d / 2, G.ARM_Z0 - 0.1, G.ARM_Z1 + 0.1, xf - L.arm_r, 0.0)  # tapped M2 for the barrel's screw
    zs = (G.CRANK_Z0 + G.ARM_Z0) / 2  # the set screw: radial, on the flat's side (+y), in the hub under the arm
    k -= ycyl(L.set_screw_d / 2, 0.0, L.hub_d, xf, zs)
    return k


PARTS = {"duct": duct, "flap": flap}  # printed
MACHINED = {"crank": crank}  # CNC aluminium, 5 off
