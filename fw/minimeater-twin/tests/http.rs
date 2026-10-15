//! The read-only HTTP endpoint on the twin (`minimeat-g7r`): what each route answers, that anything that is not a GET
//! is refused, that a client that stalls or sends junk does not stop the next one being served, and that none of it
//! touches the cook.

use minimeater_twin::{Action, Opts, Scenario, run};
use serde_json::Value;

/// (status line, headers, body) of a response.
fn parts(raw: &[u8]) -> (String, String, String) {
    let text = String::from_utf8_lossy(raw).into_owned();
    let (head, body) = text.split_once("\r\n\r\n").unwrap_or((&text, ""));
    let (line, headers) = head.split_once("\r\n").unwrap_or((head, ""));
    (line.to_string(), headers.to_string(), body.to_string())
}

#[test]
fn the_endpoint_reports_the_cook_and_changes_nothing() {
    let big: &'static str =
        Box::leak(format!("GET /status HTTP/1.1\r\nX-Pad: {}\r\n\r\n", "a".repeat(2000)).into_boxed_str());
    let mut sc = Scenario::kamado_hold(0.2);
    sc.actions.extend([
        (0.0, Action::Http("GET /health HTTP/1.1\r\nHost: x\r\n\r\n")),
        (100.0, Action::Http("GET /health HTTP/1.1\r\nHost: x\r\n\r\n")),
        (110.0, Action::Http("GET /status?pretty=1 HTTP/1.1\r\nHost: x\r\n\r\n")),
        (
            120.0,
            Action::Http("POST /status HTTP/1.1\r\nContent-Length: 0\r\n\r\n"),
        ),
        (
            130.0,
            Action::Http("PUT /health HTTP/1.1\r\nContent-Length: 2\r\n\r\n{}"),
        ),
        (140.0, Action::Http("GET /settings HTTP/1.1\r\n\r\n")),
        (150.0, Action::Http("hello there\r\n\r\n")),
        // a request that never ends: dropped after its timeout, with nothing sent
        (160.0, Action::Http("GET /status HTTP/1.1\r\nHost: x\r\n")),
        (200.0, Action::Http(big)),
        // and the server is still there
        (300.0, Action::Http("GET /health HTTP/1.1\r\n\r\n")),
    ]);
    let r = run(sc, &Opts { speed: None }, |_, _| {});
    assert_eq!(
        r.http.len(),
        10,
        "{:?}",
        r.http.iter().map(|h| &h.1).collect::<Vec<_>>()
    );
    let answer = |i: usize| parts(&r.http[i].2);

    // before the control task has published anything: not ready
    let (line, _, body) = answer(0);
    assert_eq!(line, "HTTP/1.1 503 Service Unavailable");
    let v: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["ok"], false);

    // health: ok, who and what version
    let (line, headers, body) = answer(1);
    assert_eq!(line, "HTTP/1.1 200 OK");
    assert!(headers.contains("Content-Type: application/json") && headers.contains("Connection: close"));
    assert!(
        headers.contains(&format!("Content-Length: {}", body.len())),
        "{headers}"
    );
    let v: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(
        (v["ok"].as_bool(), v["id"].as_str(), v["hw"].as_str()),
        (Some(true), Some("mm-twin01"), Some("A"))
    );
    assert!(v["up"].as_i64().unwrap() >= 90 && v["sw"].is_string());

    // status: the ~/state object
    let (line, headers, body) = answer(2);
    assert_eq!(line, "HTTP/1.1 200 OK");
    assert!(headers.contains(&format!("Content-Length: {}", body.len())));
    let v: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["v"], 1);
    assert!((v["sp"].as_f64().unwrap() - 250.0).abs() < 1.0, "{body}");
    assert!(v["pit"].is_number() && v["probes"].as_array().unwrap().len() == 4 && v["faults"].is_array());
    assert_eq!(v["diag"]["rssi"], -60);
    assert!(body.len() < 2048, "{} bytes", body.len());

    // anything but GET is refused and says so
    for i in [3, 4] {
        let (line, headers, _) = answer(i);
        assert_eq!(line, "HTTP/1.1 405 Method Not Allowed");
        assert!(headers.contains("Allow: GET"));
    }
    assert_eq!(answer(5).0, "HTTP/1.1 404 Not Found");
    assert_eq!(answer(6).0, "HTTP/1.1 400 Bad Request");
    assert!(r.http[7].2.is_empty(), "a request that never ends gets no answer");
    assert!(r.http[8].2.is_empty(), "a request past the buffer gets no answer");
    assert_eq!(answer(9).0, "HTTP/1.1 200 OK");

    // read only: no request reached the control task
    // (the boot's own requests and its first ~/config come before the first client)
    assert!(r.replies.iter().all(|x| x.0 < 1.0) && r.configs.iter().all(|x| x.0 < 1.0));
    let last = r.samples.last().unwrap();
    assert!((last.status.cook.sp_f - 250.0).abs() < 0.01);
}
