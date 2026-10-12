//! The twin's network: an in-process MQTT broker and the [`Connector`] that dials it, over a pair of
//! in-memory pipes on the virtual clock. The broker records everything the device sends, delivers
//! what a scenario publishes (with the retain flag as given, as Retain As Published subscriptions
//! see it), and can go silent or refuse connections (`Action::Broker`) to test outages.
//!
//! It implements just enough of MQTT 5 for the device: it is the codec's second user, not an
//! independent check of it (that is `fw/mqtt-interop`, against Mosquitto).

use core::sync::atomic::{AtomicBool, Ordering};

use embassy_futures::select::{Either, select, select3};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::Channel;
use embassy_sync::pipe::Pipe;
use embassy_sync::signal::Signal;
use minimeat_app::mqtt::Connector;
use minimeat_app::mqtt::packet::{self, Packet, PublishOut};

use crate::clock;

type M = CriticalSectionRawMutex;
const PIPE: usize = 4096;

/// What the broker saw the device do.
#[derive(Clone, Debug, PartialEq)]
pub enum Seen {
    Connect {
        client_id: String,
        keepalive_s: u16,
        will_topic: Option<String>,
        will_payload: Vec<u8>,
        will_retain: bool,
        username: Option<String>,
    },
    Subscribe(Vec<String>),
    Publish {
        topic: String,
        payload: Vec<u8>,
        qos: u8,
        retain: bool,
        response_topic: Option<String>,
        correlation: Option<Vec<u8>>,
    },
    Ping,
}

/// A message a scenario has the broker deliver to the device.
#[derive(Clone, Copy, Debug)]
pub struct Inject {
    pub topic: &'static str,
    pub payload: &'static str,
    pub retain: bool,
    pub response_topic: Option<&'static str>,
}

pub struct Link {
    c2b: Pipe<M, PIPE>,
    b2c: Pipe<M, PIPE>,
    up: AtomicBool,
    dial: Signal<M, ()>,
    down: Signal<M, ()>,
    inject: Channel<M, Inject, 8>,
    log: std::sync::Mutex<Vec<(f64, Seen)>>,
}

impl Link {
    pub fn new() -> Self {
        Self {
            c2b: Pipe::new(),
            b2c: Pipe::new(),
            up: AtomicBool::new(true),
            dial: Signal::new(),
            down: Signal::new(),
            inject: Channel::new(),
            log: std::sync::Mutex::new(Vec::new()),
        }
    }

    /// The broker goes silent and refuses connections (`false`), or is back.
    pub fn set_up(&self, up: bool) {
        self.up.store(up, Ordering::SeqCst);
        if !up {
            self.down.signal(());
        }
    }

    pub fn inject(&self, m: Inject) {
        if self.inject.try_send(m).is_err() {
            log::warn!("twin: broker injection queue full");
        }
    }

    pub fn take_log(&self) -> Vec<(f64, Seen)> {
        std::mem::take(&mut *self.log.lock().unwrap())
    }

    fn record(&self, s: Seen) {
        self.log
            .lock()
            .unwrap()
            .push((clock::now() as f64 / clock::TICK_HZ as f64, s));
    }
}

impl Default for Link {
    fn default() -> Self {
        Self::new()
    }
}

// ------------------------------------------------------------------------------- the client's end

pub struct TwinConn(pub &'static Link);

impl Connector for TwinConn {
    type Reader<'a> = &'static Pipe<M, PIPE>;
    type Writer<'a> = &'static Pipe<M, PIPE>;

    async fn connect<'a>(&'a mut self, _host: &str, _port: u16) -> Result<(Self::Reader<'a>, Self::Writer<'a>), ()> {
        let l = self.0;
        if !l.up.load(Ordering::SeqCst) {
            return Err(());
        }
        l.c2b.clear();
        l.b2c.clear();
        l.dial.signal(());
        Ok((&l.b2c, &l.c2b))
    }
}

// --------------------------------------------------------------------------------- the broker

/// The broker task.
pub async fn broker(link: &'static Link) -> ! {
    loop {
        link.dial.wait().await;
        link.down.reset();
        session(link).await;
    }
}

async fn read_exact(p: &Pipe<M, PIPE>, buf: &mut [u8]) {
    let mut got = 0;
    while got < buf.len() {
        got += p.read(&mut buf[got..]).await;
    }
}

