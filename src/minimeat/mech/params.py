"""Dimensions for the enclosure, in mm. Every value carries a confidence tag:
  [spec] from a datasheet or the manufacturer
  [stl]  measured from a third-party model (docs/parts/heatermeter-adapter.md)
  [forum] community measurement
  [est]  engineering estimate: replace with a measurement (the bead that does it is named)
"""

from dataclasses import dataclass, field


@dataclass(frozen=True)
class Tube:
    od: float = 34.93  # [stl] 1.375 in, BBQ Guru style; calipers still needed (.2.8)
    id: float = 31.6  # [forum] BBQ Guru-compatible adapters list ~31 mm ID (.2.8)
    free_len: float = 40.0  # [est] straight length available to the socket (.2.8)


@dataclass(frozen=True)
class Fan:
    """Noctua NF-A6x25 5V PWM, the provisional pick (.2.2)."""

    size: float = 60.0  # [spec]
    depth: float = 25.0  # [spec]
    hole_pitch: float = 50.0  # [spec]
    hole_d: float = 4.3  # [spec] Noctua's public model
    aperture_d: float = 58.0  # [spec] the frame's bore (the model's inner wall at r = 29)
    hub_d: float = 26.5  # [spec] the motor plate's hub on the outlet face (r = 13.25 at 1 mm in)
    corner_r: float = 4.0  # [spec] from the model's corner extent


@dataclass(frozen=True)
class Servo:
    """Hitec HS-5065MG (.2.9), the pick over the MG90S class: metal gears, digital. Dimensions from Hitec's drawing
    (HS-5065MG-Servo-Specs, hitecrcd.com product page). It stands
    beside the duct on the +y side, output face up, long axis along the flow."""

    body_l: float = 23.6  # [spec] Hitec drawing: long axis
    body_w: float = 11.6  # [spec]
    tab_l: float = 32.3  # [spec] over both mounting flanges
    tab_t: float = 2.0  # [spec]
    tab_hole_dx: tuple[float, float] = (-20.0, 8.6)  # [stl] x of the two screw holes on the axis line, from the output axis, read off Hitec's STEP: each end has three Ø2.0 holes, 6 mm across, the middle one 0.8 mm further out than the other two (the drawing's 19.2 and 7.8 are those two). The shelf's pilots are under the middle pair, 28.6 apart
    tab_hole_d: float = 2.0  # [spec]
    body_h: float = 20.0  # [spec] back face to the case top; the gear housing and spline stand on it
    lead_w: float = 4.0  # [est] the cable leaves the body's end opposite the output
    tab_to_face: float = 1.0  # [spec] case top to the flange's top face (flange underside 17.0, top 19.0)
    shaft_off: float = 5.9  # [spec] output axis from the short end of the body (the drawing's 19.2 is to the far holes)
    boss_d: float = 11.6  # [spec] gear housing, as wide as the case; it is a lobe 15 mm long, drawn here as a cylinder
    boss_h: float = 6.0  # [spec] housing top 26.0 above the back face
    spline_d: float = 5.0  # [spec] 24T output, outer diameter from Hitec's STEP model
    spline_h: float = 3.0  # [spec] spline top 29.0 above the back face in Hitec's STEP model (the drawing's 30.0 has the horn)
    horn_hub_d: float = 9.0  # [est] Secraft 15 mm aluminium arm (Hitec 24T): hub, arm, M2 holes at 12 and 15 mm
    horn_hub_h: float = 4.0  # [est]
    horn_arm_w: float = 5.5  # [est]
    horn_arm_t: float = 2.5  # [est] the arm, flush with the hub's top
    horn_len: float = 15.0  # [spec] hub centre to the far hole (the arm's tip); the near hole, at Linkage.arm_r, is 12


