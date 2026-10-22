"""The printed chassis parts, in the assembly frame (mm). `PARTS` maps a name to a builder.

The chassis is the duct with the fan on its flange, the servo on its shelf and the flap linkage: a working
unit on its own. The shells (shell.py) go around it.
"""

import functools
import math

from OCP.BRepBuilderAPI import BRepBuilderAPI_MakeSolid, BRepBuilderAPI_Sewing
from OCP.TopAbs import TopAbs_SHELL
from OCP.TopExp import TopExp_Explorer
from OCP.TopoDS import TopoDS
from build123d import (
    Align,
    Axis,
    Box,
    Circle,
    Cone,
    Cylinder,
    Edge,
    Face,
    Plane,
    Solid,
    Sphere,
    Torus,
    Polyline,
    Pos,
    Rot,
    Shape,
    Shell,
    Wire,
    extrude,
    loft,
    make_face,
    revolve,
)

from . import geom as G
from .cache import disk_cache
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


def smootherstep(t):
    """0 -> 1 with zero slope and curvature at both ends."""
    t = min(max(t, 0.0), 1.0)
    return t * t * t * (t * (6 * t - 15) + 10)


@functools.cache
def lobe_amp():
    """The wave's amplitude on the radius: with the mean bore grown by `ext_grow_d`, the crests' deepest reach into the tube
    is `grip`. It lies in the lead, where the transition still deepens while the opening starts, so it is solved for:
    the crest radius is linear in the amplitude."""
    c = P.coupling
    xs = [c.wave_x0 + (c.ext_len - c.wave_x0) * i / 2000 for i in range(2001)]
    lo, hi = 0.0, 5.0
    for _ in range(60):
        a = (lo + hi) / 2
        if min(_socket_r(x, 0.0, a) for x in xs) > P.tube.od / 2 - c.grip:
            lo = a
        else:
            hi = a
    return (lo + hi) / 2


def socket_r(x, th):
    """The socket's bore radius at x and angle th: round up to `wave_x0` (just in front of the O-ring groove), then on to the
    mouth the mean grows and the wave comes in along a smootherstep (crests at th = 0, 45, ...), and the last `lead_len` opens
    so the crests clear the tube."""
    return _socket_r(x, th, lobe_amp())


def _socket_r(x, th, amp):
    c = P.coupling
    s = smootherstep((x - c.wave_x0) / (c.ext_len - c.wave_x0))
    r = sock_r() + s * (c.ext_grow_d / 2 - amp * math.cos(c.lobes * th))
    open_ = P.tube.od / 2 + c.lead_clear - (sock_r() + c.ext_grow_d / 2 - amp)  # the crests' rise to clear the tube at the mouth
    return r + smootherstep((x - (c.ext_len - c.lead_len)) / c.lead_len) * open_


def _socket_stations():
    c = P.coupling
    step = c.ext_len / 20  # the same spacing all the way, so the loft's spline does not ring between sparse and dense sections
    n0 = math.ceil(-g.socket_x0 / step - 1e-9)
    xs = [max(-step * i, g.socket_x0) for i in range(n0 + 1)]  # round, from the mouth back to the shoulder
    xs += [step * i for i in range(1, 20)]  # the transition
    n = 2 * math.ceil(c.lead_len / step)
    xs += [c.ext_len - c.lead_len + c.lead_len * i / n for i in range(1, n + 1)]  # the lead, where the bore opens fastest: twice as dense
    return sorted(set(round(x, 6) for x in xs))


def socket_loft(extra=0.0, x0=None):
    """The socket's bore offset outward by `extra` (0: the bore; the wall's thickness: the extension's outside), lofted through
    sections from `x0` (default: the shoulder) to the mouth."""
    n = 16 * P.coupling.lobes
    wires = []
    xs = _socket_stations() if x0 is None else [x0] + [x for x in _socket_stations() if x > x0]
    for x in xs:
        pts = [(x, (socket_r(x, th) + extra) * math.cos(th), (socket_r(x, th) + extra) * math.sin(th))
               for th in (2 * math.pi * i / n for i in range(n))]
        wires.append(Wire([Edge.make_spline(pts, periodic=True)]))
    return Solid.make_loft(wires)


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
BLOCK_D = 7.0  # the blocks under the tabs, from the tab's underside: a 5.8 mm pilot and a 1.2 mm floor (1.0 read as
# exactly JLC3DP's minimum)


