//! The device certificate in the secure element's certificate slot (ADR 0005, `minimeat-84q`): the X.509 certificate the
//! private CA made from the signing request ([`super::csr`]), kept in data slot 8 so the board can present it in the TLS
//! handshake and the OIDC assertion.
//!
//! The slot holds 416 bytes in the clear (`SlotConfig` 0x0000, see [`super::identity`]): a two-byte big-endian length, the
//! DER certificate, then zeros. So a certificate can be at most [`DER_MAX`] = 414 bytes, which a private CA's certificate
//! for a P-256 key fits if it carries no more than a subject name, validity, key and the two or three usual extensions
//! (`docs/provisioning.md` has an `openssl` line that does that).
//!
//! The slot is not secret and not tamper-proof: anyone who can talk to the chip can write it, and it is the CA's
//! signature, not the slot, that makes the certificate worth anything. What [`store`] checks is that the bytes are a
//! DER `SEQUENCE` of the length they claim and that they carry *this chip's* public key, so a certificate for another
//! board, or a PEM file pasted by mistake, is refused before it overwrites a good one.

use embedded_hal_async::delay::DelayNs;
use embedded_hal_async::i2c::I2c;
use heapless::Vec;

use super::atecc608::{Atecc608, Error};
use super::identity::{CERT_SLOT, KEY_SLOT};

/// Bytes in the certificate slot.
pub const SLOT_LEN: usize = 416;
/// Longest certificate that fits (the slot less its length).
pub const DER_MAX: usize = SLOT_LEN - 2;

pub type Der = Vec<u8, DER_MAX>;

#[derive(Debug, PartialEq, Eq)]
pub enum Fault {
    Chip(Error),
    /// More than [`DER_MAX`] bytes.
    TooLong,
    /// Not a DER `SEQUENCE` of the length it says.
    NotCertificate,
    /// A certificate, but not for the key in this chip.
    WrongKey,
    /// The slot did not read back as it was written.
    Differs,
    /// No key has been made in the chip yet.
    NoKey,
}

impl From<Error> for Fault {
    fn from(e: Error) -> Self {
        Fault::Chip(e)
    }
}

/// The length of the outer DER element: header bytes and content, which must be all of `der`.
fn outer_len_matches(der: &[u8]) -> bool {
    let total = match der {
        [0x30, l, ..] if *l < 0x80 => 2 + usize::from(*l),
        [0x30, 0x81, l, ..] => 3 + usize::from(*l),
        [0x30, 0x82, h, l, ..] => 4 + usize::from(u16::from_be_bytes([*h, *l])),
        _ => return false,
    };
    total == der.len()
}

/// Whether `der` is a DER `SEQUENCE` of exactly the length it says: the shape of a certificate, and nothing more.
pub fn looks_like_certificate(der: &[u8]) -> bool {
    outer_len_matches(der)
}

/// What the slot holds for `der`: the length, the certificate, zeros. `None` if it does not fit.
pub fn pack(der: &[u8]) -> Option<[u8; SLOT_LEN]> {
    if der.len() > DER_MAX {
        return None;
    }
    let mut slot = [0u8; SLOT_LEN];
    slot[..2].copy_from_slice(&(der.len() as u16).to_be_bytes());
    slot[2..2 + der.len()].copy_from_slice(der);
    Some(slot)
}

/// The certificate in what the slot holds, or `None` for a blank slot or anything that is not one.
pub fn unpack(slot: &[u8; SLOT_LEN]) -> Option<&[u8]> {
    let len = usize::from(u16::from_be_bytes([slot[0], slot[1]]));
    let der = slot.get(2..2 + len)?;
    outer_len_matches(der).then_some(der)
}

/// Whether `der` is shaped like a certificate and carries `key` (the chip's public key, X then Y) as an uncompressed
/// point, which is how a certificate's `SubjectPublicKeyInfo` holds a P-256 key.
pub fn check(der: &[u8], key: &[u8; 64]) -> Result<(), Fault> {
    if der.len() > DER_MAX {
        return Err(Fault::TooLong);
    }
    if !outer_len_matches(der) {
        return Err(Fault::NotCertificate);
    }
    let mut point = [0u8; 65];
    point[0] = 0x04;
    point[1..].copy_from_slice(key);
    if der.windows(point.len()).any(|w| w == point) {
        Ok(())
    } else {
        Err(Fault::WrongKey)
    }
}

