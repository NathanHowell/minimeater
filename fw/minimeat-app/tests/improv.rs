//! The Improv serial conversation (`.4.10`) against a scripted serial port, a scripted radio and a stand-in for the
//! control task.

use std::collections::VecDeque;

use embassy_futures::block_on;
use embassy_futures::select::{Either, select};
use embassy_futures::yield_now;
use embedded_io_async::{ErrorType, Read, Write};
use heapless::Vec;
use minimeat_app::config::Wifi;
use minimeat_app::improv::{self, ErrorCode, Network, State, Station};
use minimeat_app::net::Identity;
use minimeat_app::proto::Request;
use minimeat_app::tasks::{Bus, Reply};

/// A clock that never advances: the conversation's hold timeout (half an item waiting) is created but these tests
/// never wait for it.
struct StillClock;

impl embassy_time_driver::Driver for StillClock {
    fn now(&self) -> u64 {
        0
    }

    fn schedule_wake(&self, _at: u64, _waker: &core::task::Waker) {}
}

embassy_time_driver::time_driver_impl!(static CLOCK: StillClock = StillClock);

struct Port {
    input: VecDeque<Vec<u8, 160>>,
    out: std::vec::Vec<u8>,
}

impl ErrorType for Port {
    type Error = core::convert::Infallible;
}

impl Read for Port {
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        match self.input.pop_front() {
            Some(c) => {
                buf[..c.len()].copy_from_slice(&c);
                Ok(c.len())
            }
            None => core::future::pending().await,
        }
    }
}

impl Write for Port {
    async fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
        self.out.extend_from_slice(buf);
        Ok(buf.len())
    }
}

struct Radio {
    join: bool,
    asked: Option<(std::string::String, std::string::String)>,
}

impl Station for Radio {
    async fn connect(&mut self, ssid: &str, pass: &str) -> bool {
        self.asked = Some((ssid.into(), pass.into()));
        self.join
    }

    async fn scan(&mut self) -> Vec<Network, 12> {
        let mut v = Vec::new();
        v.push(Network {
            ssid: "egg-net".try_into().unwrap(),
            rssi: -52,
            secured: true,
        })
        .unwrap();
        v.push(Network {
            ssid: "guest".try_into().unwrap(),
            rssi: -70,
            secured: false,
        })
        .unwrap();
        v
    }

    fn online(&self) -> bool {
        false
    }
}

const WHO: Identity<'static> = Identity {
    prefix: "minimeat",
    id: "mm-a1b2c3",
    sw: "0.1.0",
    hw: "A",
};

/// An RPC frame, built the way a client does.
fn rpc(cmd: u8, data: &[u8]) -> Vec<u8, 160> {
    let mut f: Vec<u8, 160> = Vec::new();
    f.extend_from_slice(b"IMPROV\x01\x03").unwrap();
    f.push(data.len() as u8 + 2).unwrap();
    f.extend_from_slice(&[cmd, data.len() as u8]).unwrap();
    f.extend_from_slice(data).unwrap();
    let c = f.iter().fold(0u8, |a, b| a.wrapping_add(*b));
    f.extend_from_slice(&[c, b'\n']).unwrap();
    f
}

fn wifi_rpc(ssid: &str, pass: &str) -> Vec<u8, 160> {
    let mut d: Vec<u8, 120> = Vec::new();
    d.push(ssid.len() as u8).unwrap();
    d.extend_from_slice(ssid.as_bytes()).unwrap();
    d.push(pass.len() as u8).unwrap();
    d.extend_from_slice(pass.as_bytes()).unwrap();
    rpc(0x01, &d)
}

/// Runs the conversation on `input` until `control` (the stand-in for the control task) is done.
fn converse(input: &[Vec<u8, 160>], join: bool, ack: bool) -> (std::vec::Vec<u8>, Radio, Option<Request>) {
    let bus = Bus::new();
    let mut port = Port {
        input: input.iter().cloned().collect(),
        out: std::vec::Vec::new(),
    };
    let mut radio = Radio { join, asked: None };
    let seen = block_on(async {
        let control = async {
            let mut seen = None;
            if ack {
                let c = bus.commands.receive().await;
                bus.replies.immediate_publisher().publish_immediate(Reply {
                    id: c.id,
                    result: Ok(()),
                });
                seen = Some(c.req);
            }
            for _ in 0..20 {
                yield_now().await;
            }
            seen
        };
        match select(improv::run(&mut port, &mut radio, &WHO, &bus), control).await {
            Either::Second(s) => s,
            Either::First(never) => never,
        }
    });
    let seen = seen.or_else(|| bus.commands.try_receive().ok().map(|c| c.req));
    (port.out, radio, seen)
}

