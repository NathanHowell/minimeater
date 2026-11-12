//! The MQTT client over TLS with a client certificate (`minimeat-a4l`, ADR 0006), end to end on the host: the app's own session
//! runs over a tokio socket with the device's TLS provider and a software key standing in for the secure element, against a small
//! TLS 1.3 MQTT peer (Python's `ssl`) that requires a client certificate from a private CA. It checks what the broker sees:
//! the certificate's name, TLS 1.3, and the device's first publish; and that a broker the pinned CA did not sign is refused.
//!
//! Needs `openssl` and `uv` on the PATH (the test passes without doing anything where they are missing).
//! `just fw::interop::tls` runs it. The chip's own signing is covered in `minimeater-app` (`tls::tests`).

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{Receiver, channel};
use std::time::{Duration, Instant};

use minimeater_app::config::{Ca, Mqtt, Settings};
use minimeater_app::drivers::csr::{self, ToBeSigned};
use minimeater_app::mqtt::{Buffers, Identity, run};
use minimeater_app::tasks::{Bus, Reply};
use minimeater_app::tls::{DigestSigner, NoClock, Prepared, Provider, RECORD, Secure, SignFailed, WRITE_RECORD};
use p256::ecdsa::SigningKey;
use p256::ecdsa::signature::hazmat::PrehashSigner;
use rand_core::OsRng;
use serde_json::Value;
use tokio::task::{LocalSet, spawn_local};

mod common;
use common::{Tcp, status};

/// A TLS 1.3 MQTT 5 peer that requires a client certificate and says what it saw, one JSON object a line.
const BROKER: &str = r#"
import json, socket, ssl, sys
ca, cert, key, port = sys.argv[1], sys.argv[2], sys.argv[3], int(sys.argv[4])
ctx = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
ctx.minimum_version = ssl.TLSVersion.TLSv1_3
ctx.load_cert_chain(cert, key)
ctx.load_verify_locations(ca)
ctx.verify_mode = ssl.CERT_REQUIRED
srv = socket.socket()
srv.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
srv.bind(("127.0.0.1", port))
srv.listen(4)
tls = ctx.wrap_socket(srv, server_side=True)
def say(**kw):
    print(json.dumps(kw), flush=True)
say(ready=True)
while True:
    try:
        conn, _ = tls.accept()
    except (ssl.SSLError, OSError) as e:
        say(handshake="failed", why=str(e))
        continue
    cn = dict(x[0] for x in conn.getpeercert()["subject"]).get("commonName")
    say(handshake="ok", version=conn.version(), cn=cn)
    def rd(n):
        b = b""
        while len(b) < n:
            c = conn.recv(n - len(b))
            if not c:
                raise EOFError
            b += c
        return b
    try:
        while True:
            h = rd(1)[0]
            mult, ln = 1, 0
            while True:
                d = rd(1)[0]
                ln += (d & 0x7F) * mult
                mult *= 128
                if not d & 0x80:
                    break
            body = rd(ln)
            t = h >> 4
            if t == 1:
                conn.sendall(b"\x20\x03\x00\x00\x00")
                say(packet="connect")
            elif t == 8:
                pid, i = body[:2], 2
                i += 1 + body[i]
                n = 0
                while i < len(body):
                    i += 2 + int.from_bytes(body[i:i + 2], "big") + 1
                    n += 1
                conn.sendall(bytes([0x90, 3 + n]) + pid + b"\x00" + b"\x00" * n)
                say(packet="subscribe", filters=n)
            elif t == 3:
                tl = int.from_bytes(body[:2], "big")
                topic = body[2:2 + tl].decode()
                if (h >> 1) & 3:
                    conn.sendall(b"\x40\x02" + body[2 + tl:4 + tl])
                say(packet="publish", topic=topic)
            elif t == 12:
                conn.sendall(b"\xd0\x00")
    except (EOFError, OSError, ssl.SSLError):
        pass
"#;

struct Peer {
    child: Child,
    lines: Receiver<Value>,
}

