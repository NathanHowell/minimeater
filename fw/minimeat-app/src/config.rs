//! Persistent configuration (`.4.9`): everything a power cut must not lose, in the board's config
//! flash region, through `sequential-storage`'s map (wear-levelled, and power-fail safe: an
//! interrupted write is detected by CRC and repaired on the next access).
//!
//! **Loading never fails.** Every record is read and validated on its own; a missing, corrupt or
//! out-of-range record falls back to its default and the rest still load. If the store itself is
//! unreadable it is erased and the device starts from defaults. A board that boots mid-cook with
//! a readable store resumes exactly: mode, setpoint, controller, autotune excitation, the learned
//! schedule and the probe setup.
//!
//! **Schema evolution:** each record has its own key and a hand-written little-endian layout. A
//! compatible change appends fields (decoders accept the shorter old layout and default the rest);
//! an incompatible one takes a new key, and [`ConfigStore::load`] migrates the old key once.
//!
//! **Wear:** [`ConfigStore`] skips writes of unchanged values. Callers rate-limit the schedule
//! (autotune can update it every few minutes); a few records an hour is far inside the flash's
//! endurance with a 24 KB region.

use embedded_storage_async::nor_flash::NorFlash;
use heapless::String;
use log::warn;
use minimeat_core::control::{Fopdt, Schedule};
use minimeat_core::food::Cut;
use minimeat_core::thermistor::{Preset, Thermistor};
use sequential_storage::cache::{Cache, Uncached};
use sequential_storage::map::{MapConfig, MapStorage, SerializationError, Value};

use crate::hal::PROBE_JACKS;

/// Record keys. Never reuse a retired key.
mod key {
    pub const COOK: u16 = 0x0101;
    pub const SCHEDULE: u16 = 0x0102;
    pub const LID: u16 = 0x0103;
    pub const PROBE0: u16 = 0x0110;
    pub const WIFI: u16 = 0x0201;
    pub const MQTT: u16 = 0x0202;
    pub const OTLP: u16 = 0x0203;
}

/// Largest serialized record plus key, word-aligned (a full schedule: 2 + 1 + 8 x 32 = 259).
const BUF: usize = 512;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Off,
    Run,
    Manual,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ControllerKind {
    Mpc,
    FfPi,
}

/// The cook in progress: what to resume after a power cut.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cook {
    pub mode: Mode,
    pub sp_f: f64,
    pub controller: ControllerKind,
    /// Jack holding the pit probe.
    pub pit_probe: u8,
    /// Autotune excitation was running.
    pub autotune: bool,
    /// Manual-mode outputs, % of travel / duty.
    pub manual_flap: f64,
    pub manual_fan: f64,
    /// The airflow that last held the pit at a setpoint, (setpoint F, CFM), so a reboot does not have
    /// to relearn how this egg differs from the model (`.4.14`). Cleared when a cook starts.
    pub hold: Option<(f64, f64)>,
    /// The setpoint a stall bump (`.4.23`) raised `sp_f` from, to go back to when the stall breaks.
    /// Cleared by any setpoint the user sets.
    pub base_sp: Option<f64>,
}

impl Default for Cook {
    fn default() -> Self {
        Self {
            mode: Mode::Off,
            sp_f: 225.0,
            controller: ControllerKind::Mpc,
            pit_probe: 0,
            autotune: false,
            manual_flap: 0.0,
            manual_fan: 0.0,
            hold: None,
            base_sp: None,
        }
    }
}

/// What a food probe's stall brings about (`.4.23`); `stall_start` and `stall_end` are raised whatever
/// it is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum StallPolicy {
    #[default]
    Notify,
    /// A `wrap_reminder` event at the stall's onset.
    Wrap,
    /// Raise the setpoint by the probe's `bump` until the stall breaks.
    Bump,
}

impl StallPolicy {
    pub const ALL: [Self; 3] = [Self::Notify, Self::Wrap, Self::Bump];

    pub fn name(self) -> &'static str {
        match self {
            Self::Notify => "notify",
            Self::Wrap => "wrap",
            Self::Bump => "bump",
        }
    }

    pub fn from_name(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|p| p.name() == s)
    }
}

/// Bounds on a stall bump, F, and its default.
pub const BUMP_MIN_F: f64 = 5.0;
pub const BUMP_MAX_F: f64 = 50.0;
pub const BUMP_DEFAULT_F: f64 = 25.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Lid {
    pub detect: bool,
    pub boost: f64,
}

