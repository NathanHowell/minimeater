//! A read-only HTTP endpoint for health checks and a look at the cook (`minimeat-g7r`): plain HTTP on the LAN, `GET`
//! only, nothing it accepts changes anything. Credentials and settings stay on the USB-C serial REPL and, for the
//! ones MQTT may set, MQTT; an HTTP route that wrote would need TLS (`docs/adr/0005`), which this does not have.
//!
//! - `GET /health`: `200` and `{"ok":true,"id":..,"sw":..,"hw":..,"up":..}` once the control task has published a
//!   status, `503` before that (`"ok":false`). For a monitor that wants to know the board is alive.
//! - `GET /status`: the `~/state` object of `docs/protocol.md` (temperatures, setpoint, outputs, faults, WiFi signal),
//!   as MQTT publishes it, or `503` before the first status.
//!
//! One client at a time, one request per connection (`Connection: close`), and a client that does not finish its
//! request line and headers in [`READ_TIMEOUT`] is dropped, so a stuck one holds the port for seconds, not for ever.
//! It only reads the bus, so a client can neither slow the control loop nor change the cook.

use core::fmt::Write as _;

use embassy_time::{Duration, Timer, with_timeout};
use embedded_io_async::{Read, Write};
use heapless::String;

use crate::net::{Identity, Listener};
use crate::proto::{self, Json};
use crate::tasks::Bus;

pub const PORT: u16 = 80;
/// How long a client has to send its request.
const READ_TIMEOUT: Duration = Duration::from_secs(5);
/// The request line and headers; a bigger request is refused.
const REQUEST: usize = 1024;
/// A `~/state` body (about 1 KB with four probes).
const BODY: usize = 2048;

/// The server's memory: give it static storage.
pub struct Buffers {
    request: [u8; REQUEST],
    body: String<BODY>,
}

impl Buffers {
    pub const fn new() -> Self {
        Self {
            request: [0; REQUEST],
            body: String::new(),
        }
    }
}

