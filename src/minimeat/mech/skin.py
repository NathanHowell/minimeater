"""The exterior: two printed shells over the v0 internals (mm, assembly frame). See docs/mech.md, "Exterior".

The parting plane is the tube axis. The `hood` is everything above it, the `pan` everything below; each carries
its half of the flat rear plate, which has the inlet grille (both halves) and the connector row and label (hood).
The hood lifts off upward; the socket leaves through the half-round notch in its front and the thumbscrew
through the slot above it. Four M2.5 screws come up through columns in the pan into bosses in the hood. The hood
prints top down, the pan on its rear plate. The v0 `lid` is not used under the skin; the tray, grille and duct are.
"""

import math

from build123d import (
    Align,
    Axis,
    Box,
    Circle,
    Plane,
    Polyline,
    Pos,
    Rectangle,
    RectangleRounded,
    Rot,
    chamfer,
    extrude,
    fillet,
    loft,
    make_face,
    offset,
)

from . import geom as G
from .parts import PCB_Z1, box, xcyl, zcyl
from .params import P

C, MIN = Align.CENTER, Align.MIN
S = P.skin
X_SLAB = S.x_rear + S.slab_t  # the rear plate's inner face
X_FLANGE = G.g.fan_face_x + G.g.flange_t  # the bottom is flat back from here, then rises to the front
X_CAV_FRONT = S.x_front - S.wall
FACET_H = S.r_top * (2 - math.sqrt(2))  # the top edge: a facet this wide on the top, then the r_top roll
FACET_V = round(FACET_H * 1.12, 2)  # and this tall: 42 deg from vertical, so the hood prints top down


def _bottom_z(x):
    """Outer bottom at x: flat behind the flange, a straight rise to the front (an r80 blend between)."""
    if x <= X_FLANGE:
        return S.z_bot
    return S.z_bot + (S.z_bot_front - S.z_bot) * (x - X_FLANGE) / (S.x_front - X_FLANGE)


# --- the body: a plan outline extruded, cut to a side profile, then the top and bottom loops rounded ----------


def plan(inset=0.0):
    """Outline in XY: parallel to the shoulder, a straight taper to the front, blended. `inset` > 0 is the
    cavity's plan: the outline offset inward, with its rear at the rear plate's inner face."""
    hw_r, hw_f = S.hw_rear, S.hw_front
    pts = [(S.x_rear, -hw_r), (S.x_shoulder, -hw_r), (S.x_front, -hw_f), (S.x_front, hw_f), (S.x_shoulder, hw_r), (S.x_rear, hw_r)]
    f = make_face(Polyline(*pts, close=True))
    bul = S.x_nose - S.x_front
    if bul > 0.01:  # a circular segment on the front edge, apex at x_nose
        R = (hw_f**2 + bul**2) / (2 * bul)
        f = f + (Pos(S.x_nose - R, 0) * Circle(R) & Pos(S.x_front + 50, 0) * Rectangle(100, 100))
    f = fillet(f.vertices().filter_by(lambda v: abs(v.X - S.x_shoulder) < 0.1), S.r_shoulder)
    f = fillet(f.vertices().filter_by(lambda v: abs(v.X - S.x_front) < 0.1), S.r_front)
    if inset:
        f = offset(f, -inset) + Pos(X_SLAB + 5, 0) * Rectangle(10, 2 * (hw_r - inset))
    return f


def side(inset=0.0):
    """Profile in XZ: flat top, flat bottom to the flange, then the rise, blended with r80."""
    x_f = S.x_nose + 1
    pts = [(S.x_rear - 1, S.z_bot), (X_FLANGE, S.z_bot), (x_f, _bottom_z(x_f)), (x_f, S.z_top), (S.x_rear - 1, S.z_top)]
    f = make_face(Polyline(*pts, close=True))
    f = fillet(f.vertices().filter_by(lambda v: abs(v.X - X_FLANGE) < 0.1 and v.Y < 0), 80.0)
    if inset:
        f = offset(f, -inset) + Pos(X_SLAB + 5, (S.z_top + S.z_bot) / 2) * Rectangle(10, S.z_top - S.z_bot - 2 * inset)
    return Plane.XZ * f


def _is_rear_or_seam(e):
    """Y-parallel lines behind x = -20: the rear edges and the seams between the bottom faces. They stay sharp."""
    a, b = e.start_point(), e.end_point()
    return abs(a.Y - b.Y) > 1 and abs(a.X - b.X) < 0.01 and a.X < -20


def _at_z(e, z):
    bb = e.bounding_box()
    return abs(bb.max.Z - z) < 0.01 and abs(bb.min.Z - z) < 0.01


def _rounded(body, rt, rb, z_top, z_low, facet=False):
    """Top loop: the facet then an rt roll (facet False: a plain rt fillet). Bottom loop: rb."""
    top = body.faces().filter_by(lambda f: abs(f.center().Z - z_top) < 0.01)[0]
    loop = [e for e in top.edges() if not _is_rear_or_seam(e)]
    if facet:
        body = chamfer(loop, FACET_H, FACET_V, reference=top)
        loop = [e for e in body.edges() if _at_z(e, z_top - FACET_V)]
    body = fillet(loop, rt)
    return fillet([e for e in body.edges() if e.bounding_box().max.Z < z_low and not _is_rear_or_seam(e)], rb)


