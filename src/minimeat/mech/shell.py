"""The exterior: one lofted pebble around the chassis, split at the tube axis (mm, assembly frame).

The `hood` is everything above the parting plane, the `pan` everything below. The pan carries the board on
four standoffs and the port bay in its -y flank; the hood carries nothing but its half of the nose and grille.
The chassis (duct, fan, servo, linkage) is captured between them: its drip collar seats in the shells' front
cone, its fan on the bumps inside the rear wall. The hood lifts straight up off the socket's notch; the pan
drops away with the board, so the chassis can stay on the tube. Four screws from below. Both shells print
standing on the rear face, nose up: every long surface is vertical, and the features inside are fins along x
or teardrop columns. See docs/mech.md, "Exterior" and "Rain".
"""

import math

from build123d import (
    Axis,
    Box,
    Plane,
    Polyline,
    Pos,
    RectangleRounded,
    Rot,
    extrude,
    loft,
    make_face,
)

from . import geom as G
from .geom import g
from .params import P
from .parts import box, prism_xz, teardrop_z, xcyl, xcone, ycyl, zcyl

S = P.shell
W = S.wall
X_FRONT = P.layout.x_front
X_FRONT_IN = X_FRONT - S.front_t
NOSE_R = S.nose_hole_d / 2
KS = P.layout.slope
R_CORNER = 48.0  # beyond the inner sections' corners
X_CONE0 = X_FRONT_IN - (R_CORNER - NOSE_R) * KS  # the cavity's front cone covers the whole section from here
REAR_T = 3.2  # the rear wall: thicker, so the louvre blades are longer
X_REAR = G.REAR_IN_X - REAR_T
WEDGE_Z = -21.0  # the pan's front wedge starts under the nose hole's bottom, above the board's keep-out
GRILLE_ZS = tuple(s * (S.grille_bar + S.grille_slot / 2 + 0.5 + S.grille_pitch * i) for s in (-1, 1) for i in range(S.grille_n))


# --- the body ------------------------------------------------------------------------------------------------


def _ease(t):
    """Smooth 0 -> 1 over t in 0..1, flat at both ends."""
    t = min(max(t, 0.0), 1.0)
    return 0.5 - 0.5 * math.cos(math.pi * t)


X_FULL0 = X_REAR + 10.0  # the rear face grows to the full section over this length


def profile(x, crown=S.crown, peak_r=S.peak_r):
    """(w, h, r, zc) of the outer section at x: the rear face eases out to the full section, a raised-cosine
    bump to the peak, then the width and the roof ease down to the nose from `x_taper` and the belly rises
    from `x_belly`."""
    zb, zt = G.BELLY_Z, G.ROOF_Z
    if x <= X_FULL0:
        t = _ease((x - X_REAR) / (X_FULL0 - X_REAR))
        w, h, r = S.rear_w + (G.FULL_W - S.rear_w) * t, S.rear_h + (G.FULL_H - S.rear_h) * t, S.rear_r + (S.full_r - S.rear_r) * t
        return w, h, r, zb + h / 2
    b = _ease((x - X_FULL0) / (S.peak_x - X_FULL0)) if x < S.peak_x else 1.0 - _ease((x - S.peak_x) / (S.x_taper - S.peak_x))
    if x <= S.x_taper:
        w, r = G.FULL_W + 0.8 * b, S.full_r + (peak_r - S.full_r) * b
        z_top = zt + crown * b + (S.nose_top - zt) * _ease((x - S.x_roof) / (X_FRONT - S.x_roof))
        return w, z_top - zb, r, (z_top + zb) / 2
    t = _ease((x - S.x_taper) / (X_FRONT - S.x_taper))
    w, r = G.FULL_W + (S.nose_w - G.FULL_W) * t, S.full_r + (S.nose_r - S.full_r) * t
    z_top = zt + crown * b + (S.nose_top - zt) * _ease((x - S.x_roof) / (X_FRONT - S.x_roof))
    z_bot = zb + (S.nose_bot - zb) * _ease((x - S.x_belly) / (X_FRONT - S.x_belly))
    return w, z_top - z_bot, r, (z_top + z_bot) / 2


def stations():
    xs = [X_REAR, X_REAR + 3.0, X_REAR + 6.5, X_FULL0]
    x = X_FULL0
    while x < S.x_taper - 1.0:
        x += 6.0
        xs.append(min(x, S.x_taper))
    if xs[-1] < S.x_taper:
        xs.append(S.x_taper)
    n = 8
    xs += [S.x_taper + (X_FRONT - S.x_taper) * i / n for i in range(1, n + 1)]
    return xs


