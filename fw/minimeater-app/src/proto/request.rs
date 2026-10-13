//! What the device is sent: `~/set/<setting>` and `~/cmd/<command>` (docs/protocol.md). [`parse`]
//! turns a topic suffix and a plain-text payload into a [`Request`], or the documented error. It
//! checks everything it can without state (syntax and limits); a mode or phase that does not allow
//! the request is up to whoever applies it (`not_allowed`).

use core::fmt::Write;

use heapless::String;
use minimeater_core::food::Cut;
use minimeater_core::thermistor::{Preset, Thermistor};

use crate::config::{BUMP_MAX_F, BUMP_MIN_F, ControllerKind, Mode, Mqtt, Otlp, ProbeKind, StallPolicy, Wifi};
use crate::hal::PROBE_JACKS;
use crate::pit::{Command, SP_MAX_F, SP_MIN_F};

/// One change to a probe's setup.
#[derive(Clone, Debug, PartialEq)]
pub enum ProbeChange {
    Name(String<24>),
    Kind(ProbeKind),
    /// `type` set to `custom`: allowed only for a probe that already has coefficients (the select in
    /// Home Assistant shows `custom` as an option; the curve itself comes from `coeffs`).
    KeepCustom,
    Offset(f64),
    /// Low alarm limit, `None` to disable.
    Lo(Option<f64>),
    Hi(Option<f64>),
    Cut(Cut),
    Stall(StallPolicy),
    /// F added to the setpoint by the `bump` stall policy.
    Bump(f64),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Request {
    /// For the pit loop: modes, setpoint, controller, lid, manual outputs, autotune, alarms, self-check.
    Pit(Command),
    Probe {
        n: usize,
        change: ProbeChange,
    },
    /// Network credentials from provisioning (`improv`), `None` to forget them. Not on MQTT: the wire cannot set it.
    Wifi(Option<Wifi>),
    /// The MQTT broker and the OTLP collector, from the serial REPL (`repl`), `None` to forget them. Not on MQTT either.
    Mqtt(Option<Mqtt>),
    Otlp(Option<Otlp>),
    /// Blink the LED for 10 s.
    Identify,
    Reboot,
}

impl From<Command> for Request {
    fn from(c: Command) -> Self {
        Request::Pit(c)
    }
}

/// `error` on the wire.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorCode {
    BadPayload,
    OutOfRange,
    Unknown,
    NotAllowed,
    Retained,
}

