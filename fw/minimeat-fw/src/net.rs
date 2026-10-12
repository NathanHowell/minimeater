//! WiFi and the IP stack (`.4.10`): station mode with DHCP, reconnect with backoff, the radio for Improv provisioning,
//! SNTP, an mDNS responder for the device's own name, and the TCP [`Connector`] the MQTT and OTLP clients use.
//!
//! [`wifi`] is the one task that owns the radio. It joins the credentials in the settings (`Bus::config`) and holds the
//! link, and serves two requests from the provisioning conversation ([`Link`]): join these credentials (answering
//! whether the network took them and gave an address) and scan. Credentials that fail are dropped and the saved ones
//! are joined again. WPA3-only networks are not supported by the driver's configuration (WPA2 and WPA/WPA2 are).

use core::cell::Cell;
use core::sync::atomic::{AtomicBool, AtomicI32, Ordering};

use embassy_futures::select::{Either3, select3};
use embassy_net::dns::DnsQueryType;
use embassy_net::tcp::{TcpReader, TcpSocket, TcpWriter};
use embassy_net::udp::{PacketMetadata, UdpSocket};
use embassy_net::{Config, DhcpConfig, IpAddress, Ipv4Address, Runner, Stack, StackResources};
use embassy_sync::blocking_mutex::CriticalSectionMutex;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::Channel;
use embassy_sync::signal::Signal;
use embassy_time::{Duration, Instant, Timer, with_timeout};
use embedded_io_async::{ErrorType, Read, Write};
use esp_radio::wifi::scan::ScanConfig;
use esp_radio::wifi::sta::StationConfig;
use esp_radio::wifi::{AuthenticationMethodConfig, Config as WifiConfig, ControllerConfig, Interface, WifiController};
use heapless::{String, Vec};
use log::{info, warn};
use minimeat_app::config::Wifi;
use minimeat_app::improv::{Network, Station};
use minimeat_app::net::{Backoff, Connector};
use minimeat_app::tasks::Bus;
use minimeat_app::{mdns, sntp};
use static_cell::StaticCell;

/// Joining a network, from the radio's request to an address from DHCP.
const JOIN_TIMEOUT: Duration = Duration::from_secs(20);
const DHCP_TIMEOUT: Duration = Duration::from_secs(15);
const SCAN_TIMEOUT: Duration = Duration::from_secs(10);
const SNTP_PERIOD: Duration = Duration::from_secs(15 * 60);
const MDNS_PORT: u16 = 5353;

static ONLINE: AtomicBool = AtomicBool::new(false);
static RSSI: AtomicI32 = AtomicI32::new(i32::MIN);
/// The last SNTP answer: Unix milliseconds, and the uptime in ms when it arrived.
static WALL: CriticalSectionMutex<Cell<Option<(u64, u64)>>> = CriticalSectionMutex::new(Cell::new(None));

/// Unix time in ns, once SNTP has run (the OTLP exporter does not export without it).
pub fn unix_ns() -> Option<u64> {
    let (unix_ms, at_ms) = WALL.lock(Cell::get)?;
    Some((unix_ms + Instant::now().as_millis().saturating_sub(at_ms)) * 1_000_000)
}

/// The signal strength in dBm while joined.
pub fn rssi() -> Option<i32> {
    let r = RSSI.load(Ordering::Relaxed);
    (ONLINE.load(Ordering::Relaxed) && r != i32::MIN).then_some(r)
}

pub fn online() -> bool {
    ONLINE.load(Ordering::Relaxed)
}

// --- the stack ------------------------------------------------------------------------------------------------

type Driver = Interface;

/// Starts the radio and the stack. `hostname` goes to the DHCP server; `seed` is for the stack's port and sequence numbers.
pub fn init(
    wifi: esp_hal::peripherals::WIFI<'static>,
    hostname: &str,
    seed: u64,
) -> (WifiController<'static>, Stack<'static>, Runner<'static, Driver>) {
    let controller = WifiController::new(wifi, ControllerConfig::default()).expect("wifi controller");
    let mut dhcp = DhcpConfig::default();
    dhcp.hostname = String::try_from(hostname).ok();
    static RESOURCES: StaticCell<StackResources<8>> = StaticCell::new();
    let (stack, runner) = embassy_net::new(
        Interface::station(),
        Config::dhcpv4(dhcp),
        RESOURCES.init(StackResources::new()),
        seed,
    );
    (controller, stack, runner)
}

