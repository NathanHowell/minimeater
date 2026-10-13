//! The serial REPL (`minimeat-s7v`): how credentials and settings that must not cross the network are set, over the
//! USB-C cable. It shares the port with Improv Wi-Fi frames and the log lines (`improv::run` feeds it every byte), so
//! the [`Editor`] keeps out of Improv's way: a line that starts `IMPROV` is a frame and is dropped, and the echo of a
//! line that could still become one is held back until it cannot.
//!
//! [`CATALOG`] is the one list of commands: completion and `help` read it, and [`parse`] accepts the same names.
//! Everything here but [`execute`] is plain code with host tests; [`execute`] talks to the control task over the
//! [`Bus`] the way Improv does, so a setting goes through the same save-and-publish path as any other.

use core::fmt::{self, Write as _};

use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::pubsub::Subscriber;
use embedded_io_async::Write;
use heapless::{String, Vec};

use crate::config::{Mode, Mqtt, Otlp, Settings, Wifi};
use crate::net::Identity;
use crate::pit::{Command, FaultCode, SP_MAX_F, SP_MIN_F};
use crate::proto::Request;
use crate::tasks::{Bus, READERS, Reply};

pub const PROMPT: &str = "minimeat> ";
/// Longest line, characters.
pub const LINE: usize = 160;
/// The id this conversation's requests carry on the bus (Improv's is 0xFF10).
const REQUEST_ID: u16 = 0xFF11;
const FRAME_START: &str = "IMPROV";
/// Lines kept for the up arrow.
const HISTORY: usize = 4;
const BEL: u8 = 0x07;

pub type Replies<'a> = Subscriber<'a, CriticalSectionRawMutex, Reply, 8, READERS, 1>;

// ----------------------------------------------------------------------------- catalog

pub struct Spec {
    pub name: &'static str,
    pub usage: &'static str,
    pub help: &'static str,
    /// The words that may follow the name, for completion.
    pub subs: &'static [&'static str],
}

pub const CATALOG: &[Spec] = &[
    Spec {
        name: "help",
        usage: "help [command]",
        help: "list the commands, or show one's usage",
        subs: &[],
    },
    Spec {
        name: "status",
        usage: "status",
        help: "mode, setpoint, temperatures, outputs, faults",
        subs: &[],
    },
    Spec {
        name: "config",
        usage: "config",
        help: "the saved settings (passwords hidden)",
        subs: &[],
    },
    Spec {
        name: "wifi",
        usage: "wifi set <ssid> [password] | wifi clear",
        help: "the network to join (open networks: no password)",
        subs: &["set", "clear"],
    },
    Spec {
        name: "mqtt",
        usage: "mqtt set <host> [port] [user] [password] | mqtt clear",
        help: "the MQTT broker (port 1883 unless given)",
        subs: &["set", "clear"],
    },
    Spec {
        name: "otlp",
        usage: "otlp set <host> [port] [interval_s] | otlp clear",
        help: "the OpenTelemetry collector (port 4318, every 60 s unless given)",
        subs: &["set", "clear"],
    },
    Spec {
        name: "setpoint",
        usage: "setpoint <F>",
        help: "the pit setpoint, 150 to 325 F",
        subs: &[],
    },
    Spec {
        name: "identify",
        usage: "identify",
        help: "blink the LED for 10 s",
        subs: &[],
    },
    Spec {
        name: "reboot",
        usage: "reboot",
        help: "restart the board (a cook resumes)",
        subs: &[],
    },
];

fn spec(name: &str) -> Option<&'static Spec> {
    CATALOG.iter().find(|s| s.name == name)
}

/// Typical values for an argument: a Tab there offers them (mqtt's port, otlp's port and interval, the setpoint).
fn values(words: &[&str], index: usize) -> &'static [&'static str] {
    match (words.first().copied(), index) {
        (Some("setpoint"), 1) => &["200", "225", "250", "275", "300"],
        (Some("mqtt"), 3) if words.get(1) == Some(&"set") => &["1883", "8883"],
        (Some("otlp"), 3) if words.get(1) == Some(&"set") => &["4318", "4317"],
        (Some("otlp"), 4) if words.get(1) == Some(&"set") => &["30", "60", "300"],
        _ => &[],
    }
}

