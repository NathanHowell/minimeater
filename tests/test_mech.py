"""The chassis builds, its parts fit with no interference through the flap's travel, and they print without support."""

import math

import pytest

from minimeater.mech import analysis, assembly as A, mock
from minimeater.mech import geom as G
from minimeater.mech.export import export
from minimeater.mech.params import P
from minimeater.mech.parts import MACHINED, PARTS
from minimeater.mech.printing import overhangs, print_pose


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
    hits = [h for h in A.interferences(allp) if set(h[:2]) not in ({"duct", "oring"}, {"duct", "tube"})]  # the ring is squeezed and the tube gripped by design
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
    assert G.SERVO_Z0 > G.PCB_Z1 + P.pcb.over_h + 2.0  # the servo clears the board's keep-out


def test_rod_stands_above_the_crank_and_under_the_bar():
    """The rod's end is gripable (0.2 to 0.4 mm over the crank's top with a +-0.1 cut) and clears the coupler, which
    lies over it when the flap is closed, by at least 0.5 mm; the pin heads stay 1.5 mm under the roof's nominal underside
    (on the hood itself see test_the_hood_clears_the_pin_heads)."""
    top = mock.rod().bounding_box().max.Z
    assert 0.2 <= top - G.ARM_Z1 <= 0.4
    assert G.COUPLER_Z0 - (top + 0.1) >= 0.5 - 1e-9
    assert P.shell.roof_in - (G.COUPLER_Z1 + P.linkage.pin_head_h) >= 1.5
    assert abs(G.COUPLER_Z1 - G.BOSS_Z0 + 0.1 - P.linkage.shoulder_len) < 1e-9  # the shoulder is 0.1 longer than the bar and its boss


@pytest.mark.parametrize("deg", [0, 5, 10, 45, 90])
def test_the_hood_clears_the_pin_heads(deg):
    """1.2 mm or more from the roof to both shoulder screws' heads over the travel: open, the horn pin is under the roof's
    corner round (0.5 mm without the hood's relief over it), closed, both are under the curve down to the nose."""
    from minimeater.mech import shell

    hood = _hood()
    pin_c, pin_h = mock.pins()
    assert hood.distance_to(A.about_shaft(pin_c, deg)) >= 1.15
    assert hood.distance_to(A.about_servo(pin_h, deg)) >= 1.15
    relief = shell.pin_relief()
    assert min(f.distance_to(relief) for f in shell.outer().faces()) >= 1.2  # the roof left over it


_HOOD = []


def _hood():
    from minimeater.mech import shell

    if not _HOOD:
        _HOOD.append(shell.hood())
    return _HOOD[0]


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


def test_shelf_pilots_keep_a_wall_to_the_body_cutout():
    """Each pilot keeps at least JLC3DP's 1.0 mm minimum wall to the servo body's cutout; the outer holes, at 0.45 and
    0.85 mm, have none."""
    sv = P.servo
    cut = (G.SERVO_X_CENTRE - sv.body_l / 2 - 0.2, G.SERVO_X_CENTRE + sv.body_l / 2 + 0.2)
    assert len(G.PILOT_HOLES) == 2
    for x, _ in G.PILOT_HOLES:
        assert min(abs(x - c) for c in cut) - P.m2_pilot / 2 >= 1.0


def test_board_lies_under_the_straight_duct():
    """The board (turned over: its back and the tails' keep-out up, the parts under it) sits beside the r = 18.2
    straight section, its rear edge a little into the cone, the keep-out one mm under the duct everywhere
    including at that edge, and the cone no steeper than 26 degrees."""
    assert G.g.straight_x0 <= G.g.board_x0 + P.layout.straight_over + 1e-9
    assert P.layout.straight_over == P.pcb.usb_end + 1.0
    cone_r = G.g.duct_r + 0.5 * (G.g.straight_x0 - G.g.board_x0)  # the cone's outer radius at the board's rear edge
    assert G.PCB_Z1 + P.pcb.over_h + 1.0 <= -cone_r + 1e-9
    assert G.g.board_x1 <= G.g.socket_x0 - 4.0
    assert G.PCB_Z1 + P.pcb.over_h <= -G.g.duct_r - 1.0 + 1e-9
    assert analysis.cone_half_angle() <= 26.1