impl Default for Buffers {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Route {
    Health,
    Status,
    /// A path that is not ours.
    NotFound,
    /// One of ours, with a method other than GET.
    NotAllowed,
    /// Not an HTTP request line.
    Bad,
}

/// The route a request's first line asks for; the query string is ignored.
pub fn route(request: &[u8]) -> Route {
    let Some(line) = request
        .split(|&b| b == b'\n')
        .next()
        .and_then(|l| core::str::from_utf8(l).ok())
    else {
        return Route::Bad;
    };
    let mut words = line.trim_end().split(' ');
    let (Some(method), Some(target), Some(version), None) = (words.next(), words.next(), words.next(), words.next())
    else {
        return Route::Bad;
    };
    if !version.starts_with("HTTP/1.") || !target.starts_with('/') {
        return Route::Bad;
    }
    let path = target.split(['?', '#']).next().unwrap_or("");
    let known = match path {
        "/health" => Route::Health,
        "/status" => Route::Status,
        _ => return Route::NotFound,
    };
    if method == "GET" { known } else { Route::NotAllowed }
}

async fn reply<W: Write>(w: &mut W, status: &str, kind: &str, body: &str, allow: bool) {
    let mut head: String<192> = String::new();
    let _ = write!(
        head,
        "HTTP/1.1 {status}\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n{}\r\n",
        body.len(),
        if allow { "Allow: GET\r\n" } else { "" }
    );
    let _ = w.write_all(head.as_bytes()).await;
    let _ = w.write_all(body.as_bytes()).await;
    let _ = w.flush().await;
}

/// Reads up to the blank line that ends the headers; false if the client sent too much, too slowly, or hung up.
async fn read_head<R: Read>(r: &mut R, buf: &mut [u8]) -> bool {
    let mut n = 0;
    let read = async {
        loop {
            if buf[..n].windows(4).any(|w| w == b"\r\n\r\n") || buf[..n].windows(2).any(|w| w == b"\n\n") {
                return true;
            }
            if n == buf.len() {
                return false;
            }
            match r.read(&mut buf[n..]).await {
                Ok(0) | Err(_) => return false,
                Ok(k) => n += k,
            }
        }
    };
    with_timeout(READ_TIMEOUT, read).await.unwrap_or(false)
}

async fn serve_one<R: Read, W: Write>(
    r: &mut R,
    w: &mut W,
    who: &Identity<'_>,
    bus: &Bus,
    rssi: &dyn Fn() -> Option<i32>,
    bufs: &mut Buffers,
) {
    if !read_head(r, &mut bufs.request).await {
        return;
    }
    let status = bus.status.try_get();
    match route(&bufs.request) {
        Route::Health => {
            let ok = status.is_some();
            let up = status.as_ref().map_or(0, |s| s.up_s);
            proto::render_into(&mut bufs.body, |j: &mut Json<_>| {
                j.begin_obj();
                j.key("ok").bool(ok);
                j.key("id").str(who.id);
                j.key("sw").str(who.sw);
                j.key("hw").str(who.hw);
                j.key("up").int(up as i64);
                j.end_obj();
            });
            let code = if ok { "200 OK" } else { "503 Service Unavailable" };
            reply(w, code, "application/json", &bufs.body, false).await;
        }
        Route::Status => match status {
            Some(s) if proto::render_into(&mut bufs.body, |j| proto::state(j, &s, rssi())) => {
                reply(w, "200 OK", "application/json", &bufs.body, false).await
            }
            Some(_) => {
                reply(
                    w,
                    "500 Internal Server Error",
                    "text/plain",
                    "state does not fit\n",
                    false,
                )
                .await
            }
            None => reply(w, "503 Service Unavailable", "text/plain", "no status yet\n", false).await,
        },
        Route::NotFound => reply(w, "404 Not Found", "text/plain", "not found\n", false).await,
        Route::NotAllowed => reply(w, "405 Method Not Allowed", "text/plain", "read only\n", true).await,
        Route::Bad => reply(w, "400 Bad Request", "text/plain", "bad request\n", false).await,
    }
}

/// Serves for ever.
pub async fn run<L: Listener>(
    l: &mut L,
    who: &Identity<'_>,
    bus: &Bus,
    rssi: &dyn Fn() -> Option<i32>,
    bufs: &mut Buffers,
) -> ! {
    loop {
        match l.accept().await {
            Ok((mut r, mut w)) => serve_one(&mut r, &mut w, who, bus, rssi, bufs).await,
            // no network to listen on: look again shortly
            Err(()) => Timer::after(Duration::from_secs(1)).await,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routes() {
        assert_eq!(route(b"GET /health HTTP/1.1\r\nHost: x\r\n\r\n"), Route::Health);
        assert_eq!(route(b"GET /status?x=1 HTTP/1.0\r\n\r\n"), Route::Status);
        assert_eq!(route(b"GET /status/ HTTP/1.1\r\n\r\n"), Route::NotFound);
        assert_eq!(route(b"GET / HTTP/1.1\r\n\r\n"), Route::NotFound);
        assert_eq!(route(b"GET /settings HTTP/1.1\r\n\r\n"), Route::NotFound);
    }

    #[test]
    fn nothing_but_get_is_allowed() {
        for m in ["POST", "PUT", "DELETE", "PATCH", "HEAD", "get"] {
            let mut r: std::string::String = m.into();
            r.push_str(" /status HTTP/1.1\r\n\r\n");
            assert_eq!(route(r.as_bytes()), Route::NotAllowed, "{m}");
        }
    }

    #[test]
    fn garbage_is_a_bad_request() {
        for r in [
            &b""[..],
            b"\r\n",
            b"GET",
            b"GET /status",
            b"GET /status HTTP/1.1 extra",
            b"GET status HTTP/1.1",
            b"GET /status SPDY/3",
            b"\xff\xfe /x HTTP/1.1",
        ] {
            assert_eq!(route(r), Route::Bad, "{r:?}");
        }
    }
}