/// Checks `der` against the key in the chip, writes it to the certificate slot and reads it back.
pub async fn store<I: I2c, T: DelayNs>(chip: &mut Atecc608<I, T>, der: &[u8]) -> Result<(), Fault> {
    let key = chip.public_key(KEY_SLOT).await.map_err(|_| Fault::NoKey)?;
    check(der, &key)?;
    let slot = pack(der).ok_or(Fault::TooLong)?;
    chip.write_slot(CERT_SLOT, &slot).await?;
    let mut back = [0u8; SLOT_LEN];
    chip.read_slot(CERT_SLOT, &mut back).await?;
    if back == slot { Ok(()) } else { Err(Fault::Differs) }
}

/// The stored certificate, if there is one.
pub async fn load<I: I2c, T: DelayNs>(chip: &mut Atecc608<I, T>) -> Result<Option<Der>, Error> {
    let mut slot = [0u8; SLOT_LEN];
    chip.read_slot(CERT_SLOT, &mut slot).await?;
    Ok(unpack(&slot).map(|d| Der::from_slice(d).unwrap_or_default()))
}

#[cfg(test)]
pub(crate) mod tests {
    extern crate std;
    use std::io::Write as _;
    use std::process::{Command, Stdio};
    use std::string::String as Str;
    use std::vec::Vec as V;

    use super::super::atecc608::ADDRESS;
    use super::super::csr::{self, ToBeSigned};
    use super::super::identity::tests::{Chip, blank};
    use super::super::identity::{self, provision};
    use super::*;
    use crate::drivers::mock::{NoDelay, block_on};

    /// A certificate-shaped blob: a SEQUENCE of the right length around `key`.
    pub(crate) fn fake(key: &[u8; 64], pad: usize) -> V<u8> {
        let mut body = V::new();
        body.extend_from_slice(&vec![0xA5; pad]);
        body.push(0x04);
        body.extend_from_slice(key);
        let mut der = V::new();
        der.push(0x30);
        match body.len() {
            n if n < 0x80 => der.push(n as u8),
            n if n < 0x100 => der.extend_from_slice(&[0x81, n as u8]),
            n => {
                der.push(0x82);
                der.extend_from_slice(&(n as u16).to_be_bytes());
            }
        }
        der.extend_from_slice(&body);
        der
    }

    fn provisioned() -> (Atecc608<Chip, NoDelay>, [u8; 64]) {
        let mut chip = Atecc608::new(Chip::new(blank()), ADDRESS, NoDelay);
        let key = block_on(provision(&mut chip, true, &mut async |_: &str| {}))
            .unwrap()
            .unwrap();
        (chip, key)
    }

    #[test]
    fn a_certificate_is_packed_with_its_length_and_comes_back_the_same() {
        let key = [7u8; 64];
        for pad in [0, 20, 100, 300] {
            let der = fake(&key, pad);
            let slot = pack(&der).unwrap();
            assert_eq!(usize::from(u16::from_be_bytes([slot[0], slot[1]])), der.len());
            assert_eq!(unpack(&slot), Some(&der[..]));
        }
        // blank or garbage slots hold no certificate
        assert_eq!(unpack(&[0u8; SLOT_LEN]), None);
        assert_eq!(unpack(&[0xFF; SLOT_LEN]), None);
        let mut s = pack(&fake(&key, 20)).unwrap();
        s[2] = 0x31; // not a SEQUENCE
        assert_eq!(unpack(&s), None);
    }

    #[test]
    fn the_biggest_certificate_fits_and_one_byte_more_does_not() {
        let key = [7u8; 64];
        // a body of 410 bytes under a 4-byte header is 414
        let mut der = fake(&key, 410 - 65);
        assert_eq!(der.len(), DER_MAX);
        assert!(pack(&der).is_some() && check(&der, &key).is_ok());
        der.push(0);
        assert!(pack(&der).is_none());
        assert_eq!(check(&der, &key), Err(Fault::TooLong));
    }

    #[test]
    fn check_refuses_what_is_not_this_chips_certificate() {
        let key = [7u8; 64];
        let good = fake(&key, 40);
        assert_eq!(check(&good, &key), Ok(()));
        // another board's key
        assert_eq!(check(&good, &[8u8; 64]), Err(Fault::WrongKey));
        // a PEM file pasted as it is
        assert_eq!(
            check(b"-----BEGIN CERTIFICATE-----\nMIIB", &key),
            Err(Fault::NotCertificate)
        );
        // cut short, or with something after it
        assert_eq!(check(&good[..good.len() - 1], &key), Err(Fault::NotCertificate));
        let mut long = good.clone();
        long.push(0);
        assert_eq!(check(&long, &key), Err(Fault::NotCertificate));
        assert_eq!(check(&[], &key), Err(Fault::NotCertificate));
    }

