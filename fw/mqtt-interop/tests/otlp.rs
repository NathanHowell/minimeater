//! The OTLP exporter (`.4.11`): decoded with an independent protobuf implementation
//! (opentelemetry-proto, prost), and end to end through a real OpenTelemetry collector that turns
//! it into Prometheus text (`just otlp-interop`).

use std::time::Duration;

use minimeater_app::config::{Otlp, Settings};
use minimeater_app::net::Identity;
use minimeater_app::otlp::{self, Buffers, Sample};
use minimeater_app::tasks::Bus;
use opentelemetry_proto::tonic::collector::metrics::v1::ExportMetricsServiceRequest;
use opentelemetry_proto::tonic::common::v1::any_value::Value;
use opentelemetry_proto::tonic::metrics::v1::metric::Data;
use opentelemetry_proto::tonic::metrics::v1::number_data_point::Value as Num;
use prost::Message;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::task::{LocalSet, spawn_local};

mod common;
use common::{Tcp, status};

const WHO: Identity<'static> = Identity {
    prefix: "minimeater",
    id: "mm-it",
    sw: "0.1.0",
    hw: "A",
};

fn names() -> [&'static str; 4] {
    ["Pit", "Brisket", "Probe 3", "Probe 4"]
}

#[test]
fn the_request_decodes_with_an_independent_protobuf_implementation() {
    let st = status(5321, 250.0);
    let mut buf = [0u8; 6 * 1024];
    let t = 1_760_000_000_000_000_000u64;
    let n = otlp::export_request(
        &mut buf,
        &WHO,
        &Sample {
            status: &st,
            names: names(),
            rssi: Some(-61),
            heap_free: Some(123_456),
            unix_ns: t,
        },
    )
    .expect("fits");
    println!("{n} bytes");
    let req = ExportMetricsServiceRequest::decode(&buf[..n]).expect("valid protobuf");
    assert_eq!(req.resource_metrics.len(), 1);
    let rm = &req.resource_metrics[0];
    let attr = |k: &str| {
        rm.resource
            .as_ref()
            .unwrap()
            .attributes
            .iter()
            .find(|a| a.key == k)
            .and_then(|a| match a.value.as_ref()?.value.as_ref()? {
                Value::StringValue(s) => Some(s.clone()),
                _ => None,
            })
    };
    assert_eq!(attr("service.name").as_deref(), Some("minimeater"));
    assert_eq!(attr("service.instance.id").as_deref(), Some("mm-it"));
    assert_eq!(attr("service.version").as_deref(), Some("0.1.0"));
    let sm = &rm.scope_metrics[0];
    assert_eq!(sm.scope.as_ref().unwrap().name, "minimeater-fw");

    // name -> data points as (attributes, value)
    let metric = |name: &str| -> Vec<(Vec<(String, String)>, f64)> {
        let m = sm
            .metrics
            .iter()
            .find(|m| m.name == name)
            .unwrap_or_else(|| panic!("no {name}"));
        let Some(Data::Gauge(g)) = &m.data else {
            panic!("{name} is not a gauge")
        };
        g.data_points
            .iter()
            .map(|d| {
                assert_eq!(d.time_unix_nano, t, "{name}");
                let v = match d.value {
                    Some(Num::AsDouble(v)) => v,
                    _ => panic!("{name}: not a double"),
                };
                let attrs = d
                    .attributes
                    .iter()
                    .map(|a| {
                        let v = match a.value.as_ref().unwrap().value.as_ref().unwrap() {
                            Value::StringValue(s) => s.clone(),
                            Value::IntValue(i) => i.to_string(),
                            _ => panic!(),
                        };
                        (a.key.clone(), v)
                    })
                    .collect();
                (attrs, v)
            })
            .collect()
    };
    let probes = metric("minimeater.probe.temperature");
    assert_eq!(probes.len(), 2, "only the probes with a reading");
    assert_eq!(
        probes[1].0,
        [("probe".into(), "1".into()), ("name".into(), "Brisket".into())]
    );
    assert_eq!(probes[1].1, 150.0);
    assert_eq!(metric("minimeater.pit.setpoint")[0].1, 250.0);
    assert!((metric("minimeater.pit.temperature")[0].1 - 249.6).abs() < 1e-9);
    assert!((metric("minimeater.ambient.temperature")[0].1 - 68.0).abs() < 1e-6);
    assert_eq!(metric("minimeater.fan.duty")[0].1, 25.0);
    assert_eq!(metric("minimeater.fan.speed")[0].1, 900.0);
    let servo = metric("minimeater.servo.current");
    assert_eq!(servo[0], (vec![("stat".to_string(), "peak".to_string())], 240.0));
    assert_eq!(servo[1], (vec![("stat".to_string(), "mean".to_string())], 70.0));
    assert_eq!(metric("minimeater.wifi.rssi")[0].1, -61.0);
    assert_eq!(metric("minimeater.uptime")[0].1, 5321.0);
    assert_eq!(metric("minimeater.heap.free")[0].1, 123_456.0);
    assert_eq!(metric("minimeater.food.eta")[0].1, 21600.0);
    assert_eq!(
        metric("minimeater.phase")[0].0,
        [("phase".to_string(), "hold".to_string())]
    );
    let faults = metric("minimeater.fault.active");
    assert_eq!(faults.len(), 5);
    assert!(faults.iter().all(|(_, v)| *v == 0.0));
    for m in &sm.metrics {
        assert!(!m.unit.is_empty() && !m.description.is_empty(), "{}", m.name);
    }
    assert!(n < 4096, "{n} bytes leaves headroom in the 6 KB buffer");
}

