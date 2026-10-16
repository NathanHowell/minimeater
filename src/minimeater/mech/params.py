"""Dimensions for the enclosure, in mm. Every value carries a confidence tag:
  [spec] from a datasheet or the manufacturer
  [stl]  measured from a third-party model (docs/parts/heatermeter-adapter.md)
  [forum] community measurement
  [est]  engineering estimate: replace with a measurement (the bead that does it is named)
"""

from dataclasses import dataclass, field


@dataclass(frozen=True)
class Tube:
    od: float = 34.89  # [meas] calipers, 2026-10-15 (.2.8): the 1.375 in BBQ Guru tube; roundness still to check
    id: float = 30.83  # [meas] a 2.03 mm wall
    free_len: float = 31.94  # [meas] straight length standing off the egg: the socket (Coupling.depth + ext_len) must not need more


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
    tab_holes: tuple[tuple[float, float], ...] = ((-20.0, 0.0), (-19.2, -3.0), (-19.2, 3.0), (8.6, 0.0), (7.8, -3.0), (7.8, 3.0))  # [stl] all six Ø2.0 screw holes, (x from the output axis, y from the axis line), read off Hitec's STEP: a group of three at each end, 6 mm across, the middle one 0.8 mm further out (the drawing's 19.2 and 7.8 are the outer pairs). The shelf has a pilot under each: fit 2, 4 or 6 screws in the field
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
    pan under the duct, turned over: the SMT face (F.Cu) down on the standoffs with the parts hanging under it, the
    back (B.Cu, the through-hole tails) up toward the duct. Board frame: x toward the egg, y as the KiCad sheet's (the
    connector edge at -y), z up. That is the real board: seen from above with its parts up, the sheet's connectors
    are at the top and its USB-C at the left; turned over about x they are at -y and the USB-C stays at -x. Its -y
    long edge carries the connectors (see envelope.py)."""

    x_len: float = 47.5  # along the flow axis: 46 for the USB-C and four jacks side by side (45), plus usb_end
    usb_end: float = 1.5  # the board's extra length past the USB-C (x_len includes it): moves the USB-C out of the first jack's 0.2 mm web (commit fd4c019)
    y_len: float = 54.0  # across
    t: float = 1.6
    hole_d: float = 2.7  # [est] M2.5
    standoff_d: float = 5.5  # the pan's columns under the holes
    # three mounting holes, board frame (centre origin, y toward +y): the corners are taken by the connector row and the
    # module, so the screws sit where the layout left room; the connector row is held by the port wall
    holes: tuple[tuple[float, float], ...] = ((-10.2, 1.5), (20.75, 2.8), (20.75, 22.6))
    axis_z: float = -28.6  # the connector axes' height: the port row, wall and bay, as before the board was turned over
    under_h: float = 6.4  # the tallest thing under the board: the right-angle KK housings, 6.36 (STEP); the jacks are 3.7, the module 3.3
    floor_gap: float = 0.9  # under_h to the pan floor
    over_h: float = 2.5  # the through-hole tails (2 mm) and their solder above the board's back face
    kk: tuple[tuple[float, float], ...] = ((4.78, 10.6), (14.91, 8.1))  # the fan and servo plugs on the right-angle KK headers at the rear edge: y centre, width (J6 4 pins, J7 3)
    kk_plug_len: float = 9.0  # [est] the plug's housing past the header's front face (the pins go 6.7 into it); the cable leaves along -x, low, under the duct's fan transition
    edge_band: tuple[float, float] = (23.5, 3.7)  # beyond this y the pan floor curves up: only SMT parts (3.7: the tallest) hang under the board there
    corner_r: float = 2.0
    edge_trim: float = 1.75  # the board's connector edge is trimmed this far back from the frame's -y edge (hw/tools/connector_edge.py)
    # so the connectors reach into the port wall: the jacks' fronts are 0.1 mm inside the frame's edge, the USB-C's shell front 0.6
    jack_face: float = 0.1
    usb_face: float = 0.6  # the metal shell's front (the plastic housing is at 0.95)
    edge_gap: float = 0.1  # the trimmed board edge to the port wall's inner face
    # connectors on the -y edge, centre x in the board frame (0 = board centre, +x toward the egg)
    usb_x: float = -18.05
    usb_w: float = 9.4  # [spec] GCT USB4105-GF-A receptacle shell
    usb_h: float = 3.3
    jack_xs: tuple[float, ...] = (-7.05, 1.65, 10.35, 19.05)  # 8.7 pitch: d8 plug overmolds 0.7 apart; the jack's pads are 7.6 wide
    jack_w: float = 5.0  # [spec] SHOU HAN PJ-242 (ADR 0002): 2.5 mm TRRS, SMD, 5 wide, 12 mm back from the edge
    jack_h: float = 3.2  # [spec] body height; the plug axis is taken at half that above the board
    jack_hole_d: float = 5.0  # [est] the plug's barrel and the jack's entry; the overmold stays outside the wall
    plug_d: float = 8.0  # [est] probe plug overmold
    usb_open_w: float = 13.2  # [est] USB-C overmold (spec maximum 12.35 x 6.5; cables run larger) passes the wall to reach the receptacle; 1.7 mm of wall to the first jack's opening
    usb_open_h: float = 7.4
    jack_open_w: float = 5.4  # the jack's housing (5.0 wide, 3.15 tall plus a 0.45 tab, axis 1.6 above the board) stands in the wall
    jack_open_z: tuple[float, float] = (-0.2, 3.8)  # above the board's top face
    # the status LED and the button on the +y edge (the side away from the ports), both facing +y
    led_x: float = 15.75  # board frame; Lite-On LTST-S220KGKT side-view green, C160469 (2.2 x 1 mm)
    btn_x: float = 19.0  # SW1 at KiCad (41.25, 41.1); ALPS SKRPACE010 top-push tact switch, C139797 (4.2 x 3.2 x 2.5 mm): inside, pressed with the case open
    led_z: float = 0.5  # emitter and actuator heights, under the board's parts face [est]
    btn_z: float = 1.0
    sensor: tuple[float, float] = (-18.85, 24.8)  # ambient sensor TMP117, board frame: on the parts face (single-sided assembly), facing down at the louvres in the pan floor, rear corner
    btn_y: float = 14.1  # the reset button (SW1) faces down: a hole in the pan floor under it takes a paper clip


@dataclass(frozen=True)
class Print:
    """FDM rules (0.4 mm nozzle), so the parts print on any FDM printer; ordered in MJF PA12S-HP (docs/jlc3dp.md), where they hold too."""

    wall: float = 2.4  # six perimeters at 0.4
    floor: float = 2.4
    slip: float = 0.2  # diametral clearance for a free-sliding fit
    press: float = 0.05  # diametral clearance for a push fit
    bridge: float = 0.2


@dataclass(frozen=True)
class Damper:
    shaft_d: float = 3.0  # steel or brass rod; filament would also do
    shaft_flat: float = 2.5  # D flat across the rod
    rod_len: float = 42.7  # [spec] +-0.1: from the blind pocket in the duct's bottom wall to 0.3 mm over the crank (`mock.rod`)
    flap_t: float = 2.0  # machined (CNC, 304 or 6061), not printed
    rib_t: float = 4.0  # the rib that carries the rod bore: the open flap's main blockage
    rib_w: float = 5.0
    rib_both_sides: bool = True  # the disc is centred on the rib (a 1 mm bump each side; JLCCNC quoted it $21.90 against $21.34 for one side); False: the disc is flush with the rib's bottom (one side machined from plate)
    edge_round: bool = True  # the disc's edge is a full round (R = flap_t / 2), not square: the edge's profile then stays inside the sphere its closed edge circle spans, so the 0.15 gap holds at every angle (a square edge's corners reach 0.03 further at ~86 deg)
    serr_n: int = 0  # serrations round the disc's edge (0 = none, the design; an LES variant, cfd/variants.py: every notch is a leak when shut)
    serr_h: float = 1.0  # their depth (90 deg V notches: 2 serr_h wide at the edge)
    rib_round: float = 2.0  # the rib's ends along the flow are rounded to this radius (half the rib's thickness: a stadium section), not square
    bore_clear: float = 0.03  # diametral, the D-bore on the rod's flat: a retaining-compound fit (a machined part; the printed one was 0.05)
    flap_tol: float = 0.03  # radial, machined (+-0.05 on the diameter plus margin)
    ream_tol: float = 0.03  # radial: the duct's bore at the flap station reamed to size; as printed it is +-0.15 (JLC: +-0.3 mm)
    print_bore_tol: float = 0.15
    bore_d: float = 32.0  # the duct's straight bore, a standard H7 reamer size (Ø32 +0.025/0), reamed at the flap station; it necks down to the tube's ID before the shoulder (Coupling.neck_len), so it is reamed from the fan's end
    flap_gap: float = 0.15  # radial; closed leak target <= 0.3 cm^2 (.2.3)
    stub_len: float = 3.0
    flap_x: float = -37.5  # shaft axis
    swing_margin: float = 1.5  # bore straight this far beyond the disc's swing, each end

    @property
    def disc_off(self):
        """How far the disc's centre plane is from the shaft axis, mm."""
        return 0.0 if self.rib_both_sides else (self.rib_t - self.flap_t) / 2



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
    pin_head_d: float = 5.0  # [spec] the shoulder screw's head, Ø5 x 2.0 (McMaster/Zoro STR601M3X4: Ø3 x 4 shoulder, M2; take the 3 mm shoulder length)
    pin_head_h: float = 2.0
    shoulder_len: float = 3.0  # a little over the bar and its boss (2.9), so the bar turns free
    lift: float = 0.8  # crank hub above the duct's crown
    stop_w: float = 3.0  # the crank's stop lug: a rib under the arm, from the hub out to stop_r1, this wide
    stop_r0: float = 5.5  # the duct's stop posts stand from this radius from the rod ...
    stop_r1: float = 9.5  # ... to this one (the lug reaches it)
    stop_t: float = 3.0  # a post's thickness, the way the lug pushes on it
    stop_top: float = 1.0  # posts stop this far under the arm's underside
    stop_foot: float = 14.5  # a post's base z: inside the duct's wall
    bar_lift: float = 0.9  # the coupler's bosses under its pin holes: the bar rides this far above the arms, so the rod's end can stand above the crank (to grip it) and still clear the bar when the flap is closed


