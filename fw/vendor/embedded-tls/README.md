> **Patched copy of embedded-tls 0.19.0** (Apache-2.0, drogue-iot/embedded-tls). The one change: `SignatureAlgorithms::parse` and `SupportedGroups::parse` skip a
> signature scheme or group it has no name for instead of failing the handshake with `DecodeError` (RFC 8446 section 4.2.3: unknown code points
> are ignored). OpenSSL 4's `CertificateRequest` lists the scheme 0x0708 and its `EncryptedExtensions` the groups 0x11ee and 0x0029, so the chip's client could not talk to a server that asks for a client
> certificate. Wired in by `[patch.crates-io]` in `fw/Cargo.toml`; remove both when a release has the fix (minimeat-3hd).

# Embedded-TLS

[![CI](https://github.com/drogue-iot/embedded-tls/actions/workflows/ci.yaml/badge.svg)](https://github.com/drogue-iot/embedded-tls/actions/workflows/ci.yaml)
[![crates.io](https://img.shields.io/crates/v/embedded-tls.svg)](https://crates.io/crates/embedded-tls)
[![docs.rs](https://docs.rs/embedded-tls/badge.svg)](https://docs.rs/embedded-tls)
[![Matrix](https://img.shields.io/matrix/drogue-iot:matrix.org)](https://matrix.to/#/#drogue-iot:matrix.org)

Embedded-TLS is a Rust-native TLS 1.3 implementation that works in a no-std environment. The Rust crate was formerly known as `drogue-tls`. The
implementation is work in progress, but the [example clients](https://github.com/drogue-iot/embedded-tls/tree/main/examples) should work against the [rustls](https://github.com/ctz/rustls) echo server.

The client supports both async and blocking modes. By default, the `std` feature is enabled, but can be disabled for bare metal usage.

To use the async mode, import `embedded_tls::*`. To use the blocking mode, import `embedded_tls::blocking::*`.

Some features and extensions are not yet implemented, have a look at [open issues](https://github.com/drogue-iot/embedded-tls/issues).

Only supports writing/receiving one frame at a time, hence using a frame buffer larger than 16k is not currently needed.  You may use a lower frame buffer size, but there is no guarantee that it will be able to parse any TLS 1.3 frame.

## Community

* [Drogue IoT Matrix Chat Room](https://matrix.to/#/#drogue-iot:matrix.org)
* We have bi-weekly calls at 9:00 AM (GMT). [Check the calendar](https://calendar.google.com/calendar/u/0/embed?src=ofuctjec399jr6kara7n0uidqg@group.calendar.google.com&pli=1) to see which week we are having the next call, and feel free to join!
* [Drogue IoT Forum](https://discourse.drogue.io/)
* [Drogue IoT YouTube channel](https://www.youtube.com/channel/UC7GZUy2hKidvY6V_3QZfCcA)
* [Follow us on Twitter!](https://twitter.com/DrogueIoT)
