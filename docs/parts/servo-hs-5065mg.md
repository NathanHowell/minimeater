# Damper servo: Hitec HS-5065MG (`.2.9`)

Chosen over the MG90S class: digital, metal gears (one resin and four metal), ball-bearing output, 3.5-6.8 V.

Sources: Hitec's product page (hitecrcd.com/hs-5065mg-high-torque-metal-gear-feather-servo) and its dimensions drawing (`HS-5065MG-Servo-Specs`, linked from that page; the numbers are in `src/minimeat/mech/params.py`, class `Servo`).

| | |
|---|---|
| Voltage | 3.5-6.8 V; we run 5 V from VBUS |
| Speed, torque at 4.8 V | 0.14 s/60 degrees, 1.8 kgf-cm (25 oz-in) |
| Size | case 23.6 x 11.6 mm, 20.0 high; flange 32.3 long, 2.0 thick, top at 19.0; gear housing top 26.0; spline top 30.0 |
| Mounting | Ø2.0 holes, 19.2 mm one side of the output axis (three, 6 mm across) and 7.8 mm the other (two): 27.0 mm apart |
| Output | 24T spline, ball bearing supported |
| Mass | 11.9 g; lead 160 mm with a 3-pin plug |

Not published by Hitec, so taken from retailers and the servo database and still to measure (`.3.9`): running current about 180-220 mA, idle 3 mA, stall about 1.2 A (the load switch limits it to 0.6 A); operating temperature, which the class puts at 60 C (ADR 0004; the shell models at 65-90 C, so the thermal check `.2.5` stays open); spline diameter (the model uses 4.8 mm).