def _section_2d():
    """The body's cross-section behind the shoulder, in (y, z) about the origin: the rear plate's outline."""
    hw, h = S.hw_rear, (S.z_top - S.z_bot) / 2
    pts = [(-hw, -h), (hw, -h), (hw, h - FACET_V), (hw - FACET_H, h), (-hw + FACET_H, h), (-hw, h - FACET_V)]
    f = make_face(Polyline(*pts, close=True))
    f = fillet(f.vertices().filter_by(lambda v: abs(v.Y - (h - FACET_V)) < 0.01), S.r_top)
    f = fillet(f.vertices().group_by(Axis.Y)[0], S.r_bot)
    return f


def outer():
    """The whole exterior as one solid."""
    body = Pos(0, 0, -100) * extrude(plan(), 200, dir=(0, 0, 1))
    body = body & extrude(side(), 60, both=True)
    body = _rounded(body, S.r_top, S.r_bot, S.z_top, S.z_bot_front + 2.0, facet=True)
    # the rear plate's bevel: a ruled loft from the section inset by `bevel` at the rear face out to the full one
    sec, zc = _section_2d(), (S.z_bot + S.z_top) / 2
    frus = loft([Plane.YZ.offset(S.x_rear - 0.01) * Pos(0, zc) * offset(sec, -S.bevel - 0.01),
                 Plane.YZ.offset(X_SLAB + 0.1) * Pos(0, zc) * offset(sec, 0.1)], ruled=True)  # 45 deg exactly
    return body & (frus + Pos(X_SLAB + 100, 0, 0) * Box(200, 200, 300))


def cavity():
    w = S.wall
    c = Pos(0, 0, -100) * extrude(plan(w), 200, dir=(0, 0, 1))
    c = c & extrude(side(w), 60, both=True)
    c = c & Pos(X_CAV_FRONT - 100, 0, 0) * Box(200, 200, 300)
    return _rounded(c, S.r_top - w, S.r_bot - w, S.z_top - w, S.z_bot_front + 2.0 + w)


def nose_opening():
    """The socket's hole through the front, with the slot above it for the thumbscrew boss and knob."""
    x0 = G.g.socket_x0 - 1.0
    hole = xcyl(S.nose_hole_d / 2, x0, 2.0)
    return hole + box(x0, 2.0, -S.key_w / 2, S.key_w / 2, 0.0, S.key_z1)


# --- fixings ------------------------------------------------------------------------------------------------


def cavity_half_width(x):
    """Inner half width at x on the straight part of the wall (z near the parting plane)."""
    strip = plan(S.wall) & Pos(x, 0) * Rectangle(0.02, 200)
    return strip.bounding_box().max.Y


def _boss_profile(x, y_in, y_out):
    """Plan of a screw column: `boss_w` wide at the inner end, a 45 degree face on the -x side so it prints when
    the rear plate is on the bed, merged into the wall between y_in and y_out (same sign)."""
    h = S.boss_w / 2
    d = abs(y_out - y_in)
    return make_face(Polyline((x - h - d, y_out), (x + h, y_out), (x + h, y_in), (x - h, y_in), close=True))


def _teardrop(x, y, r, z0, z1):
    """A Z bore with a 45 degree point toward +x: a hole that prints when the rear plate is on the bed."""
    k = r * math.sqrt(0.5)
    tri = make_face(Polyline((x, y - k), (x + 2 * k, y), (x, y + k), close=True))
    return zcyl(r, z0, z1, x, y) + Pos(0, 0, z0) * extrude(tri, amount=z1 - z0)


def boss_points():
    return [(x, s * (cavity_half_width(x) - S.boss_w / 2)) for x in S.boss_xs for s in (-1, 1)]


def _columns(z0, z1, clip, gusset=0.0):
    """The screw blocks between z0 and z1, clipped to `clip`. `gusset` > 0 adds a 45 degree wedge above z1 back
    to the wall, so the block's top prints when the hood is upside down."""
    cols = None
    for x in S.boss_xs:
        hw = cavity_half_width(x)
        for s in (-1, 1):
            prof = _boss_profile(x, s * (hw - S.boss_w), s * (hw + 3.0))
            col = Pos(0, 0, z0) * extrude(prof, amount=z1 - z0)
            if gusset:
                tri = make_face(Polyline((s * (hw - S.boss_w), z1 - 0.01), (s * (hw + 3.0), z1 - 0.01),
                                         (s * (hw + 3.0), z1 + gusset), close=True))
                col += (Pos(x - 20, 0, 0) * extrude(Plane.YZ * tri, amount=40)) & (Pos(0, 0, z0) * extrude(prof, amount=z1 - z0 + gusset))
            cols = col if cols is None else cols + col
    return cols & clip


# --- the two shells -----------------------------------------------------------------------------------------


def _shell():
    return outer() - cavity() - nose_opening() - _grille_cut()


