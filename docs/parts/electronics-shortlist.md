# Electronics part shortlist

Checked against JLCPCB stock on 2026-10-11 with `scripts/jlc_stock.py`. About 5 boards, so low stock counts are fine. The rule is the higher-quality part unless the price gap is large; at this volume it rarely is. The schematic beads (`.3.2`-`.3.5`) turn this into symbols and footprints; datasheet checks (pinouts, limits) are still to do and are marked.

| Block | Part | JLC | Stock | Price | Why / to verify |
|---|---|---|---|---|---|
| MCU | Espressif ESP32-C6-WROOM-1-N8 | C5366877 | 3.8k | $4.22 | ADR 0001 |
| USB-C receptacle | GCT USB4105-GF-A-120 | C5184243 | 1.5k | $0.92 | brand-name over the $0.19 Hropart clone (C165948); confirm the variant suffix and the edge-mount footprint |
| CC pull-downs | 5.1 k 1 % 0402, Yageo RC0402FR-075K1L | C105872 | 2.4M | $0.0025 | two, one per CC pin |
| VBUS TVS | Littelfuse SMF5.0A | C151296 | 29.6k | $0.12 | check the clamp voltage against the 5 V loads and the buck's input rating |
| Input eFuse | TI TPS259531DSGR | C2155674 | 2.4k | $0.87 | between VBUS_RAW and VBUS_F, replaces the PTC: dVdt soft start (C42 10 nF), 2.04 A limit (R35 1k), 5.7 V clamp, UVLO 3.98 V (R33/R34) |
| D+/D- and CC ESD | ST USBLC6-2SC6, two | C7519 | 47k | $0.18 | one on the data pair, one on CC1/CC2; clones are cheaper but this is the original |
| 3.3 V buck | TI TLV62569DBVR | C141836 | 470k | $0.076 | 2 A, 1.5 MHz, SOT-23-5; check input range for the 5.5 V VBUS spike |
| Buck inductor | Murata LQH3NPN2R2MMEL (2.2 uH, 3 x 3 mm) | C668451 | 7k | $0.17 | over the Sunlord C43389 ($0.047); check saturation current against the 0.4 A peak |
| Analog LDO | TI TLV75533PDBVR | C404027 | 133k | $0.14 | the ADS122C04's supply, after a ferrite: low noise |
| Servo and fan switches | TI TPS2553DBVR, two | C55266 | 56k | $0.54 | adjustable current limit (servo about 0.6 A, fan about 0.5 A to ride the start-up), enable pin, fault flag |
| Probe ADC | TI ADS122C04IPWR | C2872338 | 4.5k | $7.92 | ADR 0002 (amended): the I2C variant of the ADS1220, saves five pins; address 0x40 (A1 = A0 = GND); I2C pull-ups needed |
| Jack ESD | TI TPD4E05U06DQAR | C138714 | 105k | $0.084 | ADR 0002; check max leakage at 60 C |
| Secure element | Microchip ATECC608B-SSHDA-T | C1518769 | 1.6k | $1.00 | ADR 0005: device key pair on I2C (0x60); the blank part, not the TNGTLS pre-provisioned one; UDFN alternative C1518773 |
| Probe jacks | SHOU HAN PJ-242, four | C431536 | 20k | $0.21 | ADR 0002; 2.5 mm TRRS SMD, body 5 x 3.2 x 12 mm, -30..70 C; pinout and tip switch still unverified |
| Ambient sensor | TI TMP117AIDRVR | C699536 | 29k | $1.02 | over the $0.5-0.7 Mysentech clones |
| Button | ALPS SKRPACE010 | C139797 | 126k | $0.063 | top-push tact, 4.2 x 3.2 x 2.5 mm, inside the case and pressed with the case open (docs/mech.md); replaces the XKB TS-1187A (C318884) |
| LED | Lite-On LTST-S220KGKT, side-view green | C160469 | 39.8k | $0.047 | on the board's +y edge facing the flank, into a clear light pipe (see docs/mech.md); replaces the top-view LTST-C191KGKT (C125098) |

## Not settled

- **Fan and servo headers: hand-soldered through-hole, not assembled by JLC.** The Noctua lead ends in a standard 4-pin fan plug, which mates a Molex KK 254 right-angle header, 2.54 mm, 4 positions, 22-05-3041 (the plug's polarising ramp locks onto it; right angle, because the vertical 22-23-2041 is 11.8 mm tall and the plug would not fit under the duct). The servo plug is the same family in 3 positions, 22-05-3031. JLC's copy of the Molex part is out of stock (C428629), so these go to Mouser or Digi-Key and are left unpopulated in the JLC assembly (DNP, but with the holes on the board). Check stock at order time: I could confirm only that distributors list the part, not their current counts. The footprints are `.3.5`.
- **Servo and fan fault pins.** The two TPS2553 fault flags are wired together into one GPIO. With the I2C ADC there are free pins for this (ADR 0001, amended).
- **Board size.** The PJ-242 is 12 mm deep and 5 mm wide, much smaller across than the 7 x 6 mm the case drawings assumed, so the jack row is set by the plug overmold (about 8.7 mm pitch), not by the jack body.
- **Servo temperature.** The HS-5065MG is rated 60 C. See ADR 0004.