/// What could complete the word being typed: the command names for the first word, a command's own words for the
/// second (`help` takes command names).
pub fn candidates(line: &str) -> Vec<&'static str, 12> {
    let mut out = Vec::new();
    let ends_in_space = line.ends_with(' ');
    let typed: Vec<&str, 8> = line.split_ascii_whitespace().take(8).collect();
    let first = typed.first().copied();
    let nwords = line.split_ascii_whitespace().count();
    let (index, partial) = if ends_in_space || nwords == 0 {
        (nwords, "")
    } else {
        (nwords - 1, line.split_ascii_whitespace().last().unwrap_or(""))
    };
    match index {
        0 => {
            for s in CATALOG {
                if s.name.starts_with(partial) {
                    let _ = out.push(s.name);
                }
            }
        }
        1 => match first {
            Some("help") => {
                for s in CATALOG {
                    if s.name.starts_with(partial) {
                        let _ = out.push(s.name);
                    }
                }
            }
            Some(c) => {
                for w in spec(c).map_or(&[][..], |s| s.subs).iter().chain(values(&typed, 1)) {
                    if w.starts_with(partial) {
                        let _ = out.push(*w);
                    }
                }
            }
            None => {}
        },
        i => {
            for w in values(&typed, i) {
                if w.starts_with(partial) {
                    let _ = out.push(*w);
                }
            }
        }
    }
    out
}

