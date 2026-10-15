//! A PKCS#10 certificate signing request for the device key (ADR 0005, enrolment): the subject CN, the P-256 public key
//! from the ATECC608B and the chip's own signature over the request, as DER and as PEM. A private CA (step-ca, or
//! `openssl ca` for five boards) turns it into the certificate Mosquitto checks.
//!
//! The request is built here and only the 32-byte digest goes to the chip (`Atecc608::sign`), so the key never leaves it.

use heapless::{String, Vec};
use sha2::{Digest, Sha256};

/// Longest subject common name.
pub const CN_MAX: usize = 48;
/// The request: about 190 bytes plus the name.
pub const CSR_MAX: usize = 320;

const SEQUENCE: u8 = 0x30;
const SET: u8 = 0x31;
const INTEGER: u8 = 0x02;
const BIT_STRING: u8 = 0x03;
const OID: u8 = 0x06;
const UTF8: u8 = 0x0C;
/// `[0]` constructed: the (empty) attributes.
const CONTEXT_0: u8 = 0xA0;

const OID_COMMON_NAME: &[u8] = &[0x55, 0x04, 0x03];
const OID_EC_PUBLIC_KEY: &[u8] = &[0x2A, 0x86, 0x48, 0xCE, 0x3D, 0x02, 0x01];
const OID_PRIME256V1: &[u8] = &[0x2A, 0x86, 0x48, 0xCE, 0x3D, 0x03, 0x01, 0x07];
const OID_ECDSA_SHA256: &[u8] = &[0x2A, 0x86, 0x48, 0xCE, 0x3D, 0x04, 0x03, 0x02];

type Der = Vec<u8, CSR_MAX>;

/// Appends tag, length and `content` to `out`; `None` when it does not fit.
fn tlv(out: &mut Der, tag: u8, content: &[u8]) -> Option<()> {
    out.push(tag).ok()?;
    match content.len() {
        n @ 0..=127 => out.push(n as u8).ok()?,
        n @ 128..=255 => out.extend_from_slice(&[0x81, n as u8]).ok()?,
        n @ 256..=65535 => out.extend_from_slice(&[0x82, (n >> 8) as u8, n as u8]).ok()?,
        _ => return None,
    }
    out.extend_from_slice(content).ok()
}

/// `tag` around the concatenation of `parts`.
fn wrap(tag: u8, parts: &[&[u8]]) -> Option<Der> {
    let mut inner = Der::new();
    for p in parts {
        inner.extend_from_slice(p).ok()?;
    }
    let mut out = Der::new();
    tlv(&mut out, tag, &inner)?;
    Some(out)
}

fn element(tag: u8, content: &[u8]) -> Option<Der> {
    let mut out = Der::new();
    tlv(&mut out, tag, content)?;
    Some(out)
}

/// A non-negative INTEGER from big-endian bytes: leading zeros dropped, one put back if the top bit is set.
fn integer(be: &[u8]) -> Option<Der> {
    let start = be.iter().position(|&b| b != 0).unwrap_or(be.len().saturating_sub(1));
    let digits = &be[start..];
    let mut v: Vec<u8, 34> = Vec::new();
    if digits.first().is_none_or(|&b| b & 0x80 != 0) {
        v.push(0).ok()?;
    }
    v.extend_from_slice(digits).ok()?;
    element(INTEGER, &v)
}

/// The signature the chip returns (r then s) as the DER `Ecdsa-Sig-Value` a certificate request carries.
fn ecdsa_sig(rs: &[u8; 64]) -> Option<Der> {
    let (r, s) = (integer(&rs[..32])?, integer(&rs[32..])?);
    wrap(SEQUENCE, &[&r, &s])
}

/// What the chip signs: the SHA-256 of the `CertificationRequestInfo`.
pub struct ToBeSigned {
    der: Der,
}

impl ToBeSigned {
    /// The request body for `cn` and the public key `x` then `y`.
    pub fn new(cn: &str, public_key: &[u8; 64]) -> Option<Self> {
        if cn.is_empty() || cn.len() > CN_MAX {
            return None;
        }
        let version = element(INTEGER, &[0])?;
        let atv = wrap(
            SEQUENCE,
            &[&element(OID, OID_COMMON_NAME)?, &element(UTF8, cn.as_bytes())?],
        )?;
        let name = wrap(SEQUENCE, &[&wrap(SET, &[&atv])?])?;
        let alg = wrap(
            SEQUENCE,
            &[&element(OID, OID_EC_PUBLIC_KEY)?, &element(OID, OID_PRIME256V1)?],
        )?;
        // BIT STRING: no unused bits, then the uncompressed point
        let mut point = [0u8; 66];
        point[1] = 0x04;
        point[2..].copy_from_slice(public_key);
        let spki = wrap(SEQUENCE, &[&alg, &element(BIT_STRING, &point)?])?;
        let attributes = element(CONTEXT_0, &[])?;
        Some(Self {
            der: wrap(SEQUENCE, &[&version, &name, &spki, &attributes])?,
        })
    }

