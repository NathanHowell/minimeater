//! Improv Wi-Fi over serial (<https://www.improv-wifi.com/serial/>), the way a blank device gets its network
//! (`.4.10`): a browser (esp-web-tools, Home Assistant) talks to the USB-C serial port and sends the SSID and
//! password. [`Parser`] and the `*_packet` builders are the wire format and need no hardware; [`run`] is the
//! conversation, generic over the serial port and the [`Station`] that joins the network.
//!
//! Frame, as ESPHome's implementation writes it (the spec page leaves the checksum and the line break out):
//! `IMPROV`, version 1, type, length, data, checksum, `\n`; the checksum is the sum of every byte before it, mod 256.
//! A device that is not authorised-gated is always `Ready`: whoever holds the cable may provision it. The RPC
//! data carries no checksum of its own on serial. Hostname and device name commands answer `UnknownRpc`.

use embedded_io_async::{Read, Write};
use heapless::{String, Vec};

use crate::config::Wifi;
use crate::net::Identity;
use crate::proto::Request;
use crate::repl::{self, Editor};
use crate::tasks::Bus;

const HEADER: &[u8; 6] = b"IMPROV";
const VERSION: u8 = 1;
/// Longest RPC data: command, length, ssid length and 32, password length and 64.
const MAX_DATA: usize = 2 + 1 + 32 + 1 + 64;
const MAX_FRAME: usize = 9 + MAX_DATA + 2;
/// The id the application's requests from here carry on the bus.
const REQUEST_ID: u16 = 0xFF10;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Ready = 0x02,
    Provisioning = 0x03,
    Provisioned = 0x04,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorCode {
    None = 0x00,
    InvalidRpc = 0x01,
    UnknownRpc = 0x02,
    UnableToConnect = 0x03,
    Unknown = 0xFF,
}

/// A command from the client.
#[derive(Clone, Debug, PartialEq)]
pub enum Rpc {
    Wifi {
        ssid: String<32>,
        pass: String<64>,
    },
    State,
    Info,
    Scan,
    NetworkState,
    /// A command this device does not have (hostname, device name, any future one).
    Unknown,
    /// A frame that checks out but whose data does not parse.
    Invalid,
}

const PACKET_STATE: u8 = 0x01;
const PACKET_ERROR: u8 = 0x02;
const PACKET_RPC: u8 = 0x03;
const PACKET_RESULT: u8 = 0x04;
const CMD_WIFI: u8 = 0x01;
const CMD_STATE: u8 = 0x02;
const CMD_INFO: u8 = 0x03;
const CMD_SCAN: u8 = 0x04;
const CMD_NETWORK_STATE: u8 = 0x07;

fn sum(bytes: &[u8]) -> u8 {
    bytes.iter().fold(0u8, |a, b| a.wrapping_add(*b))
}

/// Finds frames in a byte stream: anything that is not a frame (log lines, a stray newline) is skipped, a bad
/// checksum drops the frame.
#[derive(Default)]
pub struct Parser {
    buf: Vec<u8, MAX_FRAME>,
}

impl Parser {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feeds one byte; `Some` when it completes a command from the client.
    pub fn push(&mut self, b: u8) -> Option<Rpc> {
        let n = self.buf.len();
        if n < HEADER.len() {
            if b == HEADER[n] {
                let _ = self.buf.push(b);
            } else {
                self.buf.clear();
                if b == HEADER[0] {
                    let _ = self.buf.push(b);
                }
            }
            return None;
        }
        let _ = self.buf.push(b);
        let n = self.buf.len();
        if n == 9 {
            // header, version, type, length: reject what cannot be ours before waiting for the rest
            let (version, len) = (self.buf[6], usize::from(self.buf[8]));
            if version != VERSION || len > MAX_DATA {
                self.buf.clear();
            }
            return None;
        }
        if n < 9 || n < 9 + usize::from(self.buf[8]) + 1 {
            return None;
        }
        let frame = core::mem::take(&mut self.buf);
        let (body, check) = frame.split_at(frame.len() - 1);
        if sum(body) != check[0] || body[7] != PACKET_RPC {
            return None;
        }
        Some(decode(&body[9..]))
    }
}