async fn read_packet(p: &Pipe<M, PIPE>) -> (u8, Vec<u8>) {
    let mut b = [0u8; 1];
    read_exact(p, &mut b).await;
    let first = b[0];
    let (mut len, mut mul) = (0usize, 1usize);
    loop {
        read_exact(p, &mut b).await;
        len += usize::from(b[0] & 0x7F) * mul;
        if b[0] & 0x80 == 0 {
            break;
        }
        mul *= 128;
    }
    let mut body = vec![0u8; len];
    read_exact(p, &mut body).await;
    (first, body)
}

struct Cursor<'a>(&'a [u8]);

impl<'a> Cursor<'a> {
    fn take(&mut self, n: usize) -> &'a [u8] {
        let (h, t) = self.0.split_at(n);
        self.0 = t;
        h
    }
    fn u8(&mut self) -> u8 {
        self.take(1)[0]
    }
    fn u16(&mut self) -> u16 {
        u16::from_be_bytes(self.take(2).try_into().unwrap())
    }
    fn varint(&mut self) -> usize {
        let (mut v, mut mul) = (0, 1);
        loop {
            let b = self.u8();
            v += usize::from(b & 0x7F) * mul;
            if b & 0x80 == 0 {
                return v;
            }
            mul *= 128;
        }
    }
    fn bin(&mut self) -> &'a [u8] {
        let n = usize::from(self.u16());
        self.take(n)
    }
    fn string(&mut self) -> String {
        String::from_utf8(self.bin().to_vec()).unwrap()
    }
}

fn parse_connect(body: &[u8]) -> Seen {
    let mut c = Cursor(body);
    c.bin(); // "MQTT"
    c.u8(); // level
    let flags = c.u8();
    let keepalive_s = c.u16();
    let n = c.varint();
    c.take(n);
    let client_id = c.string();
    let (mut will_topic, mut will_payload) = (None, Vec::new());
    if flags & 0b100 != 0 {
        let n = c.varint();
        c.take(n);
        will_topic = Some(c.string());
        will_payload = c.bin().to_vec();
    }
    let username = (flags & 0x80 != 0).then(|| c.string());
    Seen::Connect {
        client_id,
        keepalive_s,
        will_topic,
        will_payload,
        will_retain: flags & 0b10_0000 != 0,
        username,
    }
}

async fn session(l: &'static Link) {
    let replies: Channel<M, Vec<u8>, 8> = Channel::new();
    let subscribed = AtomicBool::new(false);
    let down = l.down.wait();
    let reader = async {
        loop {
            let (first, body) = read_packet(&l.c2b).await;
            match first >> 4 {
                1 => {
                    l.record(parse_connect(&body));
                    // CONNACK: no session, success, no properties
                    replies.send(vec![0x20, 3, 0, 0, 0]).await;
                }
                8 => {
                    let mut c = Cursor(&body);
                    let id = c.u16();
                    let n = c.varint();
                    c.take(n);
                    let mut filters = Vec::new();
                    while !c.0.is_empty() {
                        filters.push(c.string());
                        c.u8();
                    }
                    let mut suback = vec![0x90, 3 + filters.len() as u8, (id >> 8) as u8, id as u8, 0];
                    suback.extend(filters.iter().map(|_| 1u8));
                    l.record(Seen::Subscribe(filters));
                    subscribed.store(true, Ordering::SeqCst);
                    replies.send(suback).await;
                }
                3 => {
                    if let Ok(Packet::Publish(p)) = packet::parse(first, &body) {
                        if p.qos == 1 {
                            let id = p.packet_id;
                            replies.send(vec![0x40, 2, (id >> 8) as u8, id as u8]).await;
                        }
                        l.record(Seen::Publish {
                            topic: p.topic.to_string(),
                            payload: p.payload.to_vec(),
                            qos: p.qos,
                            retain: p.retain,
                            response_topic: p.response_topic.map(str::to_string),
                            correlation: p.correlation.map(<[u8]>::to_vec),
                        });
                    }
                }
                12 => {
                    l.record(Seen::Ping);
                    replies.send(vec![0xD0, 0]).await;
                }
                14 => return,
                _ => {}
            }
        }
    };
    let writer = async {
        let mut buf = vec![0u8; 8192];
        loop {
            match select(replies.receive(), l.inject.receive()).await {
                Either::First(bytes) => l.b2c.write_all(&bytes).await,
                Either::Second(m) if subscribed.load(Ordering::SeqCst) => {
                    let mut p = PublishOut::new(m.topic, m.payload.as_bytes(), 0, m.retain);
                    p.response_topic = m.response_topic;
                    p.correlation = m.response_topic.map(|_| &b"corr"[..]);
                    let n = packet::publish(&mut buf, &p).unwrap();
                    l.b2c.write_all(&buf[..n]).await;
                }
                _ => {}
            }
        }
    };
    // the session ends when the device disconnects or the broker is taken down (silently: the
    // device finds out from its pings)
    select3(reader, writer, down).await;
}

