//! The MQTT client against a real broker and an independent client (`.5.2`). Run it with
//! `just mqtt-interop`, which starts Mosquitto in a container. The device side is the app's own
//! session over a tokio TCP socket with a stub in place of the control task; the other side is
//! rumqttc, standing in for Home Assistant and a user's phone.

use std::time::Duration;

use embassy_futures::select::{Either, select};
use minimeat_app::config::{Mqtt, Settings};
use minimeat_app::mqtt::{Buffers, Identity, run};
use minimeat_app::pit::Command;
use minimeat_app::proto::Request;
use minimeat_app::tasks::{Bus, Reply};
use rumqttc::v5::mqttbytes::QoS;
use rumqttc::v5::mqttbytes::v5::PublishProperties;
use rumqttc::v5::{AsyncClient, Event, Incoming, MqttOptions};
use serde_json::{Value, json};
use tokio::sync::mpsc::{UnboundedReceiver, unbounded_channel};
use tokio::task::{LocalSet, spawn_local};

mod common;
use common::{Tcp, status};

const BASE: &str = "minimeat/mm-it";

fn broker() -> (String, u16) {
    let addr = std::env::var("MQTT_BROKER").unwrap_or_else(|_| "127.0.0.1:1883".into());
    let (h, p) = addr.rsplit_once(':').expect("host:port");
    (h.to_string(), p.parse().unwrap())
}

/// Stands in for the control task: answers requests, republishes the config when the setpoint
/// moves, and reports a status every two seconds.
async fn stub_control(bus: &'static Bus, mut settings: Settings) {
    let mut up = 0;
    loop {
        match select(bus.commands.receive(), embassy_time::Timer::after_secs(2)).await {
            Either::First(i) => {
                let result = match i.req {
                    Request::Pit(Command::Sp(v)) => {
                        settings.cook.sp_f = v;
                        bus.publish_config(&settings);
                        Ok(())
                    }
                    _ => Ok(()),
                };
                bus.replies
                    .immediate_publisher()
                    .publish_immediate(Reply { id: i.id, result });
            }
            Either::Second(()) => {
                up += 2;
                bus.status.sender().send(status(up, settings.cook.sp_f));
            }
        }
    }
}

struct Msg {
    topic: String,
    payload: Vec<u8>,
    retain: bool,
    correlation: Option<Vec<u8>>,
}

struct Peer {
    client: AsyncClient,
    rx: UnboundedReceiver<Msg>,
    seen: Vec<Msg>,
}

impl Peer {
    async fn start(id: &str, filters: &[&str]) -> Peer {
        let (host, port) = broker();
        let mut o = MqttOptions::new(id, host, port);
        o.set_keep_alive(Duration::from_secs(10));
        // the discovery message is 16 KB
        o.set_max_packet_size(Some(64 * 1024));
        let (client, mut ev) = AsyncClient::new(o, 64);
        for f in filters {
            client.subscribe(*f, QoS::AtLeastOnce).await.unwrap();
        }
        let (tx, rx) = unbounded_channel();
        spawn_local(async move {
            loop {
                let e = match ev.poll().await {
                    Ok(e) => e,
                    Err(e) => {
                        eprintln!("peer event loop ended: {e}");
                        break;
                    }
                };
                if let Event::Incoming(Incoming::Publish(p)) = e {
                    let _ = tx.send(Msg {
                        topic: String::from_utf8_lossy(&p.topic).into_owned(),
                        payload: p.payload.to_vec(),
                        retain: p.retain,
                        correlation: p.properties.and_then(|p| p.correlation_data).map(|c| c.to_vec()),
                    });
                }
            }
        });
        Peer {
            client,
            rx,
            seen: Vec::new(),
        }
    }