    pub fn digest(&self) -> [u8; 32] {
        Sha256::digest(&self.der).into()
    }

    /// The whole request once the chip has signed [`digest`](Self::digest) (`signature` is its r then s).
    pub fn finish(self, signature: &[u8; 64]) -> Option<Vec<u8, CSR_MAX>> {
        let alg = wrap(SEQUENCE, &[&element(OID, OID_ECDSA_SHA256)?])?;
        let sig = ecdsa_sig(signature)?;
        let mut bits: Vec<u8, 80> = Vec::new();
        bits.push(0).ok()?;
        bits.extend_from_slice(&sig).ok()?;
        wrap(SEQUENCE, &[&self.der, &alg, &element(BIT_STRING, &bits)?])
    }
}

/// PEM lines for `der` between the `label` markers (RFC 7468, 64 characters a line), one `String` each, so they can be
/// sent as they are.
pub fn pem<const N: usize>(label: &str, der: &[u8]) -> Option<Vec<String<72>, N>> {
    let mut buf = [0u8; 2 * CSR_MAX];
    let text = pem_rfc7468::encode(label, pem_rfc7468::LineEnding::LF, der, &mut buf).ok()?;
    let mut lines: Vec<String<72>, N> = Vec::new();
    for l in text.lines() {
        lines.push(String::try_from(l).ok()?).ok()?;
    }
    Some(lines)
}

#[cfg(test)]
mod tests {
    extern crate std;
    use std::io::Write as _;
    use std::process::{Command, Stdio};
    use std::string::String as Str;
    use std::vec::Vec as V;

    use p256::ecdsa::signature::hazmat::PrehashSigner;
    use p256::ecdsa::signature::hazmat::PrehashVerifier;
    use p256::ecdsa::{Signature, SigningKey, VerifyingKey};

    use super::*;

    fn key() -> (SigningKey, [u8; 64]) {
        let sk = SigningKey::from_slice(&[0x42; 32]).unwrap();
        let point = sk.verifying_key().to_encoded_point(false);
        let mut xy = [0u8; 64];
        xy.copy_from_slice(&point.as_bytes()[1..]);
        (sk, xy)
    }

    /// The request as the chip would make it: SHA-256 of the body signed with the key, r then s.
    fn request(cn: &str, sk: &SigningKey, xy: &[u8; 64]) -> V<u8> {
        let tbs = ToBeSigned::new(cn, xy).unwrap();
        let sig: Signature = sk.sign_prehash(&tbs.digest()).unwrap();
        tbs.finish(&sig.to_bytes().into()).unwrap().to_vec()
    }

    /// One DER element: (tag, content, what follows).
    fn read(d: &[u8]) -> (u8, &[u8], &[u8]) {
        let (len, at) = match d[1] {
            n if n < 0x80 => (usize::from(n), 2),
            0x81 => (usize::from(d[2]), 3),
            0x82 => (usize::from(u16::from_be_bytes([d[2], d[3]])), 4),
            _ => panic!("length"),
        };
        (d[0], &d[at..at + len], &d[at + len..])
    }

    #[test]
    fn the_request_has_the_pkcs10_shape_and_a_signature_that_verifies() {
        let (sk, xy) = key();
        let der = request("mm-a1b2c3", &sk, &xy);
        let (tag, body, rest) = read(&der);
        assert_eq!((tag, rest.len()), (SEQUENCE, 0));
        // the three parts: the body that was signed, the algorithm, the signature
        let (_, info, after) = read(body);
        let info_len = body.len() - after.len();
        let (alg_tag, alg, after) = read(after);
        assert_eq!(alg_tag, SEQUENCE);
        assert_eq!(read(alg).1, OID_ECDSA_SHA256);
        let (bits_tag, bits, _) = read(after);
        assert_eq!((bits_tag, bits[0]), (BIT_STRING, 0));
        // the signature is a DER SEQUENCE of two INTEGERs
        let (_, rs, _) = read(&bits[1..]);
        let (t, r, rs) = read(rs);
        let (t2, s, _) = read(rs);
        assert_eq!((t, t2), (INTEGER, INTEGER));
        let fixed = |i: &[u8]| {
            let i = i.strip_prefix(&[0]).unwrap_or(i);
            let mut out = [0u8; 32];
            out[32 - i.len()..].copy_from_slice(i);
            out
        };
        let mut raw = [0u8; 64];
        raw[..32].copy_from_slice(&fixed(r));
        raw[32..].copy_from_slice(&fixed(s));
        let vk = VerifyingKey::from_sec1_bytes(&[&[4u8][..], &xy].concat()).unwrap();
        let tbs_der = &body[..info_len];
        vk.verify_prehash(&Sha256::digest(tbs_der), &Signature::from_slice(&raw).unwrap())
            .expect("the signature covers the request body");
        // the body names the subject and carries the key
        assert!(info.windows(9).any(|w| w == b"mm-a1b2c3"));
        assert!(info.windows(64).any(|w| w == xy));
    }

