import math
"""The shells build, clear the chassis and the board, keep the envelope, breathe, line up with the board's
connectors and print without support."""

import pytest

from minimeater.mech import assembly as A, shell as K
from minimeater.mech import geom as G
from minimeater.mech.params import P
from minimeater.mech.parts import box, ycyl, zcyl
from minimeater.mech.printing import overhangs, print_pose


@pytest.fixture(scope="module")
def shells():
    return {n: f() for n, f in K.SHELLS.items()}


@pytest.mark.parametrize("name", list(K.SHELLS))
def test_shell_is_one_valid_solid(shells, name):
    s = shells[name]
    assert s.is_valid
    assert len(s.solids()) == 1
    assert s.volume > 10000


def test_shells_meet_on_the_parting_plane(shells):
    assert A.overlap(shells["hood"], shells["pan"]) < 0.5
    assert shells["hood"].bounding_box().min.Z >= P.shell.z_part - P.shell.lip_h - 0.01  # only the rib hangs below the seam
    assert shells["pan"].bounding_box().max.Z <= P.shell.z_part + max(P.shell.lip_h, K.V_APEX + K.REAR_T * K.TAN44) + 0.01  # only the tongue and the V, climbing through the rear wall


@pytest.mark.parametrize("deg", [0, 90])
def test_shells_clear_the_internals(shells, deg):
    """The chassis, the bought parts, the board's keep-out and the plugs, with the flap open and closed."""
    hits = A.interferences({**K.internals(deg), **shells})
    assert [h for h in hits if h[0] in shells or h[1] in shells] == []


def test_envelope():
    shell, with_nose = K.envelope()
    assert shell == (128.2, 75.2, 73.9)  # x, y, z: the rear face to the front ring, the width, the belly to the crown (rear_gap 14.5: +10)
    assert with_nose == (149.2, 75.2, 73.9)  # plus the exposed socket and its 10 mm extension


def test_the_bay_is_closed_over_the_ports(shells):
    """The eave must be solid between the flank and the port wall: no slot from the bay into the shell."""
    pan = shells["pan"]
    for x in (-70.0, -45.0, -25.0):
        assert pan.is_inside((x, -31.0, -20.4)), x


def test_grille_breathes():
    assert K.grille_open_area() >= 1350  # mm^2 through the gills, measured across the slot (1388: eight that open on both faces)


def test_ports_line_up_with_the_board(shells):
    """A probe through each jack hole and the USB-C opening, on the board's connector axes, passes the port wall
    and reaches the connector face; the openings are all inside the bay."""
    p = P.pcb
    pan = shells["pan"]
    for jx in p.jack_xs:
        # the plug's barrel reaches the jack's front, past the wall's inner face
        probe = ycyl(p.jack_hole_d / 2 - 0.1, K.PORT_FACE_Y - 3.0, G.PCB_Y0 + p.jack_face + 0.1, K.PCB_XC + jx, G.PCB_Z0 - p.jack_h / 2)
        assert A.overlap(probe, pan) < 0.5
        # and the jack's housing stands in the wall through its opening
        housing = box(K.PCB_XC + jx - p.jack_w / 2, K.PCB_XC + jx + p.jack_w / 2, G.PCB_Y0 + p.jack_face, G.PORT_WALL_Y + 0.05,
                      G.PCB_Z0 - p.jack_h, G.PCB_Z0)
        assert A.overlap(housing, pan) < 0.5
    zu = G.PCB_Z0 - p.usb_h / 2
    probe = box(K.PCB_XC + p.usb_x - p.usb_open_w / 2 + 1.6, K.PCB_XC + p.usb_x + p.usb_open_w / 2 - 1.6,
                K.PORT_FACE_Y - 3.0, G.PCB_Y0 + p.usb_face + 0.1, zu - p.usb_open_h / 2 + 0.1, zu + p.usb_open_h / 2 - 0.1)
    assert A.overlap(probe, pan) < 0.5
    assert K.BAY_X0 < K.PCB_XC + p.usb_x - p.usb_open_w / 2 and K.PCB_XC + p.jack_xs[-1] + p.plug_d / 2 < K.BAY_X1