fn common_prefix(words: &[&'static str]) -> usize {
    let Some(first) = words.first() else { return 0 };
    (0..=first.len())
        .rev()
        .find(|&n| words.iter().all(|w| w.len() >= n && w.as_bytes()[..n] == first.as_bytes()[..n]))
        .unwrap_or(0)
}

// ----------------------------------------------------------------------------- editor

/// What one byte did: bytes to send back to the terminal, and a finished line.
#[derive(Default)]
pub struct Step {
    pub echo: Vec<u8, 400>,
    pub line: Option<String<LINE>>,
}

impl Step {
    fn text(&mut self, s: &str) {
        let _ = self.echo.extend_from_slice(s.as_bytes());
    }
}

/// Line editing: printable characters, backspace, ^U (clear), ^W (word), ^C (cancel), Tab (complete), the arrow
/// keys for the last few lines, and CR, LF or CRLF to end a line.
pub struct Editor {
    line: String<LINE>,
    /// The line so far could still be the start of an Improv frame, so nothing of it has been echoed.
    held: bool,
    /// Inside an Improv frame: ignored up to its newline.
    frame: bool,
    after_cr: bool,
    /// 0 nothing, 1 saw ESC, 2 saw ESC [.
    esc: u8,
    history: Vec<String<LINE>, HISTORY>,
    /// How far back the arrow keys have gone (0 = the line being typed).
    back: usize,
    draft: String<LINE>,
}

impl Default for Editor {
    fn default() -> Self {
        Self::new()
    }
}

impl Editor {
    pub fn new() -> Self {
        Self {
            line: String::new(),
            held: false,
            frame: false,
            after_cr: false,
            esc: 0,
            history: Vec::new(),
            back: 0,
            draft: String::new(),
        }
    }

    fn redraw(&self, s: &mut Step) {
        s.text("\r\x1b[K");
        s.text(PROMPT);
        s.text(&self.line);
    }

    fn remember(&mut self, line: &str) {
        // a line that sets a credential is not kept
        let first = line.split_ascii_whitespace().next().unwrap_or("");
        if line.is_empty() || matches!(first, "wifi" | "mqtt") {
            return;
        }
        if self.history.last().is_some_and(|l| l == line) {
            return;
        }
        if self.history.is_full() {
            self.history.remove(0);
        }
        let mut l = String::new();
        let _ = l.push_str(line);
        let _ = self.history.push(l);
    }

    fn recall(&mut self, older: bool, s: &mut Step) {
        let n = self.history.len();
        if older {
            if self.back < n {
                if self.back == 0 {
                    self.draft = self.line.clone();
                }
                self.back += 1;
            } else {
                return;
            }
        } else if self.back > 0 {
            self.back -= 1;
        } else {
            return;
        }
        self.line = if self.back == 0 {
            self.draft.clone()
        } else {
            self.history[n - self.back].clone()
        };
        self.redraw(s);
    }

    fn complete(&mut self, s: &mut Step) {
        let found = candidates(&self.line);
        if found.is_empty() {
            s.echo.push(BEL).ok();
            return;
        }
        let partial_len = if self.line.ends_with(' ') {
            0
        } else {
            self.line.split_ascii_whitespace().last().map_or(0, str::len)
        };
        let n = common_prefix(&found);
        let add = &found[0][partial_len..n];
        if found.len() == 1 {
            let _ = self.line.push_str(add);
            let _ = self.line.push(' ');
            s.text(add);
            s.text(" ");
            return;
        }
        if !add.is_empty() {
            let _ = self.line.push_str(add);
            s.text(add);
            return;
        }
        // the candidates one to a line, then the prompt and the line again below them
        s.text("\r\n");
        for w in &found {
            s.text(w);
            s.text("\r\n");
        }
        s.text(PROMPT);
        s.text(&self.line);
    }

    /// Feeds one byte from the terminal.
    pub fn push(&mut self, b: u8) -> Step {
        let mut s = Step::default();
        if self.frame {
            if b == b'\n' {
                self.frame = false;
            }
            return s;
        }
        match self.esc {
            1 => {
                self.esc = if b == b'[' { 2 } else { 0 };
                return s;
            }
            2 => {
                self.esc = 0;
                match b {
                    b'A' => self.recall(true, &mut s),
                    b'B' => self.recall(false, &mut s),
                    _ => {}
                }
                return s;
            }
            _ => {}
        }
        let was_cr = core::mem::take(&mut self.after_cr);
        match b {
            0x1b => self.esc = 1,
            b'\r' | b'\n' => {
                if b == b'\n' && was_cr {
                    return s;
                }
                self.after_cr = b == b'\r';
                if self.held {
                    // a line that was only a piece of the frame marker: nothing to say about it
                    self.held = false;
                    self.line.clear();
                    return s;
                }
                s.text("\r\n");
                let line = core::mem::take(&mut self.line);
                let line: String<LINE> = {
                    let mut t = String::new();
                    let _ = t.push_str(line.trim());
                    t
                };
                self.remember(&line);
                self.back = 0;
                s.line = Some(line);
            }
            0x7f | 0x08 => {
                if self.line.pop().is_some() {
                    if self.held {
                        self.held = FRAME_START.starts_with(self.line.as_str()) && !self.line.is_empty();
                    } else {
                        s.text("\x08 \x08");
                    }
                }
            }
            0x15 => {
                self.line.clear();
                self.held = false;
                self.redraw(&mut s);
            }
            0x17 => {
                while self.line.ends_with(' ') {
                    self.line.pop();
                }
                while self.line.chars().last().is_some_and(|c| c != ' ') {
                    self.line.pop();
                }
                self.held = false;
                self.redraw(&mut s);
            }
            0x03 => {
                self.line.clear();
                self.held = false;
                s.text("^C\r\n");
                s.text(PROMPT);
            }
            b'\t' => {
                if !self.held {
                    self.complete(&mut s);
                }
            }
            0x20..=0x7e => {
                if self.line.push(b as char).is_err() {
                    return s;
                }
                if FRAME_START.starts_with(self.line.as_str()) {
                    if self.line == FRAME_START {
                        // an Improv frame: not ours
                        self.line.clear();
                        self.held = false;
                        self.frame = true;
                    } else {
                        self.held = true;
                    }
                } else if self.held {
                    // it was not a frame after all: show what has been typed
                    self.held = false;
                    let line = self.line.clone();
                    s.text(&line);
                } else {
                    s.echo.push(b).ok();
                }
            }
            // a byte that cannot be part of a line: refused with a bell
            _ => {
                s.echo.push(BEL).ok();
            }
        }
        s
    }
}

// ----------------------------------------------------------------------------- parser

pub type Word = String<64>;

/// Splits a line into words; a word may be quoted with `"` (with `\"` and `\\` inside), so a password may hold
/// spaces. An unterminated quote or more than 8 words is an error.
pub fn words(line: &str) -> Result<Vec<Word, 8>, &'static str> {
    let mut out: Vec<Word, 8> = Vec::new();
    let mut chars = line.chars().peekable();
    loop {
        while chars.peek().is_some_and(|c| c.is_whitespace()) {
            chars.next();
        }
        if chars.peek().is_none() {
            return Ok(out);
        }
        let mut w = Word::new();
        let mut quoted = false;
        while let Some(&c) = chars.peek() {
            if c.is_whitespace() && !quoted {
                break;
            }
            chars.next();
            match c {
                '"' => quoted = !quoted,
                '\\' if quoted => {
                    let n = chars.next().ok_or("unterminated quote")?;
                    w.push(n).map_err(|_| "word too long")?;
                }
                _ => w.push(c).map_err(|_| "word too long")?,
            }
        }
        if quoted {
            return Err("unterminated quote");
        }
        out.push(w).map_err(|_| "too many words")?;
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Cmd {
    Empty,
    Help(Option<String<16>>),
    Status,
    Config,
    Wifi(Option<Wifi>),
    Mqtt(Option<Mqtt>),
    Otlp(Option<Otlp>),
    Setpoint(f64),
    Identify,
    Reboot,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ParseError {
    Unknown(String<16>),
    /// The command is known; this is what is wrong with its arguments.
    /// The command's usage line (empty when the line did not split into words) and what is wrong.
    Usage(&'static str, &'static str),
}

fn usage(name: &'static str, why: &'static str) -> ParseError {
    ParseError::Usage(spec(name).map_or(name, |s| s.usage), why)
}

fn short<const N: usize>(s: &str, name: &'static str) -> Result<String<N>, ParseError> {
    String::try_from(s).map_err(|_| usage(name, "too long"))
}

pub fn parse(line: &str) -> Result<Cmd, ParseError> {
    let w = words(line).map_err(|e| ParseError::Usage("", e))?;
    let Some(cmd) = w.first() else { return Ok(Cmd::Empty) };
    let args = &w[1..];
    match cmd.as_str() {
        "help" | "?" => match args {
            [] => Ok(Cmd::Help(None)),
            [c] => Ok(Cmd::Help(Some(short(c, "help")?))),
            _ => Err(usage("help", "one command at most")),
        },
        "status" if args.is_empty() => Ok(Cmd::Status),
        "config" if args.is_empty() => Ok(Cmd::Config),
        "identify" if args.is_empty() => Ok(Cmd::Identify),
        "reboot" if args.is_empty() => Ok(Cmd::Reboot),
        "status" | "config" | "identify" | "reboot" => Err(usage(spec(cmd).map_or("help", |s| s.name), "takes no arguments")),
        "setpoint" => match args {
            [f] => {
                let f: f64 = f.parse().map_err(|_| usage("setpoint", "not a number"))?;
                if (SP_MIN_F..=SP_MAX_F).contains(&f) {
                    Ok(Cmd::Setpoint(f))
                } else {
                    Err(usage("setpoint", "out of range"))
                }
            }
            _ => Err(usage("setpoint", "one value")),
        },
        "wifi" => match args {
            [c] if c == "clear" => Ok(Cmd::Wifi(None)),
            [c, ssid] if c == "set" => Ok(Cmd::Wifi(Some(Wifi {
                ssid: short(ssid, "wifi")?,
                pass: String::new(),
            }))),
            [c, ssid, pass] if c == "set" => Ok(Cmd::Wifi(Some(Wifi {
                ssid: short(ssid, "wifi")?,
                pass: short(pass, "wifi")?,
            }))),
            _ => Err(usage("wifi", "")),
        },
        "mqtt" => match args {
            [c] if c == "clear" => Ok(Cmd::Mqtt(None)),
            [c, host, rest @ ..] if c == "set" && rest.len() <= 3 => {
                let port = match rest.first() {
                    Some(p) => p
                        .parse::<u16>()
                        .ok()
                        .filter(|&p| p != 0)
                        .ok_or_else(|| usage("mqtt", "the port is 1 to 65535"))?,
                    None => 1883,
                };
                Ok(Cmd::Mqtt(Some(Mqtt {
                    host: short(host, "mqtt")?,
                    port,
                    user: rest.get(1).map_or(Ok(String::new()), |u| short(u, "mqtt"))?,
                    pass: rest.get(2).map_or(Ok(String::new()), |p| short(p, "mqtt"))?,
                })))
            }
            _ => Err(usage("mqtt", "")),
        },
        "otlp" => match args {
            [c] if c == "clear" => Ok(Cmd::Otlp(None)),
            [c, host, rest @ ..] if c == "set" && rest.len() <= 2 => {
                let port = match rest.first() {
                    Some(p) => p
                        .parse::<u16>()
                        .ok()
                        .filter(|&p| p != 0)
                        .ok_or_else(|| usage("otlp", "the port is 1 to 65535"))?,
                    None => 4318,
                };
                let interval_s = match rest.get(1) {
                    Some(i) => i
                        .parse::<u16>()
                        .ok()
                        .filter(|i| (5..=3600).contains(i))
                        .ok_or_else(|| usage("otlp", "the interval is 5 to 3600 s"))?,
                    None => 60,
                };
                Ok(Cmd::Otlp(Some(Otlp {
                    host: short(host, "otlp")?,
                    port,
                    interval_s,
                })))
            }
            _ => Err(usage("otlp", "")),
        },
        other => Err(ParseError::Unknown(String::try_from(other).unwrap_or_default())),
    }
}

// ----------------------------------------------------------------------------- output

async fn say<I: Write>(io: &mut I, args: fmt::Arguments<'_>) {
    let mut s: String<200> = String::new();
    let _ = s.write_fmt(args);
    let _ = s.push_str("\r\n");
    let _ = io.write_all(s.as_bytes()).await;
}

pub async fn prompt<I: Write>(io: &mut I) {
    let _ = io.write_all(PROMPT.as_bytes()).await;
    let _ = io.flush().await;
}

/// The settings as `config` shows them: passwords are never printed.
pub fn config_lines(s: &Settings) -> Vec<String<120>, 6> {
    let mut out: Vec<String<120>, 6> = Vec::new();
    let mut l: String<120> = String::new();
    let _ = match &s.wifi {
        Some(w) => write!(l, "wifi     ssid \"{}\" password {}", w.ssid, if w.pass.is_empty() { "(none)" } else { "****" }),
        None => write!(l, "wifi     not set"),
    };
    let _ = out.push(l);
    let mut l: String<120> = String::new();
    let _ = match &s.mqtt {
        Some(m) => write!(
            l,
            "mqtt     {}:{} user \"{}\" password {}",
            m.host,
            m.port,
            m.user,
            if m.pass.is_empty() { "(none)" } else { "****" }
        ),
        None => write!(l, "mqtt     not set"),
    };
    let _ = out.push(l);
    let mut l: String<120> = String::new();
    let _ = match &s.otlp {
        Some(o) => write!(l, "otlp     {}:{} every {} s", o.host, o.port, o.interval_s),
        None => write!(l, "otlp     not set"),
    };
    let _ = out.push(l);
    let mut l: String<120> = String::new();
    let mode = match s.cook.mode {
        Mode::Off => "off",
        Mode::Run => "run",
        Mode::Manual => "manual",
    };
    let _ = write!(l, "cook     {mode}, setpoint {:.0} F", s.cook.sp_f);
    let _ = out.push(l);
    out
}

async fn request<I: Write>(io: &mut I, bus: &Bus, replies: &mut Replies<'_>, req: Request, done: &str) {
    if !bus.send(REQUEST_ID, req) {
        say(io, format_args!("busy, try again")).await;
        return;
    }
    loop {
        let r = replies.next_message_pure().await;
        if r.id != REQUEST_ID {
            continue;
        }
        match r.result {
            Ok(()) => say(io, format_args!("{done}")).await,
            Err(e) => say(io, format_args!("error: {}", e.detail)).await,
        }
        return;
    }
}

/// Runs one parsed line, printing its output on `io` (not the prompt: the caller sends that).
pub async fn execute<I: Write>(io: &mut I, line: &str, who: &Identity<'_>, bus: &Bus, replies: &mut Replies<'_>) {
    let cmd = match parse(line) {
        Ok(c) => c,
        Err(ParseError::Unknown(c)) => {
            say(io, format_args!("unknown command \"{c}\" (help lists them)")).await;
            return;
        }
        Err(ParseError::Usage(u, why)) => {
            if why.is_empty() {
                say(io, format_args!("usage: {u}")).await;
            } else if u.is_empty() {
                say(io, format_args!("{why}")).await;
            } else {
                say(io, format_args!("{why}; usage: {u}")).await;
            }
            return;
        }
    };
    match cmd {
        Cmd::Empty => {}
        Cmd::Help(None) => {
            say(io, format_args!("{} {} (hw {}), id {}", who.prefix, who.sw, who.hw, who.id)).await;
            for s in CATALOG {
                say(io, format_args!("  {:<9}{}", s.name, s.help)).await;
            }
        }
        Cmd::Help(Some(c)) => match spec(&c) {
            Some(s) => {
                say(io, format_args!("{}", s.usage)).await;
                say(io, format_args!("  {}", s.help)).await;
            }
            None => say(io, format_args!("unknown command \"{c}\"")).await,
        },
        Cmd::Config => match bus.config() {
            Some(s) => {
                for l in config_lines(&s) {
                    say(io, format_args!("{l}")).await;
                }
            }
            None => say(io, format_args!("settings not loaded yet")).await,
        },
        Cmd::Status => match bus.status.try_get() {
            Some(st) => {
                say(io, format_args!("up {} s, setpoint {:.0} F, flap {:.0} %, fan {}", st.up_s, st.cook.sp_f, st.out.flap_pct,
                    match st.fan_rpm { Some(r) => r as i64, None => -1 })).await;
                for (n, t) in st.temps.iter().enumerate() {
                    match t {
                        Some(f) => say(io, format_args!("probe {} {:.1} F", n + 1, f)).await,
                        None => say(io, format_args!("probe {} --", n + 1)).await,
                    }
                }
                let mut faults: String<120> = String::new();
                for c in FaultCode::ALL {
                    if st.faults.contains(c) {
                        let _ = write!(faults, "{} ", c.name());
                    }
                }
                if faults.is_empty() {
                    say(io, format_args!("faults none")).await;
                } else {
                    say(io, format_args!("faults {}", faults.trim_end())).await;
                }
            }
            None => say(io, format_args!("no status yet")).await,
        },
        Cmd::Setpoint(f) => request(io, bus, replies, Request::Pit(Command::Sp(f)), "ok").await,
        Cmd::Wifi(w) => {
            let done = if w.is_some() { "saved; the board joins it" } else { "network forgotten" };
            request(io, bus, replies, Request::Wifi(w), done).await
        }
        Cmd::Mqtt(m) => {
            let done = if m.is_some() { "saved; the broker is used from the next connection" } else { "broker forgotten" };
            request(io, bus, replies, Request::Mqtt(m), done).await
        }
        Cmd::Otlp(o) => {
            let done = if o.is_some() { "saved" } else { "collector forgotten" };
            request(io, bus, replies, Request::Otlp(o), done).await
        }
        Cmd::Identify => request(io, bus, replies, Request::Identify, "blinking for 10 s").await,
        Cmd::Reboot => request(io, bus, replies, Request::Reboot, "rebooting").await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn type_in(e: &mut Editor, s: &[u8]) -> (std::string::String, std::vec::Vec<std::string::String>) {
        let mut echo = std::string::String::new();
        let mut lines = std::vec::Vec::new();
        for &b in s {
            let st = e.push(b);
            echo.push_str(core::str::from_utf8(&st.echo).unwrap());
            if let Some(l) = st.line {
                lines.push(l.as_str().into());
            }
        }
        (echo, lines)
    }

    #[test]
    fn a_typed_line_is_echoed_and_returned() {
        let mut e = Editor::new();
        let (echo, lines) = type_in(&mut e, b"status\r");
        assert_eq!(echo, "status\r\n");
        assert_eq!(lines, ["status"]);
    }

    #[test]
    fn crlf_is_one_line_end_and_backspace_edits() {
        let mut e = Editor::new();
        let (_, lines) = type_in(&mut e, b"statuz\x7fs\r\nconfig\n");
        assert_eq!(lines, ["status", "config"]);
    }

    #[test]
    fn improv_frames_are_not_lines_and_are_not_echoed() {
        let mut e = Editor::new();
        let mut frame = std::vec::Vec::from(&b"IMPROV\x01\x03\x02\x02\x00"[..]);
        frame.extend_from_slice(&[0xE0, b'\n']);
        let (echo, lines) = type_in(&mut e, &frame);
        assert!(lines.is_empty() && echo.is_empty(), "{echo:?}");
        // and the next line works
        let (_, lines) = type_in(&mut e, b"help\r");
        assert_eq!(lines, ["help"]);
    }

    #[test]
    fn a_word_that_starts_like_the_marker_is_echoed_once_it_differs() {
        let mut e = Editor::new();
        let (echo, lines) = type_in(&mut e, b"IMPx\r");
        assert_eq!(echo, "IMPx\r\n");
        assert_eq!(lines, ["IMPx"]);
        let (echo, _) = type_in(&mut e, b"I");
        assert_eq!(echo, "", "held while it could be a frame");
    }

    #[test]
    fn tab_completes_commands_and_their_words() {
        let mut e = Editor::new();
        let (echo, _) = type_in(&mut e, b"wi\t");
        assert_eq!(echo, "wifi ");
        let (_, lines) = type_in(&mut e, b"cl\t\r");
        assert_eq!(lines, ["wifi clear"]);
        let mut e = Editor::new();
        let (echo, _) = type_in(&mut e, b"s\t");
        assert!(echo.contains("status") && echo.contains("setpoint"), "{echo:?}");
    }

    #[test]
    fn tab_offers_typical_values_in_argument_positions() {
        let mut e = Editor::new();
        let (echo, _) = type_in(&mut e, b"mqtt set broker \t");
        assert!(echo.contains("1883\r\n") && echo.contains("8883\r\n"), "{echo:?}");
        let mut e = Editor::new();
        let (echo, _) = type_in(&mut e, b"otlp set c 4\t");
        assert_eq!(echo, "otlp set c 431", "the shared prefix first");
        let (echo, _) = type_in(&mut e, b"\t");
        assert!(echo.contains("4318\r\n") && echo.contains("4317\r\n"), "a second Tab lists them: {echo:?}");
        let mut e = Editor::new();
        let (echo, lines) = type_in(&mut e, b"setpoint 22\t\r");
        assert_eq!(echo, "setpoint 22" .to_string() + "5 " + "\r\n");
        assert_eq!(lines, ["setpoint 225"]);
    }

    #[test]
    fn nothing_to_complete_and_bytes_that_cannot_be_typed_ring_the_bell() {
        let mut e = Editor::new();
        let (echo, _) = type_in(&mut e, b"xyz\t");
        assert!(echo.ends_with('\x07'), "{echo:?}");
        let mut e = Editor::new();
        let st = e.push(0x01);
        assert_eq!(&st.echo[..], &[0x07]);
        let st = e.push(0xE9);
        assert_eq!(&st.echo[..], &[0x07]);
        assert!(type_in(&mut e, b"\r").1 == [""], "and the line stays empty");
    }

    #[test]
    fn credential_lines_are_not_kept_for_the_up_arrow() {
        let mut e = Editor::new();
        type_in(&mut e, b"wifi set net secret\rstatus\r");
        let (echo, _) = type_in(&mut e, b"\x1b[A");
        assert!(echo.contains("status") && !echo.contains("secret"), "{echo:?}");
        let (echo, _) = type_in(&mut e, b"\x1b[A");
        assert!(!echo.contains("secret"), "{echo:?}");
    }

    #[test]
    fn ctrl_c_cancels_and_ctrl_u_clears() {
        let mut e = Editor::new();
        let (echo, lines) = type_in(&mut e, b"abc\x03def\x15ghi\r");
        assert_eq!(lines, ["ghi"]);
        assert!(echo.contains("^C"));
    }

    #[test]
    fn words_split_on_spaces_and_honour_quotes() {
        let w = words(r#"wifi set "my net" "pa ss\"w\\d""#).unwrap();
        assert_eq!(w.iter().map(|s| s.as_str()).collect::<std::vec::Vec<_>>(), ["wifi", "set", "my net", r#"pa ss"w\d"#]);
        assert_eq!(words("a \"b").unwrap_err(), "unterminated quote");
        assert!(words("").unwrap().is_empty());
    }

    #[test]
    fn wifi_commands() {
        assert_eq!(
            parse("wifi set home hunter2").unwrap(),
            Cmd::Wifi(Some(Wifi { ssid: "home".try_into().unwrap(), pass: "hunter2".try_into().unwrap() }))
        );
        assert_eq!(
            parse("wifi set guest").unwrap(),
            Cmd::Wifi(Some(Wifi { ssid: "guest".try_into().unwrap(), pass: String::new() }))
        );
        assert_eq!(parse("wifi clear").unwrap(), Cmd::Wifi(None));
        assert!(parse("wifi").is_err() && parse("wifi set").is_err());
        let long = ["wifi set ", &"x".repeat(40)].concat();
        assert!(parse(&long).is_err(), "an SSID is 32 bytes at most");
    }

    #[test]
    fn mqtt_commands_take_the_defaults() {
        let Cmd::Mqtt(Some(m)) = parse("mqtt set broker.lan").unwrap() else { panic!() };
        assert_eq!((m.host.as_str(), m.port, m.user.as_str(), m.pass.as_str()), ("broker.lan", 1883, "", ""));
        let Cmd::Mqtt(Some(m)) = parse("mqtt set 10.0.0.2 8883 egg \"a b\"").unwrap() else { panic!() };
        assert_eq!((m.host.as_str(), m.port, m.user.as_str(), m.pass.as_str()), ("10.0.0.2", 8883, "egg", "a b"));
        assert_eq!(parse("mqtt clear").unwrap(), Cmd::Mqtt(None));
        assert!(parse("mqtt set h 0").is_err() && parse("mqtt set h 70000").is_err() && parse("mqtt set h x").is_err());
        assert!(parse("mqtt set h 1 u p extra").is_err());
    }

    #[test]
    fn otlp_commands_take_the_defaults_and_check_the_interval() {
        let Cmd::Otlp(Some(o)) = parse("otlp set collector.lan").unwrap() else { panic!() };
        assert_eq!((o.port, o.interval_s), (4318, 60));
        let Cmd::Otlp(Some(o)) = parse("otlp set c 4317 30").unwrap() else { panic!() };
        assert_eq!((o.port, o.interval_s), (4317, 30));
        assert!(parse("otlp set c 4318 4").is_err() && parse("otlp set c 4318 3601").is_err());
        assert_eq!(parse("otlp clear").unwrap(), Cmd::Otlp(None));
    }

    #[test]
    fn simple_commands_and_unknown_ones() {
        assert_eq!(parse("").unwrap(), Cmd::Empty);
        assert_eq!(parse("  status ").unwrap(), Cmd::Status);
        assert_eq!(parse("setpoint 225").unwrap(), Cmd::Setpoint(225.0));
        assert!(parse("setpoint 400").is_err() && parse("setpoint hot").is_err() && parse("setpoint").is_err());
        assert!(parse("status now").is_err());
        assert!(matches!(parse("frobnicate"), Err(ParseError::Unknown(_))));
        assert_eq!(parse("help wifi").unwrap(), Cmd::Help(Some("wifi".try_into().unwrap())));
    }

    #[test]
    fn config_never_shows_a_password() {
        let mut s = Settings::default();
        s.wifi = Some(Wifi { ssid: "home".try_into().unwrap(), pass: "hunter2".try_into().unwrap() });
        s.mqtt = Some(Mqtt { host: "b".try_into().unwrap(), port: 1883, user: "u".try_into().unwrap(), pass: "s3cret".try_into().unwrap() });
        let text: std::vec::Vec<std::string::String> = config_lines(&s).iter().map(|l| l.as_str().into()).collect();
        let all = text.join("\n");
        assert!(!all.contains("hunter2") && !all.contains("s3cret"), "{all}");
        assert!(all.contains("home") && all.contains("b:1883"));
    }
}
