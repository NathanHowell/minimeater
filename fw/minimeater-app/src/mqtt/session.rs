//! The MQTT session (`.5.2`): connects with a Last Will, announces itself, publishes the device's
//! topics from the [`Bus`], takes `set` and `cmd` messages into it and answers them, and reconnects
//! with backoff for as long as the device runs. The network is a [`Connector`]: embassy-net sockets
//! on the ESP32-C6, plain TCP or an in-memory pipe on the host.
//!
//! Control never waits on any of this: it publishes to the bus and moves on.
//!
//! Per connection two futures run in one task: the reader (never cancelled mid-packet) and the
//! writer. The reader turns a PUBLISH into a request for the control task (a retained one is
//! dropped, ADR 0003) and hands acknowledgements to the writer through [`Shared`].

use core::cell::{Cell, RefCell};
use core::fmt::Write as _;

use embassy_futures::select::{Either, Either6, select, select6};
use embassy_sync::blocking_mutex::raw::NoopRawMutex;
use embassy_sync::channel::Channel;
use embassy_sync::pubsub::WaitResult;
use embassy_sync::signal::Signal;
use embassy_time::{Duration, Instant, Ticker, Timer, with_timeout};
use embedded_io_async::{Read, Write};
use heapless::{String, Vec};
use log::{info, warn};

use super::packet::{self, Connect, Packet, PublishIn, PublishOut, Will};
use crate::config::Settings;
use crate::net::{Connector, Identity};
use crate::pit::Event;
use crate::proto::request::{Error, ErrorCode, Request};
use crate::proto::{self, Device};
use crate::tasks::{Bus, Reply};

/// The largest payload the session publishes (the discovery message is about 16 KB).
pub const PAYLOAD: usize = 20 * 1024;
const TX: usize = PAYLOAD + 1024;
// a full payload still fits with the longest topic the device publishes on (`topic`'s 96 bytes)
const _: () = assert!(TX >= PAYLOAD + super::packet::publish_overhead(96));
const RX: usize = 1536;

/// The session's memory: give it static storage.
pub struct Buffers {
    tx: [u8; TX],
    rx: [u8; RX],
    payload: String<PAYLOAD>,
}

impl Buffers {
    pub const fn new() -> Self {
        Self {
            tx: [0; TX],
            rx: [0; RX],
            payload: String::new(),
        }
    }
}

impl Default for Buffers {
    fn default() -> Self {
        Self::new()
    }
}

const KEEPALIVE_S: u16 = 30;
/// A ping goes out this often; silence from the broker for [`DEAD_S`] drops the connection.
const PING_S: u64 = 10;
const DEAD_S: u64 = 25;
const ACK_TIMEOUT_S: u64 = 5;
const CONNECT_TIMEOUT_S: u64 = 10;
const BACKOFF_MAX_S: u64 = 60;
/// Requests from MQTT carry ids in this range, so they can be told from the twin's.
const ID_BASE: u16 = 0x1000;
const PENDING: usize = 8;

#[derive(Debug)]
#[allow(dead_code, reason = "the payloads are for the Debug output in the log")]
enum Fail {
    Io,
    Protocol,
    Timeout,
    Refused(u8),
    Disconnected,
    Full,
}

#[allow(clippy::large_enum_variant, reason = "a few slots in a channel, no alloc")]
/// What the reader leaves for the writer.
enum Out {
    PubAck(u16),
    /// Home Assistant came online: publish discovery again.
    Discovery,
    /// A request's outcome, to be answered.
    Done(Pending, Result<(), Error>),
}

/// What is needed to answer a request when the control task replies.
#[derive(Clone)]
struct Pending {
    suffix: String<48>,
    response_topic: Option<String<64>>,
    correlation: Option<Vec<u8, 32>>,
}

struct Shared {
    acks: Signal<NoopRawMutex, u16>,
    suback: Signal<NoopRawMutex, u16>,
    out: Channel<NoopRawMutex, Out, 8>,
    last_rx: Cell<Instant>,
    pending: RefCell<[Option<(u16, Pending)>; PENDING]>,
    next_id: Cell<u16>,
}

impl Shared {
    fn new() -> Self {
        Self {
            acks: Signal::new(),
            suback: Signal::new(),
            out: Channel::new(),
            last_rx: Cell::new(Instant::now()),
            pending: RefCell::new([const { None }; PENDING]),
            next_id: Cell::new(ID_BASE),
        }
    }