impl Default for Lid {
    fn default() -> Self {
        Self {
            detect: true,
            boost: 1.5,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ProbeKind {
    Preset(Preset),
    Custom(Thermistor),
}

impl ProbeKind {
    pub fn thermistor(self) -> Thermistor {
        match self {
            ProbeKind::Preset(p) => p.thermistor(),
            ProbeKind::Custom(t) => t,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Probe {
    pub name: String<24>,
    pub kind: ProbeKind,
    pub offset_f: f64,
    pub lo_f: Option<f64>,
    pub hi_f: Option<f64>,
    /// Food estimator priors and default target (`.4.22`).
    pub cut: Cut,
    /// What a stall does (`.4.23`), and by how much `bump` raises the setpoint, F.
    pub stall: StallPolicy,
    pub bump_f: f64,
}

impl Probe {
    pub(crate) fn default_for(jack: usize) -> Self {
        let mut name = String::new();
        let _ = core::fmt::write(&mut name, format_args!("Probe {}", jack + 1));
        Self {
            name,
            kind: ProbeKind::Preset(Preset::ThermoworksPro),
            offset_f: 0.0,
            lo_f: None,
            hi_f: None,
            cut: Cut::Other,
            stall: StallPolicy::Notify,
            bump_f: BUMP_DEFAULT_F,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Wifi {
    pub ssid: String<32>,
    pub pass: String<64>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Mqtt {
    pub host: String<64>,
    pub port: u16,
    pub user: String<32>,
    pub pass: String<64>,
}

/// Where metrics go (`.4.11`): an OTLP/HTTP collector, every `interval_s`.
#[derive(Clone, Debug, PartialEq)]
pub struct Otlp {
    pub host: String<64>,
    /// 4318 is the collector's OTLP/HTTP default.
    pub port: u16,
    pub interval_s: u16,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub cook: Cook,
    /// The learned model schedule; the nominal one until autotune has learned something.
    pub schedule: Schedule,
    pub lid: Lid,
    pub probes: [Probe; PROBE_JACKS],
    pub wifi: Option<Wifi>,
    pub mqtt: Option<Mqtt>,
    pub otlp: Option<Otlp>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            cook: Cook::default(),
            schedule: Schedule::default(),
            lid: Lid::default(),
            probes: core::array::from_fn(Probe::default_for),
            wifi: None,
            mqtt: None,
            otlp: None,
        }
    }
}

// ----------------------------------------------------------------------------- codec

struct W<'a> {
    b: &'a mut [u8],
    n: usize,
}

impl W<'_> {
    fn bytes(&mut self, v: &[u8]) -> Result<(), SerializationError> {
        let d = self
            .b
            .get_mut(self.n..self.n + v.len())
            .ok_or(SerializationError::BufferTooSmall)?;
        d.copy_from_slice(v);
        self.n += v.len();
        Ok(())
    }
    fn u8(&mut self, v: u8) -> Result<(), SerializationError> {
        self.bytes(&[v])
    }
    fn u16(&mut self, v: u16) -> Result<(), SerializationError> {
        self.bytes(&v.to_le_bytes())
    }
    fn f64(&mut self, v: f64) -> Result<(), SerializationError> {
        self.bytes(&v.to_le_bytes())
    }
    fn opt(&mut self, v: Option<f64>) -> Result<(), SerializationError> {
        self.f64(v.unwrap_or(f64::NAN))
    }
    fn str(&mut self, v: &str) -> Result<(), SerializationError> {
        self.u8(v.len() as u8)?;
        self.bytes(v.as_bytes())
    }
}

struct R<'a> {
    b: &'a [u8],
    n: usize,
}

impl R<'_> {
    fn bytes(&mut self, len: usize) -> Option<&[u8]> {
        let s = self.b.get(self.n..self.n + len)?;
        self.n += len;
        Some(s)
    }
    fn u8(&mut self) -> Option<u8> {
        self.bytes(1).map(|b| b[0])
    }
    fn u16(&mut self) -> Option<u16> {
        self.bytes(2).map(|b| u16::from_le_bytes([b[0], b[1]]))
    }
    fn f64(&mut self) -> Option<f64> {
        self.bytes(8).map(|b| f64::from_le_bytes(b.try_into().unwrap()))
    }
    /// A finite value within `lo..=hi`.
    fn f64_in(&mut self, lo: f64, hi: f64) -> Option<f64> {
        self.f64().filter(|v| (lo..=hi).contains(v))
    }
    fn opt_in(&mut self, lo: f64, hi: f64) -> Option<Option<f64>> {
        let v = self.f64()?;
        if v.is_nan() {
            Some(None)
        } else {
            (lo..=hi).contains(&v).then_some(Some(v))
        }
    }
    fn str<const N: usize>(&mut self) -> Option<String<N>> {
        let len = self.u8()? as usize;
        let s = core::str::from_utf8(self.bytes(len)?).ok()?;
        String::try_from(s).ok()
    }
    /// Present only in newer layouts: `None` at the end of an older record.
    fn more(&self) -> bool {
        self.n < self.b.len()
    }
}

trait Record: Sized {
    fn encode(&self, w: &mut W) -> Result<(), SerializationError>;
    /// Validates as it decodes: anything out of range rejects the record.
    fn decode(r: &mut R) -> Option<Self>;
}

/// Adapts a [`Record`] to sequential-storage's [`Value`].
struct Item<T>(T);

impl<'a, T: Record> Value<'a> for Item<T> {
    fn serialize_into(&self, buffer: &mut [u8]) -> Result<usize, SerializationError> {
        let mut w = W { b: buffer, n: 0 };
        self.0.encode(&mut w)?;
        Ok(w.n)
    }

    fn deserialize_from(buffer: &'a [u8]) -> Result<(Self, usize), SerializationError> {
        let mut r = R { b: buffer, n: 0 };
        let v = T::decode(&mut r).ok_or(SerializationError::InvalidData)?;
        Ok((Item(v), buffer.len()))
    }
}

impl Record for Cook {
    fn encode(&self, w: &mut W) -> Result<(), SerializationError> {
        w.u8(self.mode as u8)?;
        w.f64(self.sp_f)?;
        w.u8(self.controller as u8)?;
        w.u8(self.pit_probe)?;
        w.u8(self.autotune.into())?;
        w.f64(self.manual_flap)?;
        w.f64(self.manual_fan)?;
        let (sp, q) = self.hold.unzip();
        w.opt(sp)?;
        w.opt(q)?;
        w.opt(self.base_sp)
    }

    fn decode(r: &mut R) -> Option<Self> {
        let mode = [Mode::Off, Mode::Run, Mode::Manual].get(r.u8()? as usize).copied()?;
        let sp_f = r.f64_in(150.0, 325.0)?;
        let controller = [ControllerKind::Mpc, ControllerKind::FfPi]
            .get(r.u8()? as usize)
            .copied()?;
        let pit_probe = r.u8().filter(|&j| (j as usize) < PROBE_JACKS)?;
        let autotune = r.u8()? != 0;
        Some(Self {
            mode,
            sp_f,
            controller,
            pit_probe,
            autotune,
            manual_flap: r.f64_in(0.0, 100.0)?,
            manual_fan: r.f64_in(0.0, 100.0)?,
            hold: if r.more() {
                // a bad hold costs only itself: it is a hint
                match (r.opt_in(150.0, 325.0), r.opt_in(0.0, 20.0)) {
                    (Some(Some(sp)), Some(Some(q))) => Some((sp, q)),
                    _ => None,
                }
            } else {
                None
            },
            // a bad base costs the bump's restore, not the cook: the setpoint stands
            base_sp: if r.more() {
                r.opt_in(150.0, 325.0).flatten()
            } else {
                None
            },
        })
    }
}

impl Record for Schedule {
    fn encode(&self, w: &mut W) -> Result<(), SerializationError> {
        let ms = self.models();
        w.u8(ms.len() as u8)?;
        for m in ms {
            for v in [m.sp_f, m.k, m.tau_s, m.theta_s] {
                w.f64(v)?;
            }
        }
        Ok(())
    }

    fn decode(r: &mut R) -> Option<Self> {
        let n = r.u8()? as usize;
        if n == 0 || n > Schedule::CAPACITY {
            return None;
        }
        let mut models = [Fopdt {
            sp_f: 0.0,
            k: 0.0,
            tau_s: 0.0,
            theta_s: 0.0,
        }; Schedule::CAPACITY];
        for m in models.iter_mut().take(n) {
            // plausibility bounds well outside anything autotune can produce from the nominal model
            *m = Fopdt {
                sp_f: r.f64_in(100.0, 400.0)?,
                k: r.f64_in(1.0, 2000.0)?,
                tau_s: r.f64_in(30.0, 20_000.0)?,
                theta_s: r.f64_in(0.0, 1200.0)?,
            };
        }
        Some(Schedule::new(&models[..n]))
    }
}

impl Record for Lid {
    fn encode(&self, w: &mut W) -> Result<(), SerializationError> {
        w.u8(self.detect.into())?;
        w.f64(self.boost)
    }

    fn decode(r: &mut R) -> Option<Self> {
        Some(Self {
            detect: r.u8()? != 0,
            boost: r.f64_in(1.0, 3.0)?,
        })
    }
}

impl Record for Probe {
    fn encode(&self, w: &mut W) -> Result<(), SerializationError> {
        w.str(&self.name)?;
        match self.kind {
            ProbeKind::Preset(p) => {
                w.u8(Preset::ALL.iter().position(|&x| x == p).unwrap_or(0) as u8)?;
            }
            ProbeKind::Custom(t) => {
                w.u8(0xFF)?;
                w.f64(t.a)?;
                w.f64(t.b)?;
                w.f64(t.c)?;
            }
        }
        w.f64(self.offset_f)?;
        w.opt(self.lo_f)?;
        w.opt(self.hi_f)?;
        w.u8(Cut::ALL.iter().position(|&c| c == self.cut).unwrap_or(0) as u8)?;
        w.u8(StallPolicy::ALL.iter().position(|&p| p == self.stall).unwrap_or(0) as u8)?;
        w.f64(self.bump_f)
    }

    fn decode(r: &mut R) -> Option<Self> {
        let name = r.str::<24>().filter(|n| !n.is_empty())?;
        let kind = match r.u8()? {
            0xFF => {
                let t = Thermistor {
                    a: r.f64()?,
                    b: r.f64()?,
                    c: r.f64()?,
                };
                ProbeKind::Custom(t)
                    .thermistor()
                    .plausible()
                    .then_some(ProbeKind::Custom(t))?
            }
            i => ProbeKind::Preset(*Preset::ALL.get(i as usize)?),
        };
        Some(Self {
            name,
            kind,
            offset_f: r.f64_in(-10.0, 10.0)?,
            lo_f: r.opt_in(0.0, 700.0)?,
            hi_f: r.opt_in(0.0, 700.0)?,
            cut: if r.more() {
                *Cut::ALL.get(r.u8()? as usize)?
            } else {
                Cut::Other
            },
            stall: if r.more() {
                *StallPolicy::ALL.get(r.u8()? as usize)?
            } else {
                StallPolicy::Notify
            },
            bump_f: if r.more() {
                r.f64_in(BUMP_MIN_F, BUMP_MAX_F)?
            } else {
                BUMP_DEFAULT_F
            },
        })
    }
}

impl Record for Wifi {
    fn encode(&self, w: &mut W) -> Result<(), SerializationError> {
        w.str(&self.ssid)?;
        w.str(&self.pass)
    }

    fn decode(r: &mut R) -> Option<Self> {
        Some(Self {
            ssid: r.str()?,
            pass: r.str()?,
        })
    }
}

impl Record for Mqtt {
    fn encode(&self, w: &mut W) -> Result<(), SerializationError> {
        w.str(&self.host)?;
        w.u16(self.port)?;
        w.str(&self.user)?;
        w.str(&self.pass)
    }

    fn decode(r: &mut R) -> Option<Self> {
        let host = r.str()?;
        let port = r.u16().filter(|&p| p != 0)?;
        let (user, pass) = if r.more() {
            (r.str()?, r.str()?)
        } else {
            (String::new(), String::new())
        };
        Some(Self { host, port, user, pass })
    }
}

impl Record for Otlp {
    fn encode(&self, w: &mut W) -> Result<(), SerializationError> {
        w.str(&self.host)?;
        w.u16(self.port)?;
        w.u16(self.interval_s)
    }

    fn decode(r: &mut R) -> Option<Self> {
        Some(Self {
            host: r.str()?,
            port: r.u16().filter(|&p| p != 0)?,
            interval_s: r.u16().filter(|&i| (5..=3600).contains(&i))?,
        })
    }
}

// ----------------------------------------------------------------------------- store

/// Storage failed (the flash itself, or a full store). Saving is best effort: the caller logs this
/// and carries on; control never depends on it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StoreError;

pub struct ConfigStore<F: NorFlash> {
    map: MapStorage<u16, F, Cache<Uncached, Uncached, Uncached, u16>>,
    buf: [u8; BUF],
    /// What flash holds now, to skip redundant writes.
    saved: Settings,
}

impl<F: NorFlash> ConfigStore<F> {
    /// Opens the store over all of `flash` (a [`crate::hal::FlashRegion`] on the target) and loads
    /// it. Never fails; see the module docs.
    pub async fn open(flash: F) -> (Self, Settings) {
        let len = flash.capacity() as u32;
        let len = len - len % F::ERASE_SIZE as u32;
        let mut s = Self {
            map: MapStorage::new(flash, MapConfig::new(0..len), Cache::new_uncached()),
            buf: [0; BUF],
            saved: Settings::default(),
        };
        let settings = s.load().await;
        s.saved = settings.clone();
        (s, settings)
    }

    async fn fetch<T: Record>(&mut self, k: u16) -> Result<Option<T>, ()> {
        match self.map.fetch_item::<Item<T>>(&mut self.buf, &k).await {
            Ok(v) => Ok(v.map(|i| i.0)),
            Err(sequential_storage::Error::SerializationError(_)) => {
                warn!("config: record {k:#06x} invalid, using its default");
                Ok(None)
            }
            Err(e) => {
                warn!("config: store unreadable ({e:?})");
                Err(())
            }
        }
    }

    async fn load(&mut self) -> Settings {
        match self.try_load().await {
            Ok(s) => s,
            Err(()) => {
                warn!("config: erasing the store and starting from defaults");
                let _ = self.map.erase_all().await;
                Settings::default()
            }
        }
    }

    async fn try_load(&mut self) -> Result<Settings, ()> {
        let mut s = Settings::default();
        if let Some(v) = self.fetch(key::COOK).await? {
            s.cook = v;
        }
        if let Some(v) = self.fetch(key::SCHEDULE).await? {
            s.schedule = v;
        }
        if let Some(v) = self.fetch(key::LID).await? {
            s.lid = v;
        }
        for (i, p) in s.probes.iter_mut().enumerate() {
            if let Some(v) = self.fetch(key::PROBE0 + i as u16).await? {
                *p = v;
            }
        }
        // forgetting the credentials is stored as an empty SSID (a tombstone); provisioning never accepts one
        s.wifi = self.fetch::<Wifi>(key::WIFI).await?.filter(|w| !w.ssid.is_empty());
        // the same for the broker and the collector: an empty host is "forgotten"
        s.mqtt = self.fetch::<Mqtt>(key::MQTT).await?.filter(|m| !m.host.is_empty());
        s.otlp = self.fetch::<Otlp>(key::OTLP).await?.filter(|o| !o.host.is_empty());
        Ok(s)
    }

    async fn store<T: Record>(&mut self, k: u16, v: T) -> Result<(), StoreError> {
        self.map.store_item(&mut self.buf, &k, &Item(v)).await.map_err(|e| {
            warn!("config: store {k:#06x} failed ({e:?})");
            StoreError
        })
    }

    /// Writes whatever differs from what flash holds. Cheap when nothing changed.
    pub async fn save(&mut self, s: &Settings) -> Result<(), StoreError> {
        if s.cook != self.saved.cook {
            self.store(key::COOK, s.cook).await?;
            self.saved.cook = s.cook;
        }
        if s.schedule.models() != self.saved.schedule.models() {
            self.store(key::SCHEDULE, s.schedule.clone()).await?;
            self.saved.schedule = s.schedule.clone();
        }
        if s.lid != self.saved.lid {
            self.store(key::LID, s.lid).await?;
            self.saved.lid = s.lid;
        }
        for i in 0..PROBE_JACKS {
            if s.probes[i] != self.saved.probes[i] {
                self.store(key::PROBE0 + i as u16, s.probes[i].clone()).await?;
                self.saved.probes[i] = s.probes[i].clone();
            }
        }
        if s.wifi != self.saved.wifi {
            let w = s.wifi.clone().unwrap_or(Wifi {
                ssid: String::new(),
                pass: String::new(),
            });
            self.store(key::WIFI, w).await?;
            self.saved.wifi = s.wifi.clone();
        }
        if s.mqtt != self.saved.mqtt {
            let m = s.mqtt.clone().unwrap_or(Mqtt {
                host: String::new(),
                port: 1883,
                user: String::new(),
                pass: String::new(),
            });
            self.store(key::MQTT, m).await?;
            self.saved.mqtt = s.mqtt.clone();
        }
        if s.otlp != self.saved.otlp {
            let o = s.otlp.clone().unwrap_or(Otlp {
                host: String::new(),
                port: 4318,
                interval_s: 60,
            });
            self.store(key::OTLP, o).await?;
            self.saved.otlp = s.otlp.clone();
        }
        Ok(())
    }

    /// Factory reset: erase everything (network credentials included).
    pub async fn erase(&mut self) -> Result<(), StoreError> {
        self.map.erase_all().await.map_err(|_| StoreError)?;
        self.saved = Settings::default();
        Ok(())
    }
}
