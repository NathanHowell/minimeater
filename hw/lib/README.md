# Vendored libraries

`espressif.kicad_sym` and `espressif.pretty/` hold the ESP32-C6-WROOM-1 symbol and footprint from
<https://github.com/espressif/kicad-libraries> (CC-BY-SA 4.0 with the KiCad design exception: designs that use the
libraries are not Adapted Material). The footprint's 3D model reference points at `3d/ESP32-C6-WROOM-1.step` (offset -9, -9.75, 0: the model's corner is its origin, the antenna end at y 25.5); the pad table, outline and
antenna keepout are as published and match the module datasheet's land pattern (Figure 11-1). No other changes.

`minimeat.pretty/` and `3d/` hold footprints and STEP models for the parts KiCad's own libraries lack. The land patterns
and models come from the LCSC/EasyEDA entries for the C-numbers (JLC's own, fetched with `easyeda2kicad`) and were
checked against each part's datasheet drawing; the 3D models are the EasyEDA ones (for internal use; the LED's is a
simple block drawn from the Lite-On dimensions). Changes from the source: the TMP117 has no exposed-pad land (TI says
leave the thermal pad unsoldered); the PJ-242's two positioning holes are non-plated and its six pads are 1.8 x 1.5 as in the
drawing's PCB layout (they were 1.8 x 1.8); the SKRPACE010's four pads are numbered as its two terminals (1 and 2 joined inside, 3 and 4 joined).
`3d/ESP32-C6-WROOM-1.step` is Espressif's model for the vendored module footprint. The PJ-242 model's origin is the plug bore's axis, 1.6 mm above the housing's underside and 2.2 mm above the pegs' tips, so its footprint lifts it 1.6 mm (offset z) to seat the housing on the board with the pegs in the two 1.3 mm holes; the other models start at z = 0.
