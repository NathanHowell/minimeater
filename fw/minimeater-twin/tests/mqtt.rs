//! The MQTT session on the twin (`.5.2`): the app's client against the twin's in-process broker, on
//! the virtual clock. What it connects with, what it publishes and when, how it takes commands (and
//! drops a retained one), how it answers on a response topic, and that it survives the broker
//! going away.

use minimeater_app::config::Mqtt;
use minimeater_twin::net::{Inject, Seen};
use minimeater_twin::{Action, Opts, Scenario, run};
use serde_json::{Value, json};

const BASE: &str = "minimeater/mm-twin01";

fn msg(t: f64, topic: &'static str, payload: &'static str) -> (f64, Action) {
    (
        t,
        Action::Publish(Inject {
            topic,
            payload,
            retain: false,
            response_topic: Some("reply/x"),
        }),
    )
}

#[test]
fn the_client_publishes_takes_commands_and_survives_the_broker() {
    let h = 3600.0;
    let mut sc = Scenario::kamado_hold(1.0);
    sc.settings.mqtt = Some(Mqtt {
        host: "broker".try_into().unwrap(),
        port: 1883,
        user: "mm".try_into().unwrap(),
        pass: "pw".try_into().unwrap(),
    });
    sc.actions.extend([
        msg(300.0, "minimeater/mm-twin01/set/sp", "275"),
        msg(310.0, "minimeater/mm-twin01/set/sp", "400"),
        (
            320.0,
            Action::Publish(Inject {
                topic: "minimeater/mm-twin01/set/sp",
                payload: "300",
                retain: true,
                response_topic: Some("reply/x"),
            }),
        ),
        msg(330.0, "minimeater/mm-twin01/cmd/selfcheck", ""),
        msg(340.0, "homeassistant/status", "online"),
        msg(350.0, "minimeater/mm-twin01/set/probe/1/name", "Ribs"),
        (900.0, Action::Broker(false)),
        (1500.0, Action::Broker(true)),
    ]);
    let r = run(sc, &Opts { speed: None }, |_, _| {});
    assert!(
        r.samples.len() as f64 >= 0.99 * h / 10.0,
        "control kept its period through the outage"
    );

    let seen: Vec<_> = r.mqtt.iter().collect();
    let connects: Vec<_> = seen.iter().filter(|(_, s)| matches!(s, Seen::Connect { .. })).collect();
    assert_eq!(
        connects.len(),
        2,
        "one connect, and one after the outage: {}",
        connects.len()
    );
    let Seen::Connect {
        client_id,
        keepalive_s,
        will_topic,
        will_payload,
        will_retain,
        username,
    } = &connects[0].1
    else {
        unreachable!()
    };
    assert_eq!(
        (client_id.as_str(), *keepalive_s, username.as_deref()),
        ("mm-twin01", 30, Some("mm"))
    );
    assert_eq!(will_topic.as_deref(), Some("minimeater/mm-twin01/status"));
    assert_eq!((will_payload.as_slice(), *will_retain), (&b"offline"[..], true));
    assert!(
        connects[1].0 > 1500.0 && connects[1].0 < 1500.0 + 70.0,
        "reconnected at {:.0} s",
        connects[1].0
    );

    // after connecting: online, subscribe, discovery, config, state — in that order
    let first_conn = 0;
    let order: Vec<&str> = seen[first_conn..]
        .iter()
        .take(8)
        .map(|(_, s)| match s {
            Seen::Connect { .. } => "connect",
            Seen::Subscribe(_) => "subscribe",
            Seen::Publish { topic, .. } if topic.ends_with("/status") => "status",
            Seen::Publish { topic, .. } if topic.starts_with("homeassistant/device/") => "discovery",
            Seen::Publish { topic, .. } if topic.ends_with("/config") => "config",
            Seen::Publish { topic, .. } if topic.ends_with("/state") => "state",
            _ => "other",
        })
        .collect();
    assert_eq!(
        &order[..5],
        ["connect", "status", "subscribe", "discovery", "config"],
        "{order:?}"
    );
    let subs = seen
        .iter()
        .find_map(|(_, s)| {
            if let Seen::Subscribe(f) = s {
                Some(f.clone())
            } else {
                None
            }
        })
        .unwrap();
    assert_eq!(
        subs,
        [
            format!("{BASE}/set/#"),
            format!("{BASE}/cmd/#"),
            "homeassistant/status".to_string()
        ]
    );

    let publishes = |topic: &str| -> Vec<(f64, &Vec<u8>, u8, bool)> {
        seen.iter()
            .filter_map(|(t, s)| match s {
                Seen::Publish {
                    topic: tp,
                    payload,
                    qos,
                    retain,
                    ..
                } if tp == topic => Some((*t, payload, *qos, *retain)),
                _ => None,
            })
            .collect()
    };
    let json_of = |p: &Vec<u8>| -> Value { serde_json::from_slice(p).expect("a JSON payload") };

    // status: online, retained, QoS 1
    let status = publishes(&format!("{BASE}/status"));
    assert_eq!(
        (status[0].1.as_slice(), status[0].2, status[0].3),
        (&b"online"[..], 1, true)
    );
    assert_eq!(status.len(), 2, "online once per connection");

    // state every period while connected: retained QoS 0, valid, and nothing while the broker was gone
    let state = publishes(&format!("{BASE}/state"));
    assert!(state.iter().all(|(_, p, q, rt)| *q == 0 && *rt && json_of(p)["v"] == 1));
    let between = |a: f64, b: f64| state.iter().filter(|(t, ..)| *t > a && *t < b).count();
    assert!(
        between(60.0, 900.0) >= 80,
        "{} states before the outage",
        between(60.0, 900.0)
    );
    assert_eq!(between(960.0, 1500.0), 0, "silent while the broker is away");
    assert!(between(1600.0, 3500.0) >= 150, "and back after it");

    // discovery: retained QoS 1, the whole device; again when HA comes online; again after reconnect
    let disc = publishes("homeassistant/device/mm-twin01/config");
    assert_eq!(disc.len(), 4, "connect, HA birth, a rename, reconnect: {}", disc.len());
    assert!(disc.iter().all(|(_, _, q, rt)| *q == 1 && *rt));
    let d = json_of(disc[0].1);
    assert_eq!(d["dev"]["ids"], "mm-twin01");
    assert!(d["cmps"].as_object().unwrap().len() >= 60);
    assert!(
        (340.0..350.0).contains(&disc[1].0),
        "HA birth answered at {:.0} s",
        disc[1].0
    );
    assert_eq!(json_of(disc[1].1)["cmps"]["probe1"]["name"], "Probe 2");
    assert!(
        (350.0..360.0).contains(&disc[2].0),
        "rename republished at {:.0} s",
        disc[2].0
    );
    assert_eq!(json_of(disc[2].1)["cmps"]["probe1_eta"]["name"], "Ribs ETA");

    // config: on connect and after the accepted setpoint change, and it carries it
    let config = publishes(&format!("{BASE}/config"));
    assert!(config.iter().all(|(_, _, q, rt)| *q == 1 && *rt));
    let at = |t: f64| json_of(config.iter().rev().find(|(at, ..)| *at <= t).unwrap().1);
    assert_eq!(at(200.0)["sp"], 250.0);
    assert_eq!(at(800.0)["sp"], 275.0, "the retained sp 300 was not applied");

    // answers on the response topic, in order: ok, out_of_range, retained, ok (selfcheck), ok (rename)
    let replies: Vec<Value> = publishes("reply/x").iter().map(|(_, p, ..)| json_of(p)).collect();
    assert_eq!(
        replies,
        [
            json!({"v": 1, "ok": true}),
            json!({"v": 1, "ok": false, "error": "out_of_range", "detail": "sp 400 outside 150..325"}),
            json!({"v": 1, "ok": false, "error": "retained", "detail": "retained commands are dropped"}),
            json!({"v": 1, "ok": true}),
            json!({"v": 1, "ok": true}),
        ]
    );
    // the response carries the sender's correlation data
    assert!(
        seen.iter()
            .filter_map(|(_, s)| match s {
                Seen::Publish { topic, correlation, .. } if topic == "reply/x" => Some(correlation.as_deref()),
                _ => None,
            })
            .all(|c| c == Some(&b"corr"[..]))
    );

    // events: every rejection is also an event; the selfcheck result is one
    let events: Vec<Value> = publishes(&format!("{BASE}/event"))
        .iter()
        .map(|(_, p, q, rt)| {
            assert_eq!((*q, *rt), (1, false));
            json_of(p)
        })
        .collect();
    let kinds: Vec<&str> = events.iter().filter_map(|e| e["event_type"].as_str()).collect();
    assert_eq!(kinds.iter().filter(|k| **k == "rejected").count(), 2, "{kinds:?}");
    // the session announces the boot once, first
    assert_eq!(kinds.first(), Some(&"boot"), "{kinds:?}");
    assert_eq!(kinds.iter().filter(|k| **k == "boot").count(), 1, "{kinds:?}");
    assert_eq!(events[0]["reason"], "power_on");
    assert_eq!(events[0]["sw"], env!("CARGO_PKG_VERSION"));
    assert!(
        events
            .iter()
            .any(|e| e["event_type"] == "rejected" && e["error"] == "retained" && e["topic"] == "set/sp")
    );
    // the power-up self-check came before the connection existed: old news, not replayed
    assert_eq!(
        kinds.iter().filter(|k| **k == "selfcheck").count(),
        1,
        "only the one asked for: {kinds:?}"
    );

    // pings keep the connection alive while nothing else is said
    assert!(seen.iter().any(|(_, s)| *s == Seen::Ping));
}
