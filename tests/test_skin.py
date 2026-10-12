"""The shells build, clear the chassis and the board, keep the envelope, breathe, line up with the board's
connectors and print without support."""

import pytest

from minimeat.mech import assembly as A, shell as K
from minimeat.mech import geom as G
from minimeat.mech.params import P
from minimeat.mech.parts import box, ycyl
from minimeat.mech.printing import overhangs, print_pose


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
    assert shells["pan"].bounding_box().max.Z <= P.shell.z_part + P.shell.lip_h + 0.01  # only the tongue rises


@pytest.mark.parametrize("deg", [0, 90])
def test_shells_clear_the_internals(shells, deg):
    """The chassis, the bought parts, the board's keep-out and the plugs, with the flap open and closed."""
    hits = A.interferences({**K.internals(deg), **shells})
    assert [h for h in hits if h[0] in shells or h[1] in shells] == []


def test_envelope():
    shell, with_nose = K.envelope()
    assert shell == (115.2, 74.4, 73.1)  # x, y, z: the rear face to the front ring, the width, the belly to the crown
    assert with_nose == (126.2, 74.4, 73.1)  # plus the exposed socket and the thumbscrew boss


def test_grille_breathes():
    assert K.grille_open_area() >= 1500  # mm^2 through the louvres, measured across the slot


def test_ports_line_up_with_the_board(shells):
    """A probe through each jack hole and the USB-C opening, on the board's connector axes, passes the port wall
    and reaches the connector face; the openings are all inside the bay."""
    p = P.pcb
    pan = shells["pan"]
    for jx in p.jack_xs:
        probe = ycyl(p.jack_hole_d / 2 - 0.1, K.PORT_FACE_Y - 3.0, G.PORT_WALL_Y + 0.05, K.PCB_XC + jx, G.PCB_Z1 + p.jack_h / 2)
        assert A.overlap(probe, pan) < 0.5
    zu = G.PCB_Z1 + p.usb_h / 2
    probe = box(K.PCB_XC + p.usb_x - p.usb_open_w / 2 + 1.6, K.PCB_XC + p.usb_x + p.usb_open_w / 2 - 1.6,
                K.PORT_FACE_Y - 3.0, G.PORT_WALL_Y + 0.05, zu - p.usb_open_h / 2 + 0.1, zu + p.usb_open_h / 2 - 0.1)
    assert A.overlap(probe, pan) < 0.5
    assert K.BAY_X0 < K.PCB_XC + p.usb_x - p.usb_open_w / 2 and K.PCB_XC + p.jack_xs[-1] + p.plug_d / 2 < K.BAY_X1


def test_bay_is_under_the_eave_and_drains(shells):
    """The port wall is recessed behind the flank, the ceiling of the bay is the eave with its drip lip, and
    the rear end wall has its drain notch at the foot."""
    pan = shells["pan"]
    assert K.PORT_FACE_Y > -G.FULL_W / 2 + 4.0  # recessed at least 4 mm
    lip = box(K.BAY_X0 + 5, K.BAY_X1 - 5, -G.FULL_W / 2 - 0.1, -G.FULL_W / 2 + 0.5, K.BAY_Z1 - 1.2, K.BAY_Z1 - 0.2)
    assert A.overlap(lip, pan) > 0.5 * lip.volume
    notch = box(K.BAY_X0 - P.shell.wall + 0.3, K.BAY_X0 - 0.3, -30.9, K.PORT_FACE_Y - 0.1, G.BELLY_Z + 2.7, K.BAY_Z0 + 1.5)
    assert A.overlap(notch, pan) < 0.01
    wall_above = box(K.BAY_X0 - P.shell.wall + 0.3, K.BAY_X0 - 0.3, -30.9, K.PORT_FACE_Y - 0.1, K.BAY_Z0 + 3.0, K.BAY_Z0 + 5.0)
    assert A.overlap(wall_above, pan) > 0.9 * wall_above.volume  # the end wall is there, the notch is at its foot


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
    lip = box(K.X_REAR + 2.9, G.REAR_IN_X - 0.1, -20, 20, 0.1, K.LAP - 0.1)
    assert A.overlap(lip, pan) > 0.9 * lip.volume and A.overlap(lip, hood) < 0.01
    # a louvre: open at the outer face lower than at the inner face
    z = K.GRILLE_ZS[-1]
    outer_probe = box(K.X_REAR - 0.5, K.X_REAR + 0.3, -5, 5, z - 1.0, z + 1.0)
    inner_probe = box(G.REAR_IN_X - 0.3, G.REAR_IN_X + 0.5, -5, 5, z + K.REAR_T - 1.0, z + K.REAR_T + 1.0)
    assert A.overlap(outer_probe, hood) < 0.05 * outer_probe.volume and A.overlap(inner_probe, hood) < 0.05 * inner_probe.volume
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