def test_connectors_reach_into_the_wall():
    """The jacks' fronts and the USB-C's mouth are past the trimmed board edge and inside the wall's thickness, with
    the plug's travel from the wall's outer face short."""
    p = P.pcb
    assert G.PORT_WALL_Y == G.PCB_Y0 + p.edge_trim - p.edge_gap
    for face in (p.jack_face, p.usb_face):
        y = G.PCB_Y0 + face
        assert K.PORT_FACE_Y < y < G.PORT_WALL_Y  # inside the wall
    assert G.PCB_Y0 + p.jack_face - K.PORT_FACE_Y <= 1.0  # the plug's barrel travels at most this far past the outer face to the jack


def test_no_web_thinner_than_the_printers_minimum(shells):
    """JLC3DP flags walls under 1.2 mm (ASA's minimum): the louvre blades, the seam's tongue and rib, and the
    wall between the USB-C opening and the first jack's."""
    import math
    sh = P.shell
    assert sh.grille_pitch * math.cos(math.radians(44)) - sh.grille_slot >= 1.2  # a blade, across its faces
    assert min(sh.lip_t, sh.rib_t) >= 1.2
    p = P.pcb
    xu = K.PCB_XC + p.usb_x + p.usb_open_w / 2
    xj = K.PCB_XC + p.jack_xs[0] - p.jack_open_w / 2
    assert xj - xu >= 1.2  # the web is a printable wall
    z0, z1 = p.jack_open_z
    web = box(xu + 0.05, xj - 0.05, K.PORT_FACE_Y, G.PORT_WALL_Y, G.PCB_Z0 - z1, G.PCB_Z0 - z0)
    assert A.overlap(web, shells["pan"]) > 0.99 * web.volume  # the pan is solid there: no shared slot


def test_bay_is_under_the_eave_and_drains(shells):
    """The port wall is recessed behind the flank, the ceiling of the bay is the eave with its drip lip, and the
    rear end wall is plain down to its foot: no drain scallop (7d69a11), the bay's floor is the belly's round, open
    all along, so water runs off it."""
    pan = shells["pan"]
    assert K.PORT_FACE_Y > -G.FULL_W / 2 + 4.0  # recessed at least 4 mm
    lip = box(K.BAY_X0 + 5, K.BAY_X1 - 5, -G.FULL_W / 2 - 0.1, -G.FULL_W / 2 + 0.5, K.BAY_Z1 - 1.2, K.BAY_Z1 - 0.2)
    assert A.overlap(lip, pan) > 0.5 * lip.volume
    hull = K.outer()
    for z0, z1 in ((G.BELLY_Z + 2.7, K.BAY_Z0 + 1.5), (K.BAY_Z0 + 3.0, K.BAY_Z0 + 5.0)):  # the foot, where the scallop was, and above it
        wall = box(K.BAY_X0 - P.shell.wall + 0.3, K.BAY_X0 - 0.3, -30.9, K.PORT_FACE_Y - 0.1, z0, z1)
        assert A.overlap(wall, pan) > 0.99 * A.overlap(wall, hull)  # solid wherever it is inside the hull (the belly's round cuts the foot's corner)


