//! SNTP client packets (RFC 4330), for the wall clock the OTLP exporter timestamps with (`.4.10`). The socket is the
//! board's; this is the 48-byte request and the check of the answer.

const LEN: usize = 48;
/// Seconds from 1900-01-01 (the NTP epoch) to 1970-01-01.
const UNIX_OFFSET: u64 = 2_208_988_800;

/// A client request (version 4, mode 3). `nonce` goes in the transmit timestamp, which the server echoes back as the
/// originate timestamp: an answer that does not carry it is stale or forged.
pub fn request(nonce: u64) -> [u8; LEN] {
    let mut p = [0u8; LEN];
    p[0] = 0x23;
    p[40..48].copy_from_slice(&nonce.to_be_bytes());
    p
}

/// The server's time as Unix milliseconds, or `None` for anything that is not an answer to `nonce`: a short packet,
/// the wrong mode, a kiss-o'-death (stratum 0) or an unsynchronised server (leap indicator 3), a different originate
/// timestamp, or a zero transmit time.
pub fn parse(resp: &[u8], nonce: u64) -> Option<u64> {
    if resp.len() < LEN {
        return None;
    }
    let (li, mode, stratum) = (resp[0] >> 6, resp[0] & 7, resp[1]);
    if mode != 4 || stratum == 0 || stratum > 15 || li == 3 {
        return None;
    }
    if resp[24..32] != nonce.to_be_bytes() {
        return None;
    }
    let secs = u64::from(u32::from_be_bytes(resp[40..44].try_into().ok()?));
    let frac = u64::from(u32::from_be_bytes(resp[44..48].try_into().ok()?));
    if secs == 0 {
        return None;
    }
    // NTP era 0 ends in 2036; a time that reads as before 1970 is in era 1
    let secs = if secs >= UNIX_OFFSET { secs - UNIX_OFFSET } else { secs + (1 << 32) - UNIX_OFFSET };
    Some(secs * 1000 + ((frac * 1000) >> 32))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn answer(nonce: u64, secs: u32, frac: u32) -> [u8; LEN] {
        let mut p = [0u8; LEN];
        p[0] = 0x24; // LI 0, version 4, mode 4
        p[1] = 2;
        p[24..32].copy_from_slice(&nonce.to_be_bytes());
        p[40..44].copy_from_slice(&secs.to_be_bytes());
        p[44..48].copy_from_slice(&frac.to_be_bytes());
        p
    }

    #[test]
    fn the_request_is_a_version_4_client_packet() {
        let r = request(0x0123_4567_89AB_CDEF);
        assert_eq!(r[0], 0x23);
        assert_eq!(&r[40..48], &[0x01, 0x23, 0x45, 0x67, 0x89, 0xAB, 0xCD, 0xEF]);
        assert!(r[1..40].iter().all(|b| *b == 0));
    }

    #[test]
    fn an_answer_gives_unix_milliseconds() {
        // 2025-01-01T00:00:00Z is 1735689600 s Unix; half a second on
        let ntp = (1_735_689_600u64 + UNIX_OFFSET) as u32;
        assert_eq!(parse(&answer(7, ntp, 1 << 31), 7), Some(1_735_689_600_500));
    }

    #[test]
    fn answers_that_are_not_ours_are_refused() {
        let ntp = (1_735_689_600u64 + UNIX_OFFSET) as u32;
        let good = answer(7, ntp, 0);
        assert!(parse(&good, 8).is_none(), "another nonce");
        assert!(parse(&good[..47], 7).is_none(), "short");
        let mut p = good;
        p[0] = 0x23;
        assert!(parse(&p, 7).is_none(), "a client packet");
        let mut p = good;
        p[1] = 0;
        assert!(parse(&p, 7).is_none(), "kiss-o'-death");
        let mut p = good;
        p[0] |= 0xC0;
        assert!(parse(&p, 7).is_none(), "unsynchronised");
        assert!(parse(&answer(7, 0, 0), 7).is_none(), "no time");
    }

    #[test]
    fn times_after_2036_read_as_era_one() {
        // NTP seconds 0x0000_0100 is 256 s into era 1: 2036-02-07T06:28:16Z + 256 s
        let want = ((1u64 << 32) - UNIX_OFFSET + 256) * 1000;
        assert_eq!(parse(&answer(1, 256, 0), 1), Some(want));
    }
}
