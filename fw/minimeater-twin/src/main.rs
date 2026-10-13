//! `cargo run --release -p minimeater-twin -- [--scenario kamado-brisket] [--speed 60|max]
//! [--every MIN] [--csv FILE] [--metrics] [--mqtt HOST:PORT] [--otlp HOST:PORT]` (or `just twin ...`):
//! runs a scenario on the twin and prints the cook, or with `--metrics` only its control metrics as
//! one line of JSON. `--mqtt` and `--otlp` connect the app's clients to a real broker and collector
//! (`just stack-up`); they need a paced run (`--speed 60`), since the app's timeouts are virtual.

use std::fs::File;
use std::io::{BufWriter, Write};

use minimeater_app::config::{Mqtt, Otlp};
use minimeater_app::pit::Event;
use minimeater_core::food::Stall;
use minimeater_twin::metrics::Metrics;
use minimeater_twin::{NetMode, Opts, Sample, Scenario, run_with};

fn usage() -> ! {
    eprintln!(
        "usage: minimeater-twin [--scenario {}] [--speed N|max] [--every MIN] [--csv FILE] [--metrics] [--mqtt HOST:PORT] [--otlp HOST:PORT]",
        Scenario::NAMES.join("|")
    );
    std::process::exit(2)
}

fn endpoint(s: &str) -> (String, u16) {
    let (h, p) = s.rsplit_once(':').unwrap_or_else(|| usage());
    (h.to_string(), p.parse().unwrap_or_else(|_| usage()))
}

fn hm(s: f64) -> String {
    let m = (s / 60.0).round() as u64;
    format!("{:2}:{:02}", m / 60, m % 60)
}

fn line(s: &Sample) -> String {
    let st = &s.status;
    let o = &st.out;
    let mut l = format!(
        "{} pit {:5.1}F sp {:3.0} {:8} q {:4.2} flap {:3.0}% fan {:3.0}%",
        hm(s.t_s),
        st.temps[st.cook.pit_probe as usize].unwrap_or(f64::NAN),
        st.cook.sp_f,
        format!("{:?}", o.phase).to_lowercase(),
        o.q,
        o.flap_pct,
        o.act.fan * 100.0
    );
    for (j, f) in st.foods.iter().enumerate() {
        if let (Some(f), Some(t)) = (f, st.temps[j]) {
            l += &format!(" | probe {j} {t:5.1}F");
            if let Some(r) = f.rate_f_h {
                l += &format!(" {r:+5.1}F/h");
            }
            if let Some(e) = f.eta_s {
                l += &format!(" eta {}", hm(e));
            }
            if matches!(f.stall, Stall::In { .. }) {
                l += " STALL";
            }
        }
    }
    l
}

fn main() {
    let mut args = std::env::args().skip(1);
    let (mut name, mut speed, mut every_min, mut csv) = ("kamado-brisket".to_string(), None, 10.0, None);
    let mut metrics = false;
    let (mut mqtt, mut otlp) = (None, None);
    while let Some(a) = args.next() {
        let mut val = || args.next().unwrap_or_else(|| usage());
        match a.as_str() {
            "--scenario" => name = val(),
            "--speed" => {
                let v = val();
                speed = match v.as_str() {
                    "max" => None,
                    n => Some(n.trim_end_matches('x').parse::<f64>().unwrap_or_else(|_| usage())),
                };
            }
            "--every" => every_min = val().parse().unwrap_or_else(|_| usage()),
            "--csv" => csv = Some(val()),
            "--metrics" => metrics = true,
            "--mqtt" => mqtt = Some(endpoint(&val())),
            "--otlp" => otlp = Some(endpoint(&val())),
            _ => usage(),
        }
    }
    let mut sc = Scenario::named(&name).unwrap_or_else(|| usage());
    let net = if mqtt.is_some() || otlp.is_some() {
        if speed.is_none() {
            eprintln!("--mqtt and --otlp need a paced run: add --speed 60");
            usage();
        }
        NetMode::Real
    } else {
        NetMode::Memory
    };
    if let Some((host, port)) = mqtt {
        sc.settings.mqtt = Some(Mqtt {
            host: host.as_str().try_into().unwrap_or_else(|_| usage()),
            port,
            user: Default::default(),
            pass: Default::default(),
        });
    }
    if let Some((host, port)) = otlp {
        sc.settings.otlp = Some(Otlp {
            host: host.as_str().try_into().unwrap_or_else(|_| usage()),
            port,
            interval_s: 15,
        });
    }
    let mut out = csv.map(|p| BufWriter::new(File::create(p).expect("csv file")));
    if let Some(w) = out.as_mut() {
        writeln!(
            w,
            "t_s,pit_true,pit,sp,ref,phase,q,flap,fan,core_true,core,rate,eta_s,stall"
        )
        .unwrap();
    }
    if !metrics {
        println!(
            "twin: {} for {} h at {}",
            sc.name,
            sc.hours,
            speed.map_or("max speed".into(), |s| format!("{s}x"))
        );
    }
    let sc_for_metrics = sc.clone();
    let mut next_print = 0.0;
    let report = run_with(sc, &Opts { speed }, net, |s, events| {
        for (t, e) in events {
            if !metrics {
                println!("{} event {e:?}", hm(*t));
            }
        }
        if s.t_s >= next_print && !metrics {
            println!("{}", line(s));
            next_print = s.t_s + every_min * 60.0;
        }
        if let Some(w) = out.as_mut() {
            let st = &s.status;
            let f = st.foods[1];
            let opt = |v: Option<f64>| v.map_or(String::new(), |v| format!("{v:.2}"));
            writeln!(
                w,
                "{},{:.2},{},{},{:.2},{:?},{:.3},{:.1},{:.1},{},{},{},{},{:?}",
                s.t_s,
                s.pit_f,
                opt(st.temps[st.cook.pit_probe as usize]),
                st.cook.sp_f,
                st.out.ref_f,
                st.out.phase,
                st.out.q,
                st.out.flap_pct,
                st.out.act.fan * 100.0,
                opt(s.cores_f[0]),
                opt(st.temps[1]),
                opt(f.and_then(|f| f.rate_f_h)),
                opt(f.and_then(|f| f.eta_s)),
                f.map(|f| f.stall)
            )
            .unwrap();
        }
    });
    if metrics {
        println!(
            "{{\"scenario\": \"{}\", \"hours\": {}, \"metrics\": {}, \"events\": {{{}}}}}",
            sc_for_metrics.name,
            sc_for_metrics.hours,
            Metrics::of(&sc_for_metrics, &report).json(),
            Metrics::event_counts(&report)
                .iter()
                .map(|(n, c)| format!("\"{n}\": {c}"))
                .collect::<Vec<_>>()
                .join(", ")
        );
        return;
    }
    let done = report
        .events
        .iter()
        .filter(|(_, e)| !matches!(e, Event::LidOpen { .. } | Event::LidClosed { .. }))
        .count();
    println!("twin: {} samples, {done} events besides the lid", report.samples.len());
}