@dataclass(frozen=True)
class Pcb:
    """Provisional envelope; the board outline becomes the contract at .1.6 / .3.6. The board lies flat in the
    pan under the duct, parts up; its -y long edge carries the connectors (see envelope.py)."""

    x_len: float = 46.0  # along the flow axis (46: USB-C and four jacks side by side need 45)
    y_len: float = 54.0  # across
    t: float = 1.6
    hole_d: float = 2.7  # [est] M2.5
    # three mounting holes, board frame (centre origin, y toward +y): the corners are taken by the connector row and the
    # module, so the screws sit where the layout left room; the connector row is held by the port wall
    holes: tuple[tuple[float, float], ...] = ((-12.4, 1.7), (20.0, 2.8), (20.0, 22.6))
    top_h: float = 11.0  # [est] tallest part above the board (headers; the jacks are 3.2, the module 3.2)
    bottom_h: float = 2.5  # [est] through-hole lead clearance: the standoff height
    corner_r: float = 2.0
    edge_trim: float = 1.75  # the board's connector edge is trimmed this far back from the frame's -y edge (hw/tools/connector_edge.py)
    # so the connectors reach into the port wall: the jacks' fronts are 0.1 mm inside the frame's edge, the USB-C's shell front 0.6
    jack_face: float = 0.1
    usb_face: float = 0.6  # the metal shell's front (the plastic housing is at 0.95)
    edge_gap: float = 0.1  # the trimmed board edge to the port wall's inner face
    # connectors on the -y edge, centre x in the board frame (0 = board centre, +x toward the egg)
    usb_x: float = -17.3
    usb_w: float = 9.4  # [spec] GCT USB4105-GF-A receptacle shell
    usb_h: float = 3.3
    jack_xs: tuple[float, ...] = (-7.8, 0.9, 9.6, 18.3)  # 8.7 pitch: d8 plug overmolds 0.7 apart; the jack's pads are 7.6 wide
    jack_w: float = 5.0  # [spec] SHOU HAN PJ-242 (ADR 0002): 2.5 mm TRRS, SMD, 5 wide, 12 mm back from the edge
    jack_h: float = 3.2  # [spec] body height; the plug axis is taken at half that above the board
    jack_hole_d: float = 5.0  # [est] the plug's barrel and the jack's entry; the overmold stays outside the wall
    plug_d: float = 8.0  # [est] probe plug overmold
    usb_open_w: float = 13.2  # [est] USB-C overmold (spec maximum 12.35 x 6.5; cables run larger) passes the wall to reach the receptacle; the widest the jack beside it allows
    usb_open_h: float = 7.4
    jack_open_w: float = 5.4  # the jack's housing (5.0 wide, 3.15 tall plus a 0.45 tab, axis 1.6 above the board) stands in the wall
    jack_open_z: tuple[float, float] = (-0.2, 3.8)  # above the board's top face
    # the status LED and the button on the +y edge (the side away from the ports), both facing +y
    led_x: float = 15.0  # board frame; Lite-On LTST-S220KGKT side-view green, C160469 (2.2 x 1 mm)
    btn_x: float = 14.3  # ALPS SKRPACE010 top-push tact switch, C139797 (4.2 x 3.2 x 2.5 mm): inside, pressed with the case open
    led_z: float = 0.5  # emitter and actuator heights above the board's top face [est]
    btn_z: float = 1.0
    sensor: tuple[float, float] = (-19.6, 24.8)  # ambient sensor TMP117, board frame: top side (single-sided assembly), rear corner, at the +y flank's louvres


@dataclass(frozen=True)
class Print:
    """FDM process: ASA or PETG, 0.4 mm nozzle."""

    wall: float = 2.4  # six perimeters at 0.4
    floor: float = 2.4
    slip: float = 0.2  # diametral clearance for a free-sliding fit
    press: float = 0.05  # diametral clearance for a push fit
    bridge: float = 0.2


@dataclass(frozen=True)
class Damper:
    shaft_d: float = 3.0  # steel or brass rod; filament would also do
    shaft_flat: float = 2.5  # D flat across the rod
    flap_t: float = 2.0  # machined (CNC, 304 or 6061), not printed
    rib_t: float = 4.0  # the rib that carries the rod bore: the open flap's main blockage
    rib_w: float = 5.0
    bore_clear: float = 0.03  # diametral, the D-bore on the rod's flat: a retaining-compound fit (a machined part; the printed one was 0.05)
    flap_tol: float = 0.03  # radial, machined (+-0.05 on the diameter plus margin)
    ream_tol: float = 0.03  # radial: the duct's bore at the flap station reamed to size; as printed it is +-0.15 (JLC: +-0.3 mm)
    print_bore_tol: float = 0.15
    bore_d: float = 32.0  # the duct's straight bore, a standard H7 reamer size (Ø32 +0.025/0), reamed at the flap station; the tube's ID (31.6, to measure) is 0.2 mm smaller: the step is the tube's own thickness
    flap_gap: float = 0.15  # radial; closed leak target <= 0.3 cm^2 (.2.3)
    stub_len: float = 3.0
    flap_x: float = -37.5  # shaft axis
    swing_margin: float = 1.5  # bore straight this far beyond the disc's swing, each end


