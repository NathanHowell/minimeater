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
SHELF_X0 = G.TAB_HOLE_XS[0] - 6.5  # the rear end leans 44 deg: room for the pilots to reach full depth
SHELF_X1 = G.TAB_HOLE_XS[1] + 2.3  # short of the shells' front cone
BLOCK_D = 7.0  # the blocks under the tabs, from the tab's underside: a 6 mm pilot and a 1 mm floor
BOSS_Y0 = G.g.duct_r + 1.8  # the thumbscrew boss: from inside the socket wall out past it
BOSS_Y1 = P.coupling.od / 2 + 3.0


def servo_shelf():
    """A shelf under the servo's tabs, beside the duct on +y, with a cutout the body drops through and a solid block
    under each end's tab, with a M2 pilot under each of the six screw holes (2, 4 or 6 screws, chosen in the field).
    The corners of the shelf and both blocks away from the duct are rounded (R3; the duct side stays square) straight down to the blocks' base. The rear end of the shelf and its block lean 44 deg; the block's face toward the body at the front is a vertical
    12 mm wide bridge, as short as the block is deep. Its front end merges with the socket's step."""
    sv = P.servo
    z1 = G.TAB_Z0
    z0 = z1 - G.PAD_T
    zb = z1 - BLOCK_D
    def round_ends(b):  # the plan's free corners (the side away from the duct), at both ends of the shelf, 3 mm radius; the blocks take the same curve to their base
        far = lambda e: e.center().Y > (SHELF_Y0 + SHELF_Y1) / 2
        return b.fillet(3.0, b.edges().filter_by(Axis.Z).filter_by(far).filter_by(lambda e: e.center().X > SHELF_X1 - 1 or e.center().X < SHELF_X0 + 1))

    s = round_ends(box(SHELF_X0, SHELF_X1, SHELF_Y0, SHELF_Y1, z0, z1))
    cut_x0 = G.SERVO_X_CENTRE - sv.body_l / 2 - 0.2
    cut_x1 = G.SERVO_X_CENTRE + sv.body_l / 2 + 0.2
    by0 = G.SERVO_Y0 - 0.1  # the blocks stand under the tabs, the width of the body
    s += round_ends(box(SHELF_X0, cut_x0, by0, SHELF_Y1, zb, z0 + 0.01))
    s += round_ends(box(cut_x1, SHELF_X1, by0, SHELF_Y1, zb, z0 + 0.01))
    # the rear end faces toward -x and hangs in the print pose (flange down): it leans 44 deg, ending in a land
    land = 1.2  # the lean stops this far under the shelf's top: a land, not a knife edge
    lean = (z1 - land - zb) * P.layout.slope + 0.02
    s -= prism_xz([(SHELF_X0 - 0.1, zb - 0.1), (SHELF_X0 - 0.1 + lean, zb - 0.1), (SHELF_X0 - 0.1, z1 - land)], SHELF_Y0 - 1, SHELF_Y1 + 1)
    s -= box(cut_x0, cut_x1, G.SERVO_Y0 - 0.2, SHELF_Y1 + 1, zb - 1, z1 + 1)  # the body's cutout
    for x, y in G.TAB_HOLES:
        s -= zcyl(0.85, zb + 1.0, z1 + 0.1, x, y)  # M2 pilot, 6 mm
    return s


def thumbscrew_boss():
    """A block on the socket's +y side, flush with the mouth (the print bed), rounded on its -x edges; it takes
    an M4 heat-set insert for the thumbscrew."""
    c = P.coupling
    hw = c.boss_w / 2
    b = box(c.boss_x - hw, 0.0, BOSS_Y0, BOSS_Y1, -hw, hw)
    b = b.fillet(hw - 0.8, b.edges().filter_by(Axis.Y).filter_by(lambda e: e.center().X < c.boss_x))
    return b


def stop_posts():
    """Two posts on the duct's crown that the crank's lug stops against: open (0 deg) the lug lies along -x and its +y face
    meets the first post; closed (90 deg) the lug lies along -y and its +x face meets the second. Both stand beside the
    lug's sweep, under the arm. The face the lug hits is vertical or faces away from the print bed; the end of each
    post toward the rod faces the bed: a 3 mm bridge."""
    L = P.linkage
    xf, z1 = g.flap_x, G.ARM_Z0 - L.stop_top
    hw = L.stop_w / 2
    open_post = box(xf - L.stop_r1, xf - L.stop_r0, hw, hw + L.stop_t, L.stop_foot, z1)
    closed_post = box(xf + hw, xf + hw + L.stop_t, -L.stop_r1, -L.stop_r0, L.stop_foot, z1)
    return open_post + closed_post


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
    body = outer + servo_shelf() + stop_posts()

    # the airway and the features cut into the walls
    body -= xcone(P.fan.aperture_d / 2, br, g.fan_face_x - 0.1, g.straight_x0)
    body -= xcyl(br, g.straight_x0 - 0.01, g.socket_x0 + 0.01)
    body -= xcyl(sock_r(), g.socket_x0, 0.5)
    body -= xcone(br, sock_r() + 0.01, g.socket_x0 - (sock_r() - br) * k, g.socket_x0 + 0.01)  # the shoulder, 44 deg
    body -= xcone(sock_r(), sock_r() + c.lead_in, -c.lead_in, 0.0)  # lead-in chamfer at the mouth
    body -= xcyl(groove_r(), c.oring_x - 1.6, c.oring_x + 1.6)
    body -= zcyl(1.7, -dr + 1.2, dr + 1.0, xf, 0.0)  # shaft bore, blind at the bottom: a 1.2 mm skin
    body -= ycyl(2.8, BOSS_Y0 + 2.0, BOSS_Y1 + 0.1, c.boss_x, 0.0)  # M4 heat-set insert for the thumbscrew
    body -= ycyl(2.1, br - 0.5, BOSS_Y0 + 2.1, c.boss_x, 0.0)  # the screw reaches the tube
    for sy in (-1, 1):
        for sz in (-1, 1):
            body -= xcyl(1.3, g.fan_face_x - 0.1, g.fan_face_x + 9.0, sy * P.fan.hole_pitch / 2, sz * P.fan.hole_pitch / 2)
    return body


