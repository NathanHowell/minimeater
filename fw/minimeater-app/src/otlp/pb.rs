//! A write-only protobuf encoder into a slice, enough for OTLP's metrics messages. Nested messages
//! get a 3-byte padded length (protobuf allows non-minimal varints, and every decoder takes them),
//! so nothing is moved once written.

pub struct Pb<'a> {
    b: &'a mut [u8],
    n: usize,
    ok: bool,
}

const VARINT: u32 = 0;
const FIXED64: u32 = 1;
const LEN: u32 = 2;

impl<'a> Pb<'a> {
    pub fn new(b: &'a mut [u8]) -> Self {
        Self { b, n: 0, ok: true }
    }

    /// The bytes written, or `None` if they did not fit.
    pub fn finish(self) -> Option<usize> {
        self.ok.then_some(self.n)
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

    fn varint(&mut self, mut v: u64) {
        loop {
            let byte = (v & 0x7F) as u8;
            v >>= 7;
            if v == 0 {
                self.raw(&[byte]);
                return;
            }
            self.raw(&[byte | 0x80]);
        }
    }

    fn tag(&mut self, field: u32, wire: u32) {
        self.varint(u64::from(field << 3 | wire));
    }

    pub fn string(&mut self, field: u32, s: &str) {
        self.tag(field, LEN);
        self.varint(s.len() as u64);
        self.raw(s.as_bytes());
    }

    pub fn int(&mut self, field: u32, v: i64) {
        self.tag(field, VARINT);
        self.varint(v as u64);
    }

    pub fn bool(&mut self, field: u32, v: bool) {
        self.int(field, i64::from(v));
    }

    pub fn fixed64(&mut self, field: u32, v: u64) {
        self.tag(field, FIXED64);
        self.raw(&v.to_le_bytes());
    }

    pub fn double(&mut self, field: u32, v: f64) {
        self.fixed64(field, v.to_bits());
    }

    /// A nested message written by `f`.
    pub fn message(&mut self, field: u32, f: impl FnOnce(&mut Pb)) {
        self.tag(field, LEN);
        let at = self.n;
        self.raw(&[0; 3]);
        f(self);
        let len = self.n - at - 3;
        if len >= 1 << 21 {
            self.ok = false;
            return;
        }
        if self.ok {
            self.b[at] = (len & 0x7F) as u8 | 0x80;
            self.b[at + 1] = ((len >> 7) & 0x7F) as u8 | 0x80;
            self.b[at + 2] = (len >> 14) as u8;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scalars_follow_the_wire_format() {
        let mut b = [0u8; 64];
        let mut p = Pb::new(&mut b);
        p.int(1, 150);
        p.string(2, "hi");
        p.double(3, 1.0);
        p.bool(4, true);
        let n = p.finish().unwrap();
        assert_eq!(
            b[..n],
            [
                0x08, 0x96, 0x01, 0x12, 2, b'h', b'i', 0x19, 0, 0, 0, 0, 0, 0, 0xF0, 0x3F, 0x20, 1
            ]
        );
    }

    #[test]
    fn nested_messages_carry_padded_lengths() {
        let mut b = [0u8; 64];
        let mut p = Pb::new(&mut b);
        p.message(1, |p| {
            p.string(1, "a");
            p.message(2, |p| p.int(1, 1));
        });
        let n = p.finish().unwrap();
        // inner: tag 0x12, len 3 padded (0x83 0x80 0x00)... the outer holds 'a' (3 bytes) and the inner (1 + 3 + 2)
        assert_eq!(
            b[..n],
            [0x0A, 0x89, 0x80, 0x00, 0x0A, 1, b'a', 0x12, 0x82, 0x80, 0x00, 0x08, 1]
        );
    }

    #[test]
    fn overflow_is_reported() {
        let mut b = [0u8; 4];
        let mut p = Pb::new(&mut b);
        p.string(1, "too long for this");
        assert_eq!(p.finish(), None);
        let mut b = [0u8; 4];
        let mut p = Pb::new(&mut b);
        p.message(1, |p| p.string(1, "x"));
        assert_eq!(p.finish(), None);
    }
}
