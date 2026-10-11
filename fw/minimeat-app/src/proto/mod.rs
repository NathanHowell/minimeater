//! The MQTT protocol's payloads (`.5.6`, docs/protocol.md is normative): what the device publishes
//! ([`state`], [`config`], [`event`]) and how it reads what it is sent ([`request::parse`]). No
//! transport here: the client (`.5.2`) and Home Assistant discovery (`.5.3`) sit on top.

pub mod events;
pub mod json;
pub mod names;
pub mod request;
pub mod settings;
pub mod state;

use heapless::String;

/// Runs a writer into a buffer of `N` bytes; `None` if the payload did not fit.
pub fn render<const N: usize>(f: impl FnOnce(&mut Json<String<N>>)) -> Option<String<N>> {
    let mut j = Json::new(String::new());
    f(&mut j);
    j.finish().ok()
}

pub use events::{BootReason, boot, event, rejected};
pub use json::Json;
pub use request::{Error, ErrorCode, ProbeChange, Request, parse};
pub use settings::config;
pub use state::state;