def servo_shelf():
    """A shelf under the servo's tabs, beside the duct on +y, with a cutout the body drops through and a solid block
    under each end's tab, with a M2 pilot under the middle one of each end's three screw holes.
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
    for x, y in G.PILOT_HOLES:
        s -= zcyl(P.m2_pilot / 2, zb + 1.2, z1 + 0.1, x, y)  # M2 pilot, 5.8 mm
    return s


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


def bernstein(c, t):
    """The polynomial with Bernstein coefficients c at t in 0..1 (clamped)."""
    t = min(max(t, 0.0), 1.0)
    n = len(c) - 1
    return sum(ci * math.comb(n, i) * t**i * (1 - t) ** (n - i) for i, ci in enumerate(c))


def hub_profile(t):
    """The tail cone's radius at t = 0 (the fan's plate) .. 1 (the tip)."""
    return max(bernstein(P.hub_cone.profile, t), 0.0)


def wall_r(x):
    """The airway's wall at x: the fan's bore before its face, the contraction (`P.contraction`), the duct's bore after."""
    return bernstein(P.contraction, (x - g.fan_face_x) / (g.straight_x0 - g.fan_face_x))


def airway(offset=0.0, x0=None, x1=None):
    """The airway pushed out by `offset` from x0 to x1: the fan's bore before its face, the contraction (a loft of
    circles on the wall's curve: within 2 um of it, where a revolved spline meshed for half an hour in OCC), the duct's
    bore after it. The pieces butt end to end: overlapped, the curved wall's tangent run into a cylinder failed the
    booleans."""
    xa, xb = g.fan_face_x, g.straight_x0
    lo, hi = max(x0, xa), min(x1, xb)
    n = 16
    out = loft([Plane.YZ.offset(x) * Circle(wall_r(x) + offset) for x in (lo + (hi - lo) * i / n for i in range(n + 1))])
    if x0 < xa:
        out = xcyl(P.fan.aperture_d / 2 + offset, x0, xa) + out
    if x1 > xb:
        out = out + xcyl(g.bore_r + offset, xb, x1)
    return out


def duct_outer_r(x):
    """The duct's outside at x, from inside the flange to the socket: the contraction's wall offset by the bore's wall,
    and a smootherstep up to the socket's diameter over `Coupling.flare_x`."""
    c = P.coupling
    x0, x1 = c.flare_x
    return wall_r(x) + g.duct_r - g.bore_r + (c.od / 2 - g.duct_r) * smootherstep((x - x0) / (x1 - x0))


def duct_outer():
    """The duct's outside as a solid from 1 mm inside the flange's back face (so the two cross, not touch) to the end
    of the flare: lofts of circles on `duct_outer_r`, butt-jointed to the straight duct's cylinder between them."""
    c = P.coupling
    xb, xa = g.fan_face_x + g.flange_t - 1.0, g.straight_x0
    out = loft([Plane.YZ.offset(x) * Circle(duct_outer_r(x)) for x in (xb + (xa - xb) * i / 20 for i in range(21))])
    x0, x1 = c.flare_x
    out += xcyl(g.duct_r, xa, x0)
    out += loft([Plane.YZ.offset(x) * Circle(duct_outer_r(x)) for x in (x0 + (x1 - x0) * i / 16 for i in range(17))])
    return out


def vane_passage(x):
    """The air's annulus at x between the tail cone and the contraction's wall: (r_hub, r_wall)."""
    h, x0 = P.hub_cone, g.fan_face_x
    t = min(max((x - x0) / h.length, 0.0), 1.0)
    return hub_profile(t), wall_r(x)


def _interp(x, knots):
    """The polynomial through the (x, y) knots (Lagrange; a cubic for four), x held to their range: smooth along the
    span, where a piecewise line's kinks show in the vanes' surfaces."""
    x = min(max(x, knots[0][0]), knots[-1][0])
    total = 0.0
    for i, (xi, yi) in enumerate(knots):
        w = 1.0
        for j, (xj, _) in enumerate(knots):
            if j != i:
                w *= (x - xj) / (xi - xj)
        total += w * yi
    return total


