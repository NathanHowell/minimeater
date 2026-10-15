//! Microchip ATECC608B secure element (ADR 0005): the device's hardware key pair.
//!
//! The chip generates a P-256 private key that never leaves it and signs with it; the driver wakes
//! the chip, sends one command, reads the response and puts the chip back to sleep. Commands:
//! Info, Read (serial number and lock state), Random, GenKey (new key pair, or the public key of an
//! existing one) and Nonce + Sign (an ECDSA signature over a 32-byte digest).
//!
//! Wake: the chip wakes on SDA held low for at least 60 us. A write to address 0x00 holds it low
//! for eight bit times, which is 80 us at 100 kHz, so the I2C bus must run at 100 kHz or slower. The
//! chip NACKs that write; the error is expected.
//!
//! Not here: writing and locking the configuration. Provisioning does that once per chip and the lock
//! is one-way, so it gets its own deliberate flow (`minimeat-seq.5`, ADR 0005).

use embedded_hal_async::delay::DelayNs;
use embedded_hal_async::i2c::I2c;

/// The default 7-bit address (0xC0 as an 8-bit address).
pub const ADDRESS: u8 = 0x60;

/// Word address: the first byte of every write that is not a wake.
const WORD_SLEEP: u8 = 0x01;
const WORD_COMMAND: u8 = 0x03;

const OP_READ: u8 = 0x02;
const OP_WRITE: u8 = 0x12;
const OP_LOCK: u8 = 0x17;
const OP_NONCE: u8 = 0x16;
const OP_RANDOM: u8 = 0x1B;
const OP_INFO: u8 = 0x30;
const OP_GENKEY: u8 = 0x40;
const OP_SIGN: u8 = 0x41;

/// GenKey modes: 0x04 makes a new private key; 0x00 returns the public key of the one in the slot.
const GENKEY_CREATE: u8 = 0x04;
const GENKEY_PUBLIC: u8 = 0x00;
/// Nonce mode 3 loads the 32 bytes I send straight into TempKey; Sign mode 0x80 signs TempKey.
const NONCE_PASS_THROUGH: u8 = 0x03;
const SIGN_EXTERNAL: u8 = 0x80;
/// Read: configuration zone, 32 bytes.
const READ_CONFIG_32: u8 = 0x80;
/// Read or write 32 bytes of the data zone.
const DATA_32: u8 = 0x82;
/// Write: configuration zone, 4 bytes (the zone is bits 1:0 of the mode, 0 for configuration).
const WRITE_CONFIG_4: u8 = 0x00;
/// Lock modes with the zone's CRC checked: 0 the configuration zone, 1 the data and OTP zones together.
const LOCK_CONFIG: u8 = 0x00;
const LOCK_DATA: u8 = 0x01;
/// Added to a lock mode: the zone's CRC is not checked (the data zone holds private keys the host cannot read, so it
/// has no CRC to give).
const LOCK_NO_CRC: u8 = 0x80;
/// The configuration zone is 128 bytes; bytes 0-15 (serial number, revision) are fixed, and the lock bytes are 86 and 87.
pub const CONFIG_LEN: usize = 128;
const CONFIG_WRITABLE: core::ops::Range<usize> = 16..128;

/// A command that has not answered after this long has failed; the slowest (GenKey, Sign) take
/// under 120 ms.
const COMMAND_TIMEOUT_MS: u32 = 250;
/// The wake-up time after the low pulse (tWHI is 1.5 ms).
const WAKE_US: u32 = 1_600;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// The bus failed or the chip did not acknowledge.
    Bus,
    /// The chip stayed busy past the timeout.
    Timeout,
    /// A response failed its checksum, or had an impossible length.
    Crc,
    /// The chip answered with a non-zero status byte (0x01 miscompare, 0x03 parse error, 0x05 ECC fault,
    /// 0x0F execution error, 0xEE watchdog, 0xFF communication error).
    Status(u8),
}

