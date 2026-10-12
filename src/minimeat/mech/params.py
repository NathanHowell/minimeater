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
    hole_d: float = 4.3  # [est]
    aperture_d: float = 57.0  # [est] open area of the frame at the outlet
    hub_d: float = 24.0  # [est]
    corner_r: float = 5.0  # [est]


@dataclass(frozen=True)
class Servo:
    """Hitec HS-5065MG (.2.9), the pick over the MG90S class: metal gears, digital, same mounting. It stands
    beside the duct on the +y side, output face up, long axis along the flow."""

    body_l: float = 23.4  # [spec] long axis
    body_w: float = 11.4  # [spec]
    tab_l: float = 32.5  # [est] over both mounting tabs (MG90S value; same tab pattern class)
    tab_t: float = 2.5  # [est]
    tab_hole_pitch: float = 27.9  # [est]
    tab_hole_d: float = 2.0  # [est]
    body_h: float = 23.9  # [spec] case height, taken as back face to output face
    lead_w: float = 4.0  # [est] the cable leaves the body's end opposite the output
    tab_to_face: float = 4.1  # [est] tab's output-side face to the output face
    shaft_off: float = 6.0  # [est] output axis from the body end, along the long axis
    boss_d: float = 11.8  # [est] gear housing around the output
    boss_h: float = 2.5  # [est]
    spline_d: float = 4.8  # [spec]
    spline_h: float = 4.0  # [est]
    horn_hub_d: float = 7.0  # [est] single-arm horn: hub, arm, and a hole at Linkage.arm_r
    horn_hub_h: float = 4.0
    horn_arm_w: float = 5.0
    horn_arm_t: float = 2.0  # the arm, flush with the hub's top
    horn_len: float = 12.0  # hub centre to the arm's tip


@dataclass(frozen=True)
class Pcb:
    """Provisional envelope; the board outline becomes the contract at .1.6 / .3.6. The board lies flat in the
    pan under the duct, parts up; its -y long edge carries the connectors (see envelope.py)."""

    x_len: float = 44.0  # along the flow axis
    y_len: float = 54.0  # across
    t: float = 1.6
    hole_inset: float = 3.5
    hole_d: float = 2.7  # [est] M2.5
    top_h: float = 11.0  # [est] tallest part above the board (headers; the jacks are 3.2, the module 3.2)
    bottom_h: float = 2.5  # [est] through-hole lead clearance: the standoff height
    corner_r: float = 2.0
    edge_gap: float = 0.1  # connector faces (the board's -y edge) to the port wall's inner face
    # connectors on the -y edge, centre x in the board frame (0 = board centre, +x toward the egg)
    usb_x: float = -17.1
    usb_w: float = 9.4  # [spec] GCT USB4105-GF-A receptacle shell
    usb_h: float = 3.3
    jack_xs: tuple[float, ...] = (-7.9, 0.8, 9.5, 18.2)  # 8.7 pitch: 7 mm bodies, d8 plug overmolds 0.7 apart
    jack_w: float = 5.0  # [spec] SHOU HAN PJ-242 (ADR 0002): 2.5 mm TRRS, SMD, 5 wide, 12 mm back from the edge
    jack_h: float = 3.2  # [spec] body height; the plug axis is taken at half that above the board
    jack_hole_d: float = 5.0  # [est] the plug's barrel and the jack's entry; the overmold stays outside the wall
    plug_d: float = 8.0  # [est] probe plug overmold
    usb_open_w: float = 12.4  # [est] USB-C overmold passes the 2.4 mm wall to reach the receptacle
    usb_open_h: float = 6.8
    # the status LED and the button on the +y edge (the side away from the ports), both facing +y
    led_x: float = -2.0  # board frame; Lite-On LTST-S220KGKT side-view green, C160469 (2.2 x 1 mm)
    btn_x: float = 8.0  # ALPS SKRPACE010 top-push tact switch, C139797 (4.2 x 3.2 x 2.5 mm): inside, pressed with the case open
    led_z: float = 0.5  # emitter and actuator heights above the board's top face [est]
    btn_z: float = 1.0
    sensor: tuple[float, float] = (-5.0, 20.0)  # ambient sensor, board frame: on the underside, in the pan's vented well


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
    flap_t: float = 2.4  # six perimeters
    rib_t: float = 5.0  # the rib that carries the rod bore: the open flap's main blockage
    rib_w: float = 6.0
    flap_gap: float = 0.15  # radial; closed leak target <= 0.3 cm^2 (.2.3)
    stub_len: float = 3.0
    flap_x: float = -37.5  # shaft axis
    swing_margin: float = 1.5  # bore straight this far beyond the disc's swing, each end


@dataclass(frozen=True)
class Linkage:
    """Parallelogram between the flap rod and the servo: equal arms, a coupler bar, flap angle = servo angle.
    The plane is over the duct's crown, under the roof. Pins are M2 screws into the arms, heads retaining the
    coupler."""

    arm_r: float = 9.0  # pin radius on the crank and on the horn (the horn's hole at 9 mm [est])
    arm_w: float = 5.0
    arm_t: float = 3.0
    hub_d: float = 8.0
    hub_h: float = 3.5  # the crank's D-bored hub under its arm
    coupler_t: float = 2.5
    coupler_w: float = 6.0
    pin_d: float = 2.0  # M2
    pin_head_d: float = 3.8
    pin_head_h: float = 1.3
    lift: float = 0.8  # crank hub above the duct's crown


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

    wall: float = 2.4
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
    lip_t: float = 1.2  # the pan's tongue, up inside the hood's wall
    lip_h: float = 3.0
    rib_t: float = 1.0  # the hood's rib inside the tongue: the seam is a labyrinth
    boss_d: float = 6.0  # shell screw columns, M2.5
    screw_len: float = 20.0
    screw_engage: float = 6.0  # thread in the hood's boss
    rear_screw_y: float = 32.1  # |y| of the rear screw pair, in the fan's rear gap, outside its screw heads
    label: tuple[float, float, float, float] = (-46.0, 10.5, 40.0, 10.0)  # on the hood's -y flank (the port side): x, z, w, h
    label_depth: float = 0.8  # the flank is flat here (under the roof's round), so the recess has a floor
    grille_field: float = 58.0  # rounded square of louvres around the fan's axis
    grille_r: float = 14.0
    grille_pitch: float = 5.3  # five slots a side; the blades between are 1.1 mm at the faces
    grille_slot: float = 3.0  # slot height; cut 44 deg down and out (the first slot is centred 3.5 off the axis)
    grille_n: int = 5
    grille_bar: float = 1.6  # the slots stop this far from the parting plane
    bay_z: tuple[float, float] = (-34.0, -21.0)  # the bay's floor and ceiling (the eave)
    bay_x: tuple[float, float] = (-71.0, -19.0)
    bay_wall_t: float = 2.4  # the port wall
    vent_slots: tuple[tuple[float, float], ...] = ((-64.0, -56.0),)  # x ranges of the ambient louvres, +y wall
    vent_z: float = -33.0
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
    cone_len: float = 26.0
    screw_d: float = 2.5  # M2.5 self-tapping into plastic: pilot hole


P = Params()
