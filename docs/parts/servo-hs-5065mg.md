# Damper servo: Hitec HS-5065MG (`.2.9`)

Chosen over the MG90S class: digital, metal gears (one resin and four metal), ball-bearing output, 3.5-6.8 V.

Sources: Hitec's product page (hitecrcd.com/hs-5065mg-high-torque-metal-gear-feather-servo) and its dimensions drawing (`HS-5065MG-Servo-Specs`, linked from that page; the numbers are in `src/minimeat/mech/params.py`, class `Servo`).

| | |
|---|---|
| Voltage | 3.5-6.8 V; we run 5 V from VBUS |
| Speed, torque at 4.8 V | 0.14 s/60 degrees, 1.8 kgf-cm (25 oz-in) |
| Size | case 23.6 x 11.6 mm, 20.0 high; flange 32.3 long, 2.0 thick, top at 19.0; gear housing top 26.0; spline top 30.0 |
| Mounting | Ø2.0 holes in two groups of three, 6 mm across: the outer pair of each group at 19.2 mm one side of the output axis and 7.8 mm the other (27.0 apart, the drawing), the middle one of each 0.8 mm further out (20.0 and 8.6, 28.6 apart: the shelf's pilots are under these, from the STEP) |
| Output | 24T spline, ball bearing supported |
| Mass | 11.9 g; lead 160 mm with a 3-pin plug |

Not published by Hitec, so taken from retailers and the servo database and still to measure (`.3.9`): running current about 180-220 mA, idle 3 mA, stall about 1.2 A (the load switch limits it to 0.6 A); operating temperature, which the class puts at 60 C (ADR 0004; the shell models at 65-90 C, so the thermal check `.2.5` stays open); spline diameter (the model uses 4.8 mm).

## 3D model

`src/minimeat/mech/data/HS-5065MG.stp` is Hitec's own STEP model (from the manufacturer's CAD download for the HS-5065MG, www.hiteccs.com, file `HS-5065MG-1642816206.stp`), used as the servo in the assembly and the interference checks (`mech/mock.py`, `servo()`; `servo_box()` is the old box stand-in). It gave the spline outer diameter (5.0 mm, 24T) and the spline height (top 29.0 above the back face), and it agrees with the drawing on the 27.0 mm hole pitch. The frame in the file: X along the long axis with the output axis at x = 5.7, Y up with the back face at y = -14.0. Keep the vendor's terms in mind if the repository is ever published: the file is the manufacturer's, not ours.
