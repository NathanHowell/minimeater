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
/// in-memory pipe on the host. The halves may borrow the connector (the socket lives in it), so a
/// connection ends before the next `connect`.
#[allow(async_fn_in_trait, reason = "single-threaded embassy executor; no Send bound wanted")]
pub trait Connector {
    type Reader<'a>: Read
    where
        Self: 'a;
    type Writer<'a>: Write
    where
        Self: 'a;
    /// Opens a TCP connection and splits it. Any failure is `Err`.
    async fn connect<'a>(&'a mut self, host: &str, port: u16) -> Result<(Self::Reader<'a>, Self::Writer<'a>), ()>;
}

/// Waits between attempts to reach the network or a server: 1, 2, 4, 8, 16, 30, then 60 s, until it works again.
#[derive(Clone, Copy, Debug, Default)]
pub struct Backoff {
    failures: u32,
}

impl Backoff {
    /// The wait after one more failure, in seconds.
    pub fn next_s(&mut self) -> u64 {
        let s = match self.failures {
            0..=4 => 1u64 << self.failures,
            5 => 30,
            _ => 60,
        };
        self.failures = self.failures.saturating_add(1);
        s
    }

    pub fn reset(&mut self) {
        self.failures = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::Backoff;

    #[test]
    fn backoff_doubles_then_holds_at_a_minute_and_resets() {
        let mut b = Backoff::default();
        let waits: std::vec::Vec<u64> = (0..9).map(|_| b.next_s()).collect();
        assert_eq!(waits, [1, 2, 4, 8, 16, 30, 60, 60, 60]);
        b.reset();
        assert_eq!(b.next_s(), 1);
    }
}
