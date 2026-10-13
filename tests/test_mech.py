"""The chassis builds, its parts fit with no interference through the flap's travel, and they print without support."""

import pytest

from minimeat.mech import analysis, assembly as A, mock
from minimeat.mech import geom as G
from minimeat.mech.export import export
from minimeat.mech.params import P
from minimeat.mech.parts import MACHINED, PARTS
from minimeat.mech.printing import overhangs, print_pose


@pytest.fixture(scope="module")
def printed():
    return {n: f() for n, f in PARTS.items()}


@pytest.mark.parametrize("name", list(PARTS))
def test_part_is_one_valid_solid(printed, name):
    s = printed[name]
    assert s.is_valid
    assert len(s.solids()) == 1
    assert s.volume > 100


@pytest.mark.parametrize("deg", [0, 30, 60, 90])
def test_no_interference_at_any_flap_angle(deg):
    """Every chassis part and bought part, with the flap, crank, horn, pins and coupler at `deg`."""
    allp = {**A.printed(deg), **A.bought(deg)}
    hits = [h for h in A.interferences(allp) if set(h[:2]) != {"duct", "oring"}]  # the ring is squeezed by design
    assert hits == []


def test_the_crank_lug_stops_on_the_posts_at_open_and_closed_and_only_there():
    duct, crank = A.printed()["duct"], MACHINED["crank"]()
    for deg in (0, 20, 45, 70, 90):  # free over the whole travel, touching at the ends
        assert A.overlap(duct, A.about_shaft(crank, deg)) < 0.5
    for deg in (-2, 92):  # a little past either end it is on the post
        assert A.overlap(duct, A.about_shaft(crank, deg)) > 1.0


def test_linkage_is_a_parallelogram():
    """Equal arms, the coupler as long as the shaft spacing, the horn level with the crank, under the roof."""
    L = P.linkage
    assert G.SERVO_X == G.g.flap_x
    assert abs(A.coupler_shift(90).position.Y + L.arm_r) < 1e-9 and abs(A.coupler_shift(90).position.X - L.arm_r) < 1e-9
    assert G.CRANK_Z0 > G.g.duct_r  # the linkage plane is over the duct's crown
    assert G.COUPLER_Z1 + L.pin_head_h < P.shell.roof_in
    assert G.SERVO_Z0 > G.PCB_Z1 + P.pcb.top_h + 2.0  # the servo clears the board's keep-out


def test_rod_stands_above_the_crank_and_under_the_bar():
    """The rod's end is gripable (0.2 to 0.4 mm over the crank's top with a +-0.1 cut) and clears the coupler, which
    lies over it when the flap is closed, by at least 0.5 mm; the pin heads stay 1.5 mm under the roof's nominal underside
    (measured on the hood: 1.0 mm over the crank pin's sweep and 0.4 mm over the horn pin's, where the roof curves down; see docs/mech.md)."""
    top = mock.rod().bounding_box().max.Z
    assert 0.2 <= top - G.ARM_Z1 <= 0.4
    assert G.COUPLER_Z0 - (top + 0.1) >= 0.5 - 1e-9
    assert P.shell.roof_in - (G.COUPLER_Z1 + P.linkage.pin_head_h) >= 1.5
    assert abs(G.COUPLER_Z1 - G.BOSS_Z0 + 0.1 - P.linkage.shoulder_len) < 1e-9  # the shoulder is 0.1 longer than the bar and its boss


def test_shelf_pilots_are_under_the_servos_screw_holes():
    """The duct's six M2 pilots sit under the tab holes of Hitec's own model (the first version had two, between holes)."""
    halves = {}
    for f in mock.servo().faces():
        bb = f.bounding_box()
        if str(f.geom_type).endswith("CYLINDER") and abs(bb.size.X - 2.0) < 0.05 and abs(bb.size.Y - 1.0) < 0.05 and G.TAB_Z0 - 0.1 < bb.min.Z < G.TAB_Z0 + 0.1:
            halves.setdefault(round(bb.center().X, 2), []).append(bb.center().Y)
    holes = [(x, (y0 + y1) / 2) for x, ys in halves.items() for y0 in ys for y1 in ys if abs(y1 - y0 - 1.0) < 0.05]  # two half-cylinders a hole
    assert len(holes) >= 6
    assert len(G.TAB_HOLES) == 6
    for x, y in G.TAB_HOLES:
        assert any(abs(hx - x) < 0.05 and abs(hy - y) < 0.05 for hx, hy in holes), (x, y, holes)


def test_board_lies_under_the_straight_duct():
    """The board sits beside the r = 18.2 straight section (the parts one mm into the cone and the flare), with its
    keep-out one mm under the duct and the cone no steeper than 26 degrees. The USB-C's end, past the last part,
    goes further in: only the USB-C stands there, and it clears the cone."""
    assert G.g.straight_x0 <= G.g.board_x0 + P.layout.straight_over + 1e-9
    assert P.layout.straight_over == P.pcb.usb_end + 1.0
    cone_r = G.g.duct_r + 0.5 * (G.g.straight_x0 - G.g.board_x0)  # the cone's outer radius at the board's rear edge
    assert G.PCB_Z1 + P.pcb.usb_h + 1.0 <= -cone_r + 1e-9
    assert G.g.board_x1 <= G.g.socket_x0 - 4.0
    assert G.PCB_Z1 + P.pcb.top_h <= -G.g.duct_r - 1.0 + 1e-9
    assert analysis.cone_half_angle() <= 26.1


