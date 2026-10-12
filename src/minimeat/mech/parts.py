"""The printed parts, in the assembly frame (mm). `PARTS` maps a name to a builder."""

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
    return Pos((x0 + x1) / 2, (y0 + y1) / 2, (z0 + z1) / 2) * Box(x1 - x0, y1 - y0, z1 - z0)


def xcyl(r, x0, x1, y=0.0, z=0.0):
    return Pos(x0, y, z) * Rot(0, 90, 0) * Cylinder(r, x1 - x0, align=(C, C, MIN))


def xcone(r0, r1, x0, x1):
    """Cone along +X, radius r0 at x0 and r1 at x1."""
    return Pos(x0, 0, 0) * Rot(0, 90, 0) * Cone(r0, r1, x1 - x0, align=(C, C, MIN))


def zcyl(r, z0, z1, x=0.0, y=0.0):
    return Pos(x, y, z0) * Cylinder(r, z1 - z0, align=(C, C, MIN))


def sock_r():
    return (P.tube.od + 0.3) / 2


def groove_r():
    return sock_r() + P.coupling.groove_depth


def _prism_xz(pts, y0, y1):
    """The (x, z) polygon extruded over y0..y1."""
    # Plane.XZ's normal is -Y: the face sits at y = y1 and is extruded toward -Y down to y0
    face = make_face(Plane.XZ.offset(-y1) * Polyline(*pts, close=True))
    return extrude(face, amount=y1 - y0, dir=(0, -1, 0))


def tower_profile(grow=0.0):
    """(x, z) outline of the servo tower: its fan-side face rises at 45 degrees, so it prints without
    support. `grow` offsets the outline outward (for clearance cuts)."""
    xf = g.flap_x
    x_top0 = xf - 13.5
    k = grow * math.sqrt(2.0)
    x1 = xf + 22.4  # clears the servo's far tab and its screw
    return [(x_top0 - (G.PLATE_TOP_Z - 12.0) - k, 12.0 - grow), (x_top0 - k, G.PLATE_TOP_Z + grow),
            (x1 + grow, G.PLATE_TOP_Z + grow), (x1 + grow, 12.0 - grow)]


def duct():
    """Fan flange, contraction cone, flap station, servo tower and the socket onto the intake tube."""
    c, sv = P.coupling, P.servo
    br, dr, w = g.bore_r, g.duct_r, g.flange_w
    xf = g.flap_x
    flange = box(g.fan_face_x, g.fan_face_x + g.flange_t, -w / 2, w / 2, -w / 2, w / 2)
    flange = flange.fillet(6.0, flange.edges().filter_by(Axis.X))
    outer = flange
    outer += xcone(w / 2, dr, g.fan_face_x + g.flange_t, g.straight_x0)
    flare = 6.0  # 4.8 mm of radius over 6 mm: steeper than 45 degrees from the print direction
    outer += xcyl(dr, g.straight_x0, g.socket_x0 - flare)
    outer += xcone(dr, c.od / 2, g.socket_x0 - flare, g.socket_x0)
    outer += xcyl(c.od / 2, g.socket_x0, 0.0)
    outer += zcyl(5.0, 0.0, c.od / 2 + 3.0, x=-5.0)  # thumbscrew boss on top of the socket

    tower = _prism_xz(tower_profile(), -13.5, 24.0)
    z_roof = G.PLATE_TOP_Z - G.PLATE_T
    tower -= zcyl(10.9, 10.0, z_roof, xf, 0.0)
    tower -= _gable(xf, 10.9, 10.0, z_roof)  # pointed roof for the print pose
    tower -= box(G.SERVO_X_CENTRE - (sv.body_l + 0.4) / 2, G.SERVO_X_CENTRE + (sv.body_l + 0.4) / 2,
                 -(sv.body_w + 0.4) / 2, (sv.body_w + 0.4) / 2, z_roof - 1, G.PLATE_TOP_Z + 1)
    for s in (-1, 1):
        tower -= zcyl(0.9, G.PLATE_TOP_Z - 7.0, G.PLATE_TOP_Z + 1, G.SERVO_X_CENTRE + s * sv.tab_hole_pitch / 2, 0.0)
    body = outer + tower

    # the airway and the features cut into the walls
    body -= xcone(P.fan.aperture_d / 2, br, g.fan_face_x - 0.1, g.straight_x0)
    body -= xcyl(br, g.straight_x0 - 0.01, g.socket_x0 + 0.01)
    body -= xcyl(sock_r(), g.socket_x0, 0.5)
    body -= xcone(sock_r(), sock_r() + c.lead_in, -c.lead_in, 0.0)  # lead-in chamfer at the mouth
    body -= xcyl(groove_r(), c.oring_x - 1.6, c.oring_x + 1.6)
    body -= zcyl(1.7, -dr + 0.6, dr + 1.0, xf, 0.0)  # shaft bore, blind at the bottom
    body -= zcyl(2.8, br, c.od / 2 + 4.0, x=-5.0)  # M4 heat-set insert for the thumbscrew
    for sy in (-1, 1):
        for sz in (-1, 1):
            body -= xcyl(1.3, g.fan_face_x - 0.1, g.fan_face_x + 9.0, sy * P.fan.hole_pitch / 2, sz * P.fan.hole_pitch / 2)
        body -= zcyl(1.05, w / 2 - 9.0, w / 2 + 0.1, g.fan_face_x + g.flange_t / 2, sy * 15.0)  # tray screws
    return body