/// Whether the configuration and data zones are locked. A blank chip is unlocked in both.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Locks {
    pub config: bool,
    pub data: bool,
}

/// CRC-16 with polynomial 0x8005, data bits taken least significant first, as the chip computes it.
pub fn crc16(data: &[u8]) -> u16 {
    let mut crc: u16 = 0;
    for &byte in data {
        for bit in 0..8 {
            let data_bit = (byte >> bit) & 1 == 1;
            let crc_bit = crc >> 15 == 1;
            crc <<= 1;
            if data_bit != crc_bit {
                crc ^= 0x8005;
            }
        }
    }
    crc
}

/// The address word of 32-byte block `block` in data `slot`: block in bits 11:8, slot in 6:3, word offset 0.
fn slot_address(slot: u8, block: usize) -> u16 {
    ((block as u16) << 8) | (u16::from(slot) << 3)
}

pub struct Atecc608<I, T> {
    i2c: I,
    address: u8,
    delay: T,
}

impl<I: I2c, T: DelayNs> Atecc608<I, T> {
    pub fn new(i2c: I, address: u8, delay: T) -> Self {
        Self { i2c, address, delay }
    }

    #[cfg(test)]
    pub(crate) fn i2c(&self) -> &I {
        &self.i2c
    }

    #[cfg(test)]
    pub(crate) fn i2c_mut(&mut self) -> &mut I {
        &mut self.i2c
    }

    /// Wakes the chip and checks its wake response (count 4, status 0x11, CRC).
    async fn wake(&mut self) -> Result<(), Error> {
        // NACKed by design: only the long low pulse matters
        let _ = self.i2c.write(0x00, &[0x00]).await;
        self.delay.delay_us(WAKE_US).await;
        let mut r = [0u8; 4];
        self.i2c.read(self.address, &mut r).await.map_err(|_| Error::Bus)?;
        if r[0] != 4 || crc16(&r[..2]) != u16::from_le_bytes([r[2], r[3]]) {
            return Err(Error::Crc);
        }
        match r[1] {
            0x11 => Ok(()),
            s => Err(Error::Status(s)),
        }
    }

    async fn sleep(&mut self) {
        let _ = self.i2c.write(self.address, &[WORD_SLEEP]).await;
    }

    /// Sends a command and reads the response data into `out`; returns its length. A response of
    /// four bytes carries only a status: zero is success (no data), anything else is an error.
    async fn command(&mut self, op: u8, p1: u8, p2: u16, data: &[u8], out: &mut [u8]) -> Result<usize, Error> {
        let mut pkt = [0u8; 2 + 5 + 32 + 2];
        let n = 7 + data.len(); // count byte through CRC
        assert!(n < pkt.len()); // the word address takes one more byte
        pkt[0] = WORD_COMMAND;
        pkt[1] = n as u8;
        pkt[2] = op;
        pkt[3] = p1;
        pkt[4..6].copy_from_slice(&p2.to_le_bytes());
        pkt[6..6 + data.len()].copy_from_slice(data);
        let crc = crc16(&pkt[1..6 + data.len()]).to_le_bytes();
        pkt[6 + data.len()..8 + data.len()].copy_from_slice(&crc);
        self.i2c
            .write(self.address, &pkt[..1 + n])
            .await
            .map_err(|_| Error::Bus)?;

        // The chip NACKs its address while it executes; ask for the count byte until it answers.
        let mut count = [0u8; 1];
        let mut waited = 0;
        while self.i2c.read(self.address, &mut count).await.is_err() {
            if waited >= COMMAND_TIMEOUT_MS {
                return Err(Error::Timeout);
            }
            self.delay.delay_ms(1).await;
            waited += 1;
        }
        let total = usize::from(count[0]);
        if !(4..=4 + out.len().max(1)).contains(&total) {
            return Err(Error::Crc);
        }
        let mut rest = [0u8; 4 + 64];
        self.i2c
            .read(self.address, &mut rest[..total - 1])
            .await
            .map_err(|_| Error::Bus)?;
        let mut frame = [0u8; 1 + 4 + 64];
        frame[0] = count[0];
        frame[1..total].copy_from_slice(&rest[..total - 1]);
        let crc = u16::from_le_bytes([frame[total - 2], frame[total - 1]]);
        if crc16(&frame[..total - 2]) != crc {
            return Err(Error::Crc);
        }
        let len = total - 3;
        if len == 1 {
            // a status-only response: zero is success
            return match frame[1] {
                0x00 => Ok(0),
                s => Err(Error::Status(s)),
            };
        }
        out[..len].copy_from_slice(&frame[1..1 + len]);
        Ok(len)
    }

