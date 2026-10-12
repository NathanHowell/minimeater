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
    """Hitec HS-5065MG (.2.9), the pick over the MG90S class: metal gears, digital, same mounting. The output
    face points at the duct."""

    body_l: float = 23.4  # [spec] long axis
    body_w: float = 11.4  # [spec]
    tab_l: float = 32.5  # [est] over both mounting tabs (MG90S value; same tab pattern class)
    tab_t: float = 2.5  # [est]
    tab_hole_pitch: float = 27.9  # [est]
    tab_hole_d: float = 2.0  # [est]
    body_h: float = 23.9  # [spec] case height, taken as back face to output face
    tab_to_face: float = 4.1  # [est] tab's output-side face to the output face
    shaft_off: float = 6.0  # [est] output axis from the body end, along the long axis
    boss_d: float = 11.8  # [est] gear housing around the output
    boss_h: float = 2.5  # [est]
    spline_d: float = 4.8  # [spec]
    spline_h: float = 4.0  # [est]
    horn_d: float = 20.0  # [est] round horn
    horn_t: float = 4.0  # [est] hub + disc
    horn_hole_r: float = 7.0  # [est] screw circle on the round horn


@dataclass(frozen=True)
class Pcb:
    """Provisional envelope; the board outline becomes the contract at .1.6 / .3.6."""

    x_len: float = 44.0  # along the flow axis
    y_len: float = 54.0
    t: float = 1.6
    hole_inset: float = 3.5
    hole_d: float = 2.7  # [est] M2.5
    top_h: float = 11.0  # [est] tallest part above the board (ESP32 module 3.2, jacks ~6, headers)
    bottom_h: float = 2.5  # [est] through-hole lead clearance
    corner_r: float = 2.0
    # connectors on the rear edge, centre y (board frame, 0 = board centre) and the opening they need
    rear_gap: float = 2.0  # board edge to the rear wall's inner face (the lid's lip also needs 1.8)
    front_gap: float = 2.0
    usb_y: float = -21.5
    usb_w: float = 9.4  # [spec] USB-C receptacle shell
    usb_h: float = 3.3
    jack_ys: tuple[float, ...] = (-9.0, 1.0, 11.0, 21.0)
    jack_w: float = 7.0  # [est] 2.5 mm stereo jack body
    jack_h: float = 6.0  # [est]
    jack_hole_d: float = 8.5  # [est] plug overmold; the board edge sits 4.4 mm behind the wall's outer face
    usb_open_w: float = 13.4  # [est] USB-C overmold
    usb_open_h: float = 7.3


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
    head_d: float = 18.0  # horn link flange
    head_t: float = 3.0
    flap_x: float = -37.5  # shaft axis
    swing_margin: float = 1.5


@dataclass(frozen=True)
class Coupling:
    depth: float = 20.0  # socket depth the tube enters
    oring_id: float = 34.0  # [est] 34 x 2.5 O-ring, the seal at the joint (.2.5)
    oring_cs: float = 2.5
    groove_depth: float = 1.9  # from the bore wall outward: sets the squeeze on the tube
    oring_x: float = -10.0  # groove centre
    od: float = 46.0
    lead_in: float = 1.5


@dataclass(frozen=True)
class Skin:
    """The exterior shells (skin.py): a hood and a pan around the v0 internals. x from the socket mouth, z from the
    tube axis. The plan is straight to the shoulder, then tapers to the front; the bottom rises from the flange."""

    wall: float = 2.6  # a little over Print.wall: the walls taper 8.5 deg in plan
    x_rear: float = -112.0  # rear face; the grille part is at -108.8
    slab_t: float = 2.4  # the rear plate, half on each shell
    x_shoulder: float = -56.0  # the tray's front edge: the plan is parallel behind it
    x_front: float = -9.0  # the front face: the servo tower's corner at -15.1 needs 3.5 mm behind the inner face
    x_nose: float = -9.0  # apex of an optional front bulge in plan (= x_front: flat)
    hw_rear: float = 38.0  # half width: the tray is 66.8 wide
    hw_front: float = 31.0
    z_top: float = 62.0  # the servo top is 56.7, under the front-top fillet
    z_bot: float = -34.8  # under the fan and the flange (-31)
    z_bot_front: float = -29.5  # under the socket (-23)
    r_top: float = 14.0  # the largest that clears the tray's top corners (33.4, 51)
    r_bot: float = 15.0  # the largest that clears the flange's corners
    r_shoulder: float = 120.0  # plan blend from the parallel sides into the taper
    r_front: float = 14.0  # plan corners at the front (>= r_top, so the top fillet can turn them)
    bevel: float = 2.4  # the rear face's edges: a 2.4 x 2.4 chamfer, 45 deg in both shells' print poses
    z_part: float = 0.0  # the parting plane, through the tube axis
    nose_hole_d: float = 47.0  # the socket is 46
    key_w: float = 13.0  # slot above the nose for the thumbscrew boss (10) and its knob (12)
    key_z1: float = 34.5  # top of the slot: the knob sits at 26-33
    lip_t: float = 1.2
    lip_h: float = 3.0
    boss_xs: tuple[float, float] = (-60.0, -45.0)  # the screw columns, beside the cone: one rib, two screws a side
    boss_w: float = 9.0  # block: 9 wide at the inner end, a 45 deg face on the -x side, merged into the wall
    boss_h: float = 10.0  # hood boss height above the lip; a 45 deg gusset above it
    screw_len: float = 20.0  # M2.5 x 20, head 8 mm up the column
    label: tuple[float, float, float] = (53.5, 50.0, 9.0)  # recess in the rear plate above the connectors: z, w, h
    grille_field: float = 58.0  # rounded square around the fan's axis
    grille_r: float = 13.0
    grille_pitch: float = 4.6  # the inner grille's slots: aligned
    grille_slot: float = 2.6


@dataclass(frozen=True)
class Params:
    tube: Tube = field(default_factory=Tube)
    fan: Fan = field(default_factory=Fan)
    servo: Servo = field(default_factory=Servo)
    pcb: Pcb = field(default_factory=Pcb)
    print: Print = field(default_factory=Print)
    damper: Damper = field(default_factory=Damper)
    coupling: Coupling = field(default_factory=Coupling)
    skin: Skin = field(default_factory=Skin)
    cone_len: float = 26.0
    screw_d: float = 2.5  # M2.5 self-tapping into plastic: pilot hole


P = Params()