    /// Remembers a request; its id, or `None` if too many are waiting.
    fn remember(&self, p: Pending) -> Option<u16> {
        let mut table = self.pending.borrow_mut();
        let slot = table.iter_mut().find(|s| s.is_none())?;
        let id = self.next_id.get();
        self.next_id.set(if id >= ID_BASE + 0x0FFF { ID_BASE } else { id + 1 });
        *slot = Some((id, p));
        Some(id)
    }

    fn take(&self, id: u16) -> Option<Pending> {
        let mut table = self.pending.borrow_mut();
        let slot = table.iter_mut().find(|s| s.as_ref().is_some_and(|(i, _)| *i == id))?;
        slot.take().map(|(_, p)| p)
    }
}

/// Runs the MQTT session forever: connect, serve until it fails, back off, repeat. While the
/// settings hold no broker it waits for some.
pub async fn run<C: Connector>(
    conn: &mut C,
    who: &Identity<'_>,
    bus: &Bus,
    bufs: &mut Buffers,
    rssi: &dyn Fn() -> Option<i32>,
) -> ! {
    let mut events = bus.events.subscriber().expect("a bus reader slot for MQTT");
    let mut replies = bus.replies.subscriber().expect("a bus reader slot for MQTT");
    let mut status = bus.status.receiver().expect("a bus reader slot for MQTT");
    let mut config_rev = bus.config_rev.receiver().expect("a bus reader slot for MQTT");
    let mut backoff = 1;
    loop {
        let Some(mqtt) = bus.config().and_then(|c| c.mqtt) else {
            Timer::after_secs(5).await;
            continue;
        };
        let started = Instant::now();
        let mut ctx = Ctx {
            who,
            bus,
            events: &mut events,
            replies: &mut replies,
            status: &mut status,
            config_rev: &mut config_rev,
            rssi,
        };
        let r = serve(conn, &mut ctx, bufs, &mqtt.host, mqtt.port, &mqtt.user, &mqtt.pass).await;
        warn!("mqtt: connection ended: {:?}", r.err());
        if started.elapsed() > Duration::from_secs(60) {
            backoff = 1;
        }
        Timer::after_secs(backoff).await;
        backoff = (backoff * 2).min(BACKOFF_MAX_S);
    }
}

type Events<'a> =
    embassy_sync::pubsub::Subscriber<'a, crate::tasks::M, crate::pit::Event, 16, { crate::tasks::READERS }, 1>;
type Replies<'a> = embassy_sync::pubsub::Subscriber<'a, crate::tasks::M, Reply, 8, { crate::tasks::READERS }, 1>;
type StatusRx<'a> = embassy_sync::watch::Receiver<'a, crate::tasks::M, crate::tasks::Status, { crate::tasks::READERS }>;
type ConfigRx<'a> = embassy_sync::watch::Receiver<'a, crate::tasks::M, u32, { crate::tasks::READERS }>;

struct Ctx<'a, 'b> {
    who: &'a Identity<'a>,
    bus: &'a Bus,
    events: &'b mut Events<'a>,
    replies: &'b mut Replies<'a>,
    status: &'b mut StatusRx<'a>,
    config_rev: &'b mut ConfigRx<'a>,
    rssi: &'a dyn Fn() -> Option<i32>,
}

/// `prefix/id/tail`.
fn topic(who: &Identity, tail: &str) -> String<96> {
    let mut t = String::new();
    // a topic that does not fit is cut; the broker will refuse it and the session restarts
    let _ = write!(t, "{}/{}/{}", who.prefix, who.id, tail);
    t
}

async fn serve<C: Connector>(
    conn: &mut C,
    ctx: &mut Ctx<'_, '_>,
    bufs: &mut Buffers,
    host: &str,
    port: u16,
    user: &str,
    pass: &str,
) -> Result<(), Fail> {
    let (mut r, mut w) = with_timeout(Duration::from_secs(CONNECT_TIMEOUT_S), conn.connect(host, port))
        .await
        .map_err(|_| Fail::Timeout)?
        .map_err(|()| Fail::Io)?;
    let Buffers { tx, rx, payload } = bufs;

    let status_topic = topic(ctx.who, "status");
    let n = packet::connect(
        tx,
        &Connect {
            client_id: ctx.who.id,
            keepalive_s: KEEPALIVE_S,
            will: Some(Will {
                topic: &status_topic,
                payload: b"offline",
                qos: 1,
                retain: true,
            }),
            username: (!user.is_empty()).then_some(user),
            password: (!pass.is_empty()).then_some(pass.as_bytes()),
        },
    )
    .map_err(|_| Fail::Protocol)?;
    send(&mut w, &tx[..n]).await?;
    let (first, len) = with_timeout(Duration::from_secs(CONNECT_TIMEOUT_S), read_packet(&mut r, rx))
        .await
        .map_err(|_| Fail::Timeout)??;
    match packet::parse(first, &rx[..len.ok_or(Fail::Protocol)?]) {
        Ok(Packet::ConnAck { reason: 0, .. }) => {}
        Ok(Packet::ConnAck { reason, .. }) => return Err(Fail::Refused(reason)),
        _ => return Err(Fail::Protocol),
    }
    info!("mqtt: connected to {host}:{port}");

    let shared = Shared::new();
    let reader = reader(&mut r, rx, &shared, ctx.who, ctx.bus);
    let writer = writer(&mut w, tx, payload, &shared, ctx);
    match select(reader, writer).await {
        Either::First(e) | Either::Second(e) => e,
    }
}