fn decode(data: &[u8]) -> Rpc {
    let [cmd, len, rest @ ..] = data else {
        return Rpc::Invalid;
    };
    if usize::from(*len) != rest.len() {
        return Rpc::Invalid;
    }
    match *cmd {
        CMD_WIFI => wifi(rest).unwrap_or(Rpc::Invalid),
        CMD_STATE | CMD_INFO | CMD_SCAN | CMD_NETWORK_STATE if !rest.is_empty() => Rpc::Invalid,
        CMD_STATE => Rpc::State,
        CMD_INFO => Rpc::Info,
        CMD_SCAN => Rpc::Scan,
        CMD_NETWORK_STATE => Rpc::NetworkState,
        _ => Rpc::Unknown,
    }
}

fn wifi(d: &[u8]) -> Option<Rpc> {
    let (&n, d) = d.split_first()?;
    let (ssid, d) = d.split_at_checked(usize::from(n))?;
    let (&m, d) = d.split_first()?;
    let (pass, d) = d.split_at_checked(usize::from(m))?;
    if !d.is_empty() || ssid.is_empty() {
        return None;
    }
    Some(Rpc::Wifi {
        ssid: String::try_from(core::str::from_utf8(ssid).ok()?).ok()?,
        pass: String::try_from(core::str::from_utf8(pass).ok()?).ok()?,
    })
}

pub type Packet = Vec<u8, 192>;

fn packet(ty: u8, data: &[u8]) -> Option<Packet> {
    let mut p = Packet::new();
    p.extend_from_slice(HEADER).ok()?;
    p.extend_from_slice(&[VERSION, ty, u8::try_from(data.len()).ok()?])
        .ok()?;
    p.extend_from_slice(data).ok()?;
    let c = sum(&p);
    p.extend_from_slice(&[c, b'\n']).ok()?;
    Some(p)
}

pub fn state_packet(s: State) -> Packet {
    packet(PACKET_STATE, &[s as u8]).expect("fits")
}

pub fn error_packet(e: ErrorCode) -> Packet {
    packet(PACKET_ERROR, &[e as u8]).expect("fits")
}

/// An RPC result: the command answered and a list of strings (none is allowed). `None` if it would not fit.
pub fn result_packet(cmd: u8, strings: &[&str]) -> Option<Packet> {
    let mut d: Vec<u8, 160> = Vec::new();
    d.extend_from_slice(&[cmd, 0]).ok()?;
    for s in strings {
        d.push(u8::try_from(s.len()).ok()?).ok()?;
        d.extend_from_slice(s.as_bytes()).ok()?;
    }
    d[1] = u8::try_from(d.len() - 2).ok()?;
    packet(PACKET_RESULT, &d)
}

/// A network the radio saw.
#[derive(Clone, Debug)]
pub struct Network {
    pub ssid: String<32>,
    pub rssi: i8,
    pub secured: bool,
}

/// The radio, for provisioning.
#[allow(async_fn_in_trait, reason = "single-threaded embassy executor; no Send bound wanted")]
pub trait Station {
    /// Joins the network and returns once it has an address, or `false` after the board's own timeout.
    async fn connect(&mut self, ssid: &str, pass: &str) -> bool;
    /// The networks in range, strongest first (as many as fit).
    async fn scan(&mut self) -> Vec<Network, 12>;
    fn online(&self) -> bool;
}

/// The answer to "request device information": firmware name, version, chip, device name.
fn info(who: &Identity<'_>) -> Option<Packet> {
    result_packet(CMD_INFO, &["minimeat", who.sw, "ESP32-C6", who.id])
}