def sections(crown=S.crown, peak_r=S.peak_r):
    """[(x, w, h, r, zc)] at the stations."""
    return [(x, *profile(x, crown, peak_r)) for x in stations()]


def _loft(secs, inset=0.0, x0=None, x1=None):
    faces = []
    for i, (x, w, h, r, zc) in enumerate(secs):
        if x0 is not None and i == 0:
            x = x0
        if x1 is not None and i == len(secs) - 1:
            x = x1
        faces.append(Plane.YZ.offset(x) * Pos(0, zc) * RectangleRounded(w - 2 * inset, h - 2 * inset, r - inset))
    return loft(faces)


def outer(crown=S.crown, peak_r=S.peak_r):
    """The whole exterior as one solid: the loft, a 45 degree chamfer on the rear face and a round on the front."""
    secs = sections(crown, peak_r)
    body = _loft(secs)
    x = secs[0][0]
    c = S.rear_chamfer
    w1, h1, r1, zc1 = profile(x + c * KS, crown, peak_r)  # the loft's own section where the chamfer ends: no step
    frus = loft([Plane.YZ.offset(x - 0.01) * Pos(0, zc1) * RectangleRounded(w1 - 2 * c, h1 - 2 * c, r1 - c),
                 Plane.YZ.offset(x + c * KS) * Pos(0, zc1) * RectangleRounded(w1 + 0.1, h1 + 0.1, r1)], ruled=True)
    body = body & (frus + Pos(x + c * KS + 100, 0, 0) * Box(200, 300, 300))
    front = body.faces().filter_by(lambda f: abs(f.center().X - X_FRONT) < 0.01)[0]
    return body.fillet(S.nose_round, front.edges())


def cavity(extra=0.0, wedge=False):
    """The inside: the loft inset by the wall (plus `extra`), closed by the rear wall and, at the front, a 45
    degree cone down to the nose hole. With `wedge` (the pan) the cone is joined below the hole by a 45 degree
    plane, so the cavity keeps its lower corners back to the board's front edge; both faces print nose up."""
    secs = sections()
    c = _loft(secs, W + extra, x0=X_REAR - 5.0, x1=X_FRONT + 5.0)
    keep = box(G.REAR_IN_X - extra, X_CONE0, -100, 100, -100, 100)
    keep += xcone(100.0 + NOSE_R - extra, NOSE_R - extra, X_FRONT_IN - 100.0 * KS + extra, X_FRONT_IN + extra)
    if wedge:
        z = WEDGE_Z
        keep += prism_xz([(X_FRONT_IN + 0.01, z), (X_FRONT_IN - 100.0 * KS, z - 100.0), (X_FRONT_IN - 100.0 * KS, z + 0.01)], -50.0, 50.0)
    return c & keep


def nose_hole():
    return xcyl(NOSE_R, X_CONE0 + 1.0, 5.0)


# --- openings -------------------------------------------------------------------------------------------------


def louvre(z, x_face, t, length, slot, down_out=(1, 0, -1)):
    """A slot through a wall at x_face..x_face + t, `slot` tall, cut 45 degrees so that going outward (-x)
    it descends: spray cannot pass straight in, water inside runs out. Returns the cutter."""
    cut = Box(14.0, length, slot)
    return Pos(x_face + t / 2, 0, z + t / 2) * Rot(0, -44, 0) * cut


def grille_cut():
    """Ten louvres in the rear wall inside a rounded square around the fan's axis; they stop short of the
    parting plane so the two halves of the wall meet in a bar."""
    # the field is sheared along the louvres' tilt, so the top slot is open through the whole wall
    k = math.tan(math.radians(44))
    field = loft([Plane.YZ.offset(X_REAR + dx) * Pos(0, dx * k + REAR_T / 2 * (1 - k)) *
                  RectangleRounded(S.grille_field, S.grille_field + 4.0, S.grille_r) for dx in (-3.0, REAR_T + 4.0)], ruled=True)
    cut = None
    for z in GRILLE_ZS:
        s = louvre(z, X_REAR, REAR_T, 80.0, S.grille_slot)
        cut = s if cut is None else cut + s
    cut = cut & field
    return cut - box(X_REAR - 3, G.REAR_IN_X + 3, -50, 50, -S.grille_bar, S.grille_bar)


def vent_cut():
    """The ambient louvres in the pan's +y flank, under the board's sensor corner: slots along x, tilted so
    they descend outward."""
    cut = None
    for x0, x1 in S.vent_slots:
        s = Pos((x0 + x1) / 2, 30.0, S.vent_z) * Rot(-44, 0, 0) * Box(x1 - x0, 20.0, 1.4)
        cut = s if cut is None else cut + s
    return cut


