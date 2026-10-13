//! The MQTT 5 packets the device needs, encoded into and decoded from byte slices (`.5.2`). Only
//! what docs/protocol.md uses: CONNECT with a Last Will, PUBLISH (QoS 0 and 1, with response topic,
//! correlation data and message expiry), SUBSCRIBE with Retain As Published, PUBACK, PINGREQ and
//! DISCONNECT out; CONNACK, PUBLISH, PUBACK, SUBACK, PINGRESP and DISCONNECT in.

/// Why a packet could not be encoded or decoded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// The output buffer is too small.
    NoSpace,
    /// Not a packet MQTT 5 allows.
    Malformed,
}

type Res<T> = Result<T, Error>;

pub const PINGREQ: [u8; 2] = [0xC0, 0];
pub const DISCONNECT: [u8; 2] = [0xE0, 0];

// packet types
const CONNECT: u8 = 1;
const CONNACK: u8 = 2;
const PUBLISH: u8 = 3;
const PUBACK: u8 = 4;
const SUBSCRIBE: u8 = 8;
const SUBACK: u8 = 9;
const PINGRESP: u8 = 13;
const DISCONNECT_T: u8 = 14;

// properties
const P_EXPIRY: u8 = 0x02;
const P_RESPONSE_TOPIC: u8 = 0x08;
const P_CORRELATION: u8 = 0x09;
const P_SESSION_EXPIRY: u8 = 0x11;

/// Writes into a slice, remembering a failure instead of returning it at every call.
struct Wr<'a> {
    b: &'a mut [u8],
    n: usize,
    ok: bool,
}

impl<'a> Wr<'a> {
    fn new(b: &'a mut [u8]) -> Self {
        Self { b, n: 0, ok: true }
    }

    fn raw(&mut self, v: &[u8]) {
        match self.b.get_mut(self.n..self.n + v.len()) {
            Some(d) => {
                d.copy_from_slice(v);
                self.n += v.len();
            }
            None => self.ok = false,
        }
    }

    fn u8(&mut self, v: u8) {
        self.raw(&[v]);
    }

    fn u16(&mut self, v: u16) {
        self.raw(&v.to_be_bytes());
    }

    fn u32(&mut self, v: u32) {
        self.raw(&v.to_be_bytes());
    }

    fn varint(&mut self, mut v: u32) {
        loop {
            let byte = (v % 128) as u8;
            v /= 128;
            self.u8(if v > 0 { byte | 0x80 } else { byte });
            if v == 0 {
                break;
            }
        }
    }

    /// A length-prefixed string or binary value.
    fn bin(&mut self, v: &[u8]) {
        match u16::try_from(v.len()) {
            Ok(n) => {
                self.u16(n);
                self.raw(v);
            }
            Err(_) => self.ok = false,
        }
    }

    fn finish(self) -> Res<usize> {
        if self.ok { Ok(self.n) } else { Err(Error::NoSpace) }
    }
}

/// Bytes a variable-length integer takes.
const fn varint_len(v: u32) -> usize {
    match v {
        0..=127 => 1,
        128..=16_383 => 2,
        16_384..=2_097_151 => 3,
        _ => 4,
    }
}

/// Writes a packet: fixed header, then the body written by `body`. The body is written after a
/// 5-byte gap and moved down once its length is known.
fn packet(buf: &mut [u8], first: u8, body: impl FnOnce(&mut Wr)) -> Res<usize> {
    const GAP: usize = 5;
    if buf.len() < GAP {
        return Err(Error::NoSpace);
    }
    let (head, rest) = buf.split_at_mut(GAP);
    let mut w = Wr::new(rest);
    body(&mut w);
    let len = w.finish()?;
    let mut h = Wr::new(head);
    h.u8(first);
    h.varint(len as u32);
    let hn = h.n;
    buf.copy_within(GAP..GAP + len, hn);
    Ok(hn + len)
}

pub struct Will<'a> {
    pub topic: &'a str,
    pub payload: &'a [u8],
    pub qos: u8,
    pub retain: bool,
}

pub struct Connect<'a> {
    pub client_id: &'a str,
    pub keepalive_s: u16,
    pub will: Option<Will<'a>>,
    pub username: Option<&'a str>,
    pub password: Option<&'a [u8]>,
}