def test_oring_seals_the_joint():
    """The ring is pinched between the tube and the groove bottom: 10-30 % squeeze, under 5 % stretch."""
    squeeze, stretch = analysis.oring_squeeze()
    assert 0.10 <= squeeze <= 0.30
    assert 0 <= stretch <= 0.05
    ring = A.bought()["oring"]
    assert A.overlap(ring, A.printed()["duct"]) > 0
    assert A.overlap(ring, A.bought()["tube"]) < 0.5


def test_the_socket_grips_the_tube_over_its_lobes_only():
    """The tube seats on the shoulder and slides free through the round bore, past the O-ring in it, and the first half of
    the lobes' run; the crests then reach in to `grip` and the last `lead_len` lets them out again, so the tube starts
    freely. The lobes start just in front of the groove, so the ring sits in round, rigid bore, and they touch the tube
    only where the slot behind the sleeve leaves them free."""
    from minimeater.mech.parts import box, lobe_amp, socket_r

    c, tr = P.coupling, P.tube.od / 2
    span = c.ext_len - c.wave_x0
    crest = lambda x: min(socket_r(x, math.pi * i / 64) for i in range(128))
    assert abs(min(crest(c.wave_x0 + span * i / 100) for i in range(101)) - (tr - c.grip)) < 2e-3  # the deepest grip
    assert abs(crest(c.ext_len) - (tr + c.lead_clear)) < 1e-9  # free at the mouth
    assert c.wave_x0 >= c.oring_x + 1.6 + 0.99  # round bore round the groove
    assert crest(c.wave_x0) > tr and crest(c.wave_x0 + span * 0.45) > tr  # free up to the middle of the run
    first = next(x for x in (c.wave_x0 + span * i / 500 for i in range(501)) if crest(x) < tr)
    assert first > c.sleeve_x0 + 2.0  # the crests touch the tube only where the lobes are free
    assert abs(lobe_amp() - 0.8) < 0.02  # about +-0.8 on a mean bore 1 mm larger
    tube, duct = A.bought()["tube"], A.printed()["duct"]
    assert A.overlap(tube & box(-30, c.wave_x0 + span * 0.45, -30, 30, -30, 30), duct) < 0.01
    assert A.overlap(tube, duct) > 3  # eight crest lines
    assert duct.bounding_box().max.X == pytest.approx(c.ext_len, abs=1e-3)


def test_the_socket_loft_follows_its_formula():
    """Between the loft's sections the bore stays on `socket_r` (a smootherstep through the stations, no ringing)."""
    import math

    from build123d import Vector

    from minimeater.mech.parts import socket_loft, socket_r

    faces = socket_loft().faces()
    for x in [-5.0] + [P.coupling.ext_len * f for f in (0.05, 0.27, 0.52, 0.77, 0.96, 0.99)]:
        for th in (0.0, math.pi / 8, math.pi / 5):
            r = socket_r(x, th)
            p = Vector(x, r * math.cos(th), r * math.sin(th))
            assert min(f.distance_to(p) for f in faces) < 0.01, (x, th)


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
    from minimeater.mech.parts import flap

    f = flap()
    xf, dm = G.g.flap_x, P.damper
    # the square corner of the rib's section, on the side away from the disc, is cut away; the middle of the end is kept
    assert not f.is_inside((xf + dm.rib_w / 2 - 0.1, dm.rib_t / 2 - 0.1, 5.0))
    assert f.is_inside((xf + dm.rib_w / 2 - 0.1, 0.0, 5.0))


