# PCB layout blocks

Each IC is laid out with its own parts first, off the board, to its datasheet's layout guidance: placement, local
tracks and copper fills, ground vias to the In1 plane. Blocks are then placed on the board as units and joined by hand.
Edits go through Konnect (live KiCad over IPC). Renders: `hw/tools/pcbdump.py` (KiCad's python) then
`hw/tools/render.py`; F.Cu only, colour by net.

Staging: a temporary second board outline (x 0-130, y 64-124) and an In1 GND zone (`STAGING_In1_GND`) under it, so
the block fills fill and its vias land on a plane as they will on the board. Both are removed once every block is on
the board.

Netclasses (`hw/minimeater.kicad_pro`): Default 0.2/0.15; PowerHigh 0.8/0.2 (VBUS_RAW, VBUS_F, VFAN, VSERVO,
BUCK_SW); Power 0.5/0.2 (3V3, 3V3A, 3V3A_FILT, VBUS_LDO); GND 0.4/0.15; USB 0.27 with a 0.15 gap (90 ohm
edge-coupled on F.Cu over In1, 7628 prepreg ~0.21 mm); Analog 0.2/0.25 (TIPx, AINx, VREF_PROBE). Vias 0.6/0.3.

## U4 TLV62569 buck (3V3), origin (20, 75)

![U4](u4.png)

Datasheet SLVSDG1C section 10, figure 20 (DBV):

- Cin and the inductor as close to the IC as possible; power paths short and wide: VIN, SW and VOUT are fills, not
  traces. U4 is turned 180 degrees so the pins fall as in figure 20: VIN top left, SW top right, GND right middle,
  FB bottom left, EN bottom right.
- L1 straight above SW; the SW fill is only the overlap between pin 3 and L1 pad 1 (smallest switching area).
- C14 (100 nF) then C4 (10 uF) beside VIN on the VIN fill; their grounds drop to In1 (C4 via in pad, C14 via beside).
- C5, C6 (10 uF) under L1's output pad on the VOUT fill; ground pads with vias in the pad, a via beside pin 2, a
  ground fill from pin 2 to both, so the output loop is closed on top.
- EN is tied to VIN under the package body (0.25 mm, 0.35 mm from both pin rows): no via.
- FB is a signal: R3/C7/R4 sit below, away from SW; FB drops straight from pin 5. The 3V3 sense runs from C6's pad
  round the outside of the block (as figure 20's sense trace), so it reads the output capacitor and never passes SW.

## U5 TLV75533P analog LDO (3V3A), origin (40, 75)

![U5](u5.png)

Datasheet SBVS320D section 7.4, figure 7-7 (DBV), at the footprint's own orientation: IN top left, OUT top right,
GND left middle, EN bottom left, NC bottom right.

- Cin (C8, 1 uF) and Cout (C9 1 uF, then C15 100 nF) right at their pins, as close as the courtyards allow.
- C8's ground pad sits beside pin 2, joined on top: the input loop closes on F.Cu without going through a via.
- The ground fill wraps under the package from C8 to C9/C15 (figure 7-7's ground plane), with a via at each end.
- FB1 (from VBUS_F) feeds the VBUS_LDO fill at the far left, so VBUS_F arrives from outside the block.
- EN is tied to IN under the package body, as on U4. NC (pin 4) is left unconnected.
- FB2, R18 and C27/C28 (the VREF_PROBE filter) belong to the ADC end of 3V3A and go in the U8 block.

## U7, U6 TPS2553 load switches (fan, servo), origins (60, 75) and (75, 75)

![U6, U7](u6_u7.png)

Datasheet SLVS841F section 12, figure 30, at the footprint's own orientation: IN top left, GND left middle, EN bottom
left, OUT top right, ILIM right middle, FAULT bottom right. The two blocks are identical (U7 fan: C17, R9, R6, C13,
C12; U6 servo: C16, R8, R5, C11, C10).

- The 100 nF bypass on IN (C17/C16) sits at pin 1 with its ground pad beside GND pin 2, joined on top: the
  "low-inductance" IN-GND connection, plus a via.
- R_ILIM (R6/R5) stands right beside ILIM, under 1 mm of trace ("as short as possible" for current-limit accuracy),
  grounded with a via and through the output-cap ground fill.
- Output: 100 nF (C13/C11) then 10 uF (C12/C10, via in the ground pad) on the OUT fill, which runs over R_ILIM. The
  bulk at the connectors (C35, C36/C37) is placed with J6/J7.
- EN pull-down (R9/R8) beside EN; the MCU's EN comes in from below.
- FAULT leaves under the body and out the top between the IN and OUT fills, as in figure 30. R7 (the shared
  LOAD_FAULT pull-up) is placed when the two blocks go on the board.