/// CONNECT: clean start, session expiry 0 (docs/protocol.md).
pub fn connect(buf: &mut [u8], c: &Connect) -> Res<usize> {
    packet(buf, CONNECT << 4, |w| {
        w.bin(b"MQTT");
        w.u8(5);
        let mut flags = 0b10; // clean start
        if let Some(wl) = &c.will {
            flags |= 0b100 | (wl.qos & 3) << 3 | u8::from(wl.retain) << 5;
        }
        flags |= u8::from(c.username.is_some()) << 7 | u8::from(c.password.is_some()) << 6;
        w.u8(flags);
        w.u16(c.keepalive_s);
        // properties: session expiry interval 0
        w.varint(5);
        w.u8(P_SESSION_EXPIRY);
        w.u32(0);
        w.bin(c.client_id.as_bytes());
        if let Some(wl) = &c.will {
            w.varint(0); // will properties
            w.bin(wl.topic.as_bytes());
            w.bin(wl.payload);
        }
        if let Some(u) = c.username {
            w.bin(u.as_bytes());
        }
        if let Some(p) = c.password {
            w.bin(p);
        }
    })
}

pub struct PublishOut<'a> {
    pub topic: &'a str,
    pub payload: &'a [u8],
    /// 0 or 1.
    pub qos: u8,
    pub retain: bool,
    /// Used when `qos` is 1.
    pub packet_id: u16,
    pub response_topic: Option<&'a str>,
    pub correlation: Option<&'a [u8]>,
    /// Message expiry, s.
    pub expiry_s: Option<u32>,
}

impl<'a> PublishOut<'a> {
    pub fn new(topic: &'a str, payload: &'a [u8], qos: u8, retain: bool) -> Self {
        Self {
            topic,
            payload,
            qos,
            retain,
            packet_id: 0,
            response_topic: None,
            correlation: None,
            expiry_s: None,
        }
    }
}

pub fn publish(buf: &mut [u8], p: &PublishOut) -> Res<usize> {
    let first = PUBLISH << 4 | (p.qos & 1) << 1 | u8::from(p.retain);
    packet(buf, first, |w| {
        w.bin(p.topic.as_bytes());
        if p.qos > 0 {
            w.u16(p.packet_id);
        }
        let mut props = 0;
        if p.expiry_s.is_some() {
            props += 5;
        }
        if let Some(t) = p.response_topic {
            props += 3 + t.len();
        }
        if let Some(c) = p.correlation {
            props += 3 + c.len();
        }
        w.varint(props as u32);
        if let Some(e) = p.expiry_s {
            w.u8(P_EXPIRY);
            w.u32(e);
        }
        if let Some(t) = p.response_topic {
            w.u8(P_RESPONSE_TOPIC);
            w.bin(t.as_bytes());
        }
        if let Some(c) = p.correlation {
            w.u8(P_CORRELATION);
            w.bin(c);
        }
        w.raw(p.payload);
    })
}

/// SUBSCRIBE at QoS 1 with Retain As Published and no retained messages on subscribe (retain
/// handling 2). A command that arrives with the retain flag set is then a sender's own, never a
/// stale stored one the broker replayed, and the device drops it either way.
pub fn subscribe(buf: &mut [u8], packet_id: u16, filters: &[&str]) -> Res<usize> {
    packet(buf, SUBSCRIBE << 4 | 0b10, |w| {
        w.u16(packet_id);
        w.varint(0);
        for f in filters {
            w.bin(f.as_bytes());
            w.u8(0b10_1001);
        }
    })
}

pub fn puback(buf: &mut [u8], packet_id: u16) -> Res<usize> {
    packet(buf, PUBACK << 4, |w| w.u16(packet_id))
}

// ----------------------------------------------------------------------------------- decoding

struct Rd<'a> {
    b: &'a [u8],
}