fn collector() -> (String, u16, u16) {
    let addr = std::env::var("OTLP_COLLECTOR").unwrap_or_else(|_| "127.0.0.1".into());
    (addr, 4318, 8889)
}

async fn scrape(host: &str, port: u16) -> String {
    let mut s = TcpStream::connect((host, port)).await.expect("prometheus endpoint");
    s.write_all(format!("GET /metrics HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n").as_bytes())
        .await
        .unwrap();
    let mut out = String::new();
    s.read_to_string(&mut out).await.unwrap();
    out
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "needs a collector: run `just otlp-interop`"]
async fn a_real_collector_accepts_the_export_and_exposes_it() {
    LocalSet::new()
        .run_until(async {
            let (host, port, prom) = collector();
            let settings = Settings {
                otlp: Some(Otlp {
                    host: host.as_str().try_into().unwrap(),
                    port,
                    interval_s: 5,
                }),
                ..Default::default()
            };
            let bus: &'static Bus = Box::leak(Box::new(Bus::new()));
            bus.publish_config(&settings);
            bus.status.sender().send(status(100, 275.0));
            let bufs = Box::leak(Box::new(Buffers::new()));
            let task = spawn_local(async move {
                let now = || {
                    Some(
                        std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap()
                            .as_nanos() as u64,
                    )
                };
                otlp::run(&mut Tcp, &WHO, bus, bufs, &now, &|| Some(-55), &|| Some(200_000)).await
            });
            // the first export goes out at once; give the collector a moment to publish it
            tokio::time::sleep(Duration::from_secs(3)).await;
            let text = scrape(&host, prom).await;
            task.abort();
            assert!(text.starts_with("HTTP/1.1 200"), "{}", &text[..text.len().min(200)]);
            for want in [
                "minimeater_pit_setpoint",
                "minimeater_probe_temperature",
                "minimeater_fan_speed",
                "minimeater_wifi_rssi",
                "minimeater_fault_active",
            ] {
                assert!(text.contains(want), "no {want} in:\n{text}");
            }
            let sp = text.lines().find(|l| l.starts_with("minimeater_pit_setpoint")).unwrap();
            assert!(
                sp.contains("service_instance_id=\"mm-it\"") || sp.contains("instance=\"mm-it\""),
                "{sp}"
            );
            assert!(sp.trim_end().ends_with(" 275"), "{sp}");
            println!("{sp}");
        })
        .await;
}
