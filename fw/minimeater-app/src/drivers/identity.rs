//! Provisioning the device identity on the ATECC608B (ADR 0005, `minimeat-seq.4.34`): the configuration the chip is
//! locked with, the order it is locked in, the key made inside it, and the signing request for the CA.
//!
//! **The locks are one way.** A chip locked with a wrong configuration is a chip thrown away, so everything here is
//! built to be looked at before it is done:
//!
//! - [`provision`] with `apply == false` changes nothing: it reads the chip, says what it would write and the CRC it
//!   would lock with, and stops.
//! - With `apply == true` it writes only the words that differ, reads the zone back and compares it with what it meant
//!   to write, and locks only if they are equal.
//! - It can be run again: a step already done is skipped, and a key that exists is never replaced. A chip locked with a
//!   configuration other than this one is refused (nothing is changed).
//!
//! The order is the chip's: write the configuration, lock it, generate the key (the key slot allows `GenKey`; it can
//! only be made once the configuration is locked), lock the data zone. The certificate slot stays writable in clear after
//! the lock, because the certificate comes back from the CA after the key exists.
//!
//! The configuration changes four fields of what the chip came with, which is left as it is otherwise:
//!
//! | | slot 0 (the key) | slot 8 (the certificate, 416 bytes) |
//! |-|-|-|
//! | `SlotConfig` | `0x2081`: external signing allowed, secret (no read), `GenKey` allowed | `0x0000`: clear read, clear write |
//! | `KeyConfig`  | `0x0033`: private, public key derivable, P-256 | `0x001C`: not an ECC key (data) |

use core::fmt::Write as _;

use embedded_hal_async::delay::DelayNs;
use embedded_hal_async::i2c::I2c;
use heapless::{String, Vec};

use super::atecc608::{Atecc608, CONFIG_LEN, Error, crc16};

/// The private key: ECDSA P-256, signs, never read.
pub const KEY_SLOT: u8 = 0;
/// The device certificate.
pub const CERT_SLOT: u8 = 8;

const SLOT_CONFIG: usize = 20;
const KEY_CONFIG: usize = 96;
const KEY_SLOT_CONFIG: u16 = 0x2081;
const KEY_SLOT_KEY_CONFIG: u16 = 0x0033;
const CERT_SLOT_CONFIG: u16 = 0x0000;
const CERT_SLOT_KEY_CONFIG: u16 = 0x001C;
/// `Info` answers `00 00 60 xx` on the ATECC608 family.
const FAMILY: u8 = 0x60;

/// The 128-byte configuration to lock: `current` (what the chip holds) with the two slots set up as above.
pub fn wanted(current: &[u8; CONFIG_LEN]) -> [u8; CONFIG_LEN] {
    let mut c = *current;
    let mut put = |at: usize, v: u16| c[at..at + 2].copy_from_slice(&v.to_le_bytes());
    put(SLOT_CONFIG + 2 * usize::from(KEY_SLOT), KEY_SLOT_CONFIG);
    put(SLOT_CONFIG + 2 * usize::from(CERT_SLOT), CERT_SLOT_CONFIG);
    put(KEY_CONFIG + 2 * usize::from(KEY_SLOT), KEY_SLOT_KEY_CONFIG);
    put(KEY_CONFIG + 2 * usize::from(CERT_SLOT), CERT_SLOT_KEY_CONFIG);
    c
}

/// The byte offsets of the 4-byte words that differ between two configurations.
pub fn changes(current: &[u8; CONFIG_LEN], wanted: &[u8; CONFIG_LEN]) -> Vec<usize, 8> {
    let mut out = Vec::new();
    for at in (0..CONFIG_LEN).step_by(4) {
        if current[at..at + 4] != wanted[at..at + 4] {
            let _ = out.push(at);
        }
    }
    out
}

