//! The serial REPL (`minimeat-s7v`): how credentials and settings that must not cross the network are set, over the
//! USB-C cable. It shares the port with Improv Wi-Fi frames and the log lines: `serial::Demux` cuts the bytes into
//! Improv frames and [`Key`]s, and the [`Editor`] takes only keys, so it never sees a frame.
//!
//! [`CATALOG`] is the one list of commands: completion and `help` read it, and the `winnow` grammar in [`parse`]
//! accepts the same names. Everything here but [`execute`] is plain code with host tests; [`execute`] talks to the
//! control task over the [`Bus`] the way Improv does, so a setting goes through the same save-and-publish path as
//! any other.

use core::cell::RefCell;
use core::fmt::{self, Write as _};

use embassy_sync::blocking_mutex::Mutex;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::pubsub::Subscriber;
use embedded_io_async::Write;
use heapless::{String, Vec};

use crate::config::{Mode, Mqtt, Otlp, Settings, Wifi};
use crate::net::Identity;
use crate::pit::{Command, FaultCode, SP_MAX_F, SP_MIN_F};
use crate::proto::Request;
use crate::tasks::{Bus, READERS, Reply};

pub const PROMPT: &str = "minimeater> ";
/// Longest line, characters.
pub const LINE: usize = 160;
/// The id this conversation's requests carry on the bus (Improv's is 0xFF10).
const REQUEST_ID: u16 = 0xFF11;
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
    Spec {
        name: "factory-reset",
        usage: "factory-reset erase",
        help: "every setting back to its default, then reboot (the word erase confirms)",
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
        .find(|&n| {
            words
                .iter()
                .all(|w| w.len() >= n && w.as_bytes()[..n] == first.as_bytes()[..n])
        })
        .unwrap_or(0)
}

// ----------------------------------------------------------------------------- editor

/// What the terminal sent, one keystroke at a time (`serial::Demux` cuts the byte stream into these).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    /// A printable ASCII character.
    Char(u8),
    Cr,
    Lf,
    Backspace,
    Tab,
    CtrlC,
    CtrlU,
    CtrlW,
    Up,
    Down,
    /// A byte that cannot be part of a line: refused with a bell.
    Bad,
    /// An escape sequence that means nothing here.
    Ignore,
}

/// What one key did: bytes to send back to the terminal, and a finished line.
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

/// Longest log line the board writes, newline included.
pub const LOG_LINE: usize = 160;

/// What the board's log writer needs to put a log line on the same terminal as a half-typed command: while the prompt
/// is up it holds the text that redraws it, and [`Live::wrap`] puts a log line between an erase and that redraw, so
/// the typed line is not left with a log line in the middle of it. Empty (and `wrap` has nothing to do) until someone
/// types, while a command runs, and when no terminal is attached.
pub struct Live(Mutex<CriticalSectionRawMutex, RefCell<Option<String<{ PROMPT.len() + LINE }>>>>);

/// A log line, wrapped.
pub type Wrapped = Vec<u8, { 4 + LOG_LINE + PROMPT.len() + LINE }>;

impl Live {
    pub const fn new() -> Self {
        Self(Mutex::new(RefCell::new(None)))
    }

    fn set(&self, text: Option<&str>) {
        self.0.lock(|c| {
            *c.borrow_mut() = text.map(|t| {
                let mut s = String::new();
                let _ = s.push_str(PROMPT);
                let _ = s.push_str(t);
                s
            })
        });
    }

    /// `log` with the prompt line erased before it and redrawn after it; `None` when there is no prompt to keep tidy
    /// (write `log` as it is).
    pub fn wrap(&self, log: &[u8]) -> Option<Wrapped> {
        self.0.lock(|c| {
            let c = c.borrow();
            let redraw = c.as_ref()?;
            let mut out = Wrapped::new();
            out.extend_from_slice(b"\r\x1b[K").ok()?;
            out.extend_from_slice(log).ok()?;
            out.extend_from_slice(redraw.as_bytes()).ok()?;
            Some(out)
        })
    }
}

impl Default for Live {
    fn default() -> Self {
        Self::new()
    }
}

/// The one terminal, the USB-C port's.
pub static LIVE: Live = Live::new();