#[embassy_executor::task]
pub async fn stack_runner(mut runner: Runner<'static, Driver>) -> ! {
    runner.run().await
}

// --- the radio ------------------------------------------------------------------------------------------------

enum Cmd {
    Join(Wifi),
    Scan,
}

static CMD: Channel<CriticalSectionRawMutex, Cmd, 1> = Channel::new();
static JOINED: Signal<CriticalSectionRawMutex, bool> = Signal::new();
static SCANNED: Signal<CriticalSectionRawMutex, Vec<Network, 12>> = Signal::new();

/// The radio as the provisioning conversation sees it.
pub struct Link;

impl Station for Link {
    async fn connect(&mut self, ssid: &str, pass: &str) -> bool {
        let (Ok(ssid), Ok(pass)) = (String::try_from(ssid), String::try_from(pass)) else {
            return false;
        };
        JOINED.reset();
        CMD.send(Cmd::Join(Wifi { ssid, pass })).await;
        JOINED.wait().await
    }

    async fn scan(&mut self) -> Vec<Network, 12> {
        SCANNED.reset();
        CMD.send(Cmd::Scan).await;
        SCANNED.wait().await
    }

    fn online(&self) -> bool {
        online()
    }
}

fn station(w: &Wifi) -> Option<WifiConfig> {
    let auth = if w.pass.is_empty() {
        AuthenticationMethodConfig::Open
    } else {
        AuthenticationMethodConfig::WpaWpa2Personal(w.pass.as_str().try_into().ok()?)
    };
    Some(WifiConfig::Station(
        StationConfig::default()
            .with_ssid(w.ssid.as_str().try_into().ok()?)
            .with_authentication(auth),
    ))
}

/// Joins `w` and waits for an address; the radio is left joined only on success.
async fn join(ctl: &mut WifiController<'static>, stack: Stack<'static>, w: &Wifi) -> bool {
    let Some(config) = station(w) else { return false };
    if ctl.set_config(&config).is_err() {
        warn!("wifi: the radio refused the configuration");
        return false;
    }
    let ok = match with_timeout(JOIN_TIMEOUT, ctl.connect_async()).await {
        Ok(Ok(_)) => with_timeout(DHCP_TIMEOUT, stack.wait_config_up()).await.is_ok(),
        _ => false,
    };
    if ok {
        info!("wifi: joined {}", w.ssid);
        ONLINE.store(true, Ordering::Relaxed);
    } else {
        warn!("wifi: could not join {}", w.ssid);
        let _ = ctl.disconnect_async().await;
    }
    ok
}

async fn scan(ctl: &mut WifiController<'static>) -> Vec<Network, 12> {
    let mut out: Vec<Network, 12> = Vec::new();
    let Ok(Ok(found)) = with_timeout(SCAN_TIMEOUT, ctl.scan_async(&ScanConfig::default().with_max(12))).await else {
        return out;
    };
    for ap in found {
        let Ok(ssid) = String::try_from(ap.ssid.as_str()) else {
            continue;
        };
        if ssid.is_empty() {
            continue;
        }
        let _ = out.push(Network {
            ssid,
            rssi: ap.signal_strength,
            secured: ap
                .auth_method
                .is_some_and(|m| !matches!(m, esp_radio::wifi::AuthenticationMethod::None)),
        });
    }
    out.sort_unstable_by_key(|n| core::cmp::Reverse(n.rssi));
    out
}