impl<'a> Rd<'a> {
    fn take(&mut self, n: usize) -> Res<&'a [u8]> {
        if self.b.len() < n {
            return Err(Error::Malformed);
        }
        let (h, t) = self.b.split_at(n);
        self.b = t;
        Ok(h)
    }

    fn u8(&mut self) -> Res<u8> {
        Ok(self.take(1)?[0])
    }

    fn u16(&mut self) -> Res<u16> {
        let b = self.take(2)?;
        Ok(u16::from_be_bytes([b[0], b[1]]))
    }

    fn u32(&mut self) -> Res<u32> {
        let b = self.take(4)?;
        Ok(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn varint(&mut self) -> Res<u32> {
        let (mut v, mut mul) = (0u32, 1u32);
        for _ in 0..4 {
            let b = self.u8()?;
            v += u32::from(b & 0x7F) * mul;
            if b & 0x80 == 0 {
                return Ok(v);
            }
            mul *= 128;
        }
        Err(Error::Malformed)
    }

    fn bin(&mut self) -> Res<&'a [u8]> {
        let n = usize::from(self.u16()?);
        self.take(n)
    }

    fn str(&mut self) -> Res<&'a str> {
        core::str::from_utf8(self.bin()?).map_err(|_| Error::Malformed)
    }
}

/// What the properties of an incoming PUBLISH carry that the device uses.
#[derive(Default)]
struct Props<'a> {
    response_topic: Option<&'a str>,
    correlation: Option<&'a [u8]>,
    expiry_s: Option<u32>,
}

fn props<'a>(r: &mut Rd<'a>) -> Res<Props<'a>> {
    let len = r.varint()? as usize;
    let mut p = Rd { b: r.take(len)? };
    let mut out = Props::default();
    while !p.b.is_empty() {
        match p.u8()? {
            P_EXPIRY => out.expiry_s = Some(p.u32()?),
            P_RESPONSE_TOPIC => out.response_topic = Some(p.str()?),
            P_CORRELATION => out.correlation = Some(p.bin()?),
            // byte
            0x01 | 0x17 | 0x19 | 0x24 | 0x25 | 0x28 | 0x29 | 0x2A => {
                p.take(1)?;
            }
            // two bytes
            0x13 | 0x21 | 0x22 | 0x23 => {
                p.take(2)?;
            }
            // four bytes
            0x11 | 0x18 | 0x27 => {
                p.take(4)?;
            }
            // variable-length integer
            0x0B => {
                p.varint()?;
            }
            // strings and binary data
            0x03 | 0x12 | 0x15 | 0x16 | 0x1A | 0x1C | 0x1F => {
                p.bin()?;
            }
            // string pair
            0x26 => {
                p.bin()?;
                p.bin()?;
            }
            _ => return Err(Error::Malformed),
        }
    }
    Ok(out)
}

#[derive(Debug, PartialEq)]
pub struct PublishIn<'a> {
    pub topic: &'a str,
    pub payload: &'a [u8],
    pub qos: u8,
    /// As the sender set it (the subscription asks for Retain As Published).
    pub retain: bool,
    pub packet_id: u16,
    pub response_topic: Option<&'a str>,
    pub correlation: Option<&'a [u8]>,
    pub expiry_s: Option<u32>,
}

#[derive(Debug, PartialEq)]
pub enum Packet<'a> {
    ConnAck {
        session_present: bool,
        reason: u8,
    },
    Publish(PublishIn<'a>),
    PubAck {
        packet_id: u16,
        reason: u8,
    },
    SubAck {
        packet_id: u16,
        reasons: &'a [u8],
    },
    PingResp,
    Disconnect {
        reason: u8,
    },
    /// A packet the device never asks for (its type).
    Other(u8),
}

/// Decodes the body of a packet whose first byte is `first`.
pub fn parse(first: u8, body: &[u8]) -> Res<Packet<'_>> {
    let mut r = Rd { b: body };
    Ok(match first >> 4 {
        CONNACK => {
            let flags = r.u8()?;
            Packet::ConnAck {
                session_present: flags & 1 != 0,
                reason: r.u8()?,
            }
        }
        PUBLISH => {
            let qos = (first >> 1) & 3;
            if qos == 3 {
                return Err(Error::Malformed);
            }
            let topic = r.str()?;
            let packet_id = if qos > 0 { r.u16()? } else { 0 };
            let p = props(&mut r)?;
            Packet::Publish(PublishIn {
                topic,
                payload: r.b,
                qos,
                retain: first & 1 != 0,
                packet_id,
                response_topic: p.response_topic,
                correlation: p.correlation,
                expiry_s: p.expiry_s,
            })
        }
        PUBACK => {
            let packet_id = r.u16()?;
            // the reason code and properties are optional on a success
            let reason = if r.b.is_empty() { 0 } else { r.u8()? };
            Packet::PubAck { packet_id, reason }
        }
        SUBACK => {
            let packet_id = r.u16()?;
            props(&mut r)?;
            Packet::SubAck {
                packet_id,
                reasons: r.b,
            }
        }
        PINGRESP => Packet::PingResp,
        DISCONNECT_T => Packet::Disconnect {
            reason: if r.b.is_empty() { 0 } else { r.u8()? },
        },
        t => Packet::Other(t),
    })
}