def indicator_cuts():
    """Through the pan's +y flank, on the board's +y edge: a Ø1.9 hole for the LED's clear light pipe, a teardrop
    pointing +x so it prints standing on the rear face. The button is inside, under the hood (the case opens)."""
    p = P.pcb
    y0 = p.y_len / 2
    zl = G.PCB_Z1 + p.led_z
    xl = PCB_XC + p.led_x
    r = S.pipe_d / 2 + 0.075
    k = r * math.sqrt(0.5)
    return ycyl(r, y0, 50.0, xl, zl) + prism_xz([(xl + k, zl - k), (xl + k * (1 + KS), zl), (xl + k, zl + k)], y0, 50.0)


def label_cut():
    x, z, w, h = S.label
    f = Plane.XZ.offset(45.0) * Pos(x, z) * RectangleRounded(w, h, 2.5)
    return extrude(f, amount=45.0 - (G.FULL_W / 2 - S.label_depth), dir=(0, 1, 0))


# --- the port bay ---------------------------------------------------------------------------------------------

BAY_X0, BAY_X1 = S.bay_x
BAY_Z0, BAY_Z1 = S.bay_z
PORT_FACE_Y = G.PORT_WALL_Y - S.bay_wall_t  # the port wall's outer face
PCB_XC = (g.board_x0 + g.board_x1) / 2


def port_openings():
    """The USB-C opening and the four jack openings through the port wall, on the board's connector axes: the
    plug's round hole, which the jack's housing, standing in the wall, widens to a rectangle. The jack holes are
    teardrops pointed +x: they print standing on the rear face."""
    p = P.pcb
    y0, y1 = PORT_FACE_Y - 1.0, G.PORT_WALL_Y + 1.0
    zu = G.PCB_Z1 + p.usb_h / 2
    cut = box(PCB_XC + p.usb_x - p.usb_open_w / 2, PCB_XC + p.usb_x + p.usb_open_w / 2, y0, y1, zu - p.usb_open_h / 2, zu + p.usb_open_h / 2)
    cut = cut.fillet(1.5, cut.edges().filter_by(Axis.Y))
    zj = G.PCB_Z1 + p.jack_h / 2
    r = p.jack_hole_d / 2
    k = r * math.sqrt(0.5)
    for jx in p.jack_xs:
        x = PCB_XC + jx
        cut += ycyl(r, y0, y1, x, zj) + prism_xz([(x + k, zj - k), (x + k * (1 + KS), zj), (x + k, zj + k)], y0, y1)
        z0, z1 = p.jack_open_z
        cut += box(x - p.jack_open_w / 2, x + p.jack_open_w / 2, y0, y1, G.PCB_Z1 + z0, G.PCB_Z1 + z1)
        if jx == p.jack_xs[0]:  # the USB-C opening reaches to within 0.2 mm of this one: join them, a 0.2 mm web cannot print
            xu = PCB_XC + p.usb_x + p.usb_open_w / 2
            cut += box(xu - 0.3, x - r + 0.3, y0, y1, zj - r, zj + r)
    return cut


def bay_pocket():
    """What the bay removes from the pan's flank: everything outside the port wall between its floor, its
    ceiling (the eave) and its end walls; the front end wall is 45 degrees in plan so it prints."""
    pts = [(BAY_X0, PORT_FACE_Y), (BAY_X1, PORT_FACE_Y), (BAY_X1 + 8.0, PORT_FACE_Y - 8.0), (BAY_X1 + 8.0, -60.0), (BAY_X0, -60.0)]
    pk = Pos(0, 0, BAY_Z0) * extrude(make_face(Polyline(*pts, close=True)), amount=BAY_Z1 - BAY_Z0)
    return pk


def bay_ceiling():
    """The eave's underside: a 1.3 mm slab over the bay's footprint, from the ceiling up. Without it the cavity
    stayed open between the flank's inner face and the port wall, a 7 mm slot into the shell over every port."""
    pts = [(BAY_X0, PORT_FACE_Y), (BAY_X1, PORT_FACE_Y), (BAY_X1 + 8.0, PORT_FACE_Y - 8.0), (BAY_X1 + 8.0, -60.0), (BAY_X0 - W, -60.0), (BAY_X0 - W, PORT_FACE_Y)]
    return Pos(0, 0, BAY_Z1 - 0.01) * extrude(make_face(Polyline(*pts, close=True)), amount=S.lip_t + 0.01)


