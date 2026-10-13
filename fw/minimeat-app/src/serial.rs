//! What arrives on the one USB serial port, as one `winnow` grammar: an Improv frame (`improv::frame`) or a
//! keystroke for the REPL's editor. Both kinds start with whatever byte the host sends first, so a byte that could
//! be the start of a frame waits until the grammar can say which it is; that is all the "hold the echo" logic there
//! is, and it holds for at most the marker's six bytes.
//!
//! [`Demux`] buffers bytes and runs the grammar on them; the application reads [`Item`]s off it. A frame that is
//! damaged, or not ours, is taken whole and not reported, so none of its bytes ever reach the editor.

use heapless::Vec;
use winnow::combinator::{alt, peek};
use winnow::stream::{Partial, Stream};
use winnow::token::take_while;
use winnow::{ModalResult, Parser as _};

use crate::improv::{self, Input, Rpc, byte};
use crate::repl::Key;

/// One thing read from the port.
#[derive(Clone, Debug, PartialEq)]
pub enum Item {
    Frame(Rpc),
    Key(Key),
}

/// A byte as a key, when it is not the start of an escape sequence.
fn plain(b: u8) -> Key {
    match b {
        b'\r' => Key::Cr,
        b'\n' => Key::Lf,
        0x7f | 0x08 => Key::Backspace,
        b'\t' => Key::Tab,
        0x03 => Key::CtrlC,
        0x15 => Key::CtrlU,
        0x17 => Key::CtrlW,
        0x20..=0x7e => Key::Char(b),
        _ => Key::Bad,
    }
}

/// A keystroke: one byte, or `ESC [ parameters final` for the arrow keys. `ESC` and then anything but `[` is
/// ignored (the next byte is read as itself).
fn key(input: &mut Input<'_>) -> ModalResult<Key> {
    let b = byte(input)?;
    if b != 0x1b {
        return Ok(plain(b));
    }
    if peek(byte).parse_next(input)? != b'[' {
        return Ok(Key::Ignore);
    }
    byte(input)?;
    take_while(0.., 0x30..=0x3f).parse_next(input)?;
    Ok(match byte(input)? {
        b'A' => Key::Up,
        b'B' => Key::Down,
        _ => Key::Ignore,
    })
}

/// A frame (`None`: one that was taken but is not for us) or a keystroke.
fn item(input: &mut Input<'_>) -> ModalResult<Option<Item>> {
    alt((
        improv::frame.map(|f| f.map(Item::Frame)),
        key.map(|k| Some(Item::Key(k))),
    ))
    .parse_next(input)
}

/// Room for the longest frame and a little more.
const BUFFER: usize = 128;

/// Buffers the port's bytes and cuts them into [`Item`]s.
#[derive(Default)]
pub struct Demux {
    buf: Vec<u8, BUFFER>,
    /// A frame was just taken: the newline that ends it on the wire is not a keystroke.
    after_frame: bool,
}

impl Demux {
    pub fn new() -> Self {
        Self::default()
    }

    /// Takes a byte from the port; read what it completes with [`Demux::pop`] until that returns `None`.
    pub fn push(&mut self, b: u8) {
        if self.buf.push(b).is_err() {
            // never reached with the buffer drained after each byte; start again rather than stall
            self.buf.clear();
            let _ = self.buf.push(b);
        }
    }

    /// The next complete item, or `None` when the buffer holds only the start of one.
    pub fn pop(&mut self) -> Option<Item> {
        loop {
            if self.buf.is_empty() {
                return None;
            }
            let total = self.buf.len();
            let (found, used) = {
                let mut input = Partial::new(&self.buf[..]);
                let found = item(&mut input);
                (found, total - input.eof_offset())
            };
            match found {
                Ok(item) => {
                    self.take(used);
                    // a frame ends in a newline, which must not reach the editor as Enter
                    let was_frame = !matches!(item, Some(Item::Key(_)));
                    if matches!(item, Some(Item::Key(Key::Lf))) && self.after_frame {
                        self.after_frame = false;
                        continue;
                    }
                    self.after_frame = was_frame;
                    if item.is_some() {
                        return item;
                    }
                }
                Err(winnow::error::ErrMode::Incomplete(_)) => return None,
                // not reachable: a key always parses; drop the byte rather than loop
                Err(_) => self.take(1),
            }
        }
    }

    /// Whether bytes are waiting for the rest of an item.
    pub fn pending(&self) -> bool {
        !self.buf.is_empty()
    }

    /// The host has gone quiet with an item half sent: what was waiting is released. The start of a frame is
    /// dropped (nobody finishes it); the keystrokes that only looked like one are returned, so what the user
    /// typed is not lost.
    pub fn flush(&mut self) -> Vec<Key, 8> {
        let mut keys = Vec::new();
        if !self.buf.starts_with(b"IMPROV") && self.buf.first() != Some(&0x1b) {
            for &b in &self.buf {
                let _ = keys.push(plain(b));
            }
        }
        self.buf.clear();
        keys
    }

