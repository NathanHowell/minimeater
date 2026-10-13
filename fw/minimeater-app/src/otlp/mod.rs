//! OpenTelemetry metrics export (`.4.11`): the device's state as OTLP gauges, POSTed as protobuf
//! to a collector's `/v1/metrics` (OTLP/HTTP) at a configurable interval. There is no `no_std`
//! OpenTelemetry SDK, so the few messages needed are encoded by hand ([`pb`]).
//!
//! Metrics carry the resource attributes `service.name`, `service.instance.id` (the device id) and
//! `service.version`. The controller is model-predictive, so there are no P/I/D terms to export;
//! the commanded airflow is `minimeater.airflow`.

pub mod pb;

use embassy_time::{Duration, Timer, with_timeout};
use embedded_io_async::{Read, Write};
use heapless::String;
use log::{info, warn};
use minimeater_core::units::k_to_f;

use self::pb::Pb;
use crate::net::{Connector, Identity};
use crate::pit::FaultCode;
use crate::proto::names;
use crate::tasks::{Bus, Status};

const SERVICE: &str = "minimeater";
const SCOPE: &str = "minimeater-fw";

/// What a metrics export is made from.
pub struct Sample<'a> {
    pub status: &'a Status,
    pub names: [&'a str; 4],
    pub rssi: Option<i32>,
    pub heap_free: Option<u32>,
    /// Wall-clock now, ns since the Unix epoch.
    pub unix_ns: u64,
}

/// An attribute value.
enum V<'a> {
    S(&'a str),
    I(i64),
}

fn any_value(p: &mut Pb, v: &V) {
    match v {
        V::S(s) => p.string(1, s),
        V::I(i) => p.int(3, *i),
    }
}

fn key_value(p: &mut Pb, field: u32, k: &str, v: &V) {
    p.message(field, |p| {
        p.string(1, k);
        p.message(2, |p| any_value(p, v));
    });
}

/// One gauge with `points` as its data points.
fn gauge(p: &mut Pb, name: &str, unit: &str, description: &str, points: impl FnOnce(&mut Pb)) {
    p.message(2, |p| {
        p.string(1, name);
        p.string(2, description);
        p.string(3, unit);
        p.message(5, points);
    });
}

fn point(p: &mut Pb, t: u64, v: f64, attrs: &[(&str, V)]) {
    p.message(1, |p| {
        for (k, a) in attrs {
            key_value(p, 7, k, a);
        }
        p.fixed64(3, t);
        p.double(4, v);
    });
}

/// Encodes an `ExportMetricsServiceRequest` into `buf`; its length, or `None` if it did not fit.
pub fn export_request(buf: &mut [u8], who: &Identity, s: &Sample) -> Option<usize> {
    let mut p = Pb::new(buf);
    let (st, t) = (s.status, s.unix_ns);
    p.message(1, |p| {
        // resource: who is reporting
        p.message(1, |p| {
            key_value(p, 1, "service.name", &V::S(SERVICE));
            key_value(p, 1, "service.instance.id", &V::S(who.id));
            key_value(p, 1, "service.version", &V::S(who.sw));
        });
        p.message(2, |p| {
            p.message(1, |p| {
                p.string(1, SCOPE);
                p.string(2, who.sw);
            });
            let one = |p: &mut Pb, name: &str, unit: &str, d: &str, v: f64| {
                gauge(p, name, unit, d, |p| point(p, t, v, &[]));
            };
            let pit = st.temps.get(st.cook.pit_probe as usize).copied().flatten();
            gauge(p, "minimeater.probe.temperature", "degF", "Probe temperature", |p| {
                for (n, temp) in st.temps.iter().enumerate() {
                    if let Some(temp) = temp {
                        point(p, t, *temp, &[("probe", V::I(n as i64)), ("name", V::S(s.names[n]))]);
                    }
                }
            });
            if let Some(v) = pit {
                one(p, "minimeater.pit.temperature", "degF", "Pit temperature", v);
            }
            one(p, "minimeater.pit.setpoint", "degF", "Pit setpoint", st.cook.sp_f);
            one(
                p,
                "minimeater.pit.reference",
                "degF",
                "Reference the controller tracks",
                st.out.ref_f,
            );
            if let Some(c) = st.ambient_c {
                one(
                    p,
                    "minimeater.ambient.temperature",
                    "degF",
                    "Outside air",
                    k_to_f(c + 273.15),
                );
            }
            one(p, "minimeater.airflow", "{cfm}", "Commanded airflow", st.out.q);
            one(
                p,
                "minimeater.flap.position",
                "%",
                "Damper flap position",
                st.out.flap_pct,
            );
            one(
                p,
                "minimeater.fan.duty",
                "%",
                "Fan duty, averaged",
                st.out.act.fan * 100.0,
            );
            if let Some(r) = st.fan_rpm {
                one(p, "minimeater.fan.speed", "{rpm}", "Fan tachometer", r);
            }
            one(
                p,
                "minimeater.lid.open",
                "1",
                "The lid is open",
                f64::from(u8::from(st.out.lid)),
            );
            gauge(
                p,
                "minimeater.phase",
                "1",
                "Controller phase (1 for the current one)",
                |p| {
                    point(p, t, 1.0, &[("phase", V::S(names::phase(st.out.phase)))]);
                },
            );
            gauge(p, "minimeater.fire", "1", "Fire state (1 for the current one)", |p| {
                point(p, t, 1.0, &[("state", V::S(names::fire(st.out.fire)))]);
            });
            gauge(p, "minimeater.food.eta", "s", "Time to the target", |p| {
                for (n, f) in st.foods.iter().enumerate() {
                    if let Some(eta) = f.and_then(|f| f.eta_s) {
                        point(p, t, eta, &[("probe", V::I(n as i64)), ("name", V::S(s.names[n]))]);
                    }
                }
            });
            gauge(p, "minimeater.food.rate", "degF/h", "Core rate of rise", |p| {
                for (n, f) in st.foods.iter().enumerate() {
                    if let Some(rate) = f.and_then(|f| f.rate_f_h) {
                        point(p, t, rate, &[("probe", V::I(n as i64)), ("name", V::S(s.names[n]))]);
                    }
                }
            });
            gauge(p, "minimeater.fault.active", "1", "A fault is raised", |p| {
                for code in FaultCode::ALL {
                    point(
                        p,
                        t,
                        f64::from(u8::from(st.faults.contains(code))),
                        &[("code", V::S(code.name()))],
                    );
                }
            });
            if let Some(r) = s.rssi {
                one(p, "minimeater.wifi.rssi", "dBm", "WiFi signal", f64::from(r));
            }
            one(p, "minimeater.uptime", "s", "Time since boot", st.up_s as f64);
            if let Some(c) = st.adc_die_c {
                one(
                    p,
                    "minimeater.board.temperature",
                    "Cel",
                    "Board temperature (the ADC's die)",
                    c,
                );
            }
            if let Some(h) = s.heap_free {
                one(p, "minimeater.heap.free", "By", "Free heap", f64::from(h));
            }
        });
    });
    p.finish()
}