def test_the_flaps_edge_is_round_and_stays_inside_its_sweep():
    """The disc's edge is a full round: its square corners are gone (the edge's middle is kept), and the whole flap lies in the
    sphere its closed edge circle spans, so the radial gap is never under the nominal at any angle (a square edge's corners
    reached 0.03 mm further at about 86 deg)."""
    from build123d import Pos, Sphere

    from minimeater.mech.parts import flap

    f = flap()
    xf, dm = G.g.flap_x, P.damper
    r = G.g.bore_r - dm.flap_gap
    assert not f.is_inside((xf + r - 0.1, dm.flap_t / 2 - 0.05, 0.0))  # open, the disc lies in x-z: the edge's corner
    assert f.is_inside((xf + r - 0.1, 0.0, 0.0))
    out = f - Pos(xf, 0, 0) * Sphere(r + 1e-3)
    assert out is None or out.volume < 1e-6


def test_the_one_sided_flap_variant_is_a_valid_solid(monkeypatch):
    from minimeater.mech.parts import flap

    monkeypatch.setattr(type(P.damper), "rib_both_sides", False)  # the dataclass is frozen: patch the class default
    f = flap()
    assert f.is_valid and len(f.solids()) == 1
    assert analysis.flap_leak_cm2() < 0.3
    assert analysis.flap_blockage_open() < 0.2


# Overhang inventory: faces steeper than 45 degrees off vertical, in the print pose. What is left must be
# short bridges and small holes. A new patch or a bigger total means a change that needs a printing look.
OVERHANG_AREA_MM2 = {"duct": 1430}  # 1420 measured: the socket's smootherstep flare (steeper than 44 deg in its middle, 376 mm2: MJF only) + 1043: the six stator vanes' cambered undersides near their leading edges, about 110 mm2 each (they wrap 41-53 deg for the fan's measured 66-79 deg inflow; was 45 each at 25-43 deg; about 660: MJF only, which needs no supports; on FDM they would need supports inside the contraction), + 415 with the thumbscrew boss gone (its underside and hole rims were about 60): 340 - those + the 1.2 mm land at the shelf's rear end (27 mm2) + the front servo block's face toward the body (12 x 7 = 84 mm2, a bridge as short as it is deep) + the stop posts' ends toward the rod (two 3 mm bridges, about 8 mm2)


@pytest.mark.parametrize("name", list(PARTS))
def test_prints_without_support(name):
    patches = overhangs(print_pose(name))
    total = sum(p["area"] for p in patches)
    assert total <= OVERHANG_AREA_MM2[name], patches
    assert all(p["area"] <= (380 if name == "duct" else 230) for p in patches), patches  # the largest: the duct's socket flare (MJF), else the O-ring groove's 2 mm ceiling


def _open_edges(stl):
    """Edges of a binary STL used by only one triangle (0: a closed mesh)."""
    import numpy as np

    raw = stl.read_bytes()
    n = int.from_bytes(raw[80:84], "little")
    rec = np.frombuffer(raw[84 : 84 + 50 * n], dtype=np.dtype([("n", "<f4", 3), ("v", "<f4", (3, 3)), ("a", "<u2")]))
    _, idx = np.unique(rec["v"].reshape(-1, 3), axis=0, return_inverse=True)
    tri = idx.reshape(-1, 3)
    edges = np.sort(np.concatenate([tri[:, [0, 1]], tri[:, [1, 2]], tri[:, [2, 0]]]), axis=1)
    _, counts = np.unique(edges, axis=0, return_counts=True)
    return int((counts == 1).sum())