impl ErrorCode {
    pub fn name(self) -> &'static str {
        match self {
            ErrorCode::BadPayload => "bad_payload",
            ErrorCode::OutOfRange => "out_of_range",
            ErrorCode::Unknown => "unknown",
            ErrorCode::NotAllowed => "not_allowed",
            ErrorCode::Retained => "retained",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Error {
    pub code: ErrorCode,
    pub detail: String<48>,
}

impl Error {
    pub fn new(code: ErrorCode, detail: &str) -> Self {
        let mut d = String::new();
        // a detail that does not fit is cut, not an error
        for c in detail.chars() {
            if d.push(c).is_err() {
                break;
            }
        }
        Self { code, detail: d }
    }

    fn range(what: &str, v: f64, lo: f64, hi: f64) -> Self {
        let mut d = String::<48>::new();
        let _ = write!(d, "{what} {v} outside {lo}..{hi}");
        Self::new(ErrorCode::OutOfRange, &d)
    }
}

type Parsed = Result<Request, Error>;

/// `topic` is what follows the base topic, e.g. `set/sp`. A message with the retain flag set is
/// dropped (a stale retained `set` must never act after a reboot, ADR 0003).
pub fn parse(topic: &str, payload: &[u8], retained: bool) -> Parsed {
    if retained {
        return Err(Error::new(ErrorCode::Retained, "retained commands are dropped"));
    }
    let text = core::str::from_utf8(payload)
        .map_err(|_| Error::new(ErrorCode::BadPayload, "not UTF-8"))?
        .trim();
    if let Some(setting) = topic.strip_prefix("set/") {
        set(setting, text)
    } else if let Some(command) = topic.strip_prefix("cmd/") {
        cmd(command, text)
    } else {
        Err(Error::new(ErrorCode::Unknown, "not a set or cmd topic"))
    }
}

fn bad(detail: &str) -> Error {
    Error::new(ErrorCode::BadPayload, detail)
}

fn number(text: &str) -> Result<f64, Error> {
    text.parse::<f64>()
        .ok()
        .filter(|v| v.is_finite())
        .ok_or_else(|| bad("not a number"))
}

fn in_range(what: &str, text: &str, lo: f64, hi: f64) -> Result<f64, Error> {
    let v = number(text)?;
    if (lo..=hi).contains(&v) {
        Ok(v)
    } else {
        Err(Error::range(what, v, lo, hi))
    }
}

fn switch(text: &str) -> Result<bool, Error> {
    let is = |w: &str| text.eq_ignore_ascii_case(w);
    if is("on") || is("true") {
        Ok(true)
    } else if is("off") || is("false") {
        Ok(false)
    } else {
        Err(bad("expected ON or OFF"))
    }
}

fn set(setting: &str, text: &str) -> Parsed {
    let pit = |c| Ok(Request::Pit(c));
    match setting {
        "mode" => match text {
            "off" => pit(Command::Mode(Mode::Off)),
            // `heat` is the Home Assistant climate entity's word for run
            "run" | "heat" => pit(Command::Mode(Mode::Run)),
            "manual" => pit(Command::Mode(Mode::Manual)),
            _ => Err(bad("expected off, run or manual")),
        },
        "sp" => pit(Command::Sp(in_range("sp", text, SP_MIN_F, SP_MAX_F)?)),
        "pit_probe" => {
            if text == "tc" {
                return Err(Error::new(ErrorCode::NotAllowed, "no thermocouple fitted"));
            }
            let v = in_range("pit_probe", text, 0.0, (PROBE_JACKS - 1) as f64)?;
            if v != libm::trunc(v) {
                return Err(bad("expected a whole number"));
            }
            pit(Command::PitProbe(v as u8))
        }
        "controller" => match text {
            "mpc" => pit(Command::Controller(ControllerKind::Mpc)),
            "ffpi" => pit(Command::Controller(ControllerKind::FfPi)),
            _ => Err(bad("expected mpc or ffpi")),
        },
        "lid_detect" => pit(Command::LidDetect(switch(text)?)),
        "lid_boost" => pit(Command::LidBoost(in_range("lid_boost", text, 1.0, 3.0)?)),
        "manual_flap" => pit(Command::ManualFlap(in_range("manual_flap", text, 0.0, 100.0)?)),
        "manual_fan" => pit(Command::ManualFan(in_range("manual_fan", text, 0.0, 100.0)?)),
        _ => match setting.strip_prefix("probe/") {
            Some(rest) => probe(rest, text),
            None => Err(Error::new(ErrorCode::Unknown, "no such setting")),
        },
    }
}

fn probe(rest: &str, text: &str) -> Parsed {
    let (n, field) = rest
        .split_once('/')
        .ok_or_else(|| Error::new(ErrorCode::Unknown, "no such setting"))?;
    let n: usize = n
        .parse()
        .ok()
        .filter(|n| *n < PROBE_JACKS)
        .ok_or_else(|| Error::new(ErrorCode::Unknown, "no such probe"))?;
    let change = match field {
        "name" => {
            if text.is_empty() {
                return Err(Error::new(ErrorCode::OutOfRange, "a name is 1-24 bytes"));
            }
            ProbeChange::Name(
                String::try_from(text).map_err(|_| Error::new(ErrorCode::OutOfRange, "a name is 1-24 bytes"))?,
            )
        }
        "type" => match text {
            "custom" => ProbeChange::KeepCustom,
            t => ProbeChange::Kind(ProbeKind::Preset(
                Preset::from_name(t).ok_or_else(|| bad("expected thermoworks_pro, maverick_et732, generic_b3950"))?,
            )),
        },
        "coeffs" => {
            let (a, b, c) = coeffs(text)?;
            let t = Thermistor { a, b, c };
            if !t.plausible() {
                return Err(Error::new(ErrorCode::OutOfRange, "not a usable NTC curve"));
            }
            ProbeChange::Kind(ProbeKind::Custom(t))
        }
        "offset" => ProbeChange::Offset(in_range("offset", text, -10.0, 10.0)?),
        "lo" | "hi" => {
            let limit = if text.eq_ignore_ascii_case("off") {
                None
            } else {
                match in_range(field, text, 0.0, 700.0)? {
                    0.0 => None,
                    v => Some(v),
                }
            };
            if field == "lo" {
                ProbeChange::Lo(limit)
            } else {
                ProbeChange::Hi(limit)
            }
        }
        "cut" => ProbeChange::Cut(
            Cut::from_name(text).ok_or_else(|| bad("expected other, brisket, pork_shoulder, ribs, poultry"))?,
        ),
        "stall" => ProbeChange::Stall(StallPolicy::from_name(text).ok_or_else(|| bad("expected notify, wrap, bump"))?),
        "bump" => ProbeChange::Bump(in_range("bump", text, BUMP_MIN_F, BUMP_MAX_F)?),
        _ => return Err(Error::new(ErrorCode::Unknown, "no such probe setting")),
    };
    Ok(Request::Probe { n, change })
}

/// `{"a":..,"b":..,"c":..}`: three numbers, in any order, nothing else.
fn coeffs(text: &str) -> Result<(f64, f64, f64), Error> {
    let body = text
        .strip_prefix('{')
        .and_then(|t| t.strip_suffix('}'))
        .ok_or_else(|| bad("expected {\"a\":..,\"b\":..,\"c\":..}"))?;
    let (mut a, mut b, mut c) = (None, None, None);
    for field in body.split(',') {
        let (k, v) = field.split_once(':').ok_or_else(|| bad("expected \"key\":number"))?;
        let slot = match k.trim() {
            "\"a\"" => &mut a,
            "\"b\"" => &mut b,
            "\"c\"" => &mut c,
            _ => return Err(bad("only a, b and c")),
        };
        if slot.replace(number(v.trim())?).is_some() {
            return Err(bad("a coefficient twice"));
        }
    }
    match (a, b, c) {
        (Some(a), Some(b), Some(c)) => Ok((a, b, c)),
        _ => Err(bad("needs a, b and c")),
    }
}

fn cmd(command: &str, text: &str) -> Parsed {
    let guard = |literal: &str, r: Request| {
        if text == literal {
            Ok(r)
        } else {
            Err(bad("the payload must be the literal guard word"))
        }
    };
    match command {
        "autotune" => Ok(Request::Pit(Command::Autotune)),
        "autotune_stop" => Ok(Request::Pit(Command::AutotuneStop)),
        "model_reset" => guard("reset", Request::Pit(Command::ModelReset)),
        "selfcheck" => Ok(Request::Pit(Command::SelfCheck)),
        "alarm_ack" => Ok(Request::Pit(Command::AlarmAck)),
        "identify" => Ok(Request::Identify),
        "reboot" => guard("reboot", Request::Reboot),
        "ota" => Err(Error::new(ErrorCode::NotAllowed, "reserved")),
        _ => Err(Error::new(ErrorCode::Unknown, "no such command")),
    }
}
