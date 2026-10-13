//! Improv Wi-Fi over serial (<https://www.improv-wifi.com/serial/>), the way a blank device gets its network
//! (`.4.10`): a browser (esp-web-tools, Home Assistant) talks to the USB-C serial port and sends the SSID and
//! password. [`frame`] (a `winnow` grammar, run by [`crate::serial`]) and the `*_packet` builders are the wire format and need no hardware; [`run`] is the
//! conversation, generic over the serial port and the [`Station`] that joins the network.
//!
//! Frame, as ESPHome's implementation writes it (the spec page leaves the checksum and the line break out):
//! `IMPROV`, version 1, type, length, data, checksum, `\n`; the checksum is the sum of every byte before it, mod 256.
//! A device that is not authorised-gated is always `Ready`: whoever holds the cable may provision it. The RPC
//! data carries no checksum of its own on serial. Hostname and device name commands answer `UnknownRpc`.

use embassy_time::{Duration, with_timeout};
use embedded_io_async::{Read, Write};
use heapless::{String, Vec};
use winnow::binary::length_take;
use winnow::stream::Partial;
use winnow::token::{literal, take};
use winnow::{ModalResult, Parser as _};

use crate::config::Wifi;
use crate::net::Identity;
use crate::proto::Request;
use crate::repl::{self, Editor, Key, Replies};
use crate::serial::{Demux, Item};
use crate::tasks::Bus;

const HEADER: &[u8; 6] = b"IMPROV";
/// How long half an item waits for the rest before the port is taken to be quiet.
const HOLD: Duration = Duration::from_millis(200);
pub(crate) const VERSION: u8 = 1;
/// Longest RPC data: command, length, ssid length and 32, password length and 64.
pub(crate) const MAX_DATA: usize = 2 + 1 + 32 + 1 + 64;
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

pub(crate) type Input<'a> = Partial<&'a [u8]>;

/// One byte of a partial stream (`winnow::binary::u8` with its types fixed; named `byte` so it does not shadow the
/// type `u8`).
pub(crate) fn byte(input: &mut Input<'_>) -> ModalResult<u8> {
    winnow::binary::u8.parse_next(input)
}

/// One byte of a complete slice (the data of a frame).
fn data_byte(input: &mut &[u8]) -> ModalResult<u8> {
    winnow::binary::u8.parse_next(input)
}

/// The marker, version, type, length and data of a frame: `None` when it cannot be ours.
fn header<'a>(i: &mut Input<'a>) -> ModalResult<Option<(u8, &'a [u8])>> {
    literal(&HEADER[..]).parse_next(i)?;
    if byte(i)? != VERSION {
        return Ok(None);
    }
    let ty = byte(i)?;
    let len = byte(i)?;
    if usize::from(len) > MAX_DATA {
        return Ok(None);
    }
    Ok(Some((ty, take(usize::from(len)).parse_next(i)?)))
}

/// A frame in a partial byte stream, as the serial demultiplexer ([`crate::serial`]) reads it: the marker, the
/// version, the type, the length, that many data bytes and the checksum over everything before it.
///
/// - Not the marker (a different byte, or the marker's own prefix still arriving): `Backtrack` (`Incomplete` while
///   it could still be one), so the caller reads the bytes as keystrokes.
/// - The marker, then something that cannot be ours (another version, a length over [`MAX_DATA`]): `Ok(None)`
///   having taken what it read; a frame whose checksum is wrong or that is not an RPC is `Ok(None)` having taken
///   all of it.
/// - A good frame: `Ok(Some(rpc))`.
pub(crate) fn frame(input: &mut Input<'_>) -> ModalResult<Option<Rpc>> {
    let (head, body) = header.with_taken().parse_next(input)?;
    let Some((ty, data)) = head else { return Ok(None) };
    let check = byte(input)?;
    if sum(body) != check || ty != PACKET_RPC {
        return Ok(None);
    }
    Ok(Some(rpc(data)))
}

/// The data of an RPC frame: the command, the length of its own data and that data, which has to be all of it.
fn rpc(data: &[u8]) -> Rpc {
    let Ok((cmd, body)) = (data_byte, length_take(data_byte)).parse(data) else {
        return Rpc::Invalid;
    };
    match cmd {
        CMD_WIFI => wifi(body),
        CMD_STATE | CMD_INFO | CMD_SCAN | CMD_NETWORK_STATE if !body.is_empty() => Rpc::Invalid,
        CMD_STATE => Rpc::State,
        CMD_INFO => Rpc::Info,
        CMD_SCAN => Rpc::Scan,
        CMD_NETWORK_STATE => Rpc::NetworkState,
        _ => Rpc::Unknown,
    }
}

