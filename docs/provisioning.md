# Provisioning the device key (ATECC608B)

ADR 0005 puts the device's private key in an ATECC608B. This is how a blank chip becomes that identity, from the serial REPL
(`docs/icd.md`). The flow is `minimeater-app::drivers::identity`; the commands are `identity`, `identity provision lock` and
`identity csr`.

**The locks are one way.** A chip locked with a wrong configuration cannot be fixed. Do everything below on a spare chip first
(a socketed or dead-bugged SOIC-8 on a scratch board, or a board you can spare), check the values against the datasheet, and only
then do the real boards.

## What gets set

Everything the chip came with is kept, except four fields:

| | slot 0, the key | slot 8, the certificate (416 bytes) |
|-|-|-|
| `SlotConfig` (bytes 20.. and 36..) | `0x2081`: external signing allowed, secret (never read), `GenKey` allowed | `0x0000`: clear read, clear write |
| `KeyConfig` (bytes 96.. and 112..) | `0x0033`: private key, public key derivable, P-256 | `0x001C`: plain data |

The order is the chip's: write the changed words, read the whole zone back and compare, lock the configuration with the CRC of that
image, make the key in slot 0 (`GenKey`), lock the data zone. The data lock is made without a CRC (`mode 0x81`) because the zone
holds a private key the host cannot read. The certificate slot stays writable after the lock, because the certificate arrives
after the key exists.

## Procedure

1. `identity`: reports the chip's serial, whether the configuration and data zones are locked, how many configuration words differ
   and the CRC the lock would carry. Changes nothing. Run it as often as you like.
2. Check the report against the datasheet (see "Check before the first lock" below). The CRC is printed so it can be compared with
   one computed independently from the image.
3. `identity provision lock`: does the rest, one step per line. If power fails part-way, run it again: a step already done is
   skipped, a configuration written but not locked is not rewritten, and a key that exists is never replaced.
   - The command refuses a chip locked with another configuration, a chip that is not an ATECC608, and a configuration that does
     not read back equal to what was written (nothing is locked in that case).
4. `identity csr` prints a PEM certificate request signed by the chip, subject `CN=<device id>` (`mm-` and the MAC). Sign it with
   the private CA, for example:

   ```sh
   openssl x509 -req -in mm-a1b2c3.csr -CA ca.pem -CAkey ca.key -CAcreateserial -days 3650 -out mm-a1b2c3.pem
   ```

   `openssl req -in mm-a1b2c3.csr -verify -noout` checks the request's signature. Writing the certificate into slot 8 comes with the
   TLS work (`minimeat-seq.5.7`).

## Check before the first lock

The configuration values were written from the datasheet (Microchip DS40002239, the configuration zone and `SlotConfig` /
`KeyConfig` tables) and cryptoauthlib's defaults, not from a chip. On the spare chip, before `provision lock`, check against the
datasheet:

- the bit meanings in `0x2081` and `0x0033` (ReadKey bit 0 = external signatures, IsSecret, WriteConfig bit 13 = `GenKey`, KeyType
  P-256, Private, PubInfo) and `0x001C` for a data slot;
- that `GenKey` is allowed in slot 0 after the data zone is locked (so a spare can be re-keyed) or not, as wanted;
- that a 32-byte clear write to slot 8 is accepted after the data lock;
- that the lock command modes (`0x00` with CRC, `0x81` data without) are the ones the datasheet gives;
- that the I2C wake pulse (a write to address 0 at 100 kHz) really wakes the chip on the ESP32-C6's I2C (`identity` getting an
  answer is the proof; if it never does, the pulse is too short).

The host tests run the whole flow against a model of the chip that follows these rules, and the request it prints is parsed by
RustCrypto's `x509-cert` and verified by OpenSSL; neither says the chip agrees.
