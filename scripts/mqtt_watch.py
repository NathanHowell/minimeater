"""Prints what the device publishes (`just stack-watch`).

Usage: uv run --no-project --with paho-mqtt python -I scripts/mqtt_watch.py BROKER_HOST [seconds]
"""

import sys
import time

import paho.mqtt.client as mqtt

host = sys.argv[1]
seconds = float(sys.argv[2]) if len(sys.argv) > 2 else 30.0
seen: dict[str, int] = {}


def on_message(_c, _u, m):
    seen[m.topic] = seen.get(m.topic, 0) + 1
    if m.topic.endswith(("/state", "/config")) or m.topic.startswith("homeassistant/"):
        if seen[m.topic] > 1 and m.topic.endswith("/state") and seen[m.topic] % 10:
            return
    text = m.payload.decode(errors="replace")
    print(f"{m.topic} ({len(m.payload)} B{', retained' if m.retain else ''}): {text[:160]}")


c = mqtt.Client(mqtt.CallbackAPIVersion.VERSION2, "mm-watch", protocol=mqtt.MQTTv5)
c.on_message = on_message
c.max_inflight_messages_set(20)
c.connect(host, 1883)
c.subscribe([("minimeater/#", 0), ("homeassistant/#", 0)])
c.loop_start()
time.sleep(seconds)
c.loop_stop()
print("--- topics seen:", {k: v for k, v in sorted(seen.items())})
