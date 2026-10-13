# Fan: Noctua NF-A6x25 5V PWM (`.2.2`, bake-off `.2.11`)

60 x 60 x 25 mm, 50 mm hole pitch, 5 V, 4-pin PWM (25 kHz) with tach.

`src/minimeater/mech/data/NF-A6x25.stp` is Noctua's public CAD model (Noctua's download section, `NF-A6x25_Public-CAD.zip`; their README is beside it). It is used in the exported assembly only (`mech/mock.py`, `fan_step()`): the frame does not tessellate in our renderer and the interference checks use the simple box. Noctua says the model is exact for mounting and outer dimensions, that impellers and internal structure are deliberately altered, and that it must not be used for performance simulation or to manufacture or reproduce the product. Check their terms before publishing the repository.

From the model: 60 square, 25 deep (rubber corner pads add 0.5 at each face), rotor between y = 7.8 and 24.0 from the motor-plate face. The air goes in at the label side (rotor cap) and out at the motor-mount side; the assembly puts the motor-plate face at the duct's flange. The model replaced the earlier estimates in `params.py`: bore 58.0 (was 57), motor-plate hub 26.5 (was 24), corner radius 4.0 (was 5), screw holes 4.3 (confirmed), pitch 50. They were measured by probing the model, not read from a drawing; re-check against a real fan (`.2.11`).