@dataclass(frozen=True)
class Coupling:
    depth: float = 20.0  # socket depth the tube enters
    neck_len: float = 5.0  # the bore narrows from Damper.bore_d to the tube's ID over this length, ending at the flat shoulder, so the airway runs on flush into the tube
    seat_relief: float = 0.4  # radial: a ring pit at the shoulder's outer corner, 0.5 deep, so the print's corner round cannot hold the tube's end off the shoulder
    oring_id: float = 34.0  # [est] 34 x 2.5 O-ring, the seal at the joint (.2.5)
    oring_cs: float = 2.5
    groove_depth: float = 1.9  # from the bore wall outward: sets the squeeze on the tube
    oring_x: float = -15.0  # groove centre (3.2 wide): the seal line about 5 mm in from the tube's end, clear of its chamfer or burr; deeper is cooler (further from the egg) and less travel through the ring
    wave_land: float = 1.0  # round bore between the groove's front edge and where the lobes start, so the ring sits in rigid bore
    od: float = 46.0
    # the socket runs on past the old mouth (x = 0) toward the egg; over that length the bore goes from round to a sinusoidal
    # section along a smootherstep, the wall following it, so its crests grip the tube (no thumbscrew)
    ext_len: float = 10.0  # how far the socket reaches past x = 0: the tube's 31.94 less the 20 deep socket leaves 1.94 mm to the egg
    ext_grow_d: float = 1.0  # the mean bore's growth over the extension, on the diameter
    lobes: int = 8  # crests around the bore
    grip: float = 0.15  # radial: the crests' deepest reach into the tube, at the egg end of the transition
    lead_len: float = 3.0  # the last stretch at the mouth opens up so the tube starts freely ...
    lead_clear: float = 0.15  # ... to this radial clearance at the crests
    lobe_wall: float = 1.7  # the lobed wall, following the wave so the crests flex; thin enough to sit inside the sleeve
    sleeve_t: float = 1.2  # a smooth sleeve carries the socket's Ø46 outside on to the mouth, joined only at its base ...
    sleeve_x0: float = -10.0  # ... up to here: past it an annular slot (open at the mouth) leaves the lobes free; 3.4 mm in front of the groove, in front of the shell
    collar_x: float = -12.5  # drip collar behind the hood's front ring: a 44 deg cone r23 -> r24.5 toward -x, 0.3 off the shells' cone
    collar_r: float = 24.5

    @property
    def wave_x0(self):
        """Where the bore leaves round for the lobes: the groove's front edge plus the land."""
        return self.oring_x + 1.6 + self.wave_land