def vane_half(u, t=None, te=None):
    """A vane's half thickness at u = 0 (leading edge) .. 1 (the trailing edge's round's centre): NACA 4-digit on
    thickness t (the hub's `vane_t` by default), its chord stretched so it thins to te (`vane_te_t`) at u = 1."""
    h = P.hub_cone
    t = h.vane_t[0] if t is None else t
    te = h.vane_te_t if te is None else te

    def naca(v):
        return 5 * t * (0.2969 * math.sqrt(v) - 0.1260 * v - 0.3516 * v**2 + 0.2843 * v**3 - 0.1015 * v**4)

    lo, hi = 0.3, 1.0
    for _ in range(60):
        mid = (lo + hi) / 2
        lo, hi = (mid, hi) if naca(mid) > te / 2 else (lo, mid)
    return naca(u * lo)


def vane_fillet(s):
    """The root fillet's added half-thickness at s mm from the wall: R (1 - s/R)^2 over 0..R, none beyond; inside the
    wall (buried) carried on with its slope past the wall's 0.5 mm trim, then held (offset further, the outline folds)."""
    R = P.hub_cone.vane_fillet
    if s >= R:
        return 0.0
    if s >= 0:
        return R * (1 - s / R) ** 2
    return R + 2 * min(-s, 0.6)