@dataclass(frozen=True)
class Linkage:
    """Parallelogram between the flap rod and the servo: equal arms, a flat coupler bar, flap angle = servo angle.
    The plane is over the duct's crown, under the roof. The servo's arm is bought (Secraft 15 mm aluminium, Hitec
    24T: the spline is not worth machining); the crank and the coupler are machined (JLCPCB CNC, 5 off). The
    pivots are plain: an M2 shoulder screw through each bar hole into the arm (a nut under the horn), no ball
    joints, so the pin spacing is free."""

    arm_r: float = 12.0  # [spec] pin radius on the crank and on the horn (the Secraft 15 mm arm's near hole)
    arm_w: float = 5.5
    arm_t: float = 3.0
    hub_d: float = 8.0
    hub_h: float = 3.5  # the crank's D-bored hub under its arm
    slip: float = 0.1  # the crank's bore over the rod; the set screw clamps it
    set_screw_d: float = 1.6  # tap drill for the M2 set screw, radial on the flat
    coupler_t: float = 2.0  # 304 stainless (or 6061) flat bar
    coupler_w: float = 6.0
    pin_d: float = 3.0  # M2 shoulder screw: Ø3 shoulder (ISO 7379 pairs a Ø3 shoulder with an M2 thread; a Ø2 shoulder is M1.6)
    thread_d: float = 2.0  # its thread: into the crank's tapped hole, through the horn's M2 hole to the nyloc nut
    pin_hole_clear: float = 0.05  # radial: the bar's holes are Ø3.1
    pin_head_d: float = 5.5  # [est] the shoulder screw's head, Ø5.5 x 2.0: check the supplier's drawing
    pin_head_h: float = 2.0
    shoulder_len: float = 3.0  # a little over the bar and its boss (2.9), so the bar turns free
    lift: float = 0.8  # crank hub above the duct's crown
    bar_lift: float = 0.9  # the coupler's bosses under its pin holes: the bar rides this far above the arms, so the rod's end can stand above the crank (to grip it) and still clear the bar when the flap is closed


@dataclass(frozen=True)
class Coupling:
    depth: float = 20.0  # socket depth the tube enters
    oring_id: float = 34.0  # [est] 34 x 2.5 O-ring, the seal at the joint (.2.5)
    oring_cs: float = 2.5
    groove_depth: float = 1.9  # from the bore wall outward: sets the squeeze on the tube
    oring_x: float = -10.0  # groove centre
    od: float = 46.0
    lead_in: float = 1.5
    boss_x: float = -4.7  # thumbscrew boss centre, on the socket's +y side, outside the hood
    boss_w: float = 9.4  # the boss: a block flush with the mouth, round on top
    collar_x: float = -12.5  # drip collar behind the hood's front ring: a 44 deg cone r23 -> r24.5 toward -x, 0.3 off the shells' cone
    collar_r: float = 24.5


@dataclass(frozen=True)
class Layout:
    """x stations (from the socket mouth) that are not derived from a part: the board under the straight duct,
    the front face, the rear gap."""

    board_x1: float = -24.5  # board's front edge: under the socket flare's first mm, clear of the shells' front cone
    straight_over: float = 1.0  # the board's rear edge may sit this far into the cone (its outer r grows 0.5/mm)
    x_front: float = -11.0  # the front face: the socket's last 11 mm and the thumbscrew are outside
    rear_gap: float = 4.5  # fan's rear face to the rear wall's inner face: the fan screw heads and the inlet
    front_screw_x: float = -18.0  # the front shell screws, in the shells' thick front zone
    slope: float = 1.0355  # run per rise of every designed overhang: 44 degrees, inside the 45 the printer allows