# --- flap and its linkage ----------------------------------------------------------------------------------


def flap():
    """Butterfly flap, machined (stainless or aluminium): a disc with a square rib along the shaft axis (Z), open =
    parallel to the flow, a D-bore for the rod's flat (retaining compound), no thread. The disc is centred on the
    rib (`Damper.rib_both_sides`; False puts it flush with the rib's bottom face, cut from plate with the rib standing on it)."""
    dm = P.damper
    off = dm.disc_off  # the disc centre is this far from the shaft axis
    r = math.sqrt((g.bore_r - dm.flap_gap) ** 2 - off**2)  # its edge stays within the gap at every angle
    xf = g.flap_x
    disc = Pos(xf, -off, 0) * Rot(90, 0, 0) * Cylinder(r, dm.flap_t, align=(C, C, C))
    rr = min(dm.rib_round, dm.rib_t / 2, dm.rib_w / 2)  # the rib's section is a stadium: round ends facing and leaving the flow
    xe = dm.rib_w / 2 - rr
    rib = box(xf - xe, xf + xe, -dm.rib_t / 2, dm.rib_t / 2, -r, r)
    rib += zcyl(rr, -r, r, xf - xe, 0.0) + zcyl(rr, -r, r, xf + xe, 0.0)
    rib = rib & (Pos(xf, 0, 0) * Sphere(r - (0.02 if dm.rib_both_sides else 0.0)))  # centred, the disc's edge circle is the sphere's equator: stay just inside it
    f = disc + rib
    f -= d_bore(xf, 0.0, -r - 1, r + 1, clear=dm.bore_clear)
    return f


def _arm(x, y, z0, z1, r, w, toward=-1.0):
    """A flat arm from (x, y) out to radius r along `toward` x, rounded ends, between z0 and z1."""
    tip = x + toward * r
    bar = box(min(x, tip), max(x, tip), y - w / 2, y + w / 2, z0, z1)
    return bar + zcyl(w / 2, z0, z1, x, y) + zcyl(w / 2, z0, z1, tip, y)


def crank():
    """Machined aluminium (or stainless), on the rod's top end: a hub clamped on the rod's flat by an M2 set screw, and an arm out
    to the tapped M2 hole for the coupler's pin. Open (0 deg) points at -x."""
    L = P.linkage
    xf = g.flap_x
    k = zcyl(L.hub_d / 2, G.CRANK_Z0, G.ARM_Z1, xf, 0.0)
    k += _arm(xf, 0.0, G.ARM_Z0, G.ARM_Z1, L.arm_r, L.arm_w)
    # the stop lug: a rib under the arm, level with the hub, hits the duct's posts at 0 deg (its +y face) and at 90 deg
    k += box(xf - L.stop_r1, xf - L.hub_d / 2 + 1.0, -L.stop_w / 2, L.stop_w / 2, G.CRANK_Z0, G.ARM_Z0 + 0.01)
    k -= d_bore(xf, 0.0, G.CRANK_Z0 - 0.5, G.ARM_Z1 + 0.5, clear=L.slip)
    k -= zcyl(L.set_screw_d / 2, G.ARM_Z0 - 0.1, G.ARM_Z1 + 0.1, xf - L.arm_r, 0.0)  # tapped M2 for the coupler's pin
    zs = (G.CRANK_Z0 + G.ARM_Z0) / 2  # the set screw: radial, on the flat's side (+y), in the hub under the arm
    k -= ycyl(L.set_screw_d / 2, 0.0, L.hub_d, xf, zs)
    return k


def coupler():
    """The bar between the crank's pin and the horn's pin, at the open position: 2 mm flat with a 1 mm boss under each hole."""
    L = P.linkage
    x = g.flap_x - L.arm_r
    bar = box(x - L.coupler_w / 2, x + L.coupler_w / 2, 0.0, G.SERVO_Y, G.COUPLER_Z0, G.COUPLER_Z1)
    bar += zcyl(L.coupler_w / 2, G.COUPLER_Z0, G.COUPLER_Z1, x, 0.0)
    bar += zcyl(L.coupler_w / 2, G.COUPLER_Z0, G.COUPLER_Z1, x, G.SERVO_Y)
    for y in (0.0, G.SERVO_Y):  # a boss under each hole, on the arm: the bar rides `bar_lift` above the arms
        bar += zcyl(L.coupler_w / 2, G.BOSS_Z0, G.COUPLER_Z0 + 0.01, x, y)
    for y in (0.0, G.SERVO_Y):
        bar -= zcyl(L.pin_d / 2 + L.pin_hole_clear, G.BOSS_Z0 - 0.1, G.COUPLER_Z1 + 0.1, x, y)
    return bar


PARTS = {"duct": duct}  # printed (the shells are in shell.py)
def rod():
    """The flap's shaft as a made part: Ø3 with a 2.5 mm D flat the full length, `rod_len` long, lying along +X from the
    origin (the assembly's own copy, `mock.rod`, stands along Z). JLCCNC, 6 off (5 + 1)."""
    L = P.damper.rod_len
    return Rot(0, 90, 0) * d_bore(0.0, 0.0, 0.0, L)


MACHINED = {"flap": flap, "crank": crank, "coupler": coupler, "rod": rod}  # CNC, 5 off (the rod 6)