@functools.cache
def vane_section(eta, n=28):
    """A vane's section at span fraction `eta` (0 on the cone, 1 on the wall; past either it runs on into them), as
    (x, r, theta) points round its outline, theta in radians about +x from the vane's own clock.

    It lies on the line through the passage at this fraction of its height from leading to trailing edge. Its camber
    line starts along the air's direction there (the fan's free vortex: tan = swirl Q / A over r, c_theta against
    the rotor's clockwise turn looking downstream, so -theta) and turns at a steady rate to `vane_dev`'s overturn past
    axial; the thickness is laid either side of it, normal in the developed (meridional, r theta) plane."""
    h = P.hub_cone
    x0 = g.fan_face_x
    e = min(max(eta, 0.0), 1.0)
    xl = x0 + h.vane_le[0] + (h.vane_le[1] - h.vane_le[0]) * e - h.vane_le_bow * math.sin(math.pi * e)
    m_fine = 400

    def line(xt, et):
        xs, rs = [], []
        for i in range(m_fine + 1):
            x = xl + (xt - xl) * i / m_fine
            rh, rw = vane_passage(x)
            xs.append(x)
            rs.append(rh + et * (rw - rh))
        ms = [0.0]
        for i in range(m_fine):
            ms.append(ms[-1] + math.hypot(xs[i + 1] - xs[i], rs[i + 1] - rs[i]))
        return xs, rs, ms

    def flow(xs, rs, ms):
        beta = math.radians(_interp(e, h.vane_beta))  # the air's angle from axial at the leading edge
        pitch = 2 * math.pi * rs[m_fine // 2] / h.vanes
        return beta, ms[-1] / math.cos(beta / 2) / pitch  # the camber line's length over the pitch

    lo, hi = xl + 4.0, x0 + h.vane_te_max  # the trailing edge: the chord that makes the solidity (at the span inside the walls), within the limit
    for _ in range(40):
        mid = (lo + hi) / 2
        lo, hi = (mid, hi) if flow(*line(mid, e))[1] < h.vane_solidity else (lo, mid)
    xs, rs, ms = line(hi, eta)
    big_m = ms[-1]
    hs = [vane_passage(x)[1] - vane_passage(x)[0] for x in xs]  # the passage's height along the section
    beta, sigma = flow(xs, rs, ms)
    dev = h.vane_dev * beta / (math.sqrt(sigma) - h.vane_dev)  # camber = beta + dev, dev = m camber / sqrt(sigma)
    phi0, phi1 = -beta, dev

    def phi(u):
        return phi0 + (phi1 - phi0) * u

    th = [0.0]  # the camber line's angle about the axis, integrated along it
    for i in range(m_fine):
        u = (i + 0.5) / m_fine
        th.append(th[-1] + math.tan(phi(u)) * (ms[i + 1] - ms[i]) / ((rs[i] + rs[i + 1]) / 2))

    def at(m):
        """x, r and the camber's theta at meridional distance m (extrapolated straight past either end)."""
        j = min(max(int(m / big_m * m_fine), 0), m_fine - 1)
        f = (m - ms[j]) / (ms[j + 1] - ms[j])
        return (xs[j] + f * (xs[j + 1] - xs[j]), rs[j] + f * (rs[j + 1] - rs[j]), th[j] + f * (th[j + 1] - th[j]))

    thick = h.vane_t[0] + (h.vane_t[1] - h.vane_t[0]) * e

    def fil(m):
        """The fillet's added half-thickness at meridional m: by the mm from the nearer wall there."""
        j = min(max(int(m / big_m * m_fine), 0), m_fine)
        return vane_fillet(min(eta, 1 - eta) * hs[j])

    def point(u, side):
        """The outline at u, `side` (+1 toward +theta, -1 away) of the camber line, offset along its normal: NACA on the
        filleted thickness (its round nose at any t), thinning to the filleted trailing edge's round."""
        m = u * big_m
        p = phi(u)
        f = fil(m)
        d = side * vane_half(u, thick + 2 * f, h.vane_te_t + 2 * f)
        x, r, _ = at(m - d * math.sin(p))
        _, r_c, t_c = at(m)
        return (x, r, t_c + d * math.cos(p) / r_c)

    us = [(1 - math.cos(math.pi * i / n)) / 2 for i in range(n + 1)]  # dense at both ends
    upper = [point(u, 1) for u in reversed(us)]  # trailing edge -> leading edge
    lower = [point(u, -1) for u in us[1:]]  # leading edge -> trailing edge
    rt = h.vane_te_t / 2 + fil(big_m)
    xe, re, te = at(big_m)
    cap = []
    for j in range(1, 8):  # round the trailing edge from the lower side to the upper
        a = -math.pi / 2 + math.pi * j / 8
        dm, dy = rt * math.cos(a), rt * math.sin(a)  # along the camber's tangent, across it
        m = big_m + dm * math.cos(phi1) - dy * math.sin(phi1)
        y = dm * math.sin(phi1) + dy * math.cos(phi1)
        x, r, _ = at(m)
        cap.append((x, r, te + y / re))
    return upper + lower + cap


def vane_etas():
    """The span stations: clustered through the fillets at both ends (0.02-0.04 apart), 0.08 across the middle, and on
    past both walls into the buried ends."""
    ends = [-0.18, -0.12, -0.06, 0.0, 0.02, 0.04, 0.06, 0.09, 0.12, 0.16, 0.2]
    return ends + [0.25 + i / 12 for i in range(7)] + [1 - v for v in reversed(ends)]


def vane():
    """One stator vane at clock 0 (+z): one B-spline surface through the grid of its sections (rows the span stations,
    columns the same chord stations round every section, so no matching is left to the loft: lofted section to section
    the leading edges twisted and nicked), closed round at the trailing edge's seam, capped at both buried ends and
    sewn to a solid."""
    rows = []
    for eta in vane_etas():
        pts = [(x, -r * math.sin(t), r * math.cos(t)) for x, r, t in vane_section(round(eta, 6), 40)]
        rows.append(pts + [pts[0]])
    skin = Face.make_surface_from_array_of_points(rows, tol=0.005, max_deg=3)
    ends = sorted(skin.edges(), key=lambda e: e.length)[-2:]  # the first and last sections: the long boundary edges
    sew = BRepBuilderAPI_Sewing(1e-3)
    for f in [skin, *(Face.make_surface(Wire([e])) for e in ends)]:
        sew.Add(f.wrapped)
    sew.Perform()
    shell = TopoDS.Shell(TopExp_Explorer(sew.SewedShape(), TopAbs_SHELL).Current())
    s = Solid(BRepBuilderAPI_MakeSolid(shell).Solid())
    return s if s.volume > 0 else Solid(s.wrapped.Reversed())


@disk_cache
def hub_cone(solid=False):
    """The tail cone on the fan's motor plate and its stator vanes from the contraction's wall: printed with the duct.
    Hollow and open toward the fan unless `solid` (the CFD's air volume wants the outside only)."""
    h, x0, n = P.hub_cone, g.fan_face_x, 80
    outer = [(x0 + h.length * i / n, hub_profile(i / n)) for i in range(n + 1)]
    outer = [p for p in outer if p[1] >= 0.6]  # the point ends in a Ø1.2 flat: a surface that closes on the axis meshes open
    xe, re = outer[-1]
    prof = [Edge.make_spline([(x, 0, r) for x, r in outer]),
            Edge.make_line((xe, 0, re), (xe, 0, 0)), Edge.make_line((xe, 0, 0), (x0, 0, 0)), Edge.make_line((x0, 0, 0), (x0, 0, outer[0][1]))]
    body = revolve(Face(Wire(prof)), Axis.X, 360)
    if not solid:
        # the inside: the outer profile offset `wall` along its normal, closed on the axis where it runs out
        inner = []
        for i in range(n):
            (xa, ra), (xb, rb) = outer[i], outer[i + 1]
            d = math.hypot(xb - xa, rb - ra)
            x, r = xa + h.wall * (rb - ra) / d, ra - h.wall * (xb - xa) / d  # back along the outward normal
            if r < 0.6:
                break
            inner.append((x, r))
        void = revolve(make_face(Plane.XZ * Polyline((x0 - 0.01, 0.0), (x0 - 0.01, inner[0][1]), *inner, (inner[-1][0], 0.0), close=True)), Axis.X, 360)  # a flat end, as the outside
    # the vanes, from inside the cone out into the contraction's wall
    wall = airway(0.5, g.fan_face_x, g.straight_x0 + 10.0)
    one = vane() & wall
    for i in range(h.vanes):
        body += Rot(h.vane_clock + 360 * i / h.vanes, 0, 0) * one
    if not solid:
        body -= void  # once, after the vanes: their roots go no further than the cone's shell (cut one by one, a thin tail's slivers left it invalid)
    return body


@disk_cache
def duct():
    """Fan flange, contraction cone with the tail cone on its struts, the straight bore with the flap station, the servo
    shelf and the socket onto the intake tube, with its drip collar and its extension toward the egg, whose lobed bore
    grips the tube."""
    c = P.coupling
    br, dr, w = g.bore_r, g.duct_r, g.flange_w
    xf = g.flap_x
    flange = box(g.fan_face_x, g.fan_face_x + g.flange_t, -w / 2, w / 2, -w / 2, w / 2)
    flange = flange.fillet(g.flange_r, flange.edges().filter_by(Axis.X))
    outer = flange
    outer += duct_outer()  # the bore's 2.2 mm wall round the contraction, a smootherstep to the socket
    xb = g.fan_face_x + g.flange_t
    ring = [e for e in outer.edges() if abs(e.center().X - xb) < 1e-3 and e.geom_type.name != "LINE" and e.length > 100]  # where it leaves the flange
    outer = outer.fillet(P.duct_fillet, ring)
    k = P.layout.slope
    outer += xcyl(c.od / 2, c.flare_x[1], c.ext_len)
    # drip collar behind the hood's front ring: its front face seats on the shells' front cone, its back is
    # chamfered for the print
    dc = c.collar_r - c.od / 2
    outer += xcone(c.od / 2, c.collar_r, c.collar_x - 1.5 - 1.6 - dc * k, c.collar_x - 1.5 - 1.6)
    outer += xcyl(c.collar_r, c.collar_x - 1.5 - 1.6, c.collar_x - 1.5)
    outer += xcone(c.collar_r, c.od / 2, c.collar_x - 1.5, c.collar_x)
    # the socket's Ø46 runs on to the mouth as a smooth sleeve; inside it a slot, open at the mouth, leaves the lobed wall
    # free to flex from `sleeve_x0` on (the lobed wall follows the wave; it starts inside the solid behind the slot's floor)
    outer -= xcyl(c.od / 2 - c.sleeve_t, c.sleeve_x0, c.ext_len + 1.0)
    outer += socket_loft(c.lobe_wall, x0=c.sleeve_x0 - 1.0)
    body = outer + servo_shelf() + stop_posts()

    # the airway and the features cut into the walls
    x_neck = g.socket_x0 - c.neck_len
    body -= airway(0.0, g.fan_face_x - 0.1, x_neck + 0.01)
    body -= xcone(br, P.tube.id / 2, x_neck, g.socket_x0 + 0.01)  # the neck down to the tube's bore
    body -= socket_loft()  # it stops at the flat shoulder the tube's end face sits on
    body -= xcyl(sock_r(), g.socket_x0 - 0.5, g.socket_x0 + 0.01) - xcyl(P.tube.od / 2 - c.seat_relief, g.socket_x0 - 1, g.socket_x0 + 1)  # the seat's corner relief
    body -= xcyl(groove_r(), c.oring_x - 1.6, c.oring_x + 1.6)
    body -= zcyl(1.7, -dr + 1.2, dr + 1.0, xf, 0.0)  # shaft bore, blind at the bottom: a 1.2 mm skin
    body += hub_cone()  # in the airway, after it is cut
    for sy in (-1, 1):
        for sz in (-1, 1):
            body -= xcyl(P.m3_pilot / 2, g.fan_face_x - 0.1, g.fan_face_x + 9.0, sy * P.fan.hole_pitch / 2, sz * P.fan.hole_pitch / 2)
    return body


# --- flap and its linkage ----------------------------------------------------------------------------------


def flap(dm=None):
    """Butterfly flap, machined (stainless or aluminium): a disc with a full-round edge and a round-ended rib along the shaft axis (Z), open =
    parallel to the flow, a D-bore for the rod's flat (retaining compound), no thread. The disc is centred on the
    rib (`Damper.rib_both_sides`; False puts it flush with the rib's bottom face, cut from plate with the rib standing on it).
    `dm` is a variant of the damper's parameters (the LES compares edges, cfd/variants.py); the default is the design."""
    dm = dm or P.damper
    off = dm.disc_off  # the disc centre is this far from the shaft axis
    r = math.sqrt((g.bore_r - dm.flap_gap) ** 2 - off**2)  # its edge stays within the gap at every angle
    xf = g.flap_x
    if dm.edge_round:  # a full round: a cylinder to the round's centre circle and a torus around it
        disc = Pos(xf, -off, 0) * Rot(90, 0, 0) * (Cylinder(r - dm.flap_t / 2, dm.flap_t, align=(C, C, C)) + Torus(r - dm.flap_t / 2, dm.flap_t / 2))
    else:
        disc = Pos(xf, -off, 0) * Rot(90, 0, 0) * Cylinder(r, dm.flap_t, align=(C, C, C))
    rr = min(dm.rib_round, dm.rib_t / 2, dm.rib_w / 2)  # the rib's section is a stadium: round ends facing and leaving the flow
    xe = dm.rib_w / 2 - rr
    rib = box(xf - xe, xf + xe, -dm.rib_t / 2, dm.rib_t / 2, -r, r)
    if rr > 0:
        rib += zcyl(rr, -r, r, xf - xe, 0.0) + zcyl(rr, -r, r, xf + xe, 0.0)
    rib = rib & (Pos(xf, 0, 0) * Sphere(r - (0.02 if dm.rib_both_sides else 0.0)))  # centred, the disc's edge circle is the sphere's equator: stay just inside it
    f = disc + rib
    for k in range(dm.serr_n):  # a serrated edge: V notches (90 deg, serr_h deep) round the disc, clear of the rib
        a = 2 * math.pi * (k + 0.5) / dm.serr_n
        if abs(r * math.cos(a)) < dm.rib_w / 2 + dm.serr_h:
            continue
        f -= Pos(xf + r * math.cos(a), -off, r * math.sin(a)) * Rot(0, -math.degrees(a) + 45, 0) * Box(dm.serr_h * math.sqrt(2), dm.flap_t + 2, dm.serr_h * math.sqrt(2))
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