/// A socket whose send buffer stays full is a dead connection: give up on it.
const SEND_TIMEOUT_S: u64 = 10;

async fn send<W: Write>(w: &mut W, bytes: &[u8]) -> Result<(), Fail> {
    with_timeout(Duration::from_secs(SEND_TIMEOUT_S), async {
        w.write_all(bytes).await.map_err(|_| Fail::Io)?;
        w.flush().await.map_err(|_| Fail::Io)
    })
    .await
    .map_err(|_| Fail::Timeout)?
}

/// Reads one packet into `rx`: its first byte, and its body length, or `None` for a body too big
/// for `rx` (read and dropped).
async fn read_packet<R: Read>(r: &mut R, rx: &mut [u8]) -> Result<(u8, Option<usize>), Fail> {
    let mut b = [0u8; 1];
    r.read_exact(&mut b).await.map_err(|_| Fail::Io)?;
    let first = b[0];
    let (mut len, mut mul) = (0usize, 1usize);
    loop {
        r.read_exact(&mut b).await.map_err(|_| Fail::Io)?;
        len += usize::from(b[0] & 0x7F) * mul;
        if b[0] & 0x80 == 0 {
            break;
        }
        mul *= 128;
        if mul > 128 * 128 * 128 {
            return Err(Fail::Protocol);
        }
    }
    if len <= rx.len() {
        r.read_exact(&mut rx[..len]).await.map_err(|_| Fail::Io)?;
        return Ok((first, Some(len)));
    }
    let mut left = len;
    while left > 0 {
        let n = left.min(rx.len());
        r.read_exact(&mut rx[..n]).await.map_err(|_| Fail::Io)?;
        left -= n;
    }
    Ok((first, None))
}

async fn reader<R: Read>(r: &mut R, rx: &mut [u8], sh: &Shared, who: &Identity<'_>, bus: &Bus) -> Result<(), Fail> {
    loop {
        let (first, len) = read_packet(r, rx).await?;
        sh.last_rx.set(Instant::now());
        let Some(len) = len else {
            warn!("mqtt: dropped an oversize packet");
            continue;
        };
        match packet::parse(first, &rx[..len]).map_err(|_| Fail::Protocol)? {
            Packet::Publish(p) => {
                if p.qos == 1 && sh.out.try_send(Out::PubAck(p.packet_id)).is_err() {
                    warn!("mqtt: could not queue a PUBACK");
                }
                incoming(&p, sh, who, bus);
            }
            Packet::PubAck { packet_id, reason } if reason < 0x80 => sh.acks.signal(packet_id),
            Packet::PubAck { reason, .. } => return Err(Fail::Refused(reason)),
            Packet::SubAck { packet_id, reasons } => {
                if reasons.iter().any(|r| *r >= 0x80) {
                    return Err(Fail::Refused(reasons.iter().copied().max().unwrap_or(0x80)));
                }
                sh.suback.signal(packet_id);
            }
            Packet::Disconnect { .. } => return Err(Fail::Disconnected),
            Packet::PingResp | Packet::ConnAck { .. } | Packet::Other(_) => {}
        }
    }
}

