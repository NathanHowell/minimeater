"""Checks the device-discovery message the firmware publishes against Home Assistant's own code
(`just fw::interop::ha-discovery`): the abbreviations HA expands, the device schema, and each component's
platform schema, exactly as HA's MQTT discovery applies them, plus a syntax check of every template.

Run by `just fw::interop::ha-discovery`, which writes FILE.json from the firmware's own test and passes it here.
"""

import asyncio
import sys
import tempfile
from importlib import import_module

import jinja2
from homeassistant.components.mqtt import discovery
from homeassistant.const import CONF_PLATFORM
from homeassistant.const import CONF_DEVICE
from homeassistant.components.mqtt.discovery import CONF_COMPONENTS
from homeassistant.helpers import template as _  # noqa: F401  (HA's jinja extensions load with it)

TEMPLATE_KEYS = ("tpl", "template")


def schema_for(platform: str):
    mod = import_module(f"homeassistant.components.mqtt.{platform}")
    for name in ("DISCOVERY_SCHEMA", "PLATFORM_SCHEMA_MODERN"):
        if hasattr(mod, name):
            return getattr(mod, name)
    raise SystemExit(f"no discovery schema in mqtt.{platform}")


async def check() -> int:
    # HA's schemas validate templates against a running instance
    from homeassistant.core import HomeAssistant

    with tempfile.TemporaryDirectory() as cfg:
        hass = HomeAssistant(cfg)
        hass.config.config_dir = cfg
        try:
            return run_checks()
        finally:
            await hass.async_stop(force=True)


def run_checks() -> int:
    payload = open(sys.argv[1]).read()
    device_payload = discovery._parse_device_payload(None, payload, "mm-a1b2c3", None)
    if not device_payload:
        print("HA rejected the device payload (see the warning above)")
        return 1
    env = jinja2.Environment()
    bad = 0
    count = 0
    for key, cfg in device_payload[CONF_COMPONENTS].items():
        cfg = dict(cfg)
        platform = cfg.pop(CONF_PLATFORM)
        cfg[CONF_DEVICE] = device_payload[CONF_DEVICE]
        cfg = discovery.MQTTDiscoveryPayload(cfg)
        discovery._merge_common_device_options(cfg, device_payload)
        try:
            schema_for(platform)(cfg)
        except Exception as exc:  # noqa: BLE001
            print(f"{platform} {key}: {exc}")
            bad += 1
        for k, v in cfg.items():
            if isinstance(v, str) and (k.endswith(TEMPLATE_KEYS)):
                try:
                    env.parse(v)
                except jinja2.TemplateSyntaxError as exc:
                    print(f"{platform} {key}.{k}: template: {exc}")
                    bad += 1
        count += 1
    print(f"{count} components checked against Home Assistant, {bad} problems")
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(asyncio.run(check()))