    /// The first message, already seen or yet to come, that `pred` accepts.
    async fn wait(&mut self, what: &str, pred: impl Fn(&Msg) -> bool) -> &Msg {
        if let Some(i) = self.seen.iter().position(&pred) {
            return &self.seen[i];
        }
        let deadline = tokio::time::Instant::now() + Duration::from_secs(25);
        loop {
            let m = tokio::time::timeout_at(deadline, self.rx.recv())
                .await
                .unwrap_or_else(|_| {
                    panic!(
                        "timed out waiting for {what}; saw {:?}",
                        self.seen.iter().map(|m| &m.topic).collect::<Vec<_>>()
                    )
                })
                .expect("peer connection");
            let hit = pred(&m);
            self.seen.push(m);
            if hit {
                return self.seen.last().unwrap();
            }
        }
    }

    /// Forgets what has been seen, so the next `wait` looks at new messages only.
    fn forget(&mut self) {
        self.seen.clear();
        while self.rx.try_recv().is_ok() {}
    }

    async fn command(&self, topic: &str, payload: &str, retain: bool, correlation: &'static [u8]) {
        let props = PublishProperties {
            response_topic: Some("reply/peer".into()),
            correlation_data: Some(correlation.into()),
            ..Default::default()
        };
        self.client
            .publish_with_properties(
                format!("{BASE}/{topic}"),
                QoS::AtLeastOnce,
                retain,
                payload.as_bytes().to_vec(),
                props,
            )
            .await
            .unwrap();
    }
}

fn json_of(m: &Msg) -> Value {
    serde_json::from_slice(&m.payload).unwrap_or_else(|e| panic!("{}: {e}", m.topic))
}