/// The size of a PUBLISH's overhead for `topic`, to size buffers: fixed header, topic, packet id,
/// and the properties the device writes.
pub const fn publish_overhead(topic_len: usize) -> usize {
    1 + varint_len(1 << 21) + 2 + topic_len + 2 + 1 + 5 + 3 + 64 + 3 + 32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connect_bytes() {
        let mut b = [0u8; 64];
        let n = connect(
            &mut b,
            &Connect {
                client_id: "a",
                keepalive_s: 30,
                will: None,
                username: None,
                password: None,
            },
        )
        .unwrap();
        assert_eq!(
            b[..n],
            [
                0x10, 19, 0, 4, b'M', b'Q', b'T', b'T', 5, 0b10, 0, 30, 5, 0x11, 0, 0, 0, 0, 0, 1, b'a'
            ]
        );
    }

    #[test]
    fn connect_with_will_and_credentials() {
        let mut b = [0u8; 128];
        let n = connect(
            &mut b,
            &Connect {
                client_id: "mm-1",
                keepalive_s: 30,
                will: Some(Will {
                    topic: "t/status",
                    payload: b"offline",
                    qos: 1,
                    retain: true,
                }),
                username: Some("u"),
                password: Some(b"pw"),
            },
        )
        .unwrap();
        // flags: user, password, will retain, will qos 1, will, clean start
        assert_eq!(b[9], 0b1110_1110);
        assert_eq!(b[1] as usize, n - 2, "remaining length");
        // the will topic and payload follow the client id and an empty will-properties block
        let tail = &b[..n];
        let at = tail.windows(8).position(|w| w == b"t/status").unwrap();
        assert_eq!(tail[at - 3..at], [0, 0, 8]);
        assert_eq!(
            tail[at + 8..at + 8 + 9],
            [0, 7, b'o', b'f', b'f', b'l', b'i', b'n', b'e']
        );
        assert!(tail.ends_with(&[0, 1, b'u', 0, 2, b'p', b'w']));
    }

    #[test]
    fn publish_round_trips_with_properties() {
        let mut b = [0u8; 256];
        let mut p = PublishOut::new("minimeater/mm-1/state", br#"{"v":1}"#, 1, true);
        p.packet_id = 7;
        p.response_topic = Some("reply/1");
        p.correlation = Some(&[1, 2, 3]);
        p.expiry_s = Some(30);
        let n = publish(&mut b, &p).unwrap();
        assert_eq!(b[0], 0x33, "PUBLISH, QoS 1, retain");
        let (first, body) = (b[0], &b[2..n]);
        assert_eq!(usize::from(b[1]), body.len());
        assert_eq!(
            parse(first, body).unwrap(),
            Packet::Publish(PublishIn {
                topic: "minimeater/mm-1/state",
                payload: br#"{"v":1}"#,
                qos: 1,
                retain: true,
                packet_id: 7,
                response_topic: Some("reply/1"),
                correlation: Some(&[1, 2, 3]),
                expiry_s: Some(30),
            })
        );
    }

    #[test]
    fn long_payloads_use_multi_byte_lengths() {
        let mut b = [0u8; 20_000];
        let payload = [b'x'; 16_000];
        let n = publish(&mut b, &PublishOut::new("t", &payload, 0, false)).unwrap();
        assert_eq!(b[0], 0x30);
        // 16_000 + topic (3) + properties length (1) = 16_004 = 0x3E84: two bytes
        assert_eq!(b[1..3], [0x84, 0x7D]);
        assert_eq!(n, 3 + 16_004);
        let Packet::Publish(p) = parse(b[0], &b[3..n]).unwrap() else {
            panic!()
        };
        assert_eq!(p.payload.len(), 16_000);
        // a buffer that is too small fails cleanly
        assert_eq!(
            publish(&mut b[..100], &PublishOut::new("t", &payload, 0, false)),
            Err(Error::NoSpace)
        );
    }

    #[test]
    fn subscribe_asks_for_retain_as_published_and_no_replay() {
        let mut b = [0u8; 64];
        let n = subscribe(&mut b, 1, &["a/#", "b"]).unwrap();
        assert_eq!(
            b[..n],
            [0x82, 13, 0, 1, 0, 0, 3, b'a', b'/', b'#', 0x29, 0, 1, b'b', 0x29][..]
        );
    }

    #[test]
    fn incoming_packets() {
        assert_eq!(
            parse(0x20, &[1, 0, 0]).unwrap(),
            Packet::ConnAck {
                session_present: true,
                reason: 0
            }
        );
        assert_eq!(
            parse(0x40, &[0, 9]).unwrap(),
            Packet::PubAck {
                packet_id: 9,
                reason: 0
            }
        );
        assert_eq!(
            parse(0x40, &[0, 9, 0x10, 0]).unwrap(),
            Packet::PubAck {
                packet_id: 9,
                reason: 0x10
            }
        );
        assert_eq!(
            parse(0x90, &[0, 1, 0, 1, 0x80]).unwrap(),
            Packet::SubAck {
                packet_id: 1,
                reasons: &[1, 0x80]
            }
        );
        assert_eq!(parse(0xD0, &[]).unwrap(), Packet::PingResp);
        assert_eq!(parse(0xE0, &[0x8B]).unwrap(), Packet::Disconnect { reason: 0x8B });
        assert_eq!(parse(0xE0, &[]).unwrap(), Packet::Disconnect { reason: 0 });
        assert_eq!(parse(0xF0, &[]).unwrap(), Packet::Other(15));
        // a PUBLISH from a broker that adds properties we do not use (user property, content type,
        // subscription identifier) still parses
        let mut body = vec![0, 1, b't', 0];
        let extra = [
            0x26, 0, 1, b'k', 0, 1, b'v', 0x03, 0, 2, b'a', b'b', 0x0B, 0x05, 0x01, 1,
        ];
        body[3] = extra.len() as u8;
        body.extend_from_slice(&extra);
        body.extend_from_slice(b"hi");
        let Packet::Publish(p) = parse(0x30, &body).unwrap() else {
            panic!()
        };
        assert_eq!((p.topic, p.payload, p.retain), ("t", &b"hi"[..], false));
    }

    #[test]
    fn malformed_packets_are_errors_not_panics() {
        assert_eq!(parse(0x30, &[]), Err(Error::Malformed));
        assert_eq!(
            parse(0x30, &[0, 5, b'a']),
            Err(Error::Malformed),
            "topic longer than the packet"
        );
        assert_eq!(parse(0x36, &[0, 1, b't', 0, 1, 0]), Err(Error::Malformed), "QoS 3");
        assert_eq!(parse(0x30, &[0, 1, 0xFF, 0]), Err(Error::Malformed), "topic not UTF-8");
        assert_eq!(
            parse(0x30, &[0, 1, b't', 9, 0x08]),
            Err(Error::Malformed),
            "properties past the end"
        );
        assert_eq!(
            parse(0x30, &[0, 1, b't', 1, 0x7F]),
            Err(Error::Malformed),
            "unknown property"
        );
        assert_eq!(parse(0x20, &[1]), Err(Error::Malformed));
        // every prefix of a good packet is an error or a packet, never a panic
        let mut b = [0u8; 64];
        let n = publish(&mut b, &PublishOut::new("topic", b"payload", 1, false)).unwrap();
        for cut in 0..n {
            let _ = parse(b[0], &b[2..cut.max(2)]);
        }
    }

    #[test]
    fn puback_and_pings() {
        let mut b = [0u8; 8];
        let n = puback(&mut b, 0x0102).unwrap();
        assert_eq!(b[..n], [0x40, 2, 1, 2]);
        assert_eq!(PINGREQ, [0xC0, 0]);
        assert_eq!(DISCONNECT, [0xE0, 0]);
    }
}