/// Holds the link to the credentials in the settings, and serves the provisioning conversation.
#[embassy_executor::task]
pub async fn wifi(mut ctl: WifiController<'static>, stack: Stack<'static>, bus: &'static Bus) -> ! {
    let mut rev = bus.config_rev.receiver().expect("a reader slot");
    let mut backoff = Backoff::default();
    // the credentials to hold: from the settings once the control task has published them
    let mut saved: Option<Wifi> = bus.config().and_then(|c| c.wifi);
    // the credentials the radio is joined with, when it is
    let mut current: Option<Wifi> = None;
    let mut joined = false;
    loop {
        if !joined && let Some(w) = saved.clone() {
            joined = join(&mut ctl, stack, &w).await;
            if joined {
                current = Some(w);
                backoff.reset();
            }
        }
        // joined: wake every 10 s to refresh the signal strength; not joined: retry after the backoff
        let retry = async {
            if joined {
                Timer::after_secs(10).await
            } else if saved.is_none() {
                core::future::pending::<()>().await
            } else {
                Timer::after_secs(backoff.next_s()).await
            }
        };
        let lost = async {
            if joined {
                let _ = ctl.wait_for_disconnect_async().await;
            } else {
                core::future::pending::<()>().await
            }
        };
        match select3(retry, lost, next_event(rev.changed())).await {
            Either3::First(()) => {}
            Either3::Second(()) => {
                warn!("wifi: link lost");
                ONLINE.store(false, Ordering::Relaxed);
                joined = false;
            }
            Either3::Third(Event::Config) => {
                let new = bus.config().and_then(|c| c.wifi);
                if new != saved {
                    saved = new;
                    backoff.reset();
                    // credentials just provisioned are the ones the radio already has; anything else is a change
                    if joined && current != saved {
                        let _ = ctl.disconnect_async().await;
                        ONLINE.store(false, Ordering::Relaxed);
                        joined = false;
                    }
                }
            }
            Either3::Third(Event::Cmd(Cmd::Join(w))) => {
                if joined {
                    let _ = ctl.disconnect_async().await;
                    ONLINE.store(false, Ordering::Relaxed);
                }
                joined = join(&mut ctl, stack, &w).await;
                current = joined.then_some(w);
                JOINED.signal(joined);
            }
            Either3::Third(Event::Cmd(Cmd::Scan)) => SCANNED.signal(scan(&mut ctl).await),
        }
        if joined && let Ok(r) = ctl.rssi() {
            RSSI.store(r, Ordering::Relaxed);
        }
    }
}

enum Event {
    Config,
    Cmd(Cmd),
}

/// The next thing to react to: the settings changed (`changed` is the bus's revision receiver) or a request came in.
async fn next_event(changed: impl core::future::Future<Output = u32>) -> Event {
    match embassy_futures::select::select(changed, CMD.receive()).await {
        embassy_futures::select::Either::First(_) => Event::Config,
        embassy_futures::select::Either::Second(c) => Event::Cmd(c),
    }
}

// --- time and names -------------------------------------------------------------------------------------------

/// Asks a pool server for the time every quarter of an hour (and again soon after a miss).
#[embassy_executor::task]
pub async fn sntp_client(stack: Stack<'static>, seed: u64) -> ! {
    let mut backoff = Backoff::default();
    let mut nonce = seed;
    loop {
        stack.wait_config_up().await;
        nonce = nonce
            .wrapping_mul(6364136223846793005)
            .wrapping_add(Instant::now().as_ticks());
        if let Some(ms) = query(stack, nonce).await {
            WALL.lock(|c| c.set(Some((ms, Instant::now().as_millis()))));
            info!("sntp: time set, unix ms {ms}");
            backoff.reset();
            Timer::after(SNTP_PERIOD).await;
        } else {
            Timer::after_secs(backoff.next_s().max(5)).await;
        }
    }
}

async fn query(stack: Stack<'static>, nonce: u64) -> Option<u64> {
    let addr = *stack.dns_query("pool.ntp.org", DnsQueryType::A).await.ok()?.first()?;
    let mut rx_meta = [PacketMetadata::EMPTY; 2];
    let mut tx_meta = [PacketMetadata::EMPTY; 2];
    let mut rx = [0u8; 128];
    let mut tx = [0u8; 128];
    let mut socket = UdpSocket::new(stack, &mut rx_meta, &mut rx, &mut tx_meta, &mut tx);
    socket.bind(0).ok()?;
    socket.send_to(&sntp::request(nonce), (addr, 123)).await.ok()?;
    let mut buf = [0u8; 64];
    let (n, _) = with_timeout(Duration::from_secs(3), socket.recv_from(&mut buf))
        .await
        .ok()?
        .ok()?;
    sntp::parse(&buf[..n], nonce)
}

