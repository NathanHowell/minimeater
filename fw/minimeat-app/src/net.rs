//! What the network clients (MQTT `.5.2`, OTLP `.4.11`) need from the board: a way to open a TCP
//! connection, and who the device is.

use embedded_io_async::{Read, Write};

/// Who this device is on the network.
#[derive(Clone, Copy, Debug)]
pub struct Identity<'a> {
    /// MQTT topic prefix, `minimeat` unless changed.
    pub prefix: &'a str,
    /// `mm-` and the low three MAC bytes.
    pub id: &'a str,
    pub sw: &'a str,
    pub hw: &'a str,
}

/// How a client reaches its server: embassy-net sockets on the ESP32-C6, plain TCP or an
/// in-memory pipe on the host.
#[allow(async_fn_in_trait, reason = "single-threaded embassy executor; no Send bound wanted")]
pub trait Connector {
    type Reader: Read;
    type Writer: Write;
    /// Opens a TCP connection and splits it. Any failure is `Err`.
    async fn connect(&mut self, host: &str, port: u16) -> Result<(Self::Reader, Self::Writer), ()>;
}