/// Line editing: characters, backspace, ^U (clear), ^W (word), ^C (cancel), Tab (complete), the arrow keys for the
/// last few lines, and CR, LF or CRLF to end a line. It knows nothing of bytes or Improv: it takes [`Key`]s.
pub struct Editor {
    line: String<LINE>,
    after_cr: bool,
    history: Vec<String<LINE>, HISTORY>,
    /// How far back the arrow keys have gone (0 = the line being typed).
    back: usize,
    draft: String<LINE>,
    /// Someone has typed: there is a prompt on their screen.
    engaged: bool,
}

impl Default for Editor {
    fn default() -> Self {
        Self::new()
    }
}

impl Editor {
    pub const fn new() -> Self {
        Self {
            line: String::new(),
            after_cr: false,
            history: Vec::new(),
            back: 0,
            draft: String::new(),
            engaged: false,
        }
    }

    /// Tells `live` what is on the screen, to redraw after a log line (call after each key that did not end a line,
    /// and after the prompt that follows a command).
    pub fn show(&self, live: &Live) {
        live.set(self.engaged.then_some(self.line.as_str()));
    }

    /// A command is running: nothing to redraw until its prompt is back.
    pub fn hide(&self, live: &Live) {
        live.set(None);
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

    /// Applies one key.
    pub fn apply(&mut self, key: Key) -> Step {
        let mut s = Step::default();
        let was_cr = core::mem::take(&mut self.after_cr);
        self.engaged |= matches!(key, Key::Char(_) | Key::Cr | Key::CtrlC);
        match key {
            Key::Cr | Key::Lf => {
                if key == Key::Lf && was_cr {
                    return s;
                }
                self.after_cr = key == Key::Cr;
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
            Key::Backspace => {
                if self.line.pop().is_some() {
                    s.text("\x08 \x08");
                }
            }
            Key::CtrlU => {
                self.line.clear();
                self.redraw(&mut s);
            }
            Key::CtrlW => {
                while self.line.ends_with(' ') {
                    self.line.pop();
                }
                while self.line.chars().last().is_some_and(|c| c != ' ') {
                    self.line.pop();
                }
                self.redraw(&mut s);
            }
            Key::CtrlC => {
                self.line.clear();
                s.text("^C\r\n");
                s.text(PROMPT);
            }
            Key::Tab => self.complete(&mut s),
            Key::Up => self.recall(true, &mut s),
            Key::Down => self.recall(false, &mut s),
            Key::Char(b) => {
                if self.line.push(b as char).is_ok() {
                    s.echo.push(b).ok();
                }
            }
            Key::Bad => {
                s.echo.push(BEL).ok();
            }
            Key::Ignore => {}
        }
        s
    }
}

// ----------------------------------------------------------------------------- parser

pub type Word = String<64>;

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
    /// `factory-reset`: true once the confirmation word came with it.
    FactoryReset(bool),
}

#[derive(Clone, Debug, PartialEq)]
pub enum ParseError {
    Unknown(String<16>),
    /// The command's usage line (empty when the line did not get as far as a command) and what is wrong.
    Usage(&'static str, &'static str),
}

/// The command line as a `winnow` grammar on the text: a name, then words (bare, or in double quotes with `\"` and
/// `\\` inside, so a password may hold spaces), each command taking its own. [`parse`] runs it.
mod grammar {
    use winnow::ascii::{multispace0, multispace1};
    use winnow::combinator::{alt, cut_err, delimited, eof, opt, preceded, repeat, terminated};
    use winnow::error::{ErrMode, FromExternalError, ParserError};
    use winnow::token::{any, none_of, take_till};
    use winnow::{ModalResult, Parser as _};

    use super::*;

    type In<'a> = &'a str;
    type R<T> = ModalResult<T, ParseError>;

    impl<'a> ParserError<In<'a>> for ParseError {
        type Inner = Self;
        fn from_input(_: &In<'a>) -> Self {
            ParseError::Usage("", "")
        }
        fn append(self, _: &In<'a>, _: &<In<'a> as winnow::stream::Stream>::Checkpoint) -> Self {
            self
        }
        fn or(self, other: Self) -> Self {
            other
        }
        fn into_inner(self) -> Result<Self::Inner, Self> {
            Ok(self)
        }
    }

    impl<'a> FromExternalError<In<'a>, ParseError> for ParseError {
        fn from_external_error(_: &In<'a>, e: ParseError) -> Self {
            e
        }
    }

    fn why(w: &'static str) -> ParseError {
        ParseError::Usage("", w)
    }