def _gable(xf, r, z0, z1):
    """Diamond prism over the cavity circle, tangent to it at 45 degrees with its apex toward +x: a teardrop
    that prints without support when +x is up."""
    k = r * math.sqrt(0.5)
    tri = make_face(Polyline((xf, 0.0), (xf + k, k), (xf + 2 * k, 0.0), (xf + k, -k), close=True))
    return Pos(0, 0, z0) * extrude(tri, amount=z1 - z0)


def d_bore(x, y, z0, z1, clear=0.0):
    """A D-shaped bore along Z: the rod's round with its flat."""
    d, flat = P.damper.shaft_d, P.damper.shaft_flat
    r = d / 2 + clear / 2
    # the flat is `flat` across from the opposite side of the round: material is kept below y_flat
    y_flat = y + flat - d / 2 + clear / 2
    return zcyl(r, z0, z1, x, y) - box(x - r - 1, x + r + 1, y_flat, y + r + 1, z0 - 1, z1 + 1)


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


def link():
    """Joins the flap rod to a round servo horn: a D-bored hub below, a flange with the horn's screw circle above."""
    xf = g.flap_x
    hub = zcyl(4.0, G.LINK_Z0, G.LINK_FLANGE_Z0, xf, 0.0)
    fl = zcyl(P.damper.head_d / 2, G.LINK_FLANGE_Z0, G.LINK_FLANGE_Z1, xf, 0.0)
    ln = hub + fl
    ln -= d_bore(xf, 0.0, G.LINK_Z0 - 0.5, G.LINK_FLANGE_Z0 + 0.5, clear=P.print.press)
    for a in range(4):
        ax = xf + P.servo.horn_hole_r * math.cos(math.radians(90 * a))
        ay = P.servo.horn_hole_r * math.sin(math.radians(90 * a))
        ln -= zcyl(1.15, G.LINK_FLANGE_Z0 - 0.1, G.LINK_FLANGE_Z1 + 0.1, ax, ay)
    return ln