def bay_walls():
    """The port wall and the bay's rear end wall, added back inside the pocket, from the floor to the eave."""
    w = box(BAY_X0 - W, BAY_X1 + 0.5, PORT_FACE_Y, G.PORT_WALL_Y, G.BELLY_Z, BAY_Z1 + 0.5)
    d = G.PORT_WALL_Y + 40.0  # the end wall's inner part leans 44 degrees so it prints nose up
    end = Pos(0, 0, G.BELLY_Z) * extrude(make_face(Polyline((BAY_X0 - W - d * KS, -40.0), (BAY_X0, -40.0), (BAY_X0, G.PORT_WALL_Y),
                                                            (BAY_X0 - W, G.PORT_WALL_Y), close=True)), amount=BAY_Z1 + 0.5 - G.BELLY_Z)
    w += end
    return w


def drip_lip():
    """A ridge under the eave's outer edge, so water running down the flank drips clear of the ports."""
    return box(BAY_X0 - 1.0, BAY_X1 + 1.0, -G.FULL_W / 2 - 0.3, -G.FULL_W / 2 + 1.0, BAY_Z1 - 1.5, BAY_Z1 + 0.5)


# --- fixings and locators ---------------------------------------------------------------------------------------

FRONT_BOSS = (P.layout.front_screw_x, 28.0)
REAR_BOSS = (G.REAR_IN_X + P.layout.rear_gap / 2, S.rear_screw_y)
REAR_BOSS_D = 4.5  # M2: the fan's rear gap (4.5) sets it; the column stands in the fan shim block, so the channel's sleeve is the block
LAP = 1.2  # the rear wall's lip at the parting plane


def boss_points():
    return [(x, s * y, d) for (x, y), d in ((FRONT_BOSS, S.boss_d), (REAR_BOSS, REAR_BOSS_D)) for s in (-1, 1)]


def rear_lip(clear=0.0):
    """The rear wall's inner half rises LAP above the parting plane behind a 44 degree face (the pan); with
    `clear` it is the notch cut from the hood. Both faces print nose up."""
    x0 = X_REAR + REAR_T / 2 - clear
    h = LAP + clear
    return prism_xz([(x0, h), (G.REAR_IN_X + 1.0, h), (G.REAR_IN_X + 1.0, -0.01 - clear), (x0 + h * KS, -0.01 - clear)], -50.0, 50.0)


def fan_shims():
    """Blocks on the side walls beside the fan, from the rear wall: they locate the fan sideways and carry the
    rear screw columns. One pair per shell."""
    out = None
    for s in (-1, 1):
        for z0, z1 in ((4.0, 26.0), (-26.0, -4.0)):
            b = box(G.REAR_IN_X - 0.1, g.fan_face_x - 1.0, s * (P.fan.size / 2 + 0.4), s * 40.0, z0, z1)
            out = b if out is None else out + b
    return out


def rear_bumps():
    """Four pads on the rear wall's inner face the fan's rear face sits on (its screw heads are between them)."""
    out = None
    for sy in (-1, 1):
        for sz in (-1, 1):
            b = xcyl(2.0, G.REAR_IN_X - 0.1, G.FAN_X0 - 0.3, sy * 28.0, sz * 20.0)
            out = b if out is None else out + b
    return out


def flange_pads():
    """Two wedges on the pan floor under the fan flange; the flange rests on them. 45 degrees at the rear."""
    x0, x1 = g.fan_face_x - 0.5, g.fan_face_x + g.flange_t + 0.5
    zt = -g.flange_w / 2 - 0.3
    out = None
    for s in (-1, 1):
        pts = [(x0 - (zt - G.FLOOR_Z + 0.5) * KS, G.FLOOR_Z - 0.5), (x1, G.FLOOR_Z - 0.5), (x1, zt), (x0, zt)]
        w = prism_xz(pts, s * 10.0, s * 20.0) if s > 0 else prism_xz(pts, -20.0, -10.0)
        out = w if out is None else out + w
    return out


def standoffs():
    """Teardrop columns on the pan floor at the board's holes, M2.5 pilot."""
    p = P.pcb
    out = None
    for hx, hy in p.holes:
        x, y = PCB_XC + hx, hy
        c = teardrop_z(x, y, 2.75, G.FLOOR_Z - 2.0, G.PCB_Z0, toward=-1)
        c -= teardrop_z(x, y, 1.05, G.PCB_Z0 - 6.0, G.PCB_Z0 + 0.1, toward=1)
        out = c if out is None else out + c
    return out


