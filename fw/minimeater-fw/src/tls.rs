//! The board's side of TLS (`minimeat-seq.5.5`, ADR 0006): random numbers, the wall clock for certificate dates, and the
//! `minimeater_app::tls::Client` made of them, the secure element and two record buffers.

use embassy_time::Delay;
use esp_hal::rng::Trng;
use minimeater_app::tls::TlsClock;
use rand_core::{CryptoRng, Error, RngCore};

use crate::board::ChipBus;

/// True random numbers from the radio's entropy, taken fresh for each request. TLS only runs while the network is up, which
/// is when the radio is on; before that [`Trng::try_new`] says no and `fill_bytes` panics (a reboot), rather than hand out
/// numbers that are not random.
pub struct DeviceRng;

impl RngCore for DeviceRng {
    fn next_u32(&mut self) -> u32 {
        Trng::try_new().expect("an entropy source (the radio) for TLS").random()
    }

    fn next_u64(&mut self) -> u64 {
        u64::from(self.next_u32()) << 32 | u64::from(self.next_u32())
    }

    fn fill_bytes(&mut self, dest: &mut [u8]) {
        self.try_fill_bytes(dest)
            .expect("an entropy source (the radio) for TLS")
    }

    fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), Error> {
        let mut trng = Trng::try_new().map_err(|_| Error::from(core::num::NonZeroU32::MIN))?;
        RngCore::fill_bytes(&mut trng, dest);
        Ok(())
    }
}

impl CryptoRng for DeviceRng {}

/// Unix seconds from SNTP, for a certificate's validity dates. Until SNTP has answered there is no time and the dates are not
/// checked (the signature and the name still are); `minimeat-n1q` has the details.
pub struct DeviceClock;

impl TlsClock for DeviceClock {
    fn now() -> Option<u64> {
        crate::net::unix_ns().map(|ns| ns / 1_000_000_000)
    }
}

/// The TLS the MQTT session uses.
pub type Client = minimeater_app::tls::Client<'static, DeviceRng, DeviceClock, ChipBus, Delay>;
