//! A multicast DNS responder for the device's own name (`minimeater-a1b2c3.local`, `.4.10`), so Home Assistant and a
//! browser find it without a DHCP-registered name. Resolving other `.local` names is the network stack's. Only the
//! packet handling is here: [`answer`] takes a datagram heard on 224.0.0.251:5353 and builds the reply, if there is one.

use heapless::Vec;

/// Longest reply: the header, one name (at most 63 + 6 + 1 bytes for ours) and a 14-byte A record.
pub const MAX_REPLY: usize = 96;
const TYPE_A: u16 = 1;
const TYPE_ANY: u16 = 255;
const CLASS_IN: u16 = 1;
const TTL_S: u32 = 120;

/// Reads the name at `at` (following compression pointers), calling `f` for each label; returns where the name ends
/// in the message (after the first pointer if there was one).
fn walk<'a>(msg: &'a [u8], mut at: usize, mut f: impl FnMut(&'a [u8])) -> Option<usize> {
    let mut end = None;
    for _ in 0..16 {
        let len = *msg.get(at)?;
        match len {
            0 => return Some(end.unwrap_or(at + 1)),
            l if l & 0xC0 == 0xC0 => {
                let lo = *msg.get(at + 1)?;
                end.get_or_insert(at + 2);
                at = usize::from(l & 0x3F) << 8 | usize::from(lo);
            }
            l if l & 0xC0 == 0 => {
                f(msg.get(at + 1..at + 1 + usize::from(l))?);
                at += 1 + usize::from(l);
            }
            _ => return None,
        }
    }
    None
}

/// True if the labels spell `<host>.local`, ignoring case.
fn is_ours(labels: &[&[u8]], host: &str) -> bool {
    matches!(labels, [h, l] if h.eq_ignore_ascii_case(host.as_bytes()) && l.eq_ignore_ascii_case(b"local"))
}

/// The reply to `query` if it asks for `host`.local's address (A or ANY), `None` for everything else: another name,
/// another type, a response, a datagram that does not parse.
pub fn answer(query: &[u8], host: &str, ip: [u8; 4]) -> Option<Vec<u8, MAX_REPLY>> {
    if query.len() < 12 || query[2] & 0x80 != 0 || host.len() > 63 {
        return None;
    }
    let questions = usize::from(u16::from_be_bytes([query[4], query[5]]));
    let mut at = 12;
    let mut asked = false;
    for _ in 0..questions {
        let mut labels: Vec<&[u8], 4> = Vec::new();
        let mut too_many = false;
        at = walk(query, at, |l| too_many |= labels.push(l).is_err())?;
        let t = u16::from_be_bytes([*query.get(at)?, *query.get(at + 1)?]);
        let c = u16::from_be_bytes([*query.get(at + 2)?, *query.get(at + 3)?]) & 0x7FFF;
        at += 4;
        asked |= !too_many && is_ours(&labels, host) && (t == TYPE_A || t == TYPE_ANY) && c == CLASS_IN;
    }
    if !asked {
        return None;
    }
    let mut r: Vec<u8, MAX_REPLY> = Vec::new();
    // id 0, response and authoritative, no questions, one answer
    r.extend_from_slice(&[0, 0, 0x84, 0, 0, 0, 0, 1, 0, 0, 0, 0]).ok()?;
    r.push(host.len() as u8).ok()?;
    r.extend_from_slice(host.as_bytes()).ok()?;
    r.extend_from_slice(&[5, b'l', b'o', b'c', b'a', b'l', 0]).ok()?;
    r.extend_from_slice(&TYPE_A.to_be_bytes()).ok()?;
    r.extend_from_slice(&(CLASS_IN | 0x8000).to_be_bytes()).ok()?; // cache flush
    r.extend_from_slice(&TTL_S.to_be_bytes()).ok()?;
    r.extend_from_slice(&[0, 4]).ok()?;
    r.extend_from_slice(&ip).ok()?;
    Some(r)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn query(name: &[&str], qtype: u16, class: u16) -> std::vec::Vec<u8> {
        let mut q = vec![0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0];
        for l in name {
            q.push(l.len() as u8);
            q.extend_from_slice(l.as_bytes());
        }
        q.push(0);
        q.extend_from_slice(&qtype.to_be_bytes());
        q.extend_from_slice(&class.to_be_bytes());
        q
    }

    const IP: [u8; 4] = [192, 168, 1, 42];

    #[test]
    fn our_name_gets_our_address() {
        let r = answer(&query(&["minimeater-a1b2c3", "local"], 1, 1), "minimeater-a1b2c3", IP).unwrap();
        assert_eq!(&r[..12], &[0, 0, 0x84, 0, 0, 0, 0, 1, 0, 0, 0, 0]);
        assert_eq!(&r[r.len() - 4..], &IP);
        assert_eq!(&r[r.len() - 6..r.len() - 4], &[0, 4]);
        assert_eq!(&r[13..30], b"minimeater-a1b2c3");
    }

    #[test]
    fn case_unicast_bit_and_any_are_accepted() {
        assert!(
            answer(
                &query(&["MiniMeater-A1B2C3", "LOCAL"], 255, 0x8001),
                "minimeater-a1b2c3",
                IP
            )
            .is_some()
        );
    }

    #[test]
    fn other_names_types_and_responses_are_ignored() {
        let h = "minimeater-a1b2c3";
        assert!(answer(&query(&["other", "local"], 1, 1), h, IP).is_none());
        assert!(answer(&query(&[h, "local"], 28, 1), h, IP).is_none(), "AAAA");
        assert!(answer(&query(&[h, "example"], 1, 1), h, IP).is_none());
        assert!(answer(&query(&["a", h, "local"], 1, 1), h, IP).is_none());
        let mut resp = query(&[h, "local"], 1, 1);
        resp[2] = 0x84;
        assert!(answer(&resp, h, IP).is_none(), "a response");
        assert!(answer(&[0; 5], h, IP).is_none());
        assert!(answer(&query(&[h, "local"], 1, 1)[..20], h, IP).is_none(), "cut off");
    }

    #[test]
    fn a_compressed_second_question_is_followed() {
        let h = "minimeater-a1b2c3";
        // question 1: another name; question 2: a pointer to the first question's "local"
        let mut q = vec![0, 0, 0, 0, 0, 2, 0, 0, 0, 0, 0, 0];
        q.extend_from_slice(&[
            5, b'o', b't', b'h', b'e', b'r', 5, b'l', b'o', b'c', b'a', b'l', 0, 0, 1, 0, 1,
        ]);
        q.push(h.len() as u8);
        q.extend_from_slice(h.as_bytes());
        q.extend_from_slice(&[0xC0, 18, 0, 1, 0, 1]); // offset 18 is "local" in the first name
        assert!(answer(&q, h, IP).is_some());
    }

    #[test]
    fn pointer_loops_do_not_hang() {
        let mut q = vec![0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0];
        q.extend_from_slice(&[0xC0, 12, 0, 1, 0, 1]);
        assert!(answer(&q, "x", IP).is_none());
    }
}