    /// A word in double quotes.
    fn quoted(i: &mut In<'_>) -> R<Word> {
        delimited(
            '"',
            repeat(0.., alt((preceded('\\', any), none_of(['"', '\\'])))).try_fold(Word::new, |mut w, c: char| {
                w.push(c).map(|()| w).map_err(|_| why("word too long"))
            }),
            cut_err('"').map_err(|_: ErrMode<ParseError>| ErrMode::Cut(why("unterminated quote"))),
        )
        .parse_next(i)
    }

    /// A word without spaces or quotes.
    fn bare(i: &mut In<'_>) -> R<Word> {
        take_till(1.., [' ', '\t', '"'])
            .try_map(|s: &str| Word::try_from(s).map_err(|_| why("word too long")))
            .parse_next(i)
    }

    fn word(i: &mut In<'_>) -> R<Word> {
        alt((quoted, bare)).parse_next(i)
    }

    /// A following word.
    fn arg(i: &mut In<'_>) -> R<Word> {
        preceded(multispace1, word).parse_next(i)
    }

    /// A following word, no longer than `N` bytes.
    fn arg_s<const N: usize>(i: &mut In<'_>) -> R<String<N>> {
        arg.try_map(|w| String::<N>::try_from(w.as_str()).map_err(|_| why("too long")))
            .parse_next(i)
    }

    /// A following word that has to be this one.
    fn kw<'a>(name: &'static str) -> impl FnMut(&mut In<'a>) -> R<()> {
        move |i| arg.verify(move |w: &Word| w.as_str() == name).void().parse_next(i)
    }

    /// An optional following word, parsed with `f`: absent is `None`, present but wrong is an error.
    fn opt_arg<'a, O>(mut f: impl FnMut(Word) -> Result<O, ParseError>) -> impl FnMut(&mut In<'a>) -> R<Option<O>> {
        move |i| opt(preceded(multispace1, cut_err(word.try_map(&mut f)))).parse_next(i)
    }

    fn port(w: Word) -> Result<u16, ParseError> {
        w.parse::<u16>()
            .ok()
            .filter(|&p| p != 0)
            .ok_or(why("the port is 1 to 65535"))
    }

    fn text<const N: usize>(w: Word) -> Result<String<N>, ParseError> {
        String::<N>::try_from(w.as_str()).map_err(|_| why("too long"))
    }

    fn interval(w: Word) -> Result<u16, ParseError> {
        w.parse::<u16>()
            .ok()
            .filter(|i| (5..=3600).contains(i))
            .ok_or(why("the interval is 5 to 3600 s"))
    }

    fn setpoint(i: &mut In<'_>) -> R<Cmd> {
        preceded(
            multispace1,
            cut_err(word.try_map(|w| {
                let f: f64 = w.parse().map_err(|_| why("not a number"))?;
                if (SP_MIN_F..=SP_MAX_F).contains(&f) {
                    Ok(Cmd::Setpoint(f))
                } else {
                    Err(why("out of range"))
                }
            })),
        )
        .parse_next(i)
    }

    fn wifi(i: &mut In<'_>) -> R<Cmd> {
        alt((
            kw("clear").value(Cmd::Wifi(None)),
            preceded(kw("set"), (arg_s::<32>, opt(arg_s::<64>))).map(|(ssid, pass)| {
                Cmd::Wifi(Some(Wifi {
                    ssid,
                    pass: pass.unwrap_or_default(),
                }))
            }),
        ))
        .parse_next(i)
    }

    fn mqtt(i: &mut In<'_>) -> R<Cmd> {
        alt((
            kw("clear").value(Cmd::Mqtt(None)),
            preceded(
                kw("set"),
                (arg_s::<64>, opt_arg(port), opt_arg(text::<32>), opt_arg(text::<64>)),
            )
            .map(|(host, port, user, pass)| {
                Cmd::Mqtt(Some(Mqtt {
                    host,
                    port: port.unwrap_or(1883),
                    user: user.unwrap_or_default(),
                    pass: pass.unwrap_or_default(),
                }))
            }),
        ))
        .parse_next(i)
    }

    fn otlp(i: &mut In<'_>) -> R<Cmd> {
        alt((
            kw("clear").value(Cmd::Otlp(None)),
            preceded(kw("set"), (arg_s::<64>, opt_arg(port), opt_arg(interval))).map(|(host, port, interval_s)| {
                Cmd::Otlp(Some(Otlp {
                    host,
                    port: port.unwrap_or(4318),
                    interval_s: interval_s.unwrap_or(60),
                }))
            }),
        ))
        .parse_next(i)
    }

    /// Runs `p` for the command `name`, then wants the end of the line; an error with no usage of its own gets
    /// the command's.
    fn command_args<'a, O>(
        name: &'static str,
        p: impl winnow::Parser<In<'a>, O, ErrMode<ParseError>>,
    ) -> impl FnMut(&mut In<'a>) -> R<O> {
        let mut p = terminated(p, (multispace0, eof));
        move |i| {
            p.parse_next(i).map_err(|e| {
                e.map(|e| match e {
                    ParseError::Usage("", w) => ParseError::Usage(spec(name).map_or(name, |s| s.usage), w),
                    other => other,
                })
            })
        }
    }

    /// The whole line.
    pub fn line(i: &mut In<'_>) -> R<Cmd> {
        multispace0.parse_next(i)?;
        if i.is_empty() {
            return Ok(Cmd::Empty);
        }
        let name = take_till(1.., [' ', '\t']).parse_next(i)?;
        match name {
            "help" | "?" => command_args("help", opt(arg_s::<16>).map(Cmd::Help)).parse_next(i),
            "status" => command_args("status", winnow::combinator::empty.value(Cmd::Status)).parse_next(i),
            "config" => command_args("config", winnow::combinator::empty.value(Cmd::Config)).parse_next(i),
            "identify" => command_args("identify", winnow::combinator::empty.value(Cmd::Identify)).parse_next(i),
            "reboot" => command_args("reboot", winnow::combinator::empty.value(Cmd::Reboot)).parse_next(i),
            "factory-reset" => command_args(
                "factory-reset",
                opt(kw("erase")).map(|w| Cmd::FactoryReset(w.is_some())),
            )
            .parse_next(i),
            "setpoint" => command_args("setpoint", setpoint).parse_next(i),
            "wifi" => command_args("wifi", wifi).parse_next(i),
            "mqtt" => command_args("mqtt", mqtt).parse_next(i),
            "otlp" => command_args("otlp", otlp).parse_next(i),
            other => Err(ErrMode::Cut(ParseError::Unknown(
                String::try_from(other).unwrap_or_default(),
            ))),
        }
    }
}