impl Drop for Peer {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Peer {
    /// Starts the broker on `port` and waits until it listens.
    fn start(dir: &Path, port: u16) -> Option<Self> {
        std::fs::write(dir.join("broker.py"), BROKER).ok()?;
        let mut child = Command::new("uv")
            .args([
                "run",
                "--no-project",
                "python",
                "-I",
                "broker.py",
                "ca.pem",
                "server.pem",
                "server.key",
            ])
            .arg(port.to_string())
            .current_dir(dir)
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;
        let out = child.stdout.take()?;
        let (tx, lines) = channel();
        std::thread::spawn(move || {
            for l in BufReader::new(out).lines().map_while(Result::ok) {
                if let Ok(v) = serde_json::from_str::<Value>(&l) {
                    let _ = tx.send(v);
                }
            }
        });
        let peer = Self { child, lines };
        peer.wait(|v| v["ready"] == true, Duration::from_secs(60))?;
        Some(peer)
    }

    /// The first line `pred` takes, within `limit`.
    fn wait(&self, pred: impl Fn(&Value) -> bool, limit: Duration) -> Option<Value> {
        let end = Instant::now() + limit;
        loop {
            let left = end.checked_duration_since(Instant::now())?;
            let v = self.lines.recv_timeout(left).ok()?;
            if pred(&v) {
                return Some(v);
            }
        }
    }
}

fn openssl(dir: &Path, args: &[&str]) -> Option<Vec<u8>> {
    let o = Command::new("openssl")
        .args(args)
        .current_dir(dir)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    o.status.success().then_some(o.stdout)
}

/// The software key standing in for the secure element.
#[derive(Clone)]
struct Soft(SigningKey);

impl DigestSigner for Soft {
    fn sign(&mut self, digest: &[u8; 32]) -> Result<[u8; 64], SignFailed> {
        let sig: p256::ecdsa::Signature = self.0.sign_prehash(digest).map_err(|_| SignFailed)?;
        Ok(sig.to_bytes().into())
    }
}

struct Tls {
    rng: OsRng,
    key: Soft,
    cert: Vec<u8>,
    read: Vec<u8>,
    write: Vec<u8>,
}

impl Secure for Tls {
    type Provider<'a> = Provider<'a, &'a mut OsRng, Soft, NoClock>;

    async fn prepare<'a>(&'a mut self, ca: &'a [u8]) -> Option<Prepared<'a, Self::Provider<'a>>> {
        Some(Prepared {
            read: &mut self.read,
            write: &mut self.write,
            provider: Provider::new(&mut self.rng, ca, self.key.clone(), &self.cert),
        })
    }
}

struct Pki {
    dir: PathBuf,
    ca_der: Vec<u8>,
    tls: Tls,
}