/// Memory for the exporter: the request body.
pub struct Buffers {
    body: [u8; 6 * 1024],
}

impl Buffers {
    pub const fn new() -> Self {
        Self { body: [0; 6 * 1024] }
    }
}

impl Default for Buffers {
    fn default() -> Self {
        Self::new()
    }
}

const CONNECT_TIMEOUT_S: u64 = 10;
const EXCHANGE_TIMEOUT_S: u64 = 15;

#[derive(Debug)]
#[allow(dead_code, reason = "the payloads are for the Debug output in the log")]
enum Fail {
    Connect,
    Io,
    Timeout,
    Status(u16),
    Garbled,
}

/// POSTs `body` to `host`'s `/v1/metrics` and checks for a 2xx.
async fn post<C: Connector>(conn: &mut C, host: &str, port: u16, body: &[u8]) -> Result<(), Fail> {
    let (mut r, mut w) = with_timeout(Duration::from_secs(CONNECT_TIMEOUT_S), conn.connect(host, port))
        .await
        .map_err(|_| Fail::Timeout)?
        .map_err(|()| Fail::Connect)?;
    with_timeout(Duration::from_secs(EXCHANGE_TIMEOUT_S), async {
        let mut head = String::<192>::new();
        let _ = core::fmt::write(
            &mut head,
            format_args!(
                "POST /v1/metrics HTTP/1.1\r\nHost: {host}:{port}\r\nContent-Type: application/x-protobuf\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            ),
        );
        w.write_all(head.as_bytes()).await.map_err(|_| Fail::Io)?;
        w.write_all(body).await.map_err(|_| Fail::Io)?;
        w.flush().await.map_err(|_| Fail::Io)?;
        // the status line is all that matters
        let mut resp = [0u8; 64];
        let mut n = 0;
        while n < resp.len() && !resp[..n].contains(&b'\n') {
            let got = r.read(&mut resp[n..]).await.map_err(|_| Fail::Io)?;
            if got == 0 {
                break;
            }
            n += got;
        }
        let line = core::str::from_utf8(&resp[..n]).map_err(|_| Fail::Garbled)?;
        let code: u16 = line
            .strip_prefix("HTTP/1.")
            .and_then(|l| l.get(2..5))
            .and_then(|c| c.parse().ok())
            .ok_or(Fail::Garbled)?;
        if (200..300).contains(&code) { Ok(()) } else { Err(Fail::Status(code)) }
    })
    .await
    .map_err(|_| Fail::Timeout)?
}

/// Exports metrics forever, every configured interval, while there is a collector in the settings
/// and the clock is known (`unix_ns` is `None` until SNTP has run: a metric without a real time
/// is worse than a gap).
pub async fn run<C: Connector>(
    conn: &mut C,
    who: &Identity<'_>,
    bus: &Bus,
    bufs: &mut Buffers,
    unix_ns: &dyn Fn() -> Option<u64>,
    rssi: &dyn Fn() -> Option<i32>,
    heap_free: &dyn Fn() -> Option<u32>,
) -> ! {
    let mut status = bus.status.receiver().expect("a bus reader slot for OTLP");
    loop {
        let Some((cfg, probes)) = bus.config().and_then(|c| Some((c.otlp.clone()?, c.probes))) else {
            Timer::after_secs(5).await;
            continue;
        };
        if let (Some(st), Some(now)) = (status.try_get(), unix_ns()) {
            let names = [
                probes[0].name.as_str(),
                probes[1].name.as_str(),
                probes[2].name.as_str(),
                probes[3].name.as_str(),
            ];
            let sample = Sample {
                status: &st,
                names,
                rssi: rssi(),
                heap_free: heap_free(),
                unix_ns: now,
            };
            match export_request(&mut bufs.body, who, &sample) {
                None => warn!("otlp: the metrics do not fit the buffer"),
                Some(n) => match post(conn, &cfg.host, cfg.port, &bufs.body[..n]).await {
                    Ok(()) => info!("otlp: exported {n} bytes"),
                    Err(e) => warn!("otlp: export failed: {e:?}"),
                },
            }
        }
        Timer::after_secs(u64::from(cfg.interval_s)).await;
    }
}
