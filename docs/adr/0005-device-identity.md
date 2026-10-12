# ADR 0005: Device identity (hardware key pair)

- Status: accepted (2026-10-11)
- Beads: `minimeat-seq.3` (schematic), `minimeat-seq.4` (driver), `minimeat-seq.5.5` (TLS)
- Constraint: the PCB is assembled by JLCPCB, so every PCB part must be in JLC stock. Volume is about 5 boards.

## Context

The device should prove who it is to the MQTT broker (mutual TLS, `docs/protocol.md`) and to an OIDC provider (a signed client assertion, RFC 7523 `private_key_jwt`), without a shared password in flash. That needs a private key that cannot be copied off the board.

What the ESP32-C6 offers (ADR 0001):
- A Digital Signature peripheral for RSA, with the key stored encrypted and unlocked by an HMAC key in an eFuse block. The firmware can ask for signatures but never read the key.
- Flash encryption and secure boot, which protect a key kept in flash from someone with the board, but not from the running firmware.
- No confirmed hardware ECDSA on the C6 (the H2, H21, P4 and C61 have it). No esp-hal driver for the Digital Signature peripheral was found.

## Decision

**Microchip ATECC608B secure element on the shared I2C bus**, blank (unlocked) part: ATECC608B-SSHDA-T, SOIC-8 (JLC C1518769, about 1.6k in stock, $1.00). The UDFN-8 version is C1518773.

- **Key:** a P-256 key pair generated inside the chip (GenKey) in a private-key slot, never readable. Sign and ECDH use it; the chip returns only signatures and the public key.
- **Why it beats the C6 options:** ES256 and P-256 are the common choice for TLS client certificates and JWTs; the key is generated on the device with no import step; no eFuse is burned (irreversible, and 5 boards are for learning on); the firmware API is a small command set over a bus the board already has; each chip has a 9-byte unique serial number.
- **Bus:** I2C address 0x60 (the 8-bit 0xC0 default), with the ADS122C04 at 0x40 and the TMP117 at 0x48. No pin cost. The chip sleeps between commands at under 1 uA; a command draws about 15 mA for tens of milliseconds, which the 5 V budget (ADR 0004) absorbs.
- **Slots:** one private-key slot for the device key (sign allowed, no external read, no export); one data slot for the device certificate (up to 416 bytes). The configuration zone and slots are locked after provisioning; the lock is one-way, so the first board is provisioned twice as a trial on a spare chip.
- **Use in TLS and OIDC:**
  - mTLS to the broker: the TLS stack calls out to the chip to sign the handshake (an opaque private key). Needs to be shown to work with esp-mbedtls (`.5.5`); this is the main risk. The fallback is a key in encrypted flash.
  - OIDC: the firmware builds a JWT assertion and has the chip sign it with ES256.
- **Enrolment:** at provisioning the chip generates the key and the firmware returns a certificate signing request, signed by the chip. A private CA (for example `step-ca`, or `openssl` for the five boards) issues the certificate; Mosquitto takes it with `require_certificate true` and `use_identity_as_username true`, so the broker identifies each device by its certificate.

## What it does and does not protect

- It stops the key leaving the board: a copied flash image or a read-out of the memory has no private key in it.
- It does not stop firmware running on the board from asking for signatures. Pair it with secure boot and flash encryption (ESP32-C6) before shipping anything that matters. That is a separate, irreversible step and is not done on the first boards.

## Consequences

- **Parts:** one SOIC-8 on the I2C bus, with the shared pull-ups; nothing else. Power and ground decoupling only.
- **Firmware:** a driver for the chip's command protocol (wake, CRC-16, Info, Random, GenKey, Sign, Read, Lock), a provisioning flow, and the TLS signing hook. The wake condition needs a short low pulse on SDA, so the driver and the I2C controller must be able to produce it.
- **Docs:** the interface document lists the new part and address; the part list has the part number.