def grille():
    """Inlet guard on the fan's rear face: slots inside a circle, the fan's four screws through the corners."""
    f = P.fan
    w = f.size
    plate = box(G.GRILLE_X0, G.FAN_X0, -w / 2, w / 2, -w / 2, w / 2)
    plate = plate.fillet(f.corner_r, plate.edges().filter_by(Axis.X))
    open_area = xcyl(27.0, G.GRILLE_X0 - 0.1, G.FAN_X0 + 0.1)
    slots = None
    n = int(w // 4.6)
    for i in range(n):
        y = (i - (n - 1) / 2) * 4.6
        s = box(G.GRILLE_X0 - 0.1, G.FAN_X0 + 0.1, y - 1.3, y + 1.3, -w / 2, w / 2)
        slots = s if slots is None else slots + s
    plate -= slots & open_area
    for sy in (-1, 1):
        for sz in (-1, 1):
            plate -= xcyl(1.7, G.GRILLE_X0 - 0.1, G.FAN_X0 + 0.1, sy * f.hole_pitch / 2, sz * f.hole_pitch / 2)
    return plate


# --- electronics tray and lid --------------------------------------------------------------------
W = P.print.wall
PCB_X0 = G.TRAY_X0 + W + P.pcb.rear_gap  # rear edge of the board
TRAY_X1 = PCB_X0 + P.pcb.x_len + P.pcb.front_gap + W
TRAY_Z1 = G.TRAY_Z0 + W + G.TRAY_INNER_H
PCB_Z0 = G.TRAY_Z0 + W + G.TRAY_STANDOFF  # underside of the board
PCB_Z1 = PCB_Z0 + P.pcb.t
POSTS = [(x, s * (G.TRAY_W / 2 - W - 1.7)) for x in (PCB_X0 + 5.0, PCB_X0 + P.pcb.x_len - 5.0) for s in (-1, 1)]


def pcb_holes():
    p = P.pcb
    return [(PCB_X0 + sx, sy * (p.y_len / 2 - p.hole_inset))
            for sx in (p.hole_inset, p.x_len - p.hole_inset) for sy in (-1, 1)]


def tray():
    p = P.pcb
    ty = G.TRAY_W / 2
    t = box(G.TRAY_X0, TRAY_X1, -ty, ty, G.TRAY_Z0, TRAY_Z1)
    t = t.fillet(3.0, t.edges().filter_by(Axis.Z))
    t -= box(G.TRAY_X0 + W, TRAY_X1 - W, -ty + W, ty - W, G.TRAY_Z0 + W, TRAY_Z1 + 1)
    for (x, y) in pcb_holes():
        t += zcyl(3.0, G.TRAY_Z0 + W - 0.1, PCB_Z0, x, y)
        t -= zcyl(1.05, G.TRAY_Z0 + W + 0.5, PCB_Z0 + 0.1, x, y)
    for (x, y) in POSTS:
        t += box(x - 3.0, x + 3.0, y - 1.7 - 0.2, y + 1.7 + 0.2, G.TRAY_Z0 + W - 0.1, TRAY_Z1)
        t -= zcyl(1.05, TRAY_Z1 - 8.0, TRAY_Z1 + 0.1, x, y)
    # connector openings in the rear wall
    zc = PCB_Z1
    ow = G.TRAY_X0 - 0.1
    zu = zc + p.usb_h / 2
    t -= box(ow, ow + W + 0.2, p.usb_y - p.usb_open_w / 2, p.usb_y + p.usb_open_w / 2,
             zu - p.usb_open_h / 2, zu + p.usb_open_h / 2)
    for jy in p.jack_ys:
        t -= Pos(ow, jy, zc + p.jack_h / 2) * Rot(0, 90, 0) * Cylinder(p.jack_hole_d / 2, W + 0.2, align=(C, C, MIN))
    # ambient air window: slots in the +y wall beside the rear connectors, clear of the board's electronics
    for i in range(4):
        x = G.TRAY_X0 + W + 4.0 + i * 3.6
        t -= box(x, x + 1.6, ty - W - 0.1, ty + 0.1, PCB_Z1 + 1.0, PCB_Z1 + 9.0)
    # cables: fan lead through the floor, servo lead through the front wall
    t -= box(G.TRAY_X0 + 10, G.TRAY_X0 + 20, -ty + W + 1.0, -ty + W + 4.0, G.TRAY_Z0 - 0.1, G.TRAY_Z0 + W + 0.1)
    t -= box(TRAY_X1 - W - 0.1, TRAY_X1 + 0.1, -4.0, 4.0, PCB_Z1 + 4.0, PCB_Z1 + 8.0)
    t -= _prism_xz(tower_profile(0.6), -14.1, 24.6)  # the tower's sloped face passes under the front wall
    # screws down into the duct flange
    for sy in (-1, 1):
        x = g.fan_face_x + g.flange_t / 2
        t -= zcyl(1.4, G.TRAY_Z0 - 0.1, G.TRAY_Z0 + W + 0.1, x, sy * 15.0)
    return t


def lid():
    ty = G.TRAY_W / 2
    l = box(G.TRAY_X0, TRAY_X1, -ty, ty, TRAY_Z1, TRAY_Z1 + W)
    l = l.fillet(3.0, l.edges().filter_by(Axis.Z))
    lip = box(G.TRAY_X0 + W + 0.2, TRAY_X1 - W - 0.2, -ty + W + 0.2, ty - W - 0.2, TRAY_Z1 - 2.0, TRAY_Z1 + 0.1)
    lip -= box(G.TRAY_X0 + W + 1.6, TRAY_X1 - W - 1.6, -ty + W + 1.6, ty - W - 1.6, TRAY_Z1 - 2.1, TRAY_Z1 + 0.2)
    for (x, y) in POSTS:
        lip -= box(x - 3.4, x + 3.4, y - 2.5, y + 2.5, TRAY_Z1 - 2.1, TRAY_Z1 + 0.2)
    l += lip
    for (x, y) in POSTS:
        l -= zcyl(1.4, TRAY_Z1 - 0.1, TRAY_Z1 + W + 0.1, x, y)
    return l


PARTS = {"duct": duct, "flap": flap, "link": link, "grille": grille, "tray": tray, "lid": lid}