def test_export_writes_step_and_stl(tmp_path):
    export(tmp_path)
    for n in list(PARTS) + ["hood", "pan"]:
        assert (tmp_path / "3dprint" / f"{n}.step").stat().st_size > 1000
        assert (tmp_path / "3dprint" / f"{n}.stl").stat().st_size > 1000
    assert print_pose("duct").bounding_box().size.Z <= 360  # MJF PA12S-HP's build height
    for n in list(PARTS) + ["hood", "pan"]:
        assert _open_edges(tmp_path / "3dprint" / f"{n}.stl") == 0, n  # watertight: the slicer and JLC3DP take it as is
    for n in ("flap", "crank", "coupler"):
        assert (tmp_path / "cnc" / f"{n}.step").stat().st_size > 1000
    for n in ("flap", "crank", "coupler"):
        assert (tmp_path / "cnc" / f"{n}.pdf").stat().st_size > 1000  # a drawing named like its STEP file
    assert not (tmp_path / "cnc" / "drawings.pdf").exists()
    for folder, z in (("3dprint", "minimeater-3dprint.zip"), ("cnc", "minimeater-cnc.zip")):
        import zipfile
        names = set(zipfile.ZipFile(tmp_path / folder / z).namelist())
        assert names == {f.name for f in (tmp_path / folder).iterdir() if f.name != z}  # the zip is the folder
    assert not list(tmp_path.glob("duct.*")) and not list(tmp_path.glob("*_machined.step"))  # only the order folders hold them
    assert (tmp_path / "assembly.step").exists()
    glb = (tmp_path / "assembly.glb").read_bytes()  # a binary glTF, one mesh per part, not one per face
    assert glb[:4] == b"glTF" and glb.count(b'"POSITION"') < 40
    assert (tmp_path / "envelope.json").exists()


def _glb(path):
    """The JSON chunk of a binary glTF and its binary chunk."""
    import json
    import struct

    raw = path.read_bytes()
    n = struct.unpack_from("<I", raw, 12)[0]
    return json.loads(raw[20 : 20 + n]), raw[28 + n :]


def _node_box(doc, name):
    """A mesh node's (min, max) in glTF's frame (m, y up)."""
    node = next(n for n in doc["nodes"] if n["name"] == name)
    acc = doc["accessors"][doc["meshes"][node["mesh"]]["primitives"][0]["attributes"]["POSITION"]]
    return acc["min"], acc["max"]


def test_scene_glb_has_materials_normals_the_air_path_and_a_spinning_rotor(tmp_path):
    """The Blender file: every part has a PBR material and smooth normals, the fan's frame is in (some of its faces will not
    mesh) and its rotor is a node of its own centred on the x axis, the air path's stations are empties along the flow axis,
    and the flap angle is on the scene."""
    from minimeater.mech import scene

    scene.write(tmp_path / "a.glb", scene.assembly(55.0), flap_deg=55.0)
    doc, _ = _glb(tmp_path / "a.glb")
    assert all("NORMAL" in p["attributes"] for m in doc["meshes"] for p in m["primitives"])
    assert {m["name"] for m in doc["materials"]} == {m["name"] for m in doc["meshes"]}
    assert {"flap", "rod", "tube", "fan", "fan_rotor"} <= {m["name"] for m in doc["meshes"]}
    stations = [n for n in doc["nodes"] if "mesh" not in n]
    assert [n["name"].split(".")[2] for n in stations] == ["grille", "plenum", "fan_in", "fan_out", "bore", "flap", "socket", "tube_end"]
    xs = [n["translation"][0] for n in stations]
    assert xs == sorted(xs) and all(n["translation"][1:] == [0.0, 0.0] for n in stations)  # in order, on the axis
    assert doc["scenes"][0]["extras"]["flap_deg"] == 55.0
    lo, hi = _node_box(doc, "fan")  # the frame, not just the hub and the screws, and no stray geometry
    assert min(hi[1] - lo[1], hi[2] - lo[2]) > 0.059 and hi[0] - lo[0] > 0.024 and max(abs(c) for c in lo + hi) < 0.2
    rlo, rhi = _node_box(doc, "fan_rotor")  # the rotor: inside the frame, round the axis, so a turn about x spins it in place
    assert min(rhi[1] - rlo[1], rhi[2] - rlo[2]) > 0.05 and lo[0] <= rlo[0] and rhi[0] <= hi[0]
    assert abs(rlo[1] + rhi[1]) < 1e-3 and abs(rlo[2] + rhi[2]) < 1e-3