def test_oring_seals_the_joint():
    """The ring is pinched between the tube and the groove bottom: 10-30 % squeeze, under 5 % stretch."""
    squeeze, stretch = analysis.oring_squeeze()
    assert 0.10 <= squeeze <= 0.30
    assert 0 <= stretch <= 0.05
    ring = A.bought()["oring"]
    assert A.overlap(ring, A.printed()["duct"]) > 0
    assert A.overlap(ring, A.bought()["tube"]) < 0.5


def test_tube_seats_on_the_shoulder_without_touching_the_duct():
    assert A.overlap(A.bought()["tube"], A.printed()["duct"]) < 0.5


def test_closed_flap_leak_is_inside_the_budget():
    assert analysis.flap_leak_cm2() <= 0.3  # docs/modeling: closed leak <= 0.3 cm^2


def test_flap_fit_needs_the_bore_reamed():
    """The disc is machined, the duct's bore is printed (JLC: +-0.3 mm, +-0.15 radial): as printed the gap can close (the
    flap jams) or open to a leak over budget; reamed at the flap station (+-0.05) it stays inside both."""
    dm = P.damper
    lo, hi = analysis.flap_gap_range(dm.print_bore_tol)
    assert lo < 0 and analysis.flap_leak_worst_cm2(dm.print_bore_tol) > 0.3
    lo, hi = analysis.flap_gap_range(dm.ream_tol)
    assert lo > 0.03 and analysis.flap_leak_worst_cm2(dm.ream_tol) <= 0.3


def test_open_flap_does_not_choke_the_bore():
    assert analysis.flap_blockage_open() < 0.2  # the rib is the loss to shave in CFD (.2.13)


def test_the_flaps_rib_is_round_ended_not_square():
    from minimeat.mech.parts import flap

    f = flap()
    xf, dm = G.g.flap_x, P.damper
    # the square corner of the rib's section, on the side away from the disc, is cut away; the middle of the end is kept
    assert not f.is_inside((xf + dm.rib_w / 2 - 0.1, dm.rib_t / 2 - 0.1, 5.0))
    assert f.is_inside((xf + dm.rib_w / 2 - 0.1, 0.0, 5.0))


def test_the_one_sided_flap_variant_is_a_valid_solid(monkeypatch):
    from minimeat.mech.parts import flap

    monkeypatch.setattr(type(P.damper), "rib_both_sides", False)  # the dataclass is frozen: patch the class default
    f = flap()
    assert f.is_valid and len(f.solids()) == 1
    assert analysis.flap_leak_cm2() < 0.3
    assert analysis.flap_blockage_open() < 0.2


# Overhang inventory: faces steeper than 45 degrees off vertical, in the print pose. What is left must be
# short bridges and small holes. A new patch or a bigger total means a change that needs a printing look.
OVERHANG_AREA_MM2 = {"duct": 475}  # 340 + the 1.2 mm land at the shelf's rear end (27 mm2) + the front servo block's face toward the body (12 x 7 = 84 mm2, a bridge as short as it is deep) + the stop posts' ends toward the rod (two 3 mm bridges, about 8 mm2)


@pytest.mark.parametrize("name", list(PARTS))
def test_prints_without_support(name):
    patches = overhangs(print_pose(name))
    total = sum(p["area"] for p in patches)
    assert total <= OVERHANG_AREA_MM2[name], patches
    assert all(p["area"] <= 230 for p in patches), patches  # the largest is the O-ring groove's 2 mm ceiling


def test_export_writes_step_and_stl(tmp_path):
    export(tmp_path)
    for n in list(PARTS) + ["hood", "pan"]:
        assert (tmp_path / "3dprint" / f"{n}.step").stat().st_size > 1000
        assert (tmp_path / "3dprint" / f"{n}.stl").stat().st_size > 1000
    for n in ("flap", "crank", "coupler"):
        assert (tmp_path / "cnc" / f"{n}.step").stat().st_size > 1000
    for n in ("flap", "crank", "coupler"):
        assert (tmp_path / "cnc" / f"{n}.pdf").stat().st_size > 1000  # a drawing named like its STEP file
    assert not (tmp_path / "cnc" / "drawings.pdf").exists()
    for folder, z in (("3dprint", "minimeat-3dprint.zip"), ("cnc", "minimeat-cnc.zip")):
        import zipfile
        names = set(zipfile.ZipFile(tmp_path / folder / z).namelist())
        assert names == {f.name for f in (tmp_path / folder).iterdir() if f.name != z}  # the zip is the folder
    assert not list(tmp_path.glob("duct.*")) and not list(tmp_path.glob("*_machined.step"))  # only the order folders hold them
    assert (tmp_path / "assembly.step").exists()
    glb = (tmp_path / "assembly.glb").read_bytes()  # a binary glTF, one mesh per part, not one per face
    assert glb[:4] == b"glTF" and glb.count(b'"POSITION"') < 40
    assert (tmp_path / "envelope.json").exists()


def test_made_parts_are_valid_and_the_bar_spans_the_pins():
    """The crank (a clamp hub with a radial set screw hole and a tapped pin hole) and the coupler are one valid
    solid each; the bar's holes are as far apart as the shaft and the servo's output axis."""
    from minimeat.mech.parts import MACHINED
    for build in MACHINED.values():
        s = build()
        assert s.is_valid and len(s.solids()) == 1
    bar = MACHINED["coupler"]()
    assert abs(bar.bounding_box().size.Y - (G.SERVO_Y + P.linkage.coupler_w)) < 1e-6  # centres SERVO_Y apart, round ends
