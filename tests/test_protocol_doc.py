"""docs/protocol.md examples must stay valid JSON and self-consistent (the Rust proto tests reuse them)."""

import json
import re
from pathlib import Path

DOC = Path(__file__).parent.parent / "docs" / "protocol.md"


def examples() -> list[dict]:
    return [json.loads(b) for b in re.findall(r"```json\n(.*?)```", DOC.read_text(), re.S)]


def test_examples_parse():
    assert len(examples()) >= 6


def test_device_payloads_versioned():
    for ex in examples():
        if "cmps" not in ex:
            assert ex["v"] == 1


def test_event_types_match_table():
    text = DOC.read_text()
    table = set(re.findall(r"^\| `([a-z_]+)`(?:, `([a-z_]+)`)? \|", text.split("## `~/event`")[1].split("##")[0], re.M))
    listed = {t for pair in table for t in pair if t} - {"event_type"}
    disc = next(ex for ex in examples() if "cmps" in ex)
    assert set(disc["cmps"]["events"]["evt_typ"]) == listed
    for ex in examples():
        if "event_type" in ex:
            assert ex["event_type"] in listed


def test_state_and_config_have_four_probes():
    state = next(ex for ex in examples() if "phase" in ex)
    config = next(ex for ex in examples() if "model" in ex)
    assert len(state["probes"]) == len(config["probes"]) == 4