    /// Wake, run `f`, sleep. The chip goes back to sleep on every path.
    async fn session<R>(&mut self, f: impl AsyncFnOnce(&mut Self) -> Result<R, Error>) -> Result<R, Error> {
        self.wake().await?;
        let r = f(self).await;
        self.sleep().await;
        r
    }

    /// The device revision (Info, mode 0): `00 00 60 xx` for the ATECC608.
    pub async fn info(&mut self) -> Result<[u8; 4], Error> {
        self.session(async |s| {
            let mut out = [0u8; 4];
            s.command(OP_INFO, 0, 0, &[], &mut out).await?;
            Ok(out)
        })
        .await
    }

    /// The chip's unique 9-byte serial number (it starts 0x01 0x23 and ends 0xEE).
    pub async fn serial(&mut self) -> Result<[u8; 9], Error> {
        let b = self.read_config_block(0).await?;
        let mut sn = [0u8; 9];
        sn[..4].copy_from_slice(&b[0..4]);
        sn[4..].copy_from_slice(&b[8..13]);
        Ok(sn)
    }

    /// Whether the chip's configuration and data zones are locked (0x00 is locked, 0x55 unlocked).
    pub async fn locks(&mut self) -> Result<Locks, Error> {
        let b = self.read_config_block(2).await?;
        Ok(Locks {
            data: b[22] == 0x00,
            config: b[23] == 0x00,
        })
    }

    async fn read_config_block(&mut self, block: u16) -> Result<[u8; 32], Error> {
        self.session(async |s| {
            let mut out = [0u8; 32];
            let n = s.command(OP_READ, READ_CONFIG_32, block << 3, &[], &mut out).await?;
            (n == 32).then_some(out).ok_or(Error::Crc)
        })
        .await
    }

    /// All 128 bytes of the configuration zone.
    pub async fn read_config(&mut self) -> Result<[u8; CONFIG_LEN], Error> {
        self.session(async |s| {
            let mut all = [0u8; CONFIG_LEN];
            for (block, chunk) in all.as_chunks_mut::<32>().0.iter_mut().enumerate() {
                let mut out = [0u8; 32];
                let n = s
                    .command(OP_READ, READ_CONFIG_32, (block as u16) << 3, &[], &mut out)
                    .await?;
                if n != 32 {
                    return Err(Error::Crc);
                }
                *chunk = out;
            }
            Ok(all)
        })
        .await
    }

    /// Writes four bytes of the configuration zone at byte `offset` (a multiple of 4, from 16 on, and not the word that
    /// holds the lock bytes). The chip refuses once the configuration is locked.
    pub async fn write_config(&mut self, offset: usize, word: [u8; 4]) -> Result<(), Error> {
        if !CONFIG_WRITABLE.contains(&offset) || !offset.is_multiple_of(4) || offset == 84 {
            return Err(Error::Status(0x03)); // as the chip would answer a bad address
        }
        self.session(async |s| {
            let mut none = [0u8; 0];
            s.command(OP_WRITE, WRITE_CONFIG_4, (offset / 4) as u16, &word, &mut none)
                .await
                .map(drop)
        })
        .await
    }