def _grille_cut():
    """Vertical slots in the rear plate, in line with the inner grille's, inside a rounded square around the
    fan's axis. They stop 1.6 mm short of the parting plane: the two halves of the plate meet in a bar."""
    field = extrude(Plane.YZ.offset(S.x_rear - 1) * RectangleRounded(S.grille_field, S.grille_field, S.grille_r), amount=S.slab_t + 2)
    n = int(P.fan.size // S.grille_pitch)
    cut = None
    for i in range(n):
        y = (i - (n - 1) / 2) * S.grille_pitch
        for z0, z1 in ((-40.0, S.z_part - 1.6), (S.z_part + 1.6, 40.0)):
            s = box(S.x_rear - 1, X_SLAB + 1, y - S.grille_slot / 2, y + S.grille_slot / 2, z0, z1)
            cut = s if cut is None else cut + s
    return cut & field


def _half(shell, above):
    sgn = 1 if above else -1
    return shell & Pos(0, 0, S.z_part + sgn * 200) * Box(600, 600, 400)


def hood():
    h = _half(_shell(), True)
    o = outer()
    # screw bosses on the inside of the wall, above the pan's lip, a pilot hole each, a gusset over each
    z0 = S.z_part + S.lip_h + 0.2
    z1 = z0 + S.boss_h
    h += _columns(z0, z1, o, gusset=S.boss_w + 3.0)
    for (x, y) in boss_points():
        h -= _teardrop(x, y, 1.05, z0 - 0.1, z1 - 1.5)
    # ambient air window: four slots in the +y wall, in line with the tray's
    for i in range(4):
        x = G.TRAY_X0 + P.print.wall + 4.0 + i * 3.6
        h -= box(x, x + 1.6, 30.0, 45.0, PCB_Z1 + 1.0, PCB_Z1 + 9.0)
    # rear plate: the connector row, 1 mm bigger than the tray's openings behind it, and the label recess above
    pc = P.pcb
    zj, zu = PCB_Z1 + pc.jack_h / 2, PCB_Z1 + pc.usb_h / 2
    usb = Plane.YZ.offset(S.x_rear - 1) * Pos(pc.usb_y, zu) * RectangleRounded(pc.usb_open_w + 1.0, pc.usb_open_h + 1.0, 1.5)
    h -= extrude(usb, amount=S.slab_t + 2)
    for jy in pc.jack_ys:
        h -= xcyl((pc.jack_hole_d + 1.0) / 2, S.x_rear - 1, X_SLAB + 1, jy, zj)
    lz, lw, lh = S.label
    h -= extrude(Plane.YZ.offset(S.x_rear - 1) * Pos(0, lz) * RectangleRounded(lw, lh, 2.0), amount=1.5)
    return h


def pan():
    p = _half(_shell(), False)
    o = outer()
    # the lip the hood locates on, along the sides
    cp = plan(S.wall)
    lip = Pos(0, 0, S.z_part) * extrude(offset(cp, -0.2) - offset(cp, -0.2 - S.lip_t), amount=S.lip_h)
    lip &= box(-106.0, -26.0, -60, 60, S.z_part - 1, S.z_part + S.lip_h + 1) - box(-200, 50, -20, 20, -100, 100)
    p += lip
    # screw columns from the floor up to the lip height, inside the hood's wall above the parting plane
    inside = Pos(0, 0, S.z_part - 1) * extrude(offset(cp, -0.2), amount=S.lip_h + 2)
    p += _columns(-60.0, S.z_part, o) + _columns(S.z_part - 0.01, S.z_part + S.lip_h, inside)
    for (x, y) in boss_points():
        p -= _teardrop(x, y, 1.6, -60.0, S.z_part + S.lip_h + 0.1)
        p -= _teardrop(x, y, 3.0, -60.0, -8.0)  # the head of the M2.5 x 20 and the driver, from below
    return p


SKIN = {"hood": hood, "pan": pan}

# print poses (assembly frame -> bed), like printing.POSES
POSES = {
    "hood": Rot(180, 0, 0),  # top on the bed: the facet-and-roll top edge is there for this
    "pan": Rot(0, -90, 0),  # rear plate on the bed, nose up
}


def print_pose(name, shape=None):
    s = POSES[name] * (shape if shape is not None else SKIN[name]())
    return Pos(0, 0, -s.bounding_box().min.Z) * s


def grille_open_area():
    """mm^2 of the rear plate the fan breathes through."""
    return (_grille_cut() & box(S.x_rear, X_SLAB, -50, 50, -50, 50)).volume / S.slab_t


def internals(flap_deg=0.0):
    """What the skin must clear: every v0 part but the lid, and the bought parts with the board's keep-out."""
    from . import assembly as A

    parts = {**A.printed(flap_deg), **A.bought(flap_deg)}
    parts.pop("lid")
    return parts


def envelope():
    """Bounding box sizes (dx, dy, dz): the skin alone, and with the exposed socket."""
    skin = hood() + pan()
    b = skin.bounding_box()
    s = (skin + internals()["duct"]).bounding_box()
    return tuple(round(v, 1) for v in b.size), tuple(round(v, 1) for v in s.size)