/// Answers mDNS queries for `<hostname>.local` with the address the network gave us.
#[embassy_executor::task]
pub async fn mdns_responder(stack: Stack<'static>, hostname: &'static str) -> ! {
    let mut rx_meta = [PacketMetadata::EMPTY; 4];
    let mut tx_meta = [PacketMetadata::EMPTY; 2];
    let mut rx = [0u8; 512];
    let mut tx = [0u8; 256];
    loop {
        stack.wait_config_up().await;
        let _ = stack.join_multicast_group(Ipv4Address::new(224, 0, 0, 251));
        let mut socket = UdpSocket::new(stack, &mut rx_meta, &mut rx, &mut tx_meta, &mut tx);
        if socket.bind(MDNS_PORT).is_err() {
            Timer::after_secs(30).await;
            continue;
        }
        let mut buf = [0u8; 512];
        while stack.is_config_up() {
            let Ok(Ok((n, _))) = with_timeout(Duration::from_secs(5), socket.recv_from(&mut buf)).await else {
                continue;
            };
            let Some(ip) = stack.config_v4().map(|c| c.address.address().octets()) else {
                continue;
            };
            if let Some(reply) = mdns::answer(&buf[..n], hostname, ip) {
                let _ = socket
                    .send_to(&reply, (Ipv4Address::new(224, 0, 0, 251), MDNS_PORT))
                    .await;
            }
        }
    }
}

// --- the connector --------------------------------------------------------------------------------------------

/// The TCP connector for MQTT and OTLP: one socket with its own buffers, reused for each connection (the previous one
/// has ended before `connect` is called again).
pub struct Tcp {
    stack: Stack<'static>,
    socket: TcpSocket<'static>,
}

impl Tcp {
    pub fn new(stack: Stack<'static>, rx: &'static mut [u8], tx: &'static mut [u8]) -> Self {
        let mut socket = TcpSocket::new(stack, rx, tx);
        socket.set_timeout(Some(Duration::from_secs(30)));
        socket.set_keep_alive(Some(Duration::from_secs(20)));
        Self { stack, socket }
    }
}

/// A TCP read half as `embedded_io_async` 0.6 sees it.
pub struct Reader<'a>(TcpReader<'a>);
pub struct Writer<'a>(TcpWriter<'a>);

#[derive(Debug)]
pub struct IoError;

impl embedded_io_async::Error for IoError {
    fn kind(&self) -> embedded_io_async::ErrorKind {
        embedded_io_async::ErrorKind::Other
    }
}

impl ErrorType for Reader<'_> {
    type Error = IoError;
}

impl ErrorType for Writer<'_> {
    type Error = IoError;
}

impl Read for Reader<'_> {
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, IoError> {
        self.0.read(buf).await.map_err(|_| IoError)
    }
}

impl Write for Writer<'_> {
    async fn write(&mut self, buf: &[u8]) -> Result<usize, IoError> {
        self.0.write(buf).await.map_err(|_| IoError)
    }

    async fn flush(&mut self) -> Result<(), IoError> {
        self.0.flush().await.map_err(|_| IoError)
    }
}

impl Connector for Tcp {
    type Reader<'a> = Reader<'a>;
    type Writer<'a> = Writer<'a>;

    async fn connect<'a>(&'a mut self, host: &str, port: u16) -> Result<(Reader<'a>, Writer<'a>), ()> {
        if !online() {
            return Err(());
        }
        let addr = match host.parse::<core::net::Ipv4Addr>() {
            Ok(ip) => IpAddress::Ipv4(Ipv4Address::from(ip.octets())),
            // names, `.local` ones too (the stack resolves them over mDNS)
            Err(_) => *self
                .stack
                .dns_query(host, DnsQueryType::A)
                .await
                .map_err(|_| ())?
                .first()
                .ok_or(())?,
        };
        self.socket.abort();
        let _ = self.socket.flush().await;
        self.socket.connect((addr, port)).await.map_err(|_| ())?;
        let (r, w) = self.socket.split();
        Ok((Reader(r), Writer(w)))
    }
}
