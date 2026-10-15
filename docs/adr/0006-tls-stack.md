# ADR 0006: TLS stack (embedded-tls)

- Status: accepted (2026-10-15)
- Beads: `minimeat-seq.5.5` (MQTT over TLS), `minimeat-seq.5.7` (device identity); supersedes the "esp-mbedtls" plan in ADR 0003 and 0005
- Constraint: the private key stays in the ATECC608B (ADR 0005); the firmware allocates nothing.

## Context

ADR 0005 put the device key in a secure element and named the risk: whether the TLS stack can sign the handshake through an
external signer. `esp-mbedtls` wraps a precompiled C library, needs `esp-wifi`'s allocator, and is documented for the ESP32,
C3 and S3 only; nothing found shows an opaque-key callback.

## Decision

**`embedded-tls` 0.19** (drogue-iot): a TLS 1.3 client in Rust, `no_std`, no allocator.

- It takes a `CryptoProvider` that supplies the random generator, the server verifier, the client certificate and a
  `signature::SignerMut`. The signer is where the chip goes (`tls::DeviceKey`): TLS hands over the bytes to sign, the firmware
  hashes them and the chip signs the digest.
- `rustpki` verifies the server's certificate against one pinned CA (signed directly by it, host name matched), which is what a
  private CA for five boards needs.
- `TlsConnection::split` gives a reader and a writer, so the MQTT session's `select` keeps its shape.
- Cipher suite `TLS_AES_128_GCM_SHA256`, key exchange P-256, ECDSA P-256 signatures: what Mosquitto 2 with OpenSSL offers by
  default for TLS 1.3. TLS 1.2-only servers are not supported.

## Consequences

- **Tested on the host** (`tls.rs`): the handshake against `openssl s_server -Verify 1` with the model chip signing; the wrong key
  and the wrong pinned CA are refused.
- **The signer is synchronous** and the chip's bus is async. The board blocks on the chip for one signature (about 50 to 100 ms)
  using a `try_lock` of the shared bus (`minimeat-b5w`).
- **CPU cost:** the key exchange and the certificate checks are software P-256 on the C6 and block the executor while they run.
  `minimeat-cqv` measures it on a board; if the stall is too long TLS moves to a lower-priority executor than the control loop.
- **Memory:** two 16,640-byte record buffers per connection and 2 KiB for the server certificate; one TLS connection at a time.
- **io versions:** `embedded-tls` uses `embedded-io-async` 0.7, the rest of the app 0.6; `tls::Duplex` and its error bridge
  join them. Move the app to 0.7 when the HAL crates do and drop the bridge.
- **Certificate dates** need a clock (`minimeat-n1q`); the verifier takes one as a type parameter.