/// True if `config` already has this firmware's slots (whatever its lock state).
pub fn is_ours(config: &[u8; CONFIG_LEN]) -> bool {
    changes(config, &wanted(config)).is_empty()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fault {
    Chip(Error),
    /// `Info` is not an ATECC608: nothing was written.
    WrongChip,
    /// The configuration is locked and is not this one: the chip cannot hold the identity.
    Different,
    /// Reading the configuration back found a difference at this byte offset: it was not locked.
    Verify(usize),
    /// A lock did not take.
    NotLocked,
}

impl From<Error> for Fault {
    fn from(e: Error) -> Self {
        Fault::Chip(e)
    }
}

async fn line(say: &mut impl AsyncFnMut(&str), args: core::fmt::Arguments<'_>) {
    let mut l: String<96> = String::new();
    let _ = l.write_fmt(args);
    say(&l).await;
}

/// 64 bytes as two lines of hex.
async fn hex64(say: &mut impl AsyncFnMut(&str), label: &str, b: &[u8; 64]) {
    for (n, half) in b.chunks(32).enumerate() {
        let mut l: String<96> = String::new();
        let _ = write!(l, "{} ", if n == 0 { label } else { "   " });
        for x in half {
            let _ = write!(l, "{x:02x}");
        }
        say(&l).await;
    }
}

/// Looks at the chip and, with `apply`, does what is left of: configuration, its lock, the key, the data lock. Reports
/// each step on `say`. Returns the device public key (X then Y) once the chip has one.
pub async fn provision<I: I2c, T: DelayNs>(
    chip: &mut Atecc608<I, T>,
    apply: bool,
    say: &mut impl AsyncFnMut(&str),
) -> Result<Option<[u8; 64]>, Fault> {
    let info = chip.info().await?;
    if info[2] != FAMILY {
        return Err(Fault::WrongChip);
    }
    let sn = chip.serial().await?;
    let mut l: String<96> = String::new();
    let _ = l.push_str("chip ");
    for x in sn {
        let _ = write!(l, "{x:02x}");
    }
    say(&l).await;
    let mut locks = chip.locks().await?;
    line(
        say,
        format_args!(
            "configuration {}, data {}",
            if locks.config { "locked" } else { "open" },
            if locks.data { "locked" } else { "open" }
        ),
    )
    .await;

    let current = chip.read_config().await?;
    let want = wanted(&current);
    if locks.config {
        if !is_ours(&current) {
            line(
                say,
                format_args!("locked with a configuration this firmware does not use"),
            )
            .await;
            return Err(Fault::Different);
        }
    } else {
        let todo = changes(&current, &want);
        let crc = crc16(&want);
        if todo.is_empty() {
            line(say, format_args!("configuration written; lock CRC {crc:04x}")).await;
        } else {
            line(
                say,
                format_args!("{} configuration words to write, lock CRC {crc:04x}", todo.len()),
            )
            .await;
        }
        if !apply {
            line(say, format_args!("not done: this is a look. Locks cannot be undone.")).await;
            line(
                say,
                format_args!("then: make the key in slot {KEY_SLOT}, lock the data zone"),
            )
            .await;
            return Ok(None);
        }
        for &at in &todo {
            let w = [want[at], want[at + 1], want[at + 2], want[at + 3]];
            chip.write_config(at, w).await?;
        }
        let back = chip.read_config().await?;
        if let Some(at) = (0..CONFIG_LEN).find(|&i| back[i] != want[i]) {
            line(say, format_args!("read back differs at byte {at}: not locking")).await;
            return Err(Fault::Verify(at));
        }
        line(
            say,
            format_args!("read back equals what was written; locking the configuration"),
        )
        .await;
        chip.lock_config(crc).await?;
        locks = chip.locks().await?;
        if !locks.config {
            return Err(Fault::NotLocked);
        }
        line(say, format_args!("configuration locked")).await;
    }

    if locks.data {
        let key = chip.public_key(KEY_SLOT).await?;
        hex64(say, "public key", &key).await;
        return Ok(Some(key));
    }
    if !apply {
        line(
            say,
            format_args!("not done: make the key in slot {KEY_SLOT}, lock the data zone"),
        )
        .await;
        return Ok(None);
    }
    let key = chip.generate_key(KEY_SLOT).await?;
    hex64(say, "public key", &key).await;
    chip.lock_data_unchecked().await?;
    if !chip.locks().await?.data {
        return Err(Fault::NotLocked);
    }
    line(say, format_args!("data zone locked; the key stays in the chip")).await;
    Ok(Some(key))
}

#[cfg(test)]
pub(crate) mod tests {
    extern crate std;
    use std::cell::RefCell;
    use std::collections::VecDeque;
    use std::string::String as Str;
    use std::vec::Vec as V;

    use embedded_hal_async::i2c::{self, ErrorKind, Operation};
    use p256::ecdsa::signature::hazmat::{PrehashSigner, PrehashVerifier};
    use p256::ecdsa::{Signature, SigningKey, VerifyingKey};

    use super::super::atecc608::ADDRESS;
    use super::super::csr::ToBeSigned;
    use super::*;
    use crate::drivers::mock::{NoDelay, block_on};

    /// What a blank chip holds, as far as this flow looks: its serial number and revision, the I2C address, slot
    /// configurations like the factory's (every slot a key slot, `GenKey` allowed), unlocked.
    pub(crate) fn blank() -> [u8; CONFIG_LEN] {
        let mut c = [0u8; CONFIG_LEN];
        c[0..4].copy_from_slice(&[0x01, 0x23, 0xAB, 0xCD]);
        c[4..8].copy_from_slice(&[0x00, 0x00, 0x60, 0x03]);
        c[8..13].copy_from_slice(&[1, 2, 3, 4, 0xEE]);
        c[14] = 0x01;
        c[16] = 0xC0;
        for s in 0..16 {
            c[20 + 2 * s..22 + 2 * s].copy_from_slice(&0x8720u16.to_le_bytes());
            c[96 + 2 * s..98 + 2 * s].copy_from_slice(&0x0033u16.to_le_bytes());
        }
        c[84..88].copy_from_slice(&[0, 0, 0x55, 0x55]);
        c[88..90].copy_from_slice(&[0xFF, 0xFF]);
        c
    }

    /// A chip that behaves as the datasheet says the real one does where this flow depends on it: writes only while the
    /// configuration is open, a lock checks its CRC, a key can be made once the configuration is locked and the slot allows it,
    /// signing needs a key and the slot's permission.
    pub(crate) struct Chip {
        pub(crate) config: [u8; CONFIG_LEN],
        key: Option<SigningKey>,
        keys_made: u32,
        tempkey: [u8; 32],
        out: VecDeque<u8>,
        /// Commands seen: opcode and first parameter.
        seen: V<(u8, u8)>,
        /// Silently drop every configuration write (a broken chip).
        drop_writes: bool,
        /// Data slot 8, which the configuration makes a clear read and write slot.
        cert: [u8; 416],
        cert_writes: u32,
        info: [u8; 4],
    }

    impl Chip {
        pub(crate) fn new(config: [u8; CONFIG_LEN]) -> Self {
            Self {
                config,
                key: None,
                keys_made: 0,
                tempkey: [0; 32],
                out: VecDeque::new(),
                seen: V::new(),
                drop_writes: false,
                cert: [0; 416],
                cert_writes: 0,
                info: [0, 0, 0x60, 0x03],
            }
        }
        pub(crate) fn cert_writes(&self) -> u32 {
            self.cert_writes
        }
        /// The slot and byte range a 32-byte data-zone address names; only the certificate slot is modelled.
        fn data_at(&self, p2: u16) -> Option<usize> {
            let (slot, block) = (usize::from(p2 >> 3) & 0xF, usize::from(p2 >> 8) & 0xF);
            // clear read and write: the slot's configuration is the certificate slot's, and the block is inside it
            (slot == 8 && self.slot_config(8) == 0 && (block + 1) * 32 <= self.cert.len()).then_some(block * 32)
        }
        fn config_locked(&self) -> bool {
            self.config[87] == 0
        }
        fn data_locked(&self) -> bool {
            self.config[86] == 0
        }
        fn slot_config(&self, slot: usize) -> u16 {
            u16::from_le_bytes([self.config[20 + 2 * slot], self.config[21 + 2 * slot]])
        }

        /// The response data for one command.
        fn run(&mut self, op: u8, p1: u8, p2: u16, data: &[u8]) -> V<u8> {
            self.seen.push((op, p1));
            match op {
                0x30 => self.info.to_vec(),
                0x02 if p1 == 0x82 => match self.data_at(p2) {
                    Some(at) => self.cert[at..at + 32].to_vec(),
                    None => vec![0x0F],
                },
                0x12 if p1 == 0x82 => match self.data_at(p2) {
                    Some(at) => {
                        self.cert[at..at + 32].copy_from_slice(data);
                        self.cert_writes += 1;
                        vec![0]
                    }
                    None => vec![0x0F],
                },
                0x02 => {
                    assert_eq!(p1, 0x80, "only 32-byte configuration reads");
                    let at = usize::from(p2) * 4; // a word address
                    self.config[at..at + 32].to_vec()
                }
                0x12 => {
                    assert_eq!(p1, 0x00, "only 4-byte configuration writes");
                    let at = usize::from(p2) * 4;
                    if self.config_locked() || at < 16 || at == 84 || at >= 128 {
                        return vec![0x0F];
                    }
                    if !self.drop_writes {
                        self.config[at..at + 4].copy_from_slice(data);
                    }
                    vec![0]
                }
                0x17 => {
                    let crc = p2;
                    match p1 {
                        0x00 if self.config_locked() => vec![0x0F],
                        0x00 if crc != crc16(&self.config) => vec![0x01],
                        0x00 => {
                            self.config[87] = 0;
                            vec![0]
                        }
                        0x81 if !self.config_locked() || self.data_locked() => vec![0x0F],
                        0x81 => {
                            self.config[86] = 0;
                            vec![0]
                        }
                        m => panic!("lock mode {m:#x} is not used"),
                    }
                }
                0x40 => {
                    let slot = p2 as usize;
                    let private = self.config[96 + 2 * slot] & 1 == 1;
                    let may_gen = self.slot_config(slot) & 0x2000 != 0;
                    match p1 {
                        0x04 if !self.config_locked() || !private || !may_gen => vec![0x0F],
                        0x04 => {
                            self.keys_made += 1;
                            let mut seed = [0x42u8; 32];
                            seed[0] = self.keys_made as u8;
                            let sk = SigningKey::from_slice(&seed).unwrap();
                            self.key = Some(sk);
                            self.public()
                        }
                        0x00 if self.key.is_none() => vec![0x0F],
                        0x00 => self.public(),
                        m => panic!("genkey mode {m:#x}"),
                    }
                }
                0x16 => {
                    assert_eq!(p1, 0x03);
                    self.tempkey.copy_from_slice(data);
                    vec![0]
                }
                0x41 => {
                    let slot = p2 as usize;
                    if p1 != 0x80 || self.key.is_none() || self.slot_config(slot) & 1 == 0 {
                        return vec![0x0F];
                    }
                    let sig: Signature = self.key.as_ref().unwrap().sign_prehash(&self.tempkey).unwrap();
                    sig.to_bytes().to_vec()
                }
                op => panic!("command {op:#x} is not used"),
            }
        }

        fn public(&self) -> V<u8> {
            self.key
                .as_ref()
                .unwrap()
                .verifying_key()
                .to_encoded_point(false)
                .as_bytes()[1..]
                .to_vec()
        }
    }

    fn framed(data: &[u8]) -> V<u8> {
        let mut f = vec![(data.len() + 3) as u8];
        f.extend_from_slice(data);
        f.extend_from_slice(&crc16(&f).to_le_bytes());
        f
    }

    #[derive(Debug)]
    pub(crate) struct Nak;
    impl i2c::Error for Nak {
        fn kind(&self) -> ErrorKind {
            ErrorKind::NoAcknowledge(i2c::NoAcknowledgeSource::Address)
        }
    }
    impl i2c::ErrorType for Chip {
        type Error = Nak;
    }
    impl i2c::I2c for Chip {
        async fn transaction(&mut self, address: u8, ops: &mut [Operation<'_>]) -> Result<(), Nak> {
            for op in ops {
                match op {
                    Operation::Write(_) if address == 0x00 => {
                        self.out = framed(&[0x11]).into();
                        return Err(Nak);
                    }
                    Operation::Write(b) => {
                        assert_eq!(address, ADDRESS);
                        if b[0] == 0x03 {
                            let n = usize::from(b[1]);
                            assert_eq!(crc16(&b[1..n - 1]).to_le_bytes(), [b[n - 1], b[n]], "command CRC");
                            let p2 = u16::from_le_bytes([b[4], b[5]]);
                            let data = b[6..n - 1].to_vec();
                            let r = self.run(b[2], b[3], p2, &data);
                            self.out = framed(&r).into();
                        }
                    }
                    Operation::Read(b) => {
                        for x in b.iter_mut() {
                            *x = self.out.pop_front().expect("read past the response");
                        }
                    }
                }
            }
            Ok(())
        }
    }

    fn run(chip: &mut Atecc608<Chip, NoDelay>, apply: bool) -> (Result<Option<[u8; 64]>, Fault>, V<Str>) {
        let lines = RefCell::new(V::new());
        let r = block_on(provision(chip, apply, &mut async |l: &str| {
            lines.borrow_mut().push(l.into())
        }));
        (r, lines.into_inner())
    }

    fn atecc(config: [u8; CONFIG_LEN]) -> Atecc608<Chip, NoDelay> {
        Atecc608::new(Chip::new(config), ADDRESS, NoDelay)
    }

    fn writes(a: &Atecc608<Chip, NoDelay>) -> usize {
        a.i2c()
            .seen
            .iter()
            .filter(|o| matches!(o, (0x12, _) | (0x17, _) | (0x40, 0x04)))
            .count()
    }

    #[test]
    fn the_wanted_configuration_changes_four_fields_and_nothing_fixed() {
        let c = blank();
        let w = wanted(&c);
        let diff: V<usize> = (0..CONFIG_LEN).filter(|&i| c[i] != w[i]).collect();
        // slot 0 and 8 in SlotConfig and KeyConfig: the bytes that are not already the same value
        assert!(
            diff.iter().all(|i| [20, 21, 36, 37, 96, 97, 112, 113].contains(i)),
            "{diff:?}"
        );
        assert_eq!(w[20..22], [0x81, 0x20]);
        assert_eq!(w[36..38], [0x00, 0x00]);
        assert_eq!(w[96..98], [0x33, 0x00]);
        assert_eq!(w[112..114], [0x1C, 0x00]);
        assert!(
            w[..16] == c[..16] && w[84..90] == c[84..90],
            "serial, revision, the lock bytes"
        );
        // every word it would write is one the chip accepts
        for at in changes(&c, &w) {
            assert!(at >= 16 && at != 84, "{at}");
        }
        assert!(is_ours(&w) && !is_ours(&c));
    }

    #[test]
    fn looking_changes_nothing() {
        let mut a = atecc(blank());
        let (r, lines) = run(&mut a, false);
        assert_eq!(r, Ok(None));
        assert_eq!(writes(&a), 0, "{:?}", a.i2c().seen);
        assert_eq!(a.i2c().config, blank());
        let all = lines.join("\n");
        assert!(
            all.contains("configuration open, data open") && all.contains("configuration words to write"),
            "{all}"
        );
        assert!(
            all.contains(&format!("lock CRC {:04x}", crc16(&wanted(&blank())))),
            "{all}"
        );
        assert!(all.contains("Locks cannot be undone"), "{all}");
    }

    #[test]
    fn provisioning_writes_locks_makes_the_key_and_locks_the_data() {
        let mut a = atecc(blank());
        let (r, lines) = run(&mut a, true);
        let key = r.unwrap().expect("a public key");
        let chip = a.i2c();
        assert!(chip.config_locked() && chip.data_locked());
        assert_eq!(
            chip.config[..86],
            wanted(&blank())[..86],
            "locked with exactly the wanted configuration"
        );
        assert_eq!(chip.keys_made, 1);
        assert_eq!(key.to_vec(), chip.public());
        // the order: writes, then the configuration lock, then the key, then the data lock
        let ops: V<(u8, u8)> = chip
            .seen
            .iter()
            .copied()
            .filter(|o| matches!(o, (0x12, _) | (0x17, _) | (0x40, 0x04)))
            .collect();
        let lock = ops.iter().position(|o| *o == (0x17, 0x00)).unwrap();
        // three words differ on a factory-like chip: slot 0's and slot 8's SlotConfig, slot 8's KeyConfig
        assert!(ops[..lock].iter().all(|o| o.0 == 0x12) && lock == 3, "{ops:?}");
        assert_eq!(ops[lock + 1..], [(0x40, 0x04), (0x17, 0x81)]);
        assert!(lines.join("\n").contains("data zone locked"));
    }

    #[test]
    fn running_it_again_changes_nothing_and_never_replaces_the_key() {
        let mut a = atecc(blank());
        let first = run(&mut a, true).0.unwrap().unwrap();
        let before = writes(&a);
        let (r, lines) = run(&mut a, true);
        assert_eq!(r, Ok(Some(first)));
        assert_eq!(writes(&a), before, "no write, lock or key generation the second time");
        assert_eq!(a.i2c().keys_made, 1);
        assert!(lines.join("\n").contains("configuration locked, data locked"));
    }

    #[test]
    fn a_run_that_stopped_part_way_picks_up_where_it_was() {
        // the configuration written but not locked (power lost): nothing to rewrite, then locks and goes on
        let mut a = atecc(wanted(&blank()));
        let (r, lines) = run(&mut a, true);
        assert!(r.unwrap().is_some());
        assert!(lines.join("\n").contains("configuration written"));
        assert!(!a.i2c().seen.iter().any(|o| o.0 == 0x12), "nothing rewritten");
        // locked, key not made yet
        let mut locked = Chip::new(wanted(&blank()));
        locked.config[87] = 0;
        let mut a = Atecc608::new(locked, ADDRESS, NoDelay);
        assert!(run(&mut a, true).0.unwrap().is_some());
        assert_eq!(a.i2c().keys_made, 1);
        assert!(a.i2c().data_locked());
    }

    #[test]
    fn a_look_at_a_locked_chip_without_a_key_makes_none() {
        let mut locked = Chip::new(wanted(&blank()));
        locked.config[87] = 0;
        let mut a = Atecc608::new(locked, ADDRESS, NoDelay);
        let (r, lines) = run(&mut a, false);
        assert_eq!(r, Ok(None));
        assert_eq!(writes(&a), 0);
        assert!(lines.join("\n").contains("not done: make the key"));
    }

    #[test]
    fn a_chip_locked_with_another_configuration_is_refused_untouched() {
        let mut other = Chip::new(blank());
        other.config[87] = 0;
        let mut a = Atecc608::new(other, ADDRESS, NoDelay);
        for apply in [false, true] {
            let (r, _) = run(&mut a, apply);
            assert_eq!(r, Err(Fault::Different));
        }
        assert_eq!(writes(&a), 0);
    }

    #[test]
    fn a_write_that_did_not_stick_is_never_locked() {
        let mut a = atecc(blank());
        a.i2c_mut().drop_writes = true;
        let (r, lines) = run(&mut a, true);
        assert!(matches!(r, Err(Fault::Verify(_))), "{r:?}");
        assert!(!a.i2c().seen.iter().any(|o| o.0 == 0x17), "no lock command");
        assert!(lines.join("\n").contains("not locking"));
    }

    #[test]
    fn something_that_is_not_an_atecc608_is_left_alone() {
        let mut a = atecc(blank());
        a.i2c_mut().info = [0, 0, 0x50, 0x01];
        assert_eq!(run(&mut a, true).0, Err(Fault::WrongChip));
        assert_eq!(writes(&a), 0);
    }

    #[test]
    fn the_chip_signs_a_request_that_verifies_with_its_public_key() {
        let mut a = atecc(blank());
        let key = run(&mut a, true).0.unwrap().unwrap();
        let tbs = ToBeSigned::new("mm-a1b2c3", &key).unwrap();
        let sig = block_on(a.sign(KEY_SLOT, &tbs.digest())).unwrap();
        let vk = VerifyingKey::from_sec1_bytes(&[&[4u8][..], &key].concat()).unwrap();
        vk.verify_prehash(&tbs.digest(), &Signature::from_slice(&sig).unwrap())
            .unwrap();
        // (csr.rs checks the DER around it against OpenSSL)
        assert_eq!(tbs.finish(&sig).unwrap()[0], 0x30);
    }
}