/// The SSID and the password, each a length and its bytes.
fn wifi(body: &[u8]) -> Rpc {
    let Ok((ssid, pass)) = (length_take(data_byte), length_take(data_byte)).parse(body) else {
        return Rpc::Invalid;
    };
    let text = || {
        Some(Rpc::Wifi {
            ssid: String::try_from(core::str::from_utf8(ssid).ok()?).ok()?,
            pass: String::try_from(core::str::from_utf8(pass).ok()?).ok()?,
        })
    };
    text().filter(|_| !ssid.is_empty()).unwrap_or(Rpc::Invalid)
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

/// One keystroke: the editor's echo goes back, and a finished line is run and answered with the prompt.
async fn key<I: Write>(
    io: &mut I,
    editor: &mut Editor,
    k: Key,
    who: &Identity<'_>,
    bus: &Bus,
    replies: &mut Replies<'_>,
) {
    let step = editor.apply(k);
    if !step.echo.is_empty() {
        let _ = io.write_all(&step.echo).await;
        let _ = io.flush().await;
    }
    if let Some(line) = step.line {
        repl::execute(io, &line, who, bus, replies).await;
        repl::prompt(io).await;
    }
}

/// The conversation with a client on `io`, for ever; the serial REPL (`repl`) lives on the same bytes. The device is `Ready` (or `Provisioned` if it has
/// credentials already); credentials that join the network are handed to the control task as a request, which
/// saves them and publishes them for the network task.
pub async fn run<I: Read + Write, S: Station>(io: &mut I, station: &mut S, who: &Identity<'_>, bus: &Bus) -> ! {
    let mut replies = bus.replies.subscriber().expect("a reader slot");
    let mut demux = Demux::new();
    let mut state = if bus.config().is_some_and(|c| c.wifi.is_some()) {
        State::Provisioned
    } else {
        State::Ready
    };
    let mut editor = Editor::new();
    let mut buf = [0u8; 32];
    send(io, Some(state_packet(state))).await;
    loop {
        let n = if demux.pending() {
            // half an item is waiting: if the host goes quiet it is let go (what looked like a frame's start was typing)
            match with_timeout(HOLD, io.read(&mut buf)).await {
                Ok(r) => r.unwrap_or(0),
                Err(_) => {
                    for k in demux.flush() {
                        key(io, &mut editor, k, who, bus, &mut replies).await;
                    }
                    continue;
                }
            }
        } else {
            io.read(&mut buf).await.unwrap_or(0)
        };
        for &b in &buf[..n] {
            demux.push(b);
            while let Some(item) = demux.pop() {
                let rpc = match item {
                    Item::Key(k) => {
                        key(io, &mut editor, k, who, bus, &mut replies).await;
                        continue;
                    }
                    Item::Frame(rpc) => rpc,
                };
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

    fn feed(p: &mut Demux, bytes: &[u8]) -> Vec<Rpc, 8> {
        let mut out = Vec::new();
        for &b in bytes {
            p.push(b);
            while let Some(item) = p.pop() {
                if let Item::Frame(rpc) = item {
                    let _ = out.push(rpc);
                }
            }
        }
        out
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
        let got = feed(&mut Demux::new(), &rpc(CMD_WIFI, &data));
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
        let mut p = Demux::new();
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
        let mut p = Demux::new();
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
    fn a_marker_inside_junk_and_back_to_back_frames_are_found() {
        let mut p = Demux::new();
        let mut stream: Vec<u8, 200> = Vec::new();
        // a half marker, then a real frame, then another straight after it
        stream.extend_from_slice(b"IMPIMPRO").unwrap();
        stream.extend_from_slice(&rpc(CMD_INFO, &[])).unwrap();
        stream.extend_from_slice(&rpc(CMD_SCAN, &[])).unwrap();
        assert_eq!(&feed(&mut p, &stream)[..], &[Rpc::Info, Rpc::Scan]);
    }

    #[test]
    fn a_frame_fed_one_byte_at_a_time_and_the_parser_ready_for_the_next() {
        let mut p = Demux::new();
        let f = rpc(CMD_STATE, &[]);
        let got = feed(&mut p, &[&f[..], &f[..]].concat());
        assert_eq!(&got[..], &[Rpc::State, Rpc::State]);
    }

    #[test]
    fn a_frame_that_is_not_an_rpc_is_dropped() {
        let mut p = Demux::new();
        let mut state = std::vec::Vec::from(&state_packet(State::Ready)[..]);
        state.extend_from_slice(&rpc(CMD_INFO, &[]));
        assert_eq!(&feed(&mut p, &state)[..], &[Rpc::Info]);
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