/// A CA, a server certificate for `localhost`, and a client certificate for a software key, or `None` where there is no openssl.
fn pki(name: &str) -> Option<Pki> {
    let dir = std::env::temp_dir().join(format!("mm-tls-it-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).ok()?;
    let o = |args: &[&str]| openssl(&dir, args);
    o(&["version"])?;
    o(&["ecparam", "-name", "prime256v1", "-genkey", "-noout", "-out", "ca.key"])?;
    o(&[
        "req",
        "-x509",
        "-new",
        "-key",
        "ca.key",
        "-subj",
        "/CN=mm test ca",
        "-days",
        "30",
        "-out",
        "ca.pem",
    ])?;
    o(&[
        "ecparam",
        "-name",
        "prime256v1",
        "-genkey",
        "-noout",
        "-out",
        "server.key",
    ])?;
    o(&[
        "req",
        "-new",
        "-key",
        "server.key",
        "-subj",
        "/CN=localhost",
        "-out",
        "server.csr",
    ])?;
    std::fs::write(dir.join("ext.cnf"), "subjectAltName=DNS:localhost\n").ok()?;
    o(&[
        "x509",
        "-req",
        "-in",
        "server.csr",
        "-CA",
        "ca.pem",
        "-CAkey",
        "ca.key",
        "-CAcreateserial",
        "-days",
        "30",
        "-extfile",
        "ext.cnf",
        "-out",
        "server.pem",
    ])?;
    let ca_der = o(&["x509", "-in", "ca.pem", "-outform", "DER"])?;

    // the device's request, built and signed as the firmware does (the key here is software)
    let sk = SigningKey::random(&mut OsRng);
    let mut xy = [0u8; 64];
    xy.copy_from_slice(&sk.verifying_key().to_encoded_point(false).as_bytes()[1..]);
    let tbs = ToBeSigned::new("mm-it", &xy)?;
    let mut soft = Soft(sk);
    let sig = soft.sign(&tbs.digest()).ok()?;
    let request = tbs.finish(&sig)?;
    let mut pem = String::new();
    for l in csr::pem::<12>("CERTIFICATE REQUEST", &request)? {
        pem.push_str(&l);
        pem.push('\n');
    }
    std::fs::write(dir.join("dev.csr"), pem).ok()?;
    let cert = o(&[
        "x509",
        "-req",
        "-in",
        "dev.csr",
        "-CA",
        "ca.pem",
        "-CAkey",
        "ca.key",
        "-CAcreateserial",
        "-days",
        "30",
        "-outform",
        "DER",
    ])?;
    Some(Pki {
        dir,
        ca_der,
        tls: Tls {
            rng: OsRng,
            key: soft,
            cert,
            read: vec![0; RECORD],
            write: vec![0; WRITE_RECORD],
        },
    })
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

/// Stands in for the control task: answers requests and reports a status every second.
async fn stub_control(bus: &'static Bus) {
    use embassy_futures::select::{Either, select};
    let mut up = 0;
    loop {
        match select(bus.commands.receive(), embassy_time::Timer::after_secs(1)).await {
            Either::First(i) => bus.replies.immediate_publisher().publish_immediate(Reply {
                id: i.id,
                result: Ok(()),
            }),
            Either::Second(()) => {
                up += 1;
                bus.status.sender().send(status(up, 250.0));
            }
        }
    }
}

/// Runs the device against `port` pinned to `ca` for `limit`, with the `tls` it was given.
async fn device(port: u16, ca: &[u8], tls: Tls, limit: Duration) {
    let settings = Settings {
        mqtt: Some(Mqtt {
            host: "localhost".try_into().unwrap(),
            port,
            user: Default::default(),
            pass: Default::default(),
        }),
        mqtt_ca: Some(Ca::from_slice(ca).unwrap()),
        ..Default::default()
    };
    let bus: &'static Bus = Box::leak(Box::new(Bus::new()));
    bus.publish_config(&settings);
    spawn_local(stub_control(bus));
    let bufs = Box::leak(Box::new(Buffers::new()));
    let tls = Box::leak(Box::new(tls));
    let who = Identity {
        prefix: "minimeater",
        id: "mm-it",
        sw: "0.1.0",
        hw: "A",
    };
    let session = spawn_local(async move { run(&mut Tcp, tls, &who, bus, bufs, &|| Some(-55)).await });
    tokio::time::sleep(limit).await;
    session.abort();
}

#[tokio::test(flavor = "current_thread")]
async fn the_session_connects_over_tls_with_its_certificate_and_publishes() {
    let Some(pki) = pki("ok") else { return };
    let port = free_port();
    let Some(peer) = Peer::start(&pki.dir, port) else {
        return;
    };
    LocalSet::new()
        .run_until(device(port, &pki.ca_der, pki.tls, Duration::from_secs(6)))
        .await;
    let ok = peer
        .wait(|v| v["handshake"] == "ok", Duration::from_secs(1))
        .expect("a TLS handshake");
    assert_eq!(ok["version"], "TLSv1.3");
    assert_eq!(ok["cn"], "mm-it", "the broker sees the device certificate's name");
    let status = peer
        .wait(
            |v| v["packet"] == "publish" && v["topic"].as_str().is_some_and(|t| t.ends_with("/status")),
            Duration::from_secs(1),
        )
        .expect("the device announced itself over TLS");
    assert_eq!(status["topic"], "minimeater/mm-it/status");
}

#[tokio::test(flavor = "current_thread")]
async fn a_broker_the_pinned_ca_did_not_sign_is_not_spoken_to() {
    let Some(pki) = pki("wrong") else { return };
    let port = free_port();
    let Some(peer) = Peer::start(&pki.dir, port) else {
        return;
    };
    // pin some other CA
    let other = pki.dir.join("other");
    std::fs::create_dir_all(&other).unwrap();
    openssl(
        &other,
        &["ecparam", "-name", "prime256v1", "-genkey", "-noout", "-out", "o.key"],
    )
    .unwrap();
    openssl(
        &other,
        &[
            "req",
            "-x509",
            "-new",
            "-key",
            "o.key",
            "-subj",
            "/CN=someone else",
            "-days",
            "30",
            "-out",
            "o.pem",
        ],
    )
    .unwrap();
    let wrong = openssl(&other, &["x509", "-in", "o.pem", "-outform", "DER"]).unwrap();
    LocalSet::new()
        .run_until(device(port, &wrong, pki.tls, Duration::from_secs(4)))
        .await;
    // nothing was said in MQTT: the device hung up before sending a byte of it
    assert!(
        peer.wait(|v| v["packet"].is_string(), Duration::from_millis(200))
            .is_none(),
        "the device spoke MQTT to a broker it should not trust"
    );
}

/// Mosquitto in a container (`just fw::interop::mosquitto-tls`): a listener that requires a client certificate from the private CA, takes the
/// certificate's name as the user name, and lets a user write only under `minimeater/<its name>/`. The device's session
/// connects with its certificate and its status reaches a subscriber on the broker's local plaintext listener.
#[tokio::test(flavor = "current_thread")]
#[ignore = "needs a container runtime: run `just fw::interop::mosquitto-tls`"]
async fn the_session_works_against_mosquitto_with_the_certificate_name_as_the_user() {
    let runtime = std::env::var("MINIMEATER_CONTAINER").unwrap_or_else(|_| "podman".into());
    let pki = pki("mosq").expect("openssl");
    let dir = &pki.dir;
    std::fs::write(
        dir.join("mosquitto.conf"),
        "per_listener_settings true\n\
         listener 8883\n\
         cafile /certs/ca.pem\n\
         certfile /certs/server.pem\n\
         keyfile /certs/server.key\n\
         require_certificate true\n\
         use_identity_as_username true\n\
         tls_version tlsv1.3\n\
         allow_anonymous false\n\
         acl_file /certs/acl\n\
         listener 1883 127.0.0.1\n\
         allow_anonymous true\n",
    )
    .unwrap();
    std::fs::write(dir.join("acl"), "pattern readwrite minimeater/%u/#\n").unwrap();
    for f in ["server.key", "ca.pem", "server.pem", "mosquitto.conf", "acl"] {
        // (the broker runs as another user inside the container)
        Command::new("chmod")
            .args(["644", f])
            .current_dir(dir)
            .status()
            .unwrap();
    }
    let port = free_port();
    let name = format!("mm-tls-mosq-{}", std::process::id());
    struct Container(String, String);
    impl Drop for Container {
        fn drop(&mut self) {
            let _ = Command::new(&self.0)
                .args(["rm", "-f", &self.1])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
    }
    let _container = Container(runtime.clone(), name.clone());
    let started = Command::new(&runtime)
        .args([
            "run",
            "-d",
            "--name",
            &name,
            "-p",
            &format!("127.0.0.1:{port}:8883"),
            "-v",
        ])
        .arg(format!("{}:/certs:ro", dir.display()))
        .args([
            "docker.io/library/eclipse-mosquitto:2",
            "mosquitto",
            "-c",
            "/certs/mosquitto.conf",
        ])
        .status()
        .expect("container runtime");
    assert!(started.success());
    std::thread::sleep(Duration::from_secs(3));

    // a subscriber on the local listener, started first: it takes the first status message
    let sub = Command::new(&runtime)
        .args([
            "exec",
            &name,
            "mosquitto_sub",
            "-h",
            "127.0.0.1",
            "-p",
            "1883",
            "-t",
            "minimeater/mm-it/status",
            "-C",
            "1",
            "-W",
            "25",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("mosquitto_sub");
    LocalSet::new()
        .run_until(device(port, &pki.ca_der, pki.tls, Duration::from_secs(8)))
        .await;
    let out = sub.wait_with_output().unwrap();
    let said = String::from_utf8_lossy(&out.stdout);
    if said.trim() != "online" {
        let logs = Command::new(&runtime).args(["logs", &name]).output().unwrap();
        println!(
            "{}{}",
            String::from_utf8_lossy(&logs.stdout),
            String::from_utf8_lossy(&logs.stderr)
        );
    }
    assert_eq!(
        said.trim(),
        "online",
        "the broker accepted the device by its certificate and relayed its status"
    );
}
