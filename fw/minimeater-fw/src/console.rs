//! The one USB Serial/JTAG port, shared (`.4.10`): console logs and Improv Wi-Fi provisioning.
//!
//! The C6's USB block is one CDC serial port in silicon and nothing to add a second one to, so both uses take turns
//! on it. A [`log::Log`] formats each record into one whole line and queues it; [`writer`] sends lines; the Improv
//! conversation ([`Port`]) writes whole frames; both take the same lock for every write, so a frame never has a log
//! line in the middle (and a client's parser skips log lines between frames).
//!
//! With no host on the port (the box on a wall adapter) nothing may block: a write that finds the FIFO still full
//! from the last one, or does not finish in [`WRITE_TIMEOUT`], is dropped, and writing starts again when a host
//! drains the port. Log lines queue (up to [`QUEUE`]) before the writer task runs and are dropped when it is full.
//! The panic handler (`esp-backtrace`) still prints through `esp-println` directly: it ends the program anyway.

use core::convert::Infallible;
use core::fmt::Write as _;

use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::Channel;
use embassy_sync::mutex::Mutex;
use embassy_time::{Duration, Instant, with_timeout};
use embedded_io_async::{ErrorType, Read, Write};
use esp_hal::Async;
use esp_hal::peripherals::USB_DEVICE;
use esp_hal::usb::usb_serial_jtag::{UsbSerialJtag, UsbSerialJtagRx, UsbSerialJtagTx};
use heapless::String;
use log::{LevelFilter, Log, Metadata, Record};
use minimeater_app::repl;
use static_cell::StaticCell;

/// Longest log line, newline included; longer ones are cut.
const LINE: usize = minimeater_app::repl::LOG_LINE;
/// Log lines waiting for the writer.
const QUEUE: usize = 24;
/// A host that does not take a write in this long is not there.
const WRITE_TIMEOUT: Duration = Duration::from_millis(100);

type Tx = UsbSerialJtagTx<'static, Async>;
type Rx = UsbSerialJtagRx<'static, Async>;
type SharedTx = Mutex<CriticalSectionRawMutex, Tx>;

static LINES: Channel<CriticalSectionRawMutex, String<LINE>, QUEUE> = Channel::new();
static LOGGER: Logger = Logger;

struct Logger;

impl Log for Logger {
    fn enabled(&self, m: &Metadata) -> bool {
        m.level() <= log::max_level()
    }

    fn log(&self, r: &Record) {
        if !self.enabled(r.metadata()) {
            return;
        }
        let mut line: String<LINE> = String::new();
        let level = match r.level() {
            log::Level::Error => 'E',
            log::Level::Warn => 'W',
            log::Level::Info => 'I',
            log::Level::Debug => 'D',
            log::Level::Trace => 'T',
        };
        let _ = write!(
            line,
            "{level} ({}) {}: {}",
            Instant::now().as_millis(),
            r.target(),
            r.args()
        );
        // whole lines only: a cut one still ends in a newline
        while line.len() > LINE - 1 {
            line.pop();
        }
        let _ = line.push('\n');
        let _ = LINES.try_send(line);
    }

    fn flush(&self) {}
}

fn level() -> LevelFilter {
    match option_env!("ESP_LOG") {
        Some("error" | "ERROR") => LevelFilter::Error,
        Some("warn" | "WARN") => LevelFilter::Warn,
        Some("debug" | "DEBUG") => LevelFilter::Debug,
        Some("trace" | "TRACE") => LevelFilter::Trace,
        Some("off" | "OFF") => LevelFilter::Off,
        _ => LevelFilter::Info,
    }
}

/// One write, dropped if the host is not taking data: the first byte goes in only if the FIFO is free.
async fn write(tx: &mut Tx, data: &[u8]) {
    let Some((&first, rest)) = data.split_first() else {
        return;
    };
    if tx.write_byte_nb(first).is_err() {
        return;
    }
    let _ = with_timeout(WRITE_TIMEOUT, async {
        let _ = embedded_io_async::Write::write_all(tx, rest).await;
        let _ = embedded_io_async::Write::flush(tx).await;
    })
    .await;
}

/// Sends the queued log lines, one lock at a time.
#[embassy_executor::task]
pub async fn writer(tx: &'static SharedTx) {
    loop {
        let line = LINES.receive().await;
        // a prompt with something typed on it is kept whole: erased for the log line and drawn again after it
        match repl::LIVE.wrap(line.as_bytes()) {
            Some(w) => write(&mut *tx.lock().await, &w).await,
            None => write(&mut *tx.lock().await, line.as_bytes()).await,
        }
    }
}

/// The port as the Improv conversation sees it: reads come from the host, each write is a whole frame under the
/// lock the log writer takes.
pub struct Port {
    rx: Rx,
    tx: &'static SharedTx,
}

impl ErrorType for Port {
    type Error = Infallible;
}

impl Read for Port {
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, Infallible> {
        // an error on the port reads as no data; the conversation waits for the next byte
        Ok(self.rx.read(buf).await.unwrap_or(0))
    }
}

impl Write for Port {
    async fn write(&mut self, buf: &[u8]) -> Result<usize, Infallible> {
        write(&mut *self.tx.lock().await, buf).await;
        Ok(buf.len())
    }

    async fn flush(&mut self) -> Result<(), Infallible> {
        Ok(())
    }
}

/// Takes the USB port and installs the logger. Returns the lock the writer task takes and the port for Improv.
pub fn init(dev: USB_DEVICE<'static>) -> (&'static SharedTx, Port) {
    static TX: StaticCell<SharedTx> = StaticCell::new();
    let (rx, tx) = UsbSerialJtag::new(dev).into_async().split();
    let tx: &'static SharedTx = TX.init(Mutex::new(tx));
    // a second call returns an error: the logger is the first thing main installs
    let _ = log::set_logger(&LOGGER);
    log::set_max_level(level());
    (tx, Port { rx, tx })
}