@dataclass(frozen=True)
class Layout:
    """x stations (from the socket mouth) that are not derived from a part: the board under the straight duct,
    the front face, the rear gap."""

    board_x1: float = -24.5  # board's front edge: under the socket flare's first mm, clear of the shells' front cone
    straight_over: float = 2.5  # the board's rear edge may sit this far into the cone (its outer r grows 0.5/mm): 1.0 for the parts, plus Pcb.usb_end, where only the USB-C stands
    x_front: float = -11.0  # the front face: the socket's last 11 mm and its extension are outside
    rear_gap: float = 14.5  # fan's rear face to the rear wall's inner face: the plenum in front of the rotor. The grille CFD (cfd/grille.py, docs/cfd.md) leaves the fan 2.9 Pa of its 12.0 at 4.5 mm, 6.7 at 14.5, 7.4 at 24.5: the knee
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
    bay_x: tuple[float, float] = (-74.5, -24.0)  # the front end stops short of the front screw column (x -18, |y| 28: its teardrop reaches x -22.8), so the column stands in solid pan; the front jack's plug (to x -25.2) still clears it
    bay_wall_t: float = 2.4  # the port wall
    vent_slots: tuple[tuple[float, float], ...] = ((-71.0, -63.0),)  # x ranges of the ambient louvres through the pan floor, under the TMP117
    vent_dy: float = 1.5  # two slots, this far either side of the sensor
    vent_w: float = 1.4
    reset_d: float = 3.2  # the reset hole in the pan floor
    pipe_d: float = 1.75  # the LED's light pipe: a piece of clear 1.75 mm filament, glued flush in a 1.9 mm hole