@dataclass(frozen=True)
class Shell:
    """The exterior (shell.py): one lofted pebble split at the tube axis into the hood (above) and the pan
    (below). Sections are rounded rectangles (w, h, r) at x stations; the belly is flat behind the nose."""

    wall: float = 2.8  # seven perimeters: the seam's skirt is wall - tongue - 0.2 and both must stay over the printer's 1.2 mm minimum
    hw_in: float = 34.4  # inner half width at the full section
    roof_in: float = 32.0  # inner roof at the fan (the flange top is 31)
    crown: float = 2.0  # the roof rises this much from the fan station to the peak
    peak_x: float = -62.0
    rear_w: float = 72.0  # the rear face's section
    rear_h: float = 70.0
    rear_r: float = 14.0
    full_r: float = 16.4  # corner round of the full section at the fan (inner 14: the fan's corners need it)
    peak_r: float = 19.0  # at the peak: only the duct is there
    nose_w: float = 60.0  # the front face's section (the hole is 47)
    nose_top: float = 27.0
    nose_bot: float = -33.0  # the belly rises only this much: the board's front edge is under the rise
    nose_r: float = 18.0
    x_taper: float = -28.0  # the plan is full back from here, closes to the nose in front of it
    x_roof: float = -40.0  # the roof starts down to the nose here: earlier than the plan, so the corners stay under 45 deg
    x_belly: float = -23.0  # the belly is flat back from here
    front_t: float = 1.6  # the front ring's land around the nose hole; the cavity's 44 deg cone starts behind it
    rear_chamfer: float = 3.0  # the rear face's edge, 45 deg: the print bed face
    nose_round: float = 5.0  # the front ring's edge
    nose_hole_d: float = 47.0  # the socket is 46
    z_part: float = 0.0  # the parting plane: the tube axis
    lip_t: float = 1.3  # the pan's tongue, up inside the hood's wall; the hood's skirt outside it is wall - lip_t - 0.2 = 1.3
    lip_h: float = 3.0
    rib_t: float = 1.3  # the hood's rib inside the tongue: the seam is a labyrinth
    boss_d: float = 6.7  # shell screw columns, M2.5: the pan's head channel is d/2 - 1.05 = 2.3 radius (an M2.5 head is 4.5), leaving a 1.05 mm sleeve; it was 2.9 and a 0.1 mm sleeve. Larger columns make the pan fail to tessellate (OCC), 6.7 is the largest that works
    screw_len: float = 20.0
    screw_engage: float = 6.0  # thread in the hood's boss
    rear_screw_y: float = 33.3  # |y| of the rear screw pair, in the fan's rear gap, outside its screw heads and the fan shim block: the pan's head channel (radius 2.15) leaves 1.2 mm to the cavity wall at y 29.9, and the hull 1.3 mm outside (at 32.1 the channel left 0.08 mm)
    label: tuple[float, float, float, float] = (-46.0, 10.5, 40.0, 10.0)  # on the hood's -y flank (the port side): x, z, w, h
    label_depth: float = 0.8  # the flank is flat here (under the roof's round), so the recess has a floor
    grille_field: float = 58.0  # rounded square of louvres around the fan's axis
    grille_r: float = 14.0
    grille_pitch: float = 6.2  # five slots a side, vertical pitch; the slots are cut 44 deg, so the blades between are pitch x cos 44 - slot thick
    grille_slot: float = 3.2  # slot height; cut 44 deg down and out (the first slot is centred 3.5 off the axis)
    grille_n: int = 5
    grille_bar: float = 1.6  # the slots stop this far from the parting plane
    bay_z: tuple[float, float] = (-34.0, -21.0)  # the bay's floor and ceiling (the eave)
    bay_x: tuple[float, float] = (-73.0, -19.0)
    bay_wall_t: float = 2.4  # the port wall
    vent_slots: tuple[tuple[float, float], ...] = ((-71.0, -63.0),)  # x ranges of the ambient louvres, +y wall, at the TMP117 on the board's top
    vent_z: float = -29.0  # just above the board's top face: outside air reaches the sensor on top
    pipe_d: float = 1.75  # the LED's light pipe: a piece of clear 1.75 mm filament, glued flush in a 1.9 mm hole


@dataclass(frozen=True)
class Params:
    tube: Tube = field(default_factory=Tube)
    fan: Fan = field(default_factory=Fan)
    servo: Servo = field(default_factory=Servo)
    pcb: Pcb = field(default_factory=Pcb)
    print: Print = field(default_factory=Print)
    damper: Damper = field(default_factory=Damper)
    coupling: Coupling = field(default_factory=Coupling)
    linkage: Linkage = field(default_factory=Linkage)
    layout: Layout = field(default_factory=Layout)
    shell: Shell = field(default_factory=Shell)
    cone_len: float = 27.0
    screw_d: float = 2.5  # M2.5 self-tapping into plastic: pilot hole


P = Params()