def seam_x():
    """The flanks' labyrinth runs between the rear screw columns and the taper."""
    return G.REAR_IN_X + P.layout.rear_gap + 0.5, S.x_taper


def tongue():
    """The pan's wall: its inner half rises `lip_h` above the parting plane along both flanks."""
    x0, x1 = seam_x()
    return (cavity(-S.lip_t) - cavity(0.0)) & box(x0, x1, -50, 50, S.z_part - 0.01, S.z_part + S.lip_h)


def rebate():
    """Cut from the hood's wall: room for the tongue, 0.2 around it. The outer half of the wall comes down
    outside the tongue as a skirt."""
    x0, x1 = seam_x()
    return (cavity(-S.lip_t - 0.2) - cavity(0.21)) & box(x0 - 0.2, x1 + 0.2, -50, 50, S.z_part - 0.01, S.z_part + S.lip_h + 0.2)


def rib():
    """The hood's rib inside the tongue, hanging `lip_h` below the parting plane: with the skirt outside and
    the tongue between, the seam turns twice."""
    x0, x1 = seam_x()
    r0 = 0.2 + 0.2
    low = (cavity(r0) - cavity(r0 + S.rib_t)) & box(x0, x1, -50, 50, S.z_part - S.lip_h, S.z_part + S.lip_h + 0.2)
    high = (cavity(-0.01) - cavity(r0 + S.rib_t)) & box(x0, x1, -50, 50, S.z_part + S.lip_h + 0.19, S.z_part + S.lip_h + 3.0)
    return low + high


# --- the two shells -----------------------------------------------------------------------------------------------


def _shell(wedge=False):
    return outer() - cavity(wedge=wedge) - nose_hole() - grille_cut()


def _half(shape, above):
    sgn = 1 if above else -1
    return shape & Pos(0, 0, S.z_part + sgn * 200) * Box(600, 600, 400)


def hood():
    o = outer()
    h = _half(_shell(), True)
    h -= label_cut()
    h += _half(fan_shims(), True) & o
    h += _half(rear_bumps(), True)
    for x, y, d in boss_points():
        h += teardrop_z(x, y, d / 2, S.z_part + 0.2, 40.0, toward=-1) & o & box(G.REAR_IN_X - 0.1, 0, -50, 50, -50, 50)
        pilot = 1.05 if d == S.boss_d else 0.85
        h -= teardrop_z(x, y, pilot, S.z_part + 0.1, S.z_part + S.screw_engage + 0.5, toward=1)
    h -= rear_lip(0.2)
    h -= rebate()
    h += rib()
    return h


def pan():
    o = outer()
    p = _half(_shell(True), False) + (_shell(True) & rear_lip())
    p -= bay_pocket()
    p += (bay_walls() + bay_ceiling()) & o
    p -= port_openings()
    p += drip_lip() & o
    p -= vent_cut()
    p -= indicator_cuts()
    p += _half(fan_shims(), False) & o
    p += _half(rear_bumps(), False)
    p += (flange_pads() + standoffs()) & o
    for x, y, d in boss_points():
        p += teardrop_z(x, y, d / 2, -60.0, S.z_part, toward=-1) & o & box(G.REAR_IN_X - 0.1, 0, -50, 50, -50, 50)
        seat = S.z_part + S.screw_engage - (S.screw_len if d == S.boss_d else 16.0)
        p -= teardrop_z(x, y, d / 2 - (1.05 if d == S.boss_d else 0.1), -60.0, seat, toward=1)  # the head and the driver, from the belly; the front column keeps a 1.05 mm sleeve
        p -= teardrop_z(x, y, 1.4 if d == S.boss_d else 1.15, seat - 0.1, S.z_part + 0.1, toward=1)
    p += tongue()
    return p


SHELLS = {"hood": hood, "pan": pan}


def grille_open_area():
    """mm^2 the fan breathes through: the louvres' throat, perpendicular to the slot."""
    return (grille_cut() & box(X_REAR, G.REAR_IN_X, -50, 50, -50, 50)).volume / (REAR_T * math.sqrt(2))


def internals(flap_deg=0.0):
    """What the shells must clear: the chassis, the bought parts, the board's keep-out and the plugs."""
    from . import assembly as A

    return {**A.printed(flap_deg), **A.bought(flap_deg)}


def envelope():
    """Bounding box sizes (dx, dy, dz): the shells alone, and with the exposed socket and thumbscrew boss."""
    sh = hood() + pan()
    b = sh.bounding_box()
    s = (sh + internals()["duct"]).bounding_box()
    return tuple(round(v, 1) for v in b.size), tuple(round(v, 1) for v in s.size)