    #[test]
    fn store_writes_and_reads_back_after_the_zones_are_locked() {
        let (mut chip, key) = provisioned();
        assert!(chip.i2c().config[86] == 0 && chip.i2c().config[87] == 0, "locked");
        assert_eq!(block_on(load(&mut chip)), Ok(None), "blank to start with");
        let der = fake(&key, 150);
        assert_eq!(block_on(store(&mut chip, &der)), Ok(()));
        assert_eq!(block_on(load(&mut chip)).unwrap().as_deref(), Some(&der[..]));
        // a second certificate replaces it, and a refused one leaves it alone
        let other = fake(&key, 60);
        assert_eq!(block_on(store(&mut chip, &other)), Ok(()));
        assert_eq!(block_on(store(&mut chip, &fake(&[1u8; 64], 60))), Err(Fault::WrongKey));
        assert_eq!(block_on(load(&mut chip)).unwrap().as_deref(), Some(&other[..]));
    }

    #[test]
    fn store_needs_a_key_in_the_chip() {
        let mut chip = Atecc608::new(Chip::new(blank()), ADDRESS, NoDelay);
        assert_eq!(block_on(store(&mut chip, &fake(&[7u8; 64], 40))), Err(Fault::NoKey));
        assert_eq!(chip.i2c().cert_writes(), 0);
    }

    #[test]
    fn the_slot_is_not_used_for_anything_else() {
        let (mut chip, key) = provisioned();
        block_on(store(&mut chip, &fake(&key, 40))).unwrap();
        // the key slot is not readable as data
        let mut out = [0u8; 32];
        assert!(block_on(chip.read_slot(identity::KEY_SLOT, &mut out)).is_err());
    }

    /// The whole of it against OpenSSL: a CA signs the request the chip signed, the certificate goes into the slot and
    /// comes back, and `openssl verify` accepts the chain. Skipped where there is no openssl.
    #[test]
    fn a_certificate_from_a_real_ca_stores_and_verifies() {
        let Ok(dir) = std::env::var("CARGO_TARGET_TMPDIR").or_else(|_| {
            let d = std::env::temp_dir().join(std::format!("mm-cert-{}", std::process::id()));
            std::fs::create_dir_all(&d).map(|_| d.to_string_lossy().into_owned())
        }) else {
            return;
        };
        let sh = |args: &[&str], input: Option<&[u8]>| -> Option<V<u8>> {
            let mut c = Command::new("openssl")
                .args(args)
                .current_dir(&dir)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
                .ok()?;
            if let Some(i) = input {
                c.stdin.take()?.write_all(i).ok()?;
            }
            let o = c.wait_with_output().ok()?;
            o.status.success().then_some(o.stdout)
        };
        if sh(&["version"], None).is_none() {
            return;
        }
        // the CA
        sh(
            &["ecparam", "-name", "prime256v1", "-genkey", "-noout", "-out", "ca.key"],
            None,
        )
        .unwrap();
        sh(
            &[
                "req",
                "-x509",
                "-new",
                "-key",
                "ca.key",
                "-subj",
                "/CN=mm test ca",
                "-days",
                "3650",
                "-out",
                "ca.pem",
            ],
            None,
        )
        .unwrap();

        // the chip's request
        let (mut chip, key) = provisioned();
        let tbs = ToBeSigned::new("mm-a1b2c3", &key).unwrap();
        let sig = block_on(chip.sign(identity::KEY_SLOT, &tbs.digest())).unwrap();
        let request = tbs.finish(&sig).unwrap();
        let lines = csr::pem::<12>("CERTIFICATE REQUEST", &request).unwrap();
        let mut text = Str::new();
        for l in &lines {
            text.push_str(l);
            text.push('\n');
        }
        std::fs::write(std::path::Path::new(&dir).join("dev.csr"), &text).unwrap();

        // a certificate as small as the docs say to make it
        let der = sh(
            &[
                "x509",
                "-req",
                "-in",
                "dev.csr",
                "-CA",
                "ca.pem",
                "-CAkey",
                "ca.key",
                "-CAcreateserial",
                "-days",
                "3650",
                "-outform",
                "DER",
            ],
            None,
        )
        .unwrap();
        assert!(der.len() <= DER_MAX, "{} bytes", der.len());
        assert_eq!(block_on(store(&mut chip, &der)), Ok(()));
        let back = block_on(load(&mut chip)).unwrap().unwrap();
        assert_eq!(&back[..], &der[..]);

        // and the chain verifies from what came back out of the chip
        let pem = csr::pem::<12>("CERTIFICATE", &back).unwrap();
        let mut text = Str::new();
        for l in &pem {
            text.push_str(l);
            text.push('\n');
        }
        std::fs::write(std::path::Path::new(&dir).join("dev.pem"), &text).unwrap();
        let out = sh(&["verify", "-CAfile", "ca.pem", "dev.pem"], None).expect("openssl verify");
        assert!(Str::from_utf8_lossy(&out).contains("OK"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