fn cat(parts: &[improv::Packet]) -> std::vec::Vec<u8> {
    parts.iter().flat_map(|p| p.iter().copied()).collect()
}

#[test]
fn credentials_that_join_are_saved_through_the_control_task() {
    let (out, radio, req) = converse(&[wifi_rpc("egg-net", "hunter22")], true, true);
    assert_eq!(radio.asked, Some(("egg-net".into(), "hunter22".into())));
    assert_eq!(
        req,
        Some(Request::Wifi(Some(Wifi {
            ssid: "egg-net".try_into().unwrap(),
            pass: "hunter22".try_into().unwrap()
        })))
    );
    assert_eq!(
        out,
        cat(&[
            improv::state_packet(State::Ready),
            improv::error_packet(ErrorCode::None),
            improv::state_packet(State::Provisioning),
            improv::state_packet(State::Provisioned),
            improv::result_packet(0x01, &[]).unwrap(),
        ])
    );
}

#[test]
fn credentials_that_do_not_join_are_not_saved() {
    let (out, radio, req) = converse(&[wifi_rpc("egg-net", "wrong")], false, false);
    assert!(radio.asked.is_some());
    assert_eq!(req, None);
    assert_eq!(
        out,
        cat(&[
            improv::state_packet(State::Ready),
            improv::error_packet(ErrorCode::None),
            improv::state_packet(State::Provisioning),
            improv::error_packet(ErrorCode::UnableToConnect),
            improv::state_packet(State::Ready),
        ])
    );
}

#[test]
fn info_scan_and_the_commands_it_does_not_have() {
    let (out, _, _) = converse(
        &[rpc(0x03, &[]), rpc(0x04, &[]), rpc(0x07, &[]), rpc(0x05, &[0])],
        true,
        false,
    );
    assert_eq!(
        out,
        cat(&[
            improv::state_packet(State::Ready),
            improv::error_packet(ErrorCode::None),
            improv::result_packet(0x03, &["minimeat", "0.1.0", "ESP32-C6", "mm-a1b2c3"]).unwrap(),
            improv::error_packet(ErrorCode::None),
            improv::result_packet(0x04, &["egg-net", "-52", "YES"]).unwrap(),
            improv::result_packet(0x04, &["guest", "-70", "NO"]).unwrap(),
            improv::result_packet(0x04, &[]).unwrap(),
            improv::error_packet(ErrorCode::None),
            improv::result_packet(0x07, &["2"]).unwrap(),
            improv::error_packet(ErrorCode::None),
            improv::error_packet(ErrorCode::UnknownRpc),
        ])
    );
}

fn line(s: &str) -> Vec<u8, 160> {
    Vec::from_slice(s.as_bytes()).unwrap()
}

#[test]
fn repl_lines_set_the_broker_through_the_control_task_and_improv_still_works_beside_them() {
    use minimeat_app::config::Mqtt;
    let (out, _, req) = converse(
        &[line("mqtt set broker.lan "), line("8883 egg \"a b\"\r"), rpc(0x03, &[])],
        true,
        true,
    );
    assert_eq!(
        req,
        Some(Request::Mqtt(Some(Mqtt {
            host: "broker.lan".try_into().unwrap(),
            port: 8883,
            user: "egg".try_into().unwrap(),
            pass: "a b".try_into().unwrap(),
        })))
    );
    let text = std::string::String::from_utf8_lossy(&out).into_owned();
    // the echo hid nothing it should show, the answer came, the password was not printed back
    assert!(text.contains("mqtt set broker.lan 8883 egg \"a b\"\r\n"), "{text:?}");
    assert!(
        text.contains("saved; the broker is used from the next connection\r\nminimeat> "),
        "{text:?}"
    );
    // and the Improv information request after it was answered as a frame
    let info = improv::result_packet(0x03, &["minimeat", "0.1.0", "ESP32-C6", "mm-a1b2c3"]).unwrap();
    assert!(out.windows(info.len()).any(|w| w == &info[..]));
}

#[test]
fn an_improv_frame_prints_no_repl_text() {
    let (out, _, _) = converse(&[rpc(0x03, &[])], true, false);
    assert!(!std::string::String::from_utf8_lossy(&out).contains("unknown command"));
    assert!(!out.windows(9).any(|w| w == b"minimeat>"));
}

#[test]
fn help_and_a_bad_line_answer_in_text() {
    let (out, _, _) = converse(&[line("help\rfrobnicate\r"), line("wifi set\r")], true, false);
    let text = std::string::String::from_utf8_lossy(&out).into_owned();
    assert!(text.contains("mqtt"), "{text:?}");
    assert!(text.contains("unknown command \"frobnicate\""), "{text:?}");
    assert!(
        text.contains("usage: wifi set <ssid> [password] | wifi clear"),
        "{text:?}"
    );
}