async fn send<I: Write>(io: &mut I, p: Option<Packet>) {
    if let Some(p) = p {
        let _ = io.write_all(&p).await;
        let _ = io.flush().await;
    }
}

/// The conversation with a client on `io`, for ever; the serial REPL (`repl`) lives on the same bytes. The device is `Ready` (or `Provisioned` if it has
/// credentials already); credentials that join the network are handed to the control task as a request, which
/// saves them and publishes them for the network task.
pub async fn run<I: Read + Write, S: Station>(io: &mut I, station: &mut S, who: &Identity<'_>, bus: &Bus) -> ! {
    let mut replies = bus.replies.subscriber().expect("a reader slot");
    let mut parser = Parser::new();
    let mut state = if bus.config().is_some_and(|c| c.wifi.is_some()) {
        State::Provisioned
    } else {
        State::Ready
    };
    let mut editor = Editor::new();
    let mut buf = [0u8; 32];
    send(io, Some(state_packet(state))).await;
    loop {
        let n = io.read(&mut buf).await.unwrap_or(0);
        for &b in &buf[..n] {
            // the same bytes feed the REPL; its editor leaves Improv frames alone
            let step = editor.push(b);
            if !step.echo.is_empty() {
                let _ = io.write_all(&step.echo).await;
                let _ = io.flush().await;
            }
            if let Some(line) = step.line {
                repl::execute(io, &line, who, bus, &mut replies).await;
                repl::prompt(io).await;
            }
            let Some(rpc) = parser.push(b) else { continue };
            send(io, Some(error_packet(ErrorCode::None))).await;
            match rpc {
                Rpc::Wifi { ssid, pass } => {
                    state = State::Provisioning;
                    send(io, Some(state_packet(state))).await;
                    let joined = station.connect(&ssid, &pass).await;
                    if joined && bus.send(REQUEST_ID, Request::Wifi(Some(Wifi { ssid, pass }))) {
                        loop {
                            let r = replies.next_message_pure().await;
                            if r.id == REQUEST_ID {
                                break;
                            }
                        }
                        state = State::Provisioned;
                        send(io, Some(state_packet(state))).await;
                        send(io, result_packet(CMD_WIFI, &[])).await;
                    } else {
                        state = State::Ready;
                        send(io, Some(error_packet(ErrorCode::UnableToConnect))).await;
                        send(io, Some(state_packet(state))).await;
                    }
                }
                Rpc::State => {
                    send(io, Some(state_packet(state))).await;
                    if state == State::Provisioned {
                        send(io, result_packet(CMD_WIFI, &[])).await;
                    }
                }
                Rpc::Info => send(io, info(who)).await,
                Rpc::Scan => {
                    for net in station.scan().await {
                        let mut rssi: String<4> = String::new();
                        let _ = core::fmt::write(&mut rssi, format_args!("{}", net.rssi));
                        send(
                            io,
                            result_packet(CMD_SCAN, &[&net.ssid, &rssi, if net.secured { "YES" } else { "NO" }]),
                        )
                        .await;
                    }
                    send(io, result_packet(CMD_SCAN, &[])).await;
                }
                Rpc::NetworkState => {
                    // bit 0 online, bit 1 supports Wi-Fi
                    send(
                        io,
                        result_packet(CMD_NETWORK_STATE, &[if station.online() { "3" } else { "2" }]),
                    )
                    .await;
                }
                Rpc::Unknown => send(io, Some(error_packet(ErrorCode::UnknownRpc))).await,
                Rpc::Invalid => send(io, Some(error_packet(ErrorCode::InvalidRpc))).await,
            }
        }
        if n == 0 {
            // a closed or failing port: do not spin
            core::future::pending::<()>().await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rpc(cmd: u8, data: &[u8]) -> Packet {
        let mut d: Vec<u8, 128> = Vec::new();
        d.extend_from_slice(&[cmd, data.len() as u8]).unwrap();
        d.extend_from_slice(data).unwrap();
        packet(PACKET_RPC, &d).unwrap()
    }

    fn feed(p: &mut Parser, bytes: &[u8]) -> Vec<Rpc, 8> {
        bytes.iter().filter_map(|&b| p.push(b)).collect()
    }

    #[test]
    fn state_packet_matches_the_wire_format() {
        // IMPROV 01 01 01 02, checksum (477 + 1 + 1 + 1 + 2) mod 256, newline
        assert_eq!(&state_packet(State::Ready)[..], b"IMPROV\x01\x01\x01\x02\xE2\n");
        assert_eq!(
            &error_packet(ErrorCode::UnableToConnect)[..],
            b"IMPROV\x01\x02\x01\x03\xE4\n"
        );
    }

    #[test]
    fn wifi_settings_decode_as_in_the_spec_example() {
        let mut data: Vec<u8, 64> = Vec::new();
        data.push(12).unwrap();
        data.extend_from_slice(b"MyWirelessAP").unwrap();
        data.push(16).unwrap();
        data.extend_from_slice(b"mysecurepassword").unwrap();
        let got = feed(&mut Parser::new(), &rpc(CMD_WIFI, &data));
        assert_eq!(got.len(), 1);
        assert_eq!(
            got[0],
            Rpc::Wifi {
                ssid: "MyWirelessAP".try_into().unwrap(),
                pass: "mysecurepassword".try_into().unwrap()
            }
        );
    }

    #[test]
    fn a_bad_checksum_or_garbage_between_frames_is_skipped() {
        let mut p = Parser::new();
        let mut bad = rpc(CMD_STATE, &[]);
        let n = bad.len();
        bad[n - 2] ^= 0xFF;
        let mut stream: Vec<u8, 160> = Vec::new();
        stream.extend_from_slice(b"I (123) log line IMPR\r\n").unwrap();
        stream.extend_from_slice(&bad).unwrap();
        stream.extend_from_slice(&rpc(CMD_INFO, &[])).unwrap();
        assert_eq!(&feed(&mut p, &stream)[..], &[Rpc::Info]);
    }

    #[test]
    fn frames_split_across_reads_and_commands_that_do_not_parse() {
        let mut p = Parser::new();
        let f = rpc(CMD_STATE, &[]);
        assert!(feed(&mut p, &f[..5]).is_empty());
        assert_eq!(&feed(&mut p, &f[5..])[..], &[Rpc::State]);
        assert_eq!(&feed(&mut p, &rpc(0x05, &[3, b'a', b'b', b'c']))[..], &[Rpc::Unknown]);
        assert_eq!(&feed(&mut p, &rpc(CMD_STATE, &[1]))[..], &[Rpc::Invalid]);
        // an SSID length that runs past the data
        assert_eq!(&feed(&mut p, &rpc(CMD_WIFI, &[9, b'a', 0]))[..], &[Rpc::Invalid]);
        // a length byte over the largest frame is dropped early, and the next frame still parses
        let mut junk: Vec<u8, 32> = Vec::new();
        junk.extend_from_slice(b"IMPROV\x01\x03\xF0").unwrap();
        junk.extend_from_slice(&rpc(CMD_SCAN, &[])).unwrap();
        assert_eq!(&feed(&mut p, &junk)[..], &[Rpc::Scan]);
    }

    #[test]
    fn results_carry_length_prefixed_strings() {
        let p = result_packet(CMD_INFO, &["a", "bc"]).unwrap();
        assert_eq!(&p[..6], b"IMPROV");
        assert_eq!(
            &p[7..],
            &[PACKET_RESULT, 7, CMD_INFO, 5, 1, b'a', 2, b'b', b'c', p[16], b'\n'][..]
        );
        assert_eq!(p[16], sum(&p[..16]));
        assert_eq!(
            &result_packet(CMD_WIFI, &[]).unwrap()[7..11],
            &[PACKET_RESULT, 2, CMD_WIFI, 0]
        );
    }
}
