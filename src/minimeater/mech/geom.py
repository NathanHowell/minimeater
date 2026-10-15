"""Derived geometry shared by the parts, the mock components and the checks (mm, assembly frame)."""

from dataclasses import dataclass

from .params import P


@dataclass(frozen=True)
class G:
    bore_r: float = P.damper.bore_d / 2
    wall: float = P.print.wall
    duct_r: float = P.damper.bore_d / 2 + 2.2  # outer radius of the straight duct: a 2.2 mm wall over the bore
    socket_x0: float = -P.coupling.depth
    flap_x: float = P.damper.flap_x
    # the board lies under the straight duct: its rear edge sets where the cone may start
    board_x1: float = P.layout.board_x1
    board_x0: float = P.layout.board_x1 - P.pcb.x_len
    straight_x0: float = P.layout.board_x1 - P.pcb.x_len + P.layout.straight_over
    flange_t: float = 8.0
    fan_face_x: float = P.layout.board_x1 - P.pcb.x_len + P.layout.straight_over - P.cone_len  # fan outlet face
    flange_w: float = P.fan.size + 2.0
    flange_r: float = 8.0


g = G()

# the fan and the rear wall
FAN_X0 = g.fan_face_x - P.fan.depth
REAR_IN_X = FAN_X0 - P.layout.rear_gap  # the rear wall's inner face
REAR_X = REAR_IN_X - P.shell.wall  # the rear face

# the board, flat in the pan: standoffs on the pan floor, parts up toward the duct
PCB_Z0 = P.pcb.axis_z + P.pcb.jack_h / 2  # the board's parts face, down on the standoffs; the parts hang under it
PCB_Z1 = PCB_Z0 + P.pcb.t  # its back, up toward the duct
FLOOR_Z = PCB_Z0 - P.pcb.under_h - P.pcb.floor_gap  # inner floor: the belly
BELLY_Z = FLOOR_Z - P.shell.wall
PCB_Y0 = -P.pcb.y_len / 2  # the connector edge
PCB_EDGE_Y = PCB_Y0 + P.pcb.edge_trim  # the board's actual connector edge
PORT_WALL_Y = PCB_EDGE_Y - P.pcb.edge_gap  # inner face of the port wall: the connectors reach into it

# the linkage plane over the duct's crown, bottom up
L = P.linkage
CRANK_Z0 = g.duct_r + L.lift
ARM_Z0 = CRANK_Z0 + L.hub_h
ARM_Z1 = ARM_Z0 + L.arm_t
BOSS_Z0 = ARM_Z1  # the coupler's bosses lie on both arms
COUPLER_Z0 = BOSS_Z0 + L.bar_lift  # and the bar rides over the rod's end
COUPLER_Z1 = COUPLER_Z0 + L.coupler_t
# the servo beside the duct on +y, output face up, its horn's arm level with the crank's
SERVO_FACE_Z = ARM_Z1 - P.servo.horn_hub_h - P.servo.boss_h - 0.3
SERVO_Z0 = SERVO_FACE_Z - P.servo.body_h
SERVO_Y0 = g.duct_r + 1.0
SERVO_Y = SERVO_Y0 + P.servo.body_w / 2  # the output axis
SERVO_X = g.flap_x  # the output axis: straight across from the rod, so the coupler runs along y
SERVO_X_CENTRE = SERVO_X - (P.servo.body_l / 2 - P.servo.shaft_off)  # body toward the fan
TAB_Z1 = SERVO_FACE_Z - P.servo.tab_to_face  # top of the tabs
TAB_Z0 = TAB_Z1 - P.servo.tab_t
PAD_T = 2.5  # the duct's pads under the tabs
TAB_HOLES = tuple((SERVO_X + dx, SERVO_Y + dy) for dx, dy in P.servo.tab_holes)  # all six, in the assembly frame
TAB_HOLE_XS = (min(x for x, _ in TAB_HOLES), max(x for x, _ in TAB_HOLES))  # the group extremes

# the shell: section height and the roof
ROOF_IN_Z = P.shell.roof_in
ROOF_Z = ROOF_IN_Z + P.shell.wall
FULL_H = ROOF_Z - BELLY_Z
FULL_W = 2 * (P.shell.hw_in + P.shell.wall)