    /// Locks the configuration zone, one way: `crc` is the CRC-16 of the 128 bytes it must hold ([`crc16`]), and the chip
    /// refuses if its contents differ. Slot and key configuration cannot be changed afterwards.
    pub async fn lock_config(&mut self, crc: u16) -> Result<(), Error> {
        self.lock(LOCK_CONFIG, crc).await
    }

    /// Locks the data and OTP zones, one way, after the configuration is locked: `crc` is the CRC-16 of their contents
    /// (slots 0 to 15 in order, then OTP) as written. Keys made by GenKey before this stay; slots can no longer be written
    /// except as their configuration allows.
    pub async fn lock_data(&mut self, crc: u16) -> Result<(), Error> {
        self.lock(LOCK_DATA, crc).await
    }

    /// Locks the data and OTP zones, one way, without a CRC check: for a chip whose data zone holds a private key made
    /// by [`generate_key`](Self::generate_key), which the host cannot read to compute one. Needs the configuration
    /// locked first.
    pub async fn lock_data_unchecked(&mut self) -> Result<(), Error> {
        self.lock(LOCK_DATA | LOCK_NO_CRC, 0).await
    }

    async fn lock(&mut self, mode: u8, crc: u16) -> Result<(), Error> {
        self.session(async |s| {
            let mut none = [0u8; 0];
            s.command(OP_LOCK, mode, crc, &[], &mut none).await.map(drop)
        })
        .await
    }

    /// Reads `out.len()` bytes (a multiple of 32) from the start of data `slot`, in the clear: only a slot configured
    /// for clear reads answers (the certificate slot).
    pub async fn read_slot(&mut self, slot: u8, out: &mut [u8]) -> Result<(), Error> {
        self.session(async |s| {
            for (block, chunk) in out.as_chunks_mut::<32>().0.iter_mut().enumerate() {
                let mut got = [0u8; 32];
                let n = s
                    .command(OP_READ, DATA_32, slot_address(slot, block), &[], &mut got)
                    .await?;
                if n != 32 {
                    return Err(Error::Crc);
                }
                chunk.copy_from_slice(&got);
            }
            Ok(())
        })
        .await
    }

    /// Writes `data` (a multiple of 32 bytes) to the start of data `slot`, in the clear: only a slot configured for
    /// clear writes accepts it (the certificate slot, also once the zones are locked).
    pub async fn write_slot(&mut self, slot: u8, data: &[u8]) -> Result<(), Error> {
        self.session(async |s| {
            for (block, chunk) in data.as_chunks::<32>().0.iter().enumerate() {
                let mut none = [0u8; 0];
                s.command(OP_WRITE, DATA_32, slot_address(slot, block), chunk, &mut none)
                    .await?;
            }
            Ok(())
        })
        .await
    }

    /// 32 random bytes from the chip's generator.
    pub async fn random(&mut self) -> Result<[u8; 32], Error> {
        self.session(async |s| {
            let mut out = [0u8; 32];
            let n = s.command(OP_RANDOM, 0, 0, &[], &mut out).await?;
            (n == 32).then_some(out).ok_or(Error::Crc)
        })
        .await
    }

    /// Makes a new P-256 key pair in `slot`, replacing any key there, and returns the public key (X then Y).
    /// The slot's configuration must allow key generation.
    pub async fn generate_key(&mut self, slot: u8) -> Result<[u8; 64], Error> {
        self.genkey(GENKEY_CREATE, slot).await
    }

    /// The public key (X then Y) of the key pair in `slot`.
    pub async fn public_key(&mut self, slot: u8) -> Result<[u8; 64], Error> {
        self.genkey(GENKEY_PUBLIC, slot).await
    }

