//! Hardware-independent minimeat logic, shared by the ESP32-C6 firmware and the host digital twin.
//!
//! Ported from the Python reference models in `src/minimeat/model/`; tests pin values computed there.
//! Floating point is f64 throughout: the ESP32-C6 has no FPU, so f32 would only halve a cost that is
//! already negligible at a 10 s control period, and f64 keeps parity with the reference.
#![cfg_attr(not(any(feature = "std", test)), no_std)]

pub mod control;
pub mod model;
pub mod numeric;
pub mod thermistor;
pub mod units;
