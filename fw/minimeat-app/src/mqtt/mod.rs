//! MQTT client (`.5.2`): the packet codec and, above it, the session that publishes the device's
//! topics and takes its commands.

pub mod packet;
pub mod session;

pub use session::{Buffers, Connector, Identity, run};
