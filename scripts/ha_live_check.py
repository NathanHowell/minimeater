"""Drives a real Home Assistant (container) through onboarding, adds its MQTT integration, then
checks the minimeat device that the twin publishes appears with working entities (`just ha-live-check`).

Usage: uv run --no-project python -I scripts/ha_live_check.py HA_HOST BROKER_HOST
"""

import json
import sys
import time
import urllib.error
import urllib.parse
import urllib.request

HA, BROKER = sys.argv[1], sys.argv[2]
BASE = f"http://{HA}:8123"
CLIENT = f"{BASE}/"
token = None


def call(method, path, body=None, form=None, auth=True):
    headers = {}
    data = None
    if body is not None:
        data = json.dumps(body).encode()
        headers["Content-Type"] = "application/json"
    if form is not None:
        data = urllib.parse.urlencode(form).encode()
    if auth and token:
        headers["Authorization"] = f"Bearer {token}"
    req = urllib.request.Request(BASE + path, data=data, method=method, headers=headers)
    try:
        with urllib.request.urlopen(req, timeout=20) as r:
            raw = r.read()
            return json.loads(raw) if raw else None
    except urllib.error.HTTPError as e:
        raise SystemExit(f"{method} {path}: {e.code} {e.read().decode()[:300]}")


USER, PASSWORD = "m", "minimeat-test-1"


def onboard():
    """Creates the first user (a fresh container) or logs in as it (a reused one)."""
    global token
    steps = call("GET", "/api/onboarding", auth=False)
    if any(s["step"] == "user" and not s["done"] for s in steps):
        r = call("POST", "/api/onboarding/users", {"client_id": CLIENT, "name": USER, "username": USER,
                                                      "password": PASSWORD, "language": "en"}, auth=False)
        code = r["auth_code"]
    else:
        flow = call("POST", "/auth/login_flow", {"client_id": CLIENT, "handler": ["homeassistant", None],
                                                  "redirect_uri": CLIENT}, auth=False)
        r = call("POST", f"/auth/login_flow/{flow['flow_id']}",
                 {"username": USER, "password": PASSWORD, "client_id": CLIENT}, auth=False)
        code = r["result"]
    t = call("POST", "/auth/token", form={"grant_type": "authorization_code", "code": code, "client_id": CLIENT},
             auth=False)
    token = t["access_token"]


def add_mqtt():
    flow = call("POST", "/api/config/config_entries/flow", {"handler": "mqtt", "show_advanced_options": False})
    if flow["type"] == "form" and flow["step_id"] == "broker":
        flow = call("POST", f"/api/config/config_entries/flow/{flow['flow_id']}",
                    {"broker": BROKER, "port": 1883,
                     "other_settings": {"set_client_cert": False, "set_ca_cert": "off"}})
    if flow["type"] == "abort" and flow["reason"] == "single_instance_allowed":
        print("MQTT integration already there")
        return
    assert flow["type"] == "create_entry", flow
    print("MQTT integration added")


def states():
    return {s["entity_id"]: s for s in call("GET", "/api/states")}


def wait_for(what, pred, timeout=120):
    end = time.time() + timeout
    while time.time() < end:
        v = pred()
        if v:
            return v
        time.sleep(2)
    raise SystemExit(f"timed out waiting for {what}")


def main():
    onboard()
    add_mqtt()
    # the twin publishes the retained discovery message on connect; HA reads it when it subscribes
    ents = wait_for("64 entities", lambda: (lambda s: s if len([k for k in s if "minimeat" in k]) >= 60 else None)(states()))
    ours = {k: v for k, v in ents.items() if "minimeat" in k}
    by_domain = {}
    for k in ours:
        by_domain.setdefault(k.split(".")[0], []).append(k)
    print(f"{len(ours)} minimeat entities:", {d: len(v) for d, v in sorted(by_domain.items())})
    assert len(ours) >= 60

    def get(entity):
        return states()[entity]

    # the climate entity follows the cook
    climate = next(k for k in ours if k.startswith("climate."))
    st = wait_for("a pit reading", lambda: (lambda c: c if c["attributes"].get("current_temperature") else None)(get(climate)))
    print(climate, st["state"], st["attributes"].get("temperature"), st["attributes"].get("current_temperature"))
    assert st["state"] == "heat"
    temp = next(k for k in ours if k.endswith("pit_temperature"))
    print(temp, get(temp)["state"])
    assert float(wait_for("a numeric pit temperature", lambda: (lambda s: s if s["state"] not in ("unknown", "unavailable") else None)(get(temp)))["state"]) > 60
    # a setting: HA changes the setpoint, and the device's config follows. HA shows temperatures in
    # its own unit system (a fresh instance is metric), the device speaks F.
    metric = call("GET", "/api/config")["unit_system"]["temperature"] == "°C"
    show = (lambda f: round((f - 32) / 1.8, 1)) if metric else (lambda f: f)
    call("POST", "/api/services/climate/set_temperature", {"entity_id": climate, "temperature": show(275.0)})
    wait_for("the new setpoint", lambda: abs(get(climate)["attributes"].get("temperature") - show(275.0)) < 0.2)
    print("set_temperature 275F reached the device and came back")
    # a button
    btn = next(k for k in ours if k.endswith("probe_self_check"))
    call("POST", "/api/services/button/press", {"entity_id": btn})
    # the events entity records the device's events
    ev = next(k for k in ours if k.startswith("event."))
    wait_for("an event", lambda: get(ev)["state"] not in ("unknown", "unavailable") or None)
    print(ev, "last event", get(ev)["attributes"].get("event_type"))
    # availability: every entity is available while the device is online
    gone = [k for k, v in ours.items() if v["state"] == "unavailable"]
    print("unavailable:", gone)
    assert not gone
    print("OK")


main()