/// A PUBLISH from the broker: a command for the device, or Home Assistant coming online.
fn incoming(p: &PublishIn, sh: &Shared, who: &Identity, bus: &Bus) {
    if p.topic == "homeassistant/status" {
        if p.payload == b"online" {
            let _ = sh.out.try_send(Out::Discovery);
        }
        return;
    }
    let base = topic(who, "");
    let Some(suffix) = p.topic.strip_prefix(base.as_str()) else {
        return;
    };
    let mut pending = Pending {
        suffix: String::new(),
        response_topic: None,
        correlation: None,
    };
    // a suffix too long to keep cannot be a topic the device has
    let long = pending.suffix.push_str(suffix).is_err();
    pending.response_topic = p.response_topic.and_then(|t| String::try_from(t).ok());
    pending.correlation = p.correlation.and_then(|c| Vec::from_slice(c).ok());
    let parsed = if long {
        Err(Error::new(ErrorCode::Unknown, "no such topic"))
    } else {
        proto::parse(suffix, p.payload, p.retain)
    };
    match parsed {
        Err(e) => {
            let _ = sh.out.try_send(Out::Done(pending, Err(e)));
        }
        Ok(req) => dispatch(req, pending, sh, bus),
    }
}

fn dispatch(req: Request, pending: Pending, sh: &Shared, bus: &Bus) {
    let busy = || Err(Error::new(ErrorCode::NotAllowed, "busy"));
    match sh.remember(pending.clone()) {
        None => {
            let _ = sh.out.try_send(Out::Done(pending, busy()));
        }
        Some(id) => {
            if !bus.send(id, req) {
                sh.take(id);
                let _ = sh.out.try_send(Out::Done(pending, busy()));
            }
        }
    }
}

/// Everything the writer needs besides the socket.
struct Tx<'a, W: Write> {
    w: &'a mut W,
    buf: &'a mut [u8],
    payload: &'a mut String<PAYLOAD>,
    sh: &'a Shared,
    packet_id: u16,
}

impl<W: Write> Tx<'_, W> {
    async fn put(&mut self, topic: &str, qos: u8, retain: bool) -> Result<(), Fail> {
        self.publish(PublishOut::new(topic, &[], qos, retain), true).await
    }

    /// Publishes `self.payload` (or `out.payload` if `use_payload` is false); QoS 1 waits for the ack.
    async fn publish(&mut self, mut p: PublishOut<'_>, use_payload: bool) -> Result<(), Fail> {
        if use_payload {
            p.payload = self.payload.as_bytes();
        }
        if p.qos > 0 {
            self.packet_id = self.packet_id.checked_add(1).unwrap_or(1);
            p.packet_id = self.packet_id;
        }
        let n = packet::publish(self.buf, &p).map_err(|_| Fail::Full)?;
        send(self.w, &self.buf[..n]).await?;
        if p.qos > 0 {
            let id = p.packet_id;
            let sh = self.sh;
            with_timeout(Duration::from_secs(ACK_TIMEOUT_S), async {
                while sh.acks.wait().await != id {}
            })
            .await
            .map_err(|_| Fail::Timeout)?;
        }
        Ok(())
    }

    /// Renders with `f` into the payload buffer, then publishes it.
    async fn json(
        &mut self,
        topic: &str,
        qos: u8,
        retain: bool,
        f: impl FnOnce(&mut proto::Json<&mut String<PAYLOAD>>),
    ) -> Result<(), Fail> {
        if !proto::render_into(self.payload, f) {
            warn!("mqtt: {topic}: payload does not fit");
            return Ok(());
        }
        self.put(topic, qos, retain).await
    }
}