def test_made_parts_are_valid_and_the_bar_spans_the_pins():
    """The crank (a clamp hub with a radial set screw hole and a tapped pin hole) and the coupler are one valid
    solid each; the bar's holes are as far apart as the shaft and the servo's output axis."""
    from minimeater.mech.parts import MACHINED
    for build in MACHINED.values():
        s = build()
        assert s.is_valid and len(s.solids()) == 1
    bar = MACHINED["coupler"]()
    assert abs(bar.bounding_box().size.Y - (G.SERVO_Y + P.linkage.coupler_w)) < 1e-6  # centres SERVO_Y apart, round ends


def test_the_made_rod_matches_the_assemblys_rod():
    """The CNC rod is Ø3 with a 2.5 mm flat and as long as the one in the assembly (blind pocket to 0.3 mm over the crank)."""
    from minimeater.mech import mock
    made, mocked = MACHINED["rod"]().bounding_box(), mock.rod().bounding_box()
    assert abs(made.size.X - P.damper.rod_len) < 1e-6 and abs(mocked.size.Z - P.damper.rod_len) < 1e-6
    assert abs(max(made.size.Y, made.size.Z) - P.damper.shaft_d) < 1e-6 and abs(min(made.size.Y, made.size.Z) - P.damper.shaft_flat) < 1e-6


def test_the_airway_runs_flush_into_the_tube():
    """The bore necks down to the tube's ID at the shoulder, and the tube's end face sits on a flat shoulder: no end
    face meets the flow, and the corner relief keeps the print's corner round from holding the tube off."""
    from build123d import Vector

    duct, x0, t = A.printed()["duct"], -P.coupling.depth, P.tube
    solid = lambda x, r: duct.is_inside(Vector(x, 0.0, r))
    assert solid(x0 - 0.1, t.id / 2 + 0.05) and not solid(x0 - 0.1, t.id / 2 - 0.05)  # the neck ends at the tube's bore
    assert solid(x0 - 0.1, (t.id + t.od) / 4)  # the shoulder under the tube's wall
    assert not solid(x0 - 0.2, t.od / 2 - 0.1)  # the relief at its outer corner
    assert not solid(x0 - P.coupling.neck_len - 0.1, P.damper.bore_d / 2 - 0.05)  # the full bore before the neck


def test_the_tail_cone_fills_the_fan_hub_and_the_area_falls():
    """The tail cone stands on the fan's motor plate (flush with the outlet face, the plate's Ø26.5) and closes at the bore's
    start: the annulus' area falls all the way from the fan to the bore, so the flow accelerates and stays attached. It is
    hollow, open toward the fan, and held by its vanes in one solid with the duct."""
    from build123d import Vector

    from minimeater.mech.parts import hub_cone, hub_profile

    h, x0 = P.hub_cone, G.g.fan_face_x
    assert hub_profile(0.0) == pytest.approx(P.fan.hub_d / 2) and hub_profile(1.0) == 0.0
    wall = lambda x: P.fan.aperture_d / 2 + (G.g.bore_r - P.fan.aperture_d / 2) * min((x - x0) / P.cone_len, 1.0)
    area = [math.pi * (wall(x0 + h.length * i / 50) ** 2 - hub_profile(i / 50) ** 2) for i in range(51)]
    assert all(b < a for a, b in zip(area, area[1:]))
    assert area[-1] == pytest.approx(math.pi * G.g.bore_r**2, rel=1e-6)
    duct = A.printed()["duct"]
    assert duct.is_valid and len(duct.solids()) == 1
    assert not duct.is_inside(Vector(x0 + 1.0, 0.0, 0.0))  # the cup is open toward the fan
    assert duct.is_inside(Vector(x0 + 1.0, 0.0, P.fan.hub_d / 2 - h.wall / 2))  # its wall
    assert hub_cone(solid=True).volume > hub_cone().volume