@dataclass(frozen=True)
class Mass:
    """What the box weighs (analysis.mass_budget): densities for the CAD's volumes, and the bought parts by their data."""

    pa12: float = 1.01  # [spec] g/cm^3, MJF PA12 parts (HP)
    machined: float = 7.93  # [spec] 304 stainless; 6061 is 2.70
    fr4: float = 1.85  # [spec]
    fan_g: float = 50.0  # [spec] Noctua's NF-A6x25 5V PWM, without its pads
    servo_g: float = 11.9  # [spec] HS-5065MG (docs/parts/servo-hs-5065mg.md)
    horn_g: float = 0.9  # [est] the Secraft aluminium arm (0.34 cm^3 of 6061)
    board_parts_g: float = 5.0  # [est] the components and connectors on the board
    hardware_g: float = 7.0  # [est] the screws (fan, servo, board, shells), the shoulder screws' nuts, the O-ring
    hardware_x: float = -70.0  # [est] where they sit, on average


@dataclass(frozen=True)
class HubCone:
    """A tail cone printed with the duct, on the fan's motor plate (stationary: part of Noctua's frame, a flat Ø26.5
    face flush with the outlet), held by stator vanes from the contraction's wall (.2.22, .2.25). It fills the dead zone
    behind the plate so the annulus runs smoothly into the bore. A starting shape: the CFD tunes it (.2.13)."""

    length: float = 27.0  # the tip at the bore's start (Params.cone_len): the annulus' area falls all the way
    exponent: float = 2.0  # radius r0 (1 - t^n): parallel to the axis at the plate, so the flow leaves its edge without a turn
    tail: float = 1.0  # ... to the power b: 1 ends in a point (44 deg at n = 2), above 1 the tail runs into the axis tangent (a cusp)
    wall: float = 1.6  # hollow, open toward the fan: MJF is priced by volume and a closed hollow traps powder
    # stator vanes from the contraction's wall carry it and take out the fan's swirl (.2.25): the rotor leaves the air
    # turning with it, a free vortex (r c_theta constant) whose dynamic head is otherwise lost in the tube
    vanes: int = 6  # turning ~50-70 deg wants a solidity near 1; 6 is co-prime with the rotor's 7 blades (no common tone)
    vane_clock: float = 0.0  # deg from +z: one straight up, one straight down
    swirl: float = 18.0  # Euler's floor, r c_theta / Q in 1/m on the model's line (dp = rho omega r c_theta): the CFD's free-vortex inlet. The fan's own (MRF, cfd/fan.py) is about 39, flux-weighted
    # the air's angle from axial at the leading edge by span (0 the cone, 1 the wall), deg: the duct CFD on the fan's measured
    # exit profile (cfd/fan.py at 4.2 CFM, 3000 rpm, near stall: the flow leaves through the outer third, turning hard),
    # 66-79 deg over r 14-28 at 2 mm behind the face: the least-squares cubic through those, by these four points (the
    # wall's eased 2 deg toward the last bin); within 3.5 deg of every bin
    vane_beta: tuple = ((0.0, 71.8), (1 / 3, 75.3), (2 / 3, 66.8), (1.0, 72.0))
    vane_le: tuple[float, float] = (4.0, 7.0)  # the leading edge behind the plate at the hub and at the wall, mm ...
    vane_le_bow: float = 2.0  # ... bowed forward this much at mid span (a swept, crescent leading edge)
    vane_solidity: float = 1.3  # (1.2 with the free vortex's 50-70 deg) each section's chord is what makes camber length / pitch this: the pitch grows with the radius, so the chord does, longest at the wall
    vane_te_max: float = 31.0  # mm behind the plate: no trailing edge past this (the bore's start is 27, the flap's swing 43)
    vane_t: tuple[float, float] = (2.2, 1.5)  # the section: NACA 4-digit thickness on its camber line, at the hub and at the wall ...
    vane_te_t: float = 1.2  # ... cut where it thins to this (the thin-wall limit) and closed round
    vane_flare: float = 0.9  # extra thickness (times vane_t) where a vane meets the cone and the wall: a fillet's blend ...
    vane_flare_w: float = 0.12  # ... over this fraction of the span
    vane_dev: float = 0.28  # (0.23 left 9 % of the swirl, with the rotor) Carter's m: the air leaves a cambered vane short of its metal angle by m camber / sqrt(solidity); the vane overturns by that


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
    hub_cone: HubCone = field(default_factory=HubCone)
    mass: Mass = field(default_factory=Mass)
    cone_len: float = 27.0
    contraction: str = "cone"  # the airway's wall from the fan's bore to the duct's: "cone" (straight, a corner at each end), "cubic" or "quintic" (smoothstep, smootherstep: tangent to both)
    screw_d: float = 2.5  # M2.5 self-tapping into plastic: pilot hole


P = Params()