/// Parses one command line.
pub fn parse(line: &str) -> Result<Cmd, ParseError> {
    use winnow::error::ErrMode;
    let mut input = line;
    match grammar::line(&mut input) {
        Ok(c) => Ok(c),
        Err(ErrMode::Backtrack(e) | ErrMode::Cut(e)) => Err(e),
        Err(ErrMode::Incomplete(_)) => Err(ParseError::Usage("", "incomplete")),
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
        Some(w) => write!(
            l,
            "wifi     ssid \"{}\" password {}",
            w.ssid,
            if w.pass.is_empty() { "(none)" } else { "****" }
        ),
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
            say(
                io,
                format_args!("{} {} (hw {}), id {}", who.prefix, who.sw, who.hw, who.id),
            )
            .await;
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
                say(
                    io,
                    format_args!(
                        "up {} s, setpoint {:.0} F, flap {:.0} %, fan {}",
                        st.up_s,
                        st.cook.sp_f,
                        st.out.flap_pct,
                        match st.fan_rpm {
                            Some(r) => r as i64,
                            None => -1,
                        }
                    ),
                )
                .await;
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
            let done = if w.is_some() {
                "saved; the board joins it"
            } else {
                "network forgotten"
            };
            request(io, bus, replies, Request::Wifi(w), done).await
        }
        Cmd::Mqtt(m) => {
            let done = if m.is_some() {
                "saved; the broker is used from the next connection"
            } else {
                "broker forgotten"
            };
            request(io, bus, replies, Request::Mqtt(m), done).await
        }
        Cmd::Otlp(o) => {
            let done = if o.is_some() { "saved" } else { "collector forgotten" };
            request(io, bus, replies, Request::Otlp(o), done).await
        }
        Cmd::Identify => request(io, bus, replies, Request::Identify, "blinking for 10 s").await,
        Cmd::Reboot => request(io, bus, replies, Request::Reboot, "rebooting").await,
        Cmd::FactoryReset(false) => {
            say(
                io,
                format_args!("this erases the cook, probe limits, learned schedule, WiFi, MQTT and OTLP; type factory-reset erase to do it"),
            )
            .await
        }
        Cmd::FactoryReset(true) => request(io, bus, replies, Request::Defaults, "everything reset; rebooting").await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::serial::{Demux, Item};

    /// What a terminal's bytes do: through the demultiplexer, as the serial task runs them.
    struct Term {
        demux: Demux,
        editor: Editor,
    }

    impl Term {
        fn new() -> Self {
            Self {
                demux: Demux::new(),
                editor: Editor::new(),
            }
        }

        fn type_in(&mut self, s: &[u8]) -> (std::string::String, std::vec::Vec<std::string::String>) {
            let mut echo = std::string::String::new();
            let mut lines = std::vec::Vec::new();
            for &b in s {
                self.demux.push(b);
                while let Some(item) = self.demux.pop() {
                    if let Item::Key(k) = item {
                        let st = self.editor.apply(k);
                        echo.push_str(core::str::from_utf8(&st.echo).unwrap());
                        if let Some(l) = st.line {
                            lines.push(l.as_str().into());
                        }
                    }
                }
            }
            (echo, lines)
        }
    }

    fn type_in(e: &mut Term, s: &[u8]) -> (std::string::String, std::vec::Vec<std::string::String>) {
        e.type_in(s)
    }

    #[test]
    fn a_typed_line_is_echoed_and_returned() {
        let mut e = Term::new();
        let (echo, lines) = type_in(&mut e, b"status\r");
        assert_eq!(echo, "status\r\n");
        assert_eq!(lines, ["status"]);
    }

    #[test]
    fn crlf_is_one_line_end_and_backspace_edits() {
        let mut e = Term::new();
        let (_, lines) = type_in(&mut e, b"statuz\x7fs\r\nconfig\n");
        assert_eq!(lines, ["status", "config"]);
    }

    #[test]
    fn improv_frames_are_not_lines_and_are_not_echoed() {
        let mut e = Term::new();
        let mut frame = std::vec::Vec::from(&b"IMPROV\x01\x03\x02\x02\x00"[..]);
        frame.extend_from_slice(&[0xE0, b'\n']);
        let (echo, lines) = type_in(&mut e, &frame);
        assert!(lines.is_empty() && echo.is_empty(), "{echo:?}");
        // and the next line works
        let (_, lines) = type_in(&mut e, b"help\r");
        assert_eq!(lines, ["help"]);
    }

    #[test]
    fn a_frame_with_newline_bytes_inside_it_is_skipped_whole() {
        // a length byte of 10 (0x0A) and a checksum of 0x0A: neither ends the frame
        let mut frame = std::vec::Vec::from(&b"IMPROV\x01\x03\x0a"[..]);
        frame.extend_from_slice(b"\x01\x08 ab cd e"); // ten data bytes with printable ones and spaces
        frame.extend_from_slice(&[0x0a, b'\n']);
        let mut e = Term::new();
        let (echo, lines) = type_in(&mut e, &frame);
        assert!(lines.is_empty() && echo.is_empty(), "{echo:?} {lines:?}");
        let (_, lines) = type_in(&mut e, b"status\r");
        assert_eq!(lines, ["status"]);
    }

    #[test]
    fn a_marker_with_the_wrong_version_is_not_a_frame() {
        let mut e = Term::new();
        let (_, lines) = type_in(&mut e, b"IMPROV\x07status\r");
        assert_eq!(lines, ["status"]);
    }

    #[test]
    fn a_word_that_starts_like_the_marker_is_echoed_once_it_differs() {
        let mut e = Term::new();
        let (echo, lines) = type_in(&mut e, b"IMPx\r");
        assert_eq!(echo, "IMPx\r\n");
        assert_eq!(lines, ["IMPx"]);
        let (echo, _) = type_in(&mut e, b"I");
        assert_eq!(echo, "", "held while it could be a frame");
    }

    #[test]
    fn tab_completes_commands_and_their_words() {
        let mut e = Term::new();
        let (echo, _) = type_in(&mut e, b"wi\t");
        assert_eq!(echo, "wifi ");
        let (_, lines) = type_in(&mut e, b"cl\t\r");
        assert_eq!(lines, ["wifi clear"]);
        let mut e = Term::new();
        let (echo, _) = type_in(&mut e, b"s\t");
        assert!(echo.contains("status") && echo.contains("setpoint"), "{echo:?}");
    }

    #[test]
    fn tab_offers_typical_values_in_argument_positions() {
        let mut e = Term::new();
        let (echo, _) = type_in(&mut e, b"mqtt set broker \t");
        assert!(echo.contains("1883\r\n") && echo.contains("8883\r\n"), "{echo:?}");
        let mut e = Term::new();
        let (echo, _) = type_in(&mut e, b"otlp set c 4\t");
        assert_eq!(echo, "otlp set c 431", "the shared prefix first");
        let (echo, _) = type_in(&mut e, b"\t");
        assert!(
            echo.contains("4318\r\n") && echo.contains("4317\r\n"),
            "a second Tab lists them: {echo:?}"
        );
        let mut e = Term::new();
        let (echo, lines) = type_in(&mut e, b"setpoint 22\t\r");
        assert_eq!(echo, "setpoint 22".to_string() + "5 " + "\r\n");
        assert_eq!(lines, ["setpoint 225"]);
    }

    #[test]
    fn nothing_to_complete_and_bytes_that_cannot_be_typed_ring_the_bell() {
        let mut e = Term::new();
        let (echo, _) = type_in(&mut e, b"xyz\t");
        assert!(echo.ends_with('\x07'), "{echo:?}");
        let mut e = Term::new();
        let (echo, _) = type_in(&mut e, b"\x01\xE9");
        assert_eq!(echo, "\x07\x07");
        assert!(type_in(&mut e, b"\r").1 == [""], "and the line stays empty");
    }

    #[test]
    fn credential_lines_are_not_kept_for_the_up_arrow() {
        let mut e = Term::new();
        type_in(&mut e, b"wifi set net secret\rstatus\r");
        let (echo, _) = type_in(&mut e, b"\x1b[A");
        assert!(echo.contains("status") && !echo.contains("secret"), "{echo:?}");
        let (echo, _) = type_in(&mut e, b"\x1b[A");
        assert!(!echo.contains("secret"), "{echo:?}");
    }

    #[test]
    fn ctrl_c_cancels_and_ctrl_u_clears() {
        let mut e = Term::new();
        let (echo, lines) = type_in(&mut e, b"abc\x03def\x15ghi\r");
        assert_eq!(lines, ["ghi"]);
        assert!(echo.contains("^C"));
    }

    #[test]
    fn words_split_on_spaces_and_honour_quotes() {
        let Cmd::Wifi(Some(w)) = parse(r#"wifi set "my net" "pa ss\"w\\d""#).unwrap() else {
            panic!()
        };
        assert_eq!((w.ssid.as_str(), w.pass.as_str()), ("my net", r#"pa ss"w\d"#));
        assert!(matches!(
            parse("wifi set \"x").unwrap_err(),
            ParseError::Usage(_, "unterminated quote")
        ));
        assert_eq!(parse("").unwrap(), Cmd::Empty);
        assert_eq!(parse("   ").unwrap(), Cmd::Empty);
        // a quoted word is one word, an empty one too
        let Cmd::Mqtt(Some(m)) = parse(r#"mqtt set h 1883 "" """#).unwrap() else {
            panic!()
        };
        assert_eq!((m.user.as_str(), m.pass.as_str()), ("", ""));
    }

    #[test]
    fn errors_name_the_command_and_say_what_is_wrong() {
        assert_eq!(
            parse("mqtt set h 0").unwrap_err(),
            ParseError::Usage(spec("mqtt").unwrap().usage, "the port is 1 to 65535")
        );
        assert_eq!(parse("status now").unwrap_err(), ParseError::Usage("status", ""));
        assert_eq!(
            parse("wifi").unwrap_err(),
            ParseError::Usage(spec("wifi").unwrap().usage, "")
        );
        assert_eq!(
            parse("frobnicate x").unwrap_err(),
            ParseError::Unknown("frobnicate".try_into().unwrap())
        );
    }

    #[test]
    fn wifi_commands() {
        assert_eq!(
            parse("wifi set home hunter2").unwrap(),
            Cmd::Wifi(Some(Wifi {
                ssid: "home".try_into().unwrap(),
                pass: "hunter2".try_into().unwrap()
            }))
        );
        assert_eq!(
            parse("wifi set guest").unwrap(),
            Cmd::Wifi(Some(Wifi {
                ssid: "guest".try_into().unwrap(),
                pass: String::new()
            }))
        );
        assert_eq!(parse("wifi clear").unwrap(), Cmd::Wifi(None));
        assert!(parse("wifi").is_err() && parse("wifi set").is_err());
        let long = ["wifi set ", &"x".repeat(40)].concat();
        assert!(parse(&long).is_err(), "an SSID is 32 bytes at most");
    }

    #[test]
    fn mqtt_commands_take_the_defaults() {
        let Cmd::Mqtt(Some(m)) = parse("mqtt set broker.lan").unwrap() else {
            panic!()
        };
        assert_eq!(
            (m.host.as_str(), m.port, m.user.as_str(), m.pass.as_str()),
            ("broker.lan", 1883, "", "")
        );
        let Cmd::Mqtt(Some(m)) = parse("mqtt set 10.0.0.2 8883 egg \"a b\"").unwrap() else {
            panic!()
        };
        assert_eq!(
            (m.host.as_str(), m.port, m.user.as_str(), m.pass.as_str()),
            ("10.0.0.2", 8883, "egg", "a b")
        );
        assert_eq!(parse("mqtt clear").unwrap(), Cmd::Mqtt(None));
        assert!(parse("mqtt set h 0").is_err() && parse("mqtt set h 70000").is_err() && parse("mqtt set h x").is_err());
        assert!(parse("mqtt set h 1 u p extra").is_err());
    }

    #[test]
    fn otlp_commands_take_the_defaults_and_check_the_interval() {
        let Cmd::Otlp(Some(o)) = parse("otlp set collector.lan").unwrap() else {
            panic!()
        };
        assert_eq!((o.port, o.interval_s), (4318, 60));
        let Cmd::Otlp(Some(o)) = parse("otlp set c 4317 30").unwrap() else {
            panic!()
        };
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
    fn a_log_line_does_not_split_the_typed_line() {
        let live = Live::new();
        let mut e = Editor::new();
        // nobody has typed: the log line goes out as it is
        e.show(&live);
        assert!(live.wrap(b"I log\n").is_none());
        // half a command on the screen: erase, the log line, then the prompt and the command again
        for &b in b"wifi cl" {
            e.apply(Key::Char(b));
        }
        e.show(&live);
        let w = live.wrap(b"I log\n").unwrap();
        assert_eq!(w.as_slice(), b"\r\x1b[KI log\nminimeater> wifi cl");
        // edits are followed, and the line is shown empty once it is run
        e.apply(Key::Backspace);
        e.show(&live);
        assert!(live.wrap(b"x\n").unwrap().ends_with(b"minimeater> wifi c"));
        let step = e.apply(Key::Cr);
        assert!(step.line.is_some());
        e.hide(&live);
        assert!(live.wrap(b"x\n").is_none(), "nothing to redraw while a command runs");
        e.show(&live);
        assert!(live.wrap(b"x\n").unwrap().ends_with(b"minimeater> "));
        // the longest log line and the longest typed line fit
        let long = [b'a'; LOG_LINE];
        for _ in 0..LINE {
            e.apply(Key::Char(b'a'));
        }
        e.show(&live);
        assert!(live.wrap(&long).is_some());
    }

    #[test]
    fn factory_reset_needs_its_confirmation_word() {
        assert_eq!(parse("factory-reset").unwrap(), Cmd::FactoryReset(false));
        assert_eq!(parse("factory-reset erase").unwrap(), Cmd::FactoryReset(true));
        assert!(parse("factory-reset yes").is_err() && parse("factory-reset erase now").is_err());
        assert!(parse("factory-reset Erase").is_err() && parse("factoryreset erase").is_err());
        // a Tab never types the word for you
        assert_eq!(candidates("factory-reset ").len(), 0);
        assert_eq!(candidates("fac").as_slice(), &["factory-reset"]);
    }

    #[test]
    fn config_never_shows_a_password() {
        let s = Settings {
            wifi: Some(Wifi {
                ssid: "home".try_into().unwrap(),
                pass: "hunter2".try_into().unwrap(),
            }),
            mqtt: Some(Mqtt {
                host: "b".try_into().unwrap(),
                port: 1883,
                user: "u".try_into().unwrap(),
                pass: "s3cret".try_into().unwrap(),
            }),
            ..Settings::default()
        };
        let text: std::vec::Vec<std::string::String> = config_lines(&s).iter().map(|l| l.as_str().into()).collect();
        let all = text.join("\n");
        assert!(!all.contains("hunter2") && !all.contains("s3cret"), "{all}");
        assert!(all.contains("home") && all.contains("b:1883"));
    }
}
