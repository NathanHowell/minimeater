//! A small JSON writer: objects, arrays, strings and numbers into any [`core::fmt::Write`] (a
//! `heapless::String` on the device). Write errors (a full buffer) are remembered and reported by
//! [`Json::finish`], so a writer function does not need `?` on every field.

use core::fmt::Write;

const MAX_DEPTH: usize = 8;

pub struct Json<W: Write> {
    w: W,
    /// Per nesting level: has an element been written (so the next needs a comma).
    wrote: [bool; MAX_DEPTH],
    depth: usize,
    ok: bool,
}

impl<W: Write> Json<W> {
    pub fn new(w: W) -> Self {
        Self {
            w,
            wrote: [false; MAX_DEPTH],
            depth: 0,
            ok: true,
        }
    }

    /// The writer, or `Err` if anything did not fit or a container was left open.
    pub fn finish(self) -> Result<W, core::fmt::Error> {
        if self.ok && self.depth == 0 {
            Ok(self.w)
        } else {
            Err(core::fmt::Error)
        }
    }

    fn put(&mut self, s: &str) {
        self.ok &= self.w.write_str(s).is_ok();
    }

    fn fmt(&mut self, args: core::fmt::Arguments) {
        self.ok &= self.w.write_fmt(args).is_ok();
    }

    /// The comma before the next element of the current container.
    fn sep(&mut self) {
        if self.wrote[self.depth] {
            self.put(",");
        }
        self.wrote[self.depth] = true;
    }

    fn open(&mut self, c: &str) {
        self.sep();
        self.put(c);
        if self.depth + 1 < MAX_DEPTH {
            self.depth += 1;
            self.wrote[self.depth] = false;
        } else {
            self.ok = false;
        }
    }

    fn close(&mut self, c: &str) {
        self.depth = self.depth.saturating_sub(1);
        self.put(c);
    }

    pub fn begin_obj(&mut self) -> &mut Self {
        self.open("{");
        self
    }

    pub fn end_obj(&mut self) -> &mut Self {
        self.close("}");
        self
    }

    pub fn begin_arr(&mut self) -> &mut Self {
        self.open("[");
        self
    }

    pub fn end_arr(&mut self) -> &mut Self {
        self.close("]");
        self
    }

    /// `"key":`, to be followed by one value.
    pub fn key(&mut self, k: &str) -> &mut Self {
        self.sep();
        self.string_raw(k);
        self.put(":");
        // the value that follows is not a new element of this object
        self.wrote[self.depth] = false;
        self
    }

    fn string_raw(&mut self, s: &str) {
        self.put("\"");
        for c in s.chars() {
            match c {
                '"' => self.put("\\\""),
                '\\' => self.put("\\\\"),
                '\n' => self.put("\\n"),
                '\r' => self.put("\\r"),
                '\t' => self.put("\\t"),
                c if (c as u32) < 0x20 => self.fmt(format_args!("\\u{:04x}", c as u32)),
                c => {
                    let mut b = [0u8; 4];
                    self.put(c.encode_utf8(&mut b));
                }
            }
        }
        self.put("\"");
    }

    // ---- values (as an array element, or after `key`)

    pub fn str(&mut self, s: &str) -> &mut Self {
        self.sep_value();
        self.string_raw(s);
        self
    }

    /// A number with `decimals` places; NaN and infinities are `null`.
    pub fn num(&mut self, v: f64, decimals: usize) -> &mut Self {
        self.sep_value();
        if v.is_finite() {
            self.fmt(format_args!("{v:.decimals$}"));
        } else {
            self.put("null");
        }
        self
    }

    /// A number in the shortest exponent form (`7.3431e-4`), for Steinhart-Hart coefficients.
    pub fn sci(&mut self, v: f64) -> &mut Self {
        self.sep_value();
        if v.is_finite() {
            self.fmt(format_args!("{v:e}"));
        } else {
            self.put("null");
        }
        self
    }

    pub fn int(&mut self, v: i64) -> &mut Self {
        self.sep_value();
        self.fmt(format_args!("{v}"));
        self
    }

    pub fn bool(&mut self, v: bool) -> &mut Self {
        self.sep_value();
        self.put(if v { "true" } else { "false" });
        self
    }

    pub fn null(&mut self) -> &mut Self {
        self.sep_value();
        self.put("null");
        self
    }

    pub fn opt_num(&mut self, v: Option<f64>, decimals: usize) -> &mut Self {
        match v {
            Some(v) => self.num(v, decimals),
            None => self.null(),
        }
    }

    pub fn opt_str(&mut self, v: Option<&str>) -> &mut Self {
        match v {
            Some(v) => self.str(v),
            None => self.null(),
        }
    }

    /// A value is an array element (needs a comma) unless it follows a key.
    fn sep_value(&mut self) {
        // `key` leaves `wrote` false, so the first value after it adds no comma and sets the flag; a
        // second value in an object without a key would be a caller bug. Array elements work as is.
        self.sep();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use heapless::String;

    fn json(f: impl FnOnce(&mut Json<String<256>>)) -> std::string::String {
        let mut j = Json::new(String::new());
        f(&mut j);
        j.finish().unwrap().as_str().into()
    }

    #[test]
    fn objects_arrays_and_commas() {
        let out = json(|j| {
            j.begin_obj();
            j.key("v").int(1);
            j.key("a").begin_arr();
            j.int(1).num(2.0, 1).null();
            j.begin_obj();
            j.key("x").bool(true);
            j.end_obj();
            j.end_arr();
            j.key("o").begin_obj().end_obj();
            j.key("s").str("hi");
            j.end_obj();
        });
        assert_eq!(out, r#"{"v":1,"a":[1,2.0,null,{"x":true}],"o":{},"s":"hi"}"#);
        serde_json::from_str::<serde_json::Value>(&out).expect("valid JSON");
    }

    #[test]
    fn numbers_precision_and_nonfinite() {
        let out = json(|j| {
            j.begin_arr();
            j.num(224.64, 1)
                .num(0.8249, 2)
                .num(f64::NAN, 1)
                .num(f64::INFINITY, 1)
                .num(-0.04, 1);
            j.sci(7.3431e-4).sci(9.51568577e-8).sci(2.0);
            j.opt_num(None, 1).opt_num(Some(1.2345), 3);
            j.end_arr();
        });
        assert_eq!(
            out,
            "[224.6,0.82,null,null,-0.0,7.3431e-4,9.51568577e-8,2e0,null,1.234]"
        );
    }

    #[test]
    fn strings_are_escaped() {
        let out = json(|j| {
            j.begin_arr();
            j.str("a\"b\\c\nd\u{1}é🔥");
            j.end_arr();
        });
        assert_eq!(out, "[\"a\\\"b\\\\c\\nd\\u0001é🔥\"]");
    }

    #[test]
    fn a_full_buffer_is_an_error_not_a_panic() {
        let mut j = Json::new(String::<8>::new());
        j.begin_obj().key("a long key").str("and a long value").end_obj();
        assert!(j.finish().is_err());
        // and an unclosed container too
        let mut j = Json::new(String::<8>::new());
        j.begin_obj();
        assert!(j.finish().is_err());
    }
}