// ------------------------------------------------------------------- real sockets (paced runs)

/// A connection to a real server (Mosquitto, an OpenTelemetry collector) for a paced twin run.
/// The twin's tasks see a pair of pipes; two threads move bytes between them and the TCP socket,
/// and waking a pipe's reader wakes the twin's main loop (see `clock`).
///
/// Only sensible when the twin runs at a modest multiple of real time: timeouts are virtual, so at
/// `--speed max` an acknowledgement would arrive after the client gave up.
pub struct RealConn {
    to_server: &'static Pipe<M, PIPE>,
    from_server: &'static Pipe<M, PIPE>,
    generation: &'static std::sync::atomic::AtomicU64,
}

impl RealConn {
    pub fn new() -> Self {
        RealConn {
            to_server: Box::leak(Box::new(Pipe::new())),
            from_server: Box::leak(Box::new(Pipe::new())),
            generation: Box::leak(Box::new(std::sync::atomic::AtomicU64::new(0))),
        }
    }
}

impl Default for RealConn {
    fn default() -> Self {
        Self::new()
    }
}

impl Connector for RealConn {
    type Reader<'a> = &'static Pipe<M, PIPE>;
    type Writer<'a> = &'static Pipe<M, PIPE>;

    async fn connect<'a>(&'a mut self, host: &str, port: u16) -> Result<(Self::Reader<'a>, Self::Writer<'a>), ()> {
        use std::io::{Read as _, Write as _};
        use std::sync::atomic::Ordering::SeqCst;
        let (to_server, from_server, generation) = (self.to_server, self.from_server, self.generation);
        // the previous connection's threads see the new generation and stop
        let mine = generation.fetch_add(1, SeqCst) + 1;
        to_server.clear();
        from_server.clear();
        let done: &'static Signal<M, bool> = Box::leak(Box::new(Signal::new()));
        let addr = format!("{host}:{port}");
        std::thread::spawn(move || {
            let Ok(sock) = std::net::TcpStream::connect(&addr) else {
                done.signal(false);
                return;
            };
            let _ = sock.set_nodelay(true);
            let Ok(mut reader) = sock.try_clone() else {
                done.signal(false);
                return;
            };
            done.signal(true);
            let mut writer = sock;
            // server -> device
            let rt = std::thread::spawn(move || {
                let mut buf = [0u8; 2048];
                while generation.load(SeqCst) == mine {
                    match reader.read(&mut buf) {
                        Ok(0) | Err(_) => return,
                        Ok(n) => {
                            let mut left = &buf[..n];
                            while !left.is_empty() && generation.load(SeqCst) == mine {
                                match from_server.try_write(left) {
                                    Ok(w) => left = &left[w..],
                                    Err(_) => std::thread::sleep(std::time::Duration::from_millis(1)),
                                }
                            }
                        }
                    }
                }
            });
            // device -> server
            let mut buf = [0u8; 2048];
            while generation.load(SeqCst) == mine {
                match to_server.try_read(&mut buf) {
                    Ok(n) if n > 0 => {
                        if writer.write_all(&buf[..n]).is_err() {
                            break;
                        }
                    }
                    _ => std::thread::sleep(std::time::Duration::from_millis(1)),
                }
            }
            let _ = writer.shutdown(std::net::Shutdown::Both);
            let _ = rt.join();
        });
        if done.wait().await {
            Ok((from_server, to_server))
        } else {
            Err(())
        }
    }
}

/// Either the in-process broker or a real server.
pub enum AnyConn {
    Mem(TwinConn),
    Real(RealConn),
}

impl Connector for AnyConn {
    type Reader<'a> = &'static Pipe<M, PIPE>;
    type Writer<'a> = &'static Pipe<M, PIPE>;

    async fn connect<'a>(&'a mut self, host: &str, port: u16) -> Result<(Self::Reader<'a>, Self::Writer<'a>), ()> {
        match self {
            AnyConn::Mem(c) => c.connect(host, port).await,
            AnyConn::Real(c) => c.connect(host, port).await,
        }
    }
}
