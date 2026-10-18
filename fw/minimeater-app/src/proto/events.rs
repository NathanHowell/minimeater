//! `~/event` (docs/protocol.md): what happened, one JSON object each, `event_type` first.

use core::fmt::Write;

use super::json::Json;
use crate::pit::Event;
use crate::probes::Check;

fn head<W: Write>(j: &mut Json<W>, kind: &str) {
    j.begin_obj();
    j.key("v").int(1);
    j.key("event_type").str(kind);
}

/// Writes the `~/event` object for an event the control loop raised.
pub fn event<W: Write>(j: &mut Json<W>, e: &Event) {
    if let Event::Boot(reason) = e {
        return boot(j, *reason, env!("CARGO_PKG_VERSION"));
    }
    head(j, e.name());
    match e {
        Event::LidOpen { pit_f } | Event::LidClosed { pit_f } => {
            j.key("pit").num(*pit_f, 1);
        }
        Event::FireLow { pit_f, q } | Event::FireOut { pit_f, q } => {
            j.key("pit").num(*pit_f, 1);
            j.key("q").num(*q, 2);
        }
        Event::SetpointReached { sp_f } => {
            j.key("sp").num(*sp_f, 1);
        }
        Event::StallStart { probe, t_f } | Event::StallEnd { probe, t_f } | Event::WrapReminder { probe, t_f } => {
            j.key("probe").int(i64::from(*probe));
            j.key("t").num(*t_f, 1);
        }
        Event::StallBump { probe, sp_f } => {
            j.key("probe").int(i64::from(*probe));
            j.key("sp").num(*sp_f, 1);
        }
        Event::StallRestore { sp_f } => {
            j.key("sp").num(*sp_f, 1);
        }
        Event::AutotuneDone { model, accepted } => {
            j.key("sp").num(model.sp_f, 1);
            j.key("k").num(model.k, 1);
            j.key("tau").num(model.tau_s, 0);
            j.key("dead").num(model.theta_s, 0);
            j.key("accepted").int(i64::from(*accepted));
        }
        Event::Alarm { probe, which, t_f } | Event::AlarmClear { probe, which, t_f } => {
            j.key("probe").int(i64::from(*probe));
            j.key("which").str(which.name());
            j.key("t").num(*t_f, 1);
        }
        Event::SelfCheck { results } => {
            j.key("results").begin_arr();
            for (n, r) in results.iter().enumerate() {
                j.begin_obj();
                j.key("probe").int(n as i64);
                match r {
                    Check::Delta(d) => {
                        j.key("delta").num(*d, 1);
                    }
                    Check::St(st) => {
                        j.key("st").str(st.name());
                    }
                }
                j.end_obj();
            }
            j.end_arr();
        }
        Event::Fault(code) | Event::FaultClear(code) => {
            j.key("code").str(code.name());
        }
        Event::ServoHome(Ok(p)) => {
            j.key("ok").bool(true);
            j.key("closed").int(i64::from(p.closed_us));
            j.key("open").int(i64::from(p.open_us));
        }
        Event::ServoHome(Err(why)) => {
            j.key("ok").bool(false);
            j.key("why").str(why.name());
        }
        Event::Boot(_) => {}
    }
    j.end_obj();
}

/// A rejected `set` or `cmd`: `topic` is the suffix after the base topic, e.g. `set/sp`.
pub fn rejected<W: Write>(j: &mut Json<W>, topic: &str, error: &str, detail: &str) {
    head(j, "rejected");
    j.key("topic").str(topic);
    j.key("error").str(error);
    j.key("detail").str(detail);
    j.end_obj();
}

/// The result published on a request's MQTT 5 response topic (docs/protocol.md).
pub fn response<W: Write>(j: &mut Json<W>, result: &Result<(), super::request::Error>) {
    j.begin_obj();
    j.key("v").int(1);
    match result {
        Ok(()) => {
            j.key("ok").bool(true);
        }
        Err(e) => {
            j.key("ok").bool(false);
            j.key("error").str(e.code.name());
            j.key("detail").str(&e.detail);
        }
    }
    j.end_obj();
}

/// Why the device started.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BootReason {
    PowerOn,
    Watchdog,
    Panic,
    Ota,
    User,
}

impl BootReason {
    pub fn name(self) -> &'static str {
        match self {
            BootReason::PowerOn => "power_on",
            BootReason::Watchdog => "watchdog",
            BootReason::Panic => "panic",
            BootReason::Ota => "ota",
            BootReason::User => "user",
        }
    }
}

pub fn boot<W: Write>(j: &mut Json<W>, reason: BootReason, sw: &str) {
    head(j, "boot");
    j.key("reason").str(reason.name());
    j.key("sw").str(sw);
    j.end_obj();
}