def test_rain_features(shells):
    """Louvres descend outward, the flanks' seam is a labyrinth (tongue inside the hood's wall, rib inside the
    tongue), the rear wall has its lip, the socket's collar seats behind the front ring."""
    hood, pan = shells["hood"], shells["pan"]
    x0, x1 = K.seam_x()
    xm = (x0 + x1) / 2
    hw = K.profile(xm)[0] / 2 - K.W  # the inner half width at the seam's middle
    tongue = box(xm - 5, xm + 5, hw + 0.1, hw + P.shell.lip_t - 0.1, 0.5, P.shell.lip_h - 0.5)
    assert A.overlap(tongue, pan) > 0.9 * tongue.volume
    rib = box(xm - 5, xm + 5, hw - 0.4 - P.shell.rib_t + 0.1, hw - 0.5, -P.shell.lip_h + 0.5, -0.5)
    assert A.overlap(rib, hood) > 0.9 * rib.volume
    # the V seam climbs 44 degrees through the rear wall with the gills: under the split, in the wall's middle and
    # at its inner face, it is the pan; over it, the hood
    za = K.seam_z(0.0)
    for x in (K.X_REAR + K.REAR_T / 2, G.REAR_IN_X - 0.4):
        zs = K.seam_at(x, 0.0)
        below = box(x - 0.3, x + 0.3, -2, 2, zs - 1.5, zs - 0.6)
        above = box(x - 0.3, x + 0.3, -2, 2, zs + 0.6, zs + 1.5)
        assert A.overlap(below, pan) > 0.9 * below.volume and A.overlap(below, hood) < 0.01, x
        assert A.overlap(above, hood) > 0.9 * above.volume and A.overlap(above, pan) < 0.01, x
    assert K.seam_at(G.REAR_IN_X, 0.0) - za > K.GRILLE_H  # no level line of sight through the split either
    # a gill: open at the outer face lower than at the inner face (44 degrees up through the wall)
    zc, half = [sl for sl in K.grille_slots() if sl[0] > za][1]
    t = math.tan(math.radians(44))
    outer_probe = box(K.X_REAR - 0.5, K.X_REAR + 0.3, -3, 3, zc - 1.0, zc + 0.6)
    inner_probe = box(G.REAR_IN_X - 0.3, G.REAR_IN_X + 0.5, -3, 3, zc + K.REAR_T * t - 1.0, zc + K.REAR_T * t + 0.6)
    assert A.overlap(outer_probe, hood) < 0.05 * outer_probe.volume and A.overlap(inner_probe, hood) < 0.05 * inner_probe.volume
    # no level line of sight through the gills: the rise across the wall is more than a slot's height on the face
    assert K.REAR_T * t - K.GRILLE_H >= 0.4
    assert P.coupling.collar_r > K.NOSE_R  # the collar cannot pass the nose hole


# Overhang inventory in the print pose (both standing on the rear face, nose up). What is left is listed in
# docs/mech.md, "Printing".
OVERHANG_AREA_MM2 = {"hood": 150, "pan": 150}


@pytest.mark.parametrize("name", list(K.SHELLS))
def test_prints_without_support(shells, name):
    patches = overhangs(print_pose(name, shells[name]))
    total = sum(p["area"] for p in patches)
    assert total <= OVERHANG_AREA_MM2[name], patches
    assert all(p["area"] <= 60 for p in patches), patches


def test_led_pipe_passes_the_flank(shells):
    """The light pipe runs from the board's edge out through the +y flank, clear of the ambient louvres."""
    p = P.pcb
    pan = shells["pan"]
    xl = K.PCB_XC + p.led_x
    zl = G.PCB_Z0 - p.led_z
    pipe = ycyl(P.shell.pipe_d / 2, p.y_len / 2 + 0.6, 45.0, xl, zl)
    assert A.overlap(pipe, pan) < 0.5  # a clear way through
    lo, hi = P.shell.vent_slots[0]
    assert not (lo - 1.5 < xl < hi + 1.5)


def test_reset_hole_and_louvres_open_the_floor_under_the_parts(shells):
    """The reset button and the sensor face the pan floor: a paper clip's way up to the button, and the louvre slots, are open."""
    p, pan = P.pcb, shells["pan"]
    clip = zcyl(1.0, G.BELLY_Z - 5.0, G.PCB_Z0 - p.btn_z - 0.5, K.PCB_XC + p.btn_x, p.btn_y)  # a 2 mm wire, up to the actuator
    assert A.overlap(clip, pan) < 0.01
    assert P.shell.reset_d >= 3.0
    for x0, x1 in P.shell.vent_slots:
        for dy in (-P.shell.vent_dy, P.shell.vent_dy):
            slot = box(x0 + 0.1, x1 - 0.1, p.sensor[1] + dy - 0.5, p.sensor[1] + dy + 0.5, G.BELLY_Z - 0.5, G.FLOOR_Z + 0.5)
            assert A.overlap(slot, pan) < 0.01
    # the floor is otherwise closed beside them
    solid = box(P.shell.vent_slots[0][0] + 0.1, P.shell.vent_slots[0][1] - 0.1, p.sensor[1] - 0.5, p.sensor[1] + 0.5, G.BELLY_Z + 1.1, G.FLOOR_Z - 0.2)  # the rib between the slots
    assert A.overlap(solid, pan) > 0.5 * solid.volume


def test_the_louvres_stay_under_the_rear_chamfer():
    """Climbing inward, the top slots would open through the chamfer over the grille, an upward face rain runs into."""
    assert (K.grille_cut() - K.under_chamfer(K.GRILLE_SKIN - 0.01)).volume < 1e-6