    async fn genkey(&mut self, mode: u8, slot: u8) -> Result<[u8; 64], Error> {
        self.session(async |s| {
            let mut out = [0u8; 64];
            let n = s.command(OP_GENKEY, mode, u16::from(slot), &[], &mut out).await?;
            (n == 64).then_some(out).ok_or(Error::Crc)
        })
        .await
    }

    /// An ECDSA signature (r then s) over a SHA-256 `digest`, made with the key in `slot`.
    pub async fn sign(&mut self, slot: u8, digest: &[u8; 32]) -> Result<[u8; 64], Error> {
        self.session(async |s| {
            let mut none = [0u8; 0];
            s.command(OP_NONCE, NONCE_PASS_THROUGH, 0, digest, &mut none).await?;
            let mut out = [0u8; 64];
            let n = s
                .command(OP_SIGN, SIGN_EXTERNAL, u16::from(slot), &[], &mut out)
                .await?;
            (n == 64).then_some(out).ok_or(Error::Crc)
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use std::collections::VecDeque;
    use std::vec;
    use std::vec::Vec;

    use embedded_hal_async::i2c::{self, ErrorKind, Operation};

    use super::*;
    use crate::drivers::mock::{NoDelay, block_on};

    /// A chip model: a wake pulse (write to 0x00) queues the wake response; each command write queues
    /// the next scripted response data, after `busy` NACKed reads.
    struct Chip {
        responses: VecDeque<Vec<u8>>,
        out: VecDeque<u8>,
        busy: u32,
        busy_reads: u32,
        commands: Vec<Vec<u8>>,
        sleeps: u32,
        wakes: u32,
        /// Flip a bit in each command response after it is framed, so its CRC is wrong.
        corrupt: bool,
    }

    impl Chip {
        fn new(responses: &[&[u8]]) -> Self {
            Self {
                responses: responses.iter().map(|r| r.to_vec()).collect(),
                out: VecDeque::new(),
                busy: 0,
                busy_reads: 0,
                commands: Vec::new(),
                sleeps: 0,
                wakes: 0,
                corrupt: false,
            }
        }
    }

    fn framed(data: &[u8]) -> Vec<u8> {
        let mut f = vec![(data.len() + 3) as u8];
        f.extend_from_slice(data);
        f.extend_from_slice(&crc16(&f).to_le_bytes());
        f
    }

    #[derive(Debug)]
    struct Nak;
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
                    Operation::Write(b) if address == 0x00 => {
                        let _ = b;
                        self.wakes += 1;
                        self.out = framed(&[0x11]).into();
                        return Err(Nak);
                    }
                    Operation::Write(b) => {
                        assert_eq!(address, ADDRESS);
                        match b[0] {
                            WORD_COMMAND => {
                                self.commands.push(b.to_vec());
                                let mut f = framed(&self.responses.pop_front().expect("unscripted command"));
                                if self.corrupt {
                                    f[1] ^= 0x01;
                                }
                                self.out = f.into();
                                self.busy_reads = self.busy;
                            }
                            WORD_SLEEP => self.sleeps += 1,
                            w => panic!("unknown word address {w:#x}"),
                        }
                    }
                    Operation::Read(b) => {
                        if address != ADDRESS {
                            return Err(Nak);
                        }
                        if self.busy_reads > 0 {
                            self.busy_reads -= 1;
                            return Err(Nak);
                        }
                        for x in b.iter_mut() {
                            *x = self.out.pop_front().expect("read past the response");
                        }
                    }
                }
            }
            Ok(())
        }
    }

    fn chip(responses: &[&[u8]]) -> Atecc608<Chip, NoDelay> {
        Atecc608::new(Chip::new(responses), ADDRESS, NoDelay)
    }

    #[test]
    fn crc_known_vectors() {
        // the chip's wake response and the Info command, as in Microchip's examples
        assert_eq!(crc16(&[0x04, 0x11]).to_le_bytes(), [0x33, 0x43]);
        assert_eq!(crc16(&[0x07, 0x30, 0x00, 0x00, 0x00]).to_le_bytes(), [0x03, 0x5D]);
    }

    #[test]
    fn info_sends_the_command_and_sleeps() {
        let mut a = chip(&[&[0x00, 0x00, 0x60, 0x03]]);
        assert_eq!(block_on(a.info()), Ok([0x00, 0x00, 0x60, 0x03]));
        assert_eq!(a.i2c.commands, [vec![0x03, 0x07, 0x30, 0x00, 0x00, 0x00, 0x03, 0x5D]]);
        assert_eq!((a.i2c.wakes, a.i2c.sleeps), (1, 1), "woken and put to sleep once");
    }

    #[test]
    fn serial_joins_the_two_config_fields() {
        let mut block = [0u8; 32];
        block[0..4].copy_from_slice(&[0x01, 0x23, 0xAA, 0xBB]);
        block[8..13].copy_from_slice(&[1, 2, 3, 4, 0xEE]);
        let mut a = chip(&[&block]);
        assert_eq!(block_on(a.serial()), Ok([0x01, 0x23, 0xAA, 0xBB, 1, 2, 3, 4, 0xEE]));
        // Read, 32 bytes of the configuration zone, block 0
        assert_eq!(a.i2c.commands[0][2..6], [OP_READ, 0x80, 0x00, 0x00]);
    }

    #[test]
    fn lock_state_reads_the_two_lock_bytes() {
        let mut blank = [0u8; 32];
        blank[22] = 0x55;
        blank[23] = 0x55;
        let mut a = chip(&[&blank]);
        assert_eq!(
            block_on(a.locks()),
            Ok(Locks {
                config: false,
                data: false
            })
        );
        assert_eq!(
            a.i2c.commands[0][2..6],
            [OP_READ, 0x80, 16, 0x00],
            "block 2 is word address 16"
        );

        let mut locked = [0u8; 32];
        locked[22] = 0x00;
        locked[23] = 0x00;
        let mut a = chip(&[&locked]);
        assert_eq!(
            block_on(a.locks()),
            Ok(Locks {
                config: true,
                data: true
            })
        );
    }

    #[test]
    fn generate_and_read_public_key() {
        let key: Vec<u8> = (0..64).collect();
        let mut a = chip(&[&key, &key]);
        let k = block_on(a.generate_key(2)).unwrap();
        assert_eq!(k.to_vec(), key);
        assert_eq!(a.i2c.commands[0][2..6], [OP_GENKEY, 0x04, 2, 0]);
        block_on(a.public_key(2)).unwrap();
        assert_eq!(a.i2c.commands[1][2..6], [OP_GENKEY, 0x00, 2, 0]);
    }

    #[test]
    fn sign_loads_the_digest_then_signs() {
        let sig: Vec<u8> = (100..164).collect();
        let mut a = chip(&[&[0x00], &sig]);
        let digest = [0xAB; 32];
        assert_eq!(block_on(a.sign(0, &digest)).unwrap().to_vec(), sig);
        let nonce = &a.i2c.commands[0];
        assert_eq!(nonce[1], 7 + 32);
        assert_eq!(nonce[2..6], [OP_NONCE, 0x03, 0, 0]);
        assert_eq!(nonce[6..38], digest);
        assert_eq!(a.i2c.commands[1][2..6], [OP_SIGN, 0x80, 0, 0]);
        assert_eq!(a.i2c.sleeps, 1, "one session for both commands");
    }

    #[test]
    fn config_is_read_in_four_blocks_in_one_session() {
        let blocks: Vec<Vec<u8>> = (0..4u8).map(|b| (0..32).map(|i| b * 32 + i).collect()).collect();
        let mut a = chip(&[&blocks[0], &blocks[1], &blocks[2], &blocks[3]]);
        let cfg = block_on(a.read_config()).unwrap();
        assert_eq!(cfg.to_vec(), (0..128).collect::<Vec<u8>>());
        let words: Vec<u8> = a.i2c.commands.iter().map(|c| c[4]).collect();
        assert_eq!(words, [0, 8, 16, 24], "block n is word address n << 3");
        assert!(a.i2c.commands.iter().all(|c| c[2..4] == [OP_READ, 0x80]));
        assert_eq!((a.i2c.wakes, a.i2c.sleeps), (1, 1));
    }

    #[test]
    fn config_words_go_to_their_word_address() {
        let mut a = chip(&[&[0x00], &[0x00]]);
        block_on(a.write_config(20, [0x8F, 0x20, 0xC4, 0x8F])).unwrap();
        block_on(a.write_config(96, [1, 2, 3, 4])).unwrap();
        let c = &a.i2c.commands[0];
        assert_eq!(c[1], 7 + 4);
        assert_eq!(c[2..6], [OP_WRITE, 0x00, 5, 0], "byte 20 is word 5");
        assert_eq!(c[6..10], [0x8F, 0x20, 0xC4, 0x8F]);
        assert_eq!(a.i2c.commands[1][4], 24, "byte 96 is word 24");
    }

    #[test]
    fn config_writes_refuse_the_fixed_and_lock_bytes_before_touching_the_bus() {
        let mut a = chip(&[]);
        for offset in [0, 12, 15, 18, 84, 128, 200] {
            assert!(block_on(a.write_config(offset, [0; 4])).is_err(), "{offset}");
        }
        assert_eq!(a.i2c.wakes, 0);
    }

    #[test]
    fn locks_carry_the_crc_of_what_they_lock() {
        let mut a = chip(&[&[0x00], &[0x00]]);
        block_on(a.lock_config(0xBEEF)).unwrap();
        block_on(a.lock_data(0x1234)).unwrap();
        assert_eq!(a.i2c.commands[0][2..6], [OP_LOCK, 0x00, 0xEF, 0xBE]);
        assert_eq!(a.i2c.commands[1][2..6], [OP_LOCK, 0x01, 0x34, 0x12]);
        // a chip that finds a different CRC says so with its miscompare status, and the lock stays off
        let mut b = chip(&[&[0x01]]);
        assert_eq!(block_on(b.lock_config(0)), Err(Error::Status(0x01)));
    }

    #[test]
    fn busy_chip_is_polled_then_answers() {
        let mut a = chip(&[&[0x00, 0x00, 0x60, 0x03]]);
        a.i2c.busy = 5;
        assert!(block_on(a.info()).is_ok());
    }

    #[test]
    fn chip_that_never_answers_times_out_and_sleeps() {
        let mut a = chip(&[&[0x00, 0x00, 0x60, 0x03]]);
        a.i2c.busy = u32::MAX;
        assert_eq!(block_on(a.info()), Err(Error::Timeout));
        assert_eq!(a.i2c.sleeps, 1);
    }

    #[test]
    fn status_byte_becomes_an_error() {
        let mut a = chip(&[&[0x0F]]);
        assert_eq!(block_on(a.random()), Err(Error::Status(0x0F)));
    }

    #[test]
    fn corrupt_response_is_rejected() {
        let mut a = chip(&[&[0x00, 0x00, 0x60, 0x03]]);
        a.i2c.corrupt = true;
        assert_eq!(block_on(a.info()), Err(Error::Crc));
        assert_eq!(a.i2c.sleeps, 1);
    }

    #[test]
    fn absent_chip_is_a_bus_error() {
        struct Dead;
        impl i2c::ErrorType for Dead {
            type Error = Nak;
        }
        impl i2c::I2c for Dead {
            async fn transaction(&mut self, _: u8, _: &mut [Operation<'_>]) -> Result<(), Nak> {
                Err(Nak)
            }
        }
        let mut a = Atecc608::new(Dead, ADDRESS, NoDelay);
        assert_eq!(block_on(a.info()), Err(Error::Bus));
    }
}
