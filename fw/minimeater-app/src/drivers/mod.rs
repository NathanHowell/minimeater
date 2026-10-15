//! Drivers for the board's chips, generic over `embedded-hal(-async)` buses so they run on the
//! ESP32-C6 and are tested on the host. Each implements a [`crate::hal`] trait.

pub mod ads122c04;
pub mod atecc608;
pub mod cert;
pub mod csr;
pub mod identity;
pub mod tmp117;
pub mod usb_cc;

#[cfg(test)]
pub(crate) mod mock;