    #[test]
    fn integers_are_minimal_and_never_negative() {
        assert_eq!(integer(&[0, 0, 0x7F]).unwrap().as_slice(), [INTEGER, 1, 0x7F]);
        assert_eq!(integer(&[0, 0x80, 1]).unwrap().as_slice(), [INTEGER, 3, 0, 0x80, 1]);
        assert_eq!(integer(&[0, 0, 0]).unwrap().as_slice(), [INTEGER, 1, 0]);
    }

    #[test]
    fn names_that_do_not_fit_are_refused() {
        assert!(ToBeSigned::new("", &[0; 64]).is_none());
        assert!(ToBeSigned::new(&"a".repeat(CN_MAX + 1), &[0; 64]).is_none());
        assert!(ToBeSigned::new(&"a".repeat(CN_MAX), &[0xFF; 64]).is_some());
    }

    #[test]
    fn pem_is_base64_in_64_column_lines() {
        let lines = pem::<8>("TEST", b"Man").unwrap();
        assert_eq!(
            lines.iter().map(|l| l.as_str()).collect::<V<_>>(),
            ["-----BEGIN TEST-----", "TWFu", "-----END TEST-----"]
        );
        let two = pem::<8>("T", b"M").unwrap();
        assert_eq!(two[1].as_str(), "TQ==");
        let three = pem::<8>("T", b"Ma").unwrap();
        assert_eq!(three[1].as_str(), "TWE=");
        let long = pem::<8>("T", &[0u8; 100]).unwrap();
        // 100 bytes are 136 characters: two full lines and 8
        assert_eq!(long.len(), 5);
        assert_eq!((long[1].len(), long[2].len(), long[3].len()), (64, 64, 8));
    }

    /// An independent parser (RustCrypto's `x509-cert`) reads what was built: the name, the key, the algorithms.
    #[test]
    fn x509_cert_parses_the_request() {
        use x509_cert::der::{Decode, DecodePem, Encode};
        use x509_cert::request::CertReq;

        let (sk, xy) = key();
        let der = request("mm-a1b2c3", &sk, &xy);
        let req = CertReq::from_der(&der).expect("a PKCS#10 request");
        assert_eq!(req.info.subject.to_string(), "CN=mm-a1b2c3");
        assert_eq!(req.info.public_key.algorithm.oid.to_string(), "1.2.840.10045.2.1");
        assert_eq!(req.info.public_key.subject_public_key.raw_bytes()[1..], xy);
        assert_eq!(req.algorithm.oid.to_string(), "1.2.840.10045.4.3.2");
        assert_eq!(req.to_der().unwrap(), der, "re-encoding gives the same bytes");
        // and the PEM we print is the PEM it reads
        let mut text = Str::new();
        for l in pem::<16>("CERTIFICATE REQUEST", &der).unwrap() {
            text.push_str(&l);
            text.push('\n');
        }
        assert_eq!(CertReq::from_pem(&text).unwrap(), req);
    }

    /// OpenSSL, if it is installed, agrees: the request parses, its signature verifies and the subject is ours.
    #[test]
    fn openssl_accepts_the_request() {
        let Ok(mut child) = Command::new("openssl")
            .args(["req", "-verify", "-noout", "-subject"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
        else {
            return;
        };
        let (sk, xy) = key();
        let der = request("mm-a1b2c3", &sk, &xy);
        let mut text = Str::new();
        for l in pem::<16>("CERTIFICATE REQUEST", &der).unwrap() {
            text.push_str(&l);
            text.push('\n');
        }
        child.stdin.take().unwrap().write_all(text.as_bytes()).unwrap();
        let out = child.wait_with_output().unwrap();
        let (so, se) = (Str::from_utf8_lossy(&out.stdout), Str::from_utf8_lossy(&out.stderr));
        assert!(out.status.success(), "{:?} {so} {se}", out.status);
        assert!(so.contains("mm-a1b2c3"), "{so}");
        // (the verdict goes to stdout or stderr, by version)
        assert!(format!("{so}{se}").contains("verify OK"), "{so} {se}");
    }
}