def test_the_vanes_meet_the_swirl_and_are_no_thinner_than_the_printer_allows():
    """Each vane's section is NACA thickness on a camber line (`vane_t`, thinning from the hub to the wall), round-nosed
    and closed round at `vane_te_t` (the thin-wall limit), thicker toward its roots. The camber line leaves the leading edge along the fan's swirl (the rotor
    turns -theta about +x) and turns back past axial at the trailing edge."""
    from minimeater.mech.parts import vane_half, vane_section

    h = P.hub_cone
    us = [i / 400 for i in range(401)]
    assert vane_half(0.0) == 0.0
    for t in h.vane_t:
        assert 2 * max(vane_half(u, t) for u in us) == pytest.approx(t, abs=0.01)
        assert 2 * vane_half(1.0, t) == pytest.approx(h.vane_te_t, abs=1e-6)
    for eta in (0.0, 0.5, 1.0):
        sec = vane_section(eta)
        le = min(sec, key=lambda p: p[0])
        te = max(sec, key=lambda p: p[0])
        mid = min(sec, key=lambda p: p[2])
        assert mid[2] < le[2] - 0.2 and te[2] > mid[2]  # it wraps with the swirl, then turns back

def test_the_extension_is_smooth_outside_and_its_lobes_are_free():
    """Outside, the socket's Ø46 runs on to the mouth; inside it the lobed wall stands free in an open slot (at least 0.8 mm,
    for the lobes to flex and the powder to leave), joined only at the sleeve's base."""
    from build123d import Vector

    from minimeater.mech.parts import socket_r

    c, duct = P.coupling, A.printed()["duct"]
    ths = [math.pi * j / 64 for j in range(129)]
    lobes_out = max(socket_r(c.ext_len * i / 100, t) for i in range(101) for t in ths) + c.lobe_wall
    assert c.od / 2 - c.sleeve_t - lobes_out >= 0.8
    for x in (c.sleeve_x0 + 0.5, c.ext_len / 2, c.ext_len - 0.2):
        for th in (0.0, math.pi / 8, 1.0):
            at = lambda r: Vector(x, r * math.cos(th), r * math.sin(th))
            assert duct.is_inside(at(c.od / 2 - c.sleeve_t / 2))  # the sleeve
            assert not duct.is_inside(at(c.od / 2 - c.sleeve_t - 0.4))  # the slot
            assert not duct.is_inside(at(c.od / 2 + 0.1))  # nothing outside the Ø46


def test_the_mass_budget_and_the_load_on_the_tube():
    """About 270 g with its centre about 70 mm behind the mouth: a lever on the tube's 30 mm, a few newtons at each end of
    the engagement and a fraction of a newton-metre at the egg. The numbers move as the parts do; the bounds catch a slip."""
    total, cg = analysis.mass_and_cg()
    assert 240 < total < 310 and -80 < cg < -60
    up, down, at_egg = analysis.tube_loads()
    assert 0 < up < 12 and 0 < down < 8 and at_egg < 0.3


def test_fan_screws_are_m3_into_pilots_in_the_flange(printed):
    """The fan's four M3 x 30 self-tap into Ø2.5 pilots through the flange (a hand-placed insert drills them to Ø4.0)."""
    from minimeater.mech.parts import xcyl
    x0, x1 = G.g.fan_face_x + 0.5, G.g.fan_face_x + 7.5
    for sy in (-1, 1):
        for sz in (-1, 1):
            y, z = sy * P.fan.hole_pitch / 2, sz * P.fan.hole_pitch / 2
            inner = xcyl(P.m3_pilot / 2 - 0.05, x0, x1, y, z)
            ring = xcyl(P.m3_pilot / 2 + 0.3, x0, x1, y, z) - xcyl(P.m3_pilot / 2 + 0.05, x0, x1, y, z)
            assert A.overlap(inner, printed["duct"]) < 1e-3
            assert A.overlap(ring, printed["duct"]) > 0.95 * ring.volume