    fn take(&mut self, n: usize) {
        let n = n.min(self.buf.len());
        self.buf.copy_within(n.., 0);
        self.buf.truncate(self.buf.len() - n);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all(d: &mut Demux, bytes: &[u8]) -> std::vec::Vec<Item> {
        let mut out = std::vec::Vec::new();
        for &b in bytes {
            d.push(b);
            while let Some(i) = d.pop() {
                out.push(i);
            }
        }
        out
    }

    fn chars(s: &str) -> std::vec::Vec<Item> {
        s.bytes().map(|b| Item::Key(Key::Char(b))).collect()
    }

    #[test]
    fn keys_come_out_one_each_and_arrow_keys_are_one_key() {
        let mut d = Demux::new();
        let got = all(&mut d, b"ab\r\x7f\t\x03\x15\x17\x1b[A\x1b[B\x1b[1;5C\xe9\x01");
        assert_eq!(
            got,
            [
                Item::Key(Key::Char(b'a')),
                Item::Key(Key::Char(b'b')),
                Item::Key(Key::Cr),
                Item::Key(Key::Backspace),
                Item::Key(Key::Tab),
                Item::Key(Key::CtrlC),
                Item::Key(Key::CtrlU),
                Item::Key(Key::CtrlW),
                Item::Key(Key::Up),
                Item::Key(Key::Down),
                Item::Key(Key::Ignore),
                Item::Key(Key::Bad),
                Item::Key(Key::Bad),
            ]
        );
    }

    #[test]
    fn a_lone_escape_waits_and_escape_then_a_letter_is_ignored_then_the_letter() {
        let mut d = Demux::new();
        assert!(all(&mut d, b"\x1b").is_empty() && d.pending());
        assert_eq!(all(&mut d, b"x"), [Item::Key(Key::Ignore), Item::Key(Key::Char(b'x'))]);
    }

    #[test]
    fn what_could_be_the_marker_waits_and_is_released_when_it_is_not() {
        let mut d = Demux::new();
        assert!(all(&mut d, b"IMPR").is_empty() && d.pending());
        assert_eq!(all(&mut d, b"x"), chars("IMPRx"));
        assert!(!d.pending());
    }

    #[test]
    fn a_frame_is_one_item_and_no_key_reaches_the_editor() {
        let mut d = Demux::new();
        let f = frame_bytes(3, &[0x02, 0x00]);
        let mut stream = std::vec::Vec::from(&b"ab"[..]);
        stream.extend_from_slice(&f);
        stream.extend_from_slice(b"c\r");
        let got = all(&mut d, &stream);
        assert_eq!(
            got,
            [
                Item::Key(Key::Char(b'a')),
                Item::Key(Key::Char(b'b')),
                Item::Frame(Rpc::State),
                Item::Key(Key::Char(b'c')),
                Item::Key(Key::Cr),
            ]
        );
    }

    #[test]
    fn newline_bytes_inside_a_frame_do_not_end_it() {
        // a length of 10 and a checksum of 0x0A, and printable bytes in the data
        let mut d = Demux::new();
        let mut f = std::vec::Vec::from(&b"IMPROV\x01\x03\x0c"[..]);
        f.extend_from_slice(&[0x99, 0x0a]); // an unknown command, 10 bytes of data
        f.extend_from_slice(b" ab cd efg");
        let c = f.iter().fold(0u8, |a, b| a.wrapping_add(*b));
        f.push(c);
        f.push(b'\n');
        assert_eq!(all(&mut d, &f), [Item::Frame(Rpc::Unknown)]);
    }

    #[test]
    fn damaged_or_foreign_frames_are_taken_whole_and_not_reported() {
        let mut d = Demux::new();
        // a bad checksum, a state packet (not an RPC), then a good one
        let mut bad = frame_bytes(3, &[0x02, 0x00]);
        let n = bad.len();
        bad[n - 2] ^= 0xFF; // the checksum, before the newline
        let mut stream = bad;
        stream.extend_from_slice(&frame_bytes(1, &[0x02]));
        stream.extend_from_slice(&frame_bytes(3, &[0x03, 0x00]));
        assert_eq!(all(&mut d, &stream), [Item::Frame(Rpc::Info)]);
    }

    #[test]
    fn a_wrong_version_or_a_length_over_the_limit_leaves_the_rest_as_keys() {
        let mut d = Demux::new();
        assert_eq!(all(&mut d, b"IMPROV\x07ok"), chars("ok"));
        let mut d = Demux::new();
        let got = all(&mut d, b"IMPROV\x01\x03\xF0z");
        assert_eq!(got, chars("z"));
    }

    #[test]
    fn a_newline_that_is_not_after_a_frame_is_a_key() {
        let mut d = Demux::new();
        assert_eq!(all(&mut d, b"a\n"), [Item::Key(Key::Char(b'a')), Item::Key(Key::Lf)]);
    }

    #[test]
    fn frames_back_to_back_and_one_byte_at_a_time() {
        let mut d = Demux::new();
        let mut stream = frame_bytes(3, &[0x02, 0x00]);
        stream.extend_from_slice(&frame_bytes(3, &[0x04, 0x00]));
        assert_eq!(all(&mut d, &stream), [Item::Frame(Rpc::State), Item::Frame(Rpc::Scan)]);
    }

    #[test]
    fn flush_releases_typed_text_and_drops_half_a_frame() {
        let mut d = Demux::new();
        all(&mut d, b"IMP");
        assert_eq!(&d.flush()[..], &[Key::Char(b'I'), Key::Char(b'M'), Key::Char(b'P')]);
        assert!(!d.pending());
        all(&mut d, b"IMPROV\x01\x03");
        assert!(d.flush().is_empty() && !d.pending());
        all(&mut d, b"\x1b[");
        assert!(d.flush().is_empty());
    }

    /// An RPC-style frame with `data`, checksummed, newline-terminated.
    fn frame_bytes(ty: u8, data: &[u8]) -> std::vec::Vec<u8> {
        let mut f = std::vec::Vec::from(&b"IMPROV\x01"[..]);
        f.push(ty);
        f.push(data.len() as u8);
        f.extend_from_slice(data);
        let c = f.iter().fold(0u8, |a, b| a.wrapping_add(*b));
        f.push(c);
        f.push(b'\n');
        f
    }
}