fn start_device(bus: &'static Bus) -> tokio::task::JoinHandle<()> {
    let bufs = Box::leak(Box::new(Buffers::new()));
    spawn_local(async move {
        let who = Identity {
            prefix: "minimeat",
            id: "mm-it",
            sw: "0.1.0",
            hw: "A",
        };
        run(&mut Tcp, &who, bus, bufs, &|| Some(-55)).await
    })
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "needs a broker: run `just mqtt-interop`"]
async fn the_client_works_against_mosquitto() {
    LocalSet::new()
        .run_until(async {
            let (host, port) = broker();
            let settings = Settings {
                mqtt: Some(Mqtt {
                    host: host.as_str().try_into().unwrap(),
                    port,
                    user: Default::default(),
                    pass: Default::default(),
                }),
                ..Default::default()
            };
            let bus: &'static Bus = Box::leak(Box::new(Bus::new()));
            bus.publish_config(&settings);
            spawn_local(stub_control(bus, settings));

            // clear anything an earlier run left retained
            let clean = Peer::start("mm-it-clean", &[]).await;
            for t in ["status", "state", "config", "set/sp"] {
                clean
                    .client
                    .publish(format!("{BASE}/{t}"), QoS::AtLeastOnce, true, Vec::<u8>::new())
                    .await
                    .unwrap();
            }
            clean
                .client
                .publish(
                    "homeassistant/device/mm-it/config",
                    QoS::AtLeastOnce,
                    true,
                    Vec::<u8>::new(),
                )
                .await
                .unwrap();
            tokio::time::sleep(Duration::from_millis(500)).await;

            let mut peer = Peer::start(
                "mm-it-peer",
                &[&format!("{BASE}/#"), "reply/#", "homeassistant/device/mm-it/config"],
            )
            .await;
            let device = start_device(bus);

            // it announces itself: online, discovery, config, state
            let online = peer.wait("online", |m| m.topic == format!("{BASE}/status")).await;
            assert_eq!(online.payload, b"online");
            let disc = json_of(
                peer.wait("discovery", |m| m.topic == "homeassistant/device/mm-it/config")
                    .await,
            );
            assert_eq!(disc["dev"]["ids"], "mm-it");
            assert!(disc["cmps"].as_object().unwrap().len() >= 60);
            let cfg = json_of(peer.wait("config", |m| m.topic == format!("{BASE}/config")).await);
            assert_eq!(cfg["v"], 1);
            let state = json_of(peer.wait("state", |m| m.topic == format!("{BASE}/state")).await);
            assert_eq!(state["mode"], "run");
            assert_eq!(state["diag"]["rssi"], -55);

            // a setting: answered on the response topic with the sender's correlation data, then
            // the config follows with the new value
            peer.forget();
            peer.command("set/sp", "275", false, b"c1").await;
            let reply = peer.wait("reply", |m| m.topic == "reply/peer").await;
            assert_eq!(json_of(reply), json!({"v": 1, "ok": true}));
            assert_eq!(reply.correlation.as_deref(), Some(&b"c1"[..]));
            let cfg = json_of(
                peer.wait("config with sp", |m| {
                    m.topic == format!("{BASE}/config") && json_of(m)["sp"] == 275.0
                })
                .await,
            );
            assert_eq!(cfg["sp"], 275.0);

            // a bad one: refused with the documented code, and an event
            peer.forget();
            peer.command("set/sp", "400", false, b"c2").await;
            let reply = json_of(peer.wait("error reply", |m| m.topic == "reply/peer").await);
            assert_eq!(reply["error"], "out_of_range");
            let ev = json_of(
                peer.wait("rejected event", |m| m.topic == format!("{BASE}/event"))
                    .await,
            );
            assert_eq!(
                (ev["event_type"].as_str(), ev["topic"].as_str()),
                (Some("rejected"), Some("set/sp"))
            );

            // a retained command is dropped, however it got there; the broker keeps the message
            peer.forget();
            peer.command("set/sp", "300", true, b"c3").await;
            let reply = json_of(peer.wait("retained reply", |m| m.topic == "reply/peer").await);
            assert_eq!(reply["error"], "retained");
            // (the stub's state reports the setpoint it holds)
            peer.forget();
            let state = json_of(peer.wait("a later state", |m| m.topic == format!("{BASE}/state")).await);
            assert_eq!(state["sp"], 275.0, "the retained sp 300 did not apply");

            // the device dies without a goodbye: the broker publishes the Last Will
            peer.forget();
            device.abort();
            let will = peer
                .wait("offline", |m| {
                    m.topic == format!("{BASE}/status") && m.payload == b"offline"
                })
                .await;
            assert_eq!(will.payload, b"offline");

            // a late subscriber gets the retained will; and the device, restarted with the retained
            // `set/sp` 300 still stored, does not replay it (retain handling 2)
            let mut late = Peer::start("mm-it-late", &[&format!("{BASE}/status")]).await;
            let m = late
                .wait("retained status", |m| m.topic == format!("{BASE}/status"))
                .await;
            assert_eq!((m.payload.as_slice(), m.retain), (&b"offline"[..], true));
            peer.forget();
            let device = start_device(bus);
            let online = peer
                .wait("online again", |m| {
                    m.topic == format!("{BASE}/status") && m.payload == b"online"
                })
                .await;
            assert_eq!(online.payload, b"online");
            tokio::time::sleep(Duration::from_secs(3)).await;
            peer.forget();
            tokio::time::sleep(Duration::from_secs(1)).await;
            while let Ok(m) = peer.rx.try_recv() {
                assert!(
                    m.topic != format!("{BASE}/event"),
                    "no rejection from a replayed command"
                );
            }
            device.abort();
            // leave the broker tidy
            for t in ["status", "state", "config", "set/sp"] {
                clean
                    .client
                    .publish(format!("{BASE}/{t}"), QoS::AtLeastOnce, true, Vec::<u8>::new())
                    .await
                    .unwrap();
            }
            clean
                .client
                .publish(
                    "homeassistant/device/mm-it/config",
                    QoS::AtLeastOnce,
                    true,
                    Vec::<u8>::new(),
                )
                .await
                .unwrap();
            tokio::time::sleep(Duration::from_millis(300)).await;
        })
        .await;
}