async fn writer<W: Write>(
    w: &mut W,
    buf: &mut [u8],
    payload: &mut String<PAYLOAD>,
    sh: &Shared,
    ctx: &mut Ctx<'_, '_>,
) -> Result<(), Fail> {
    let who = ctx.who;
    let mut tx = Tx {
        w,
        buf,
        payload,
        sh,
        packet_id: 0,
    };
    let t = |tail: &str| topic(who, tail);

    // announce, subscribe, then publish what Home Assistant and everyone else needs first
    tx.payload.clear();
    let _ = tx.payload.push_str("online");
    tx.put(&t("status"), 1, true).await?;
    let filters = [
        t("set/#"),
        t("cmd/#"),
        String::try_from("homeassistant/status").unwrap_or_default(),
    ];
    let n = packet::subscribe(tx.buf, 1, &[&filters[0], &filters[1], &filters[2]]).map_err(|_| Fail::Protocol)?;
    send(tx.w, &tx.buf[..n]).await?;
    with_timeout(Duration::from_secs(ACK_TIMEOUT_S), sh.suback.wait())
        .await
        .map_err(|_| Fail::Timeout)?;
    let mut named = publish_discovery(&mut tx, ctx).await?;
    publish_config(&mut tx, ctx).await?;
    // what happened while the broker was away is old news
    while ctx.events.try_next_message().is_some() {}
    while ctx.replies.try_next_message().is_some() {}
    // the first session of a boot says why the device started (after the old news is dropped)
    if let Some(reason) = ctx.bus.boot.lock(Cell::take) {
        ctx.bus.emit(Event::Boot(reason));
    }
    if let Some(s) = ctx.status.try_get() {
        publish_state(&mut tx, ctx, &s).await?;
    }

    let mut ping = Ticker::every(Duration::from_secs(PING_S));
    loop {
        match select6(
            ctx.status.changed(),
            ctx.events.next_message(),
            ctx.config_rev.changed(),
            ctx.replies.next_message(),
            sh.out.receive(),
            ping.next(),
        )
        .await
        {
            Either6::First(s) => publish_state(&mut tx, ctx, &s).await?,
            Either6::Second(WaitResult::Message(e)) => {
                tx.json(&t("event"), 1, false, |j| proto::event(j, &e)).await?;
            }
            Either6::Second(WaitResult::Lagged(_)) => {}
            Either6::Third(_) => {
                publish_config(&mut tx, ctx).await?;
                // a renamed probe renames its entities: discovery says so again
                if ctx.bus.config().is_some_and(|c| names(&c) != named) {
                    named = publish_discovery(&mut tx, ctx).await?;
                }
            }
            Either6::Fourth(WaitResult::Message(Reply { id, result })) => {
                if let Some(p) = sh.take(id) {
                    answer(&mut tx, who, &p, result).await?;
                }
            }
            Either6::Fourth(WaitResult::Lagged(_)) => {}
            Either6::Fifth(Out::PubAck(id)) => {
                let n = packet::puback(tx.buf, id).map_err(|_| Fail::Protocol)?;
                send(tx.w, &tx.buf[..n]).await?;
            }
            Either6::Fifth(Out::Discovery) => named = publish_discovery(&mut tx, ctx).await?,
            Either6::Fifth(Out::Done(p, result)) => answer(&mut tx, who, &p, result).await?,
            Either6::Sixth(()) => {
                if sh.last_rx.get().elapsed() > Duration::from_secs(DEAD_S) {
                    return Err(Fail::Timeout);
                }
                send(tx.w, &packet::PINGREQ).await?;
            }
        }
    }
}

async fn publish_state<W: Write>(tx: &mut Tx<'_, W>, ctx: &Ctx<'_, '_>, s: &crate::tasks::Status) -> Result<(), Fail> {
    let rssi = (ctx.rssi)();
    tx.json(&topic(ctx.who, "state"), 0, true, |j| proto::state(j, s, rssi))
        .await
}

async fn publish_config<W: Write>(tx: &mut Tx<'_, W>, ctx: &Ctx<'_, '_>) -> Result<(), Fail> {
    match ctx.bus.config() {
        Some(c) => {
            tx.json(&topic(ctx.who, "config"), 1, true, |j| proto::config(j, &c))
                .await
        }
        None => Ok(()),
    }
}

/// Publishes the discovery message; the probe names it carried.
async fn publish_discovery<W: Write>(tx: &mut Tx<'_, W>, ctx: &Ctx<'_, '_>) -> Result<[String<24>; 4], Fail> {
    let Some(c) = ctx.bus.config() else {
        return Ok(Default::default());
    };
    let names = names(&c);
    let d = Device {
        prefix: ctx.who.prefix,
        id: ctx.who.id,
        sw: ctx.who.sw,
        hw: ctx.who.hw,
    };
    let mut t = String::<64>::new();
    let _ = proto::discovery::topic(&mut t, &d);
    tx.json(&t, 1, true, |j| {
        proto::discovery(j, &d, [&names[0], &names[1], &names[2], &names[3]])
    })
    .await?;
    Ok(names)
}

fn names(c: &Settings) -> [String<24>; 4] {
    core::array::from_fn(|i| c.probes[i].name.clone())
}

/// Answers a request: on its response topic if it asked for one, and a rejection on `~/event`.
async fn answer<W: Write>(
    tx: &mut Tx<'_, W>,
    who: &Identity<'_>,
    p: &Pending,
    result: Result<(), Error>,
) -> Result<(), Fail> {
    if let Some(rt) = &p.response_topic {
        proto::render_into(tx.payload, |j| proto::response(j, &result));
        let mut out = PublishOut::new(rt, &[], 0, false);
        out.correlation = p.correlation.as_deref();
        tx.publish(out, true).await?;
    }
    if let Err(e) = &result {
        tx.json(&topic(who, "event"), 1, false, |j| {
            proto::rejected(j, &p.suffix, e.code.name(), &e.detail)
        })
        .await?;
    }
    Ok(())
}
