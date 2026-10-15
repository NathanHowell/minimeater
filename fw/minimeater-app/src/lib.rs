//! minimeater application: everything above the peripheral boundary. Generic over a [`hal::Board`], so
//! the ESP32-C6 binary (`minimeater-fw`) and the host twin (`minimeater-sim`, `.4.16.2`) run the same
//! code; this crate never touches esp-hal and has no target `cfg`s.
#![cfg_attr(not(test), no_std)]

pub mod alarms;
pub mod ambient;
pub mod config;
pub mod drive;
pub mod drivers;
pub mod food;
pub mod hal;
pub mod http;
pub mod improv;
pub mod mdns;
pub mod mqtt;
pub mod net;
pub mod otlp;
pub mod pit;
pub mod probes;
pub mod proto;
pub mod repl;
pub mod selftest;
pub mod serial;
pub mod sntp;
pub mod tasks;
pub mod ui;
