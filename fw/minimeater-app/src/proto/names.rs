//! Wire names (docs/protocol.md) for the application's enums.

use minimeater_core::control::Fire;
use minimeater_core::food::Stall;

use crate::config::{ControllerKind, Mode};
use crate::pit::Phase;

pub fn mode(m: Mode) -> &'static str {
    match m {
        Mode::Off => "off",
        Mode::Run => "run",
        Mode::Manual => "manual",
    }
}

pub fn controller(c: ControllerKind) -> &'static str {
    match c {
        ControllerKind::Mpc => "mpc",
        ControllerKind::FfPi => "ffpi",
    }
}

pub fn phase(p: Phase) -> &'static str {
    match p {
        Phase::Idle => "idle",
        Phase::Startup => "startup",
        Phase::Hold => "hold",
        Phase::Lid => "lid",
        Phase::Recover => "recover",
        Phase::Manual => "manual",
        Phase::Fault => "fault",
    }
}

pub fn fire(f: Fire) -> &'static str {
    match f {
        Fire::Unknown => "unknown",
        Fire::Ok => "ok",
        Fire::Low => "low",
        Fire::Out => "out",
    }
}

pub fn stall(s: Stall) -> &'static str {
    match s {
        Stall::None => "none",
        Stall::In { .. } => "stall",
        Stall::Done => "done",
    }
}
