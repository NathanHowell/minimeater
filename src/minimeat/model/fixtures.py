"""Parity fixtures for the Rust port -> fw/minimeat-core/tests/fixtures/{control,model}.json.

Run: uv run minimeat-fixtures

Each trajectory runs the Python reference controller (FfPi or Mpc) closed-loop against the nonlinear
plant and records, per control step, exactly what `minimeat-core` sees (pit, setpoint, lid, airflow
limits) and the airflow it should command. The Rust test replays the inputs open-loop and compares
outputs. Feedforward is a shared 1F table so both sides evaluate the identical function.

`model.json` pins the airflow, steady-state and actuator-inversion model at sample points.
"""

import argparse
import json
from pathlib import Path

import numpy as np

from . import thermal
from .control import AirflowMap, FeedForward, FfPi, Mpc, Schedule
from .control_report import FAN, H, autotune
from .params import DEFAULT_EGG, DEFAULT_INTAKE, TOP_VENT, Ambient
from .sim import Event, air_flow, simulate
from .units import CFM, c_to_k, f_to_k

AMB = Ambient(c_to_k(20), 0)
FF_TEMPS_F = np.arange(60.0, 461.0, 1.0)

SCENARIOS = {
    # cold light-off to 250F, lid 2 min at 2.5 h, stall load at 3 h
    "brisket": dict(setpoint_f=250.0, init_f=None, duration_s=4 * H, top="slider 1/4",
                    events=[Event(2.5 * H, lid_open_s=120), Event(3 * H, food_load_w=150.0)]),
    # steady 225F, setpoint up to 275F then down to 200F; firmware assumes top 1/4, real top is open petals
    "steps": dict(setpoint_f=225.0, init_f=225.0, duration_s=4.5 * H, top="petals open",
                  events=[Event(0.5 * H, setpoint_f=275.0), Event(2.5 * H, setpoint_f=200.0)]),
}


class TableFeedForward:
    def __init__(self, qs: np.ndarray):
        self.qs = qs

    def q_cfm(self, amb: Ambient, t_f: float) -> float:
        return float(np.interp(t_f, FF_TEMPS_F, self.qs))


class RecordingMap:
    """AirflowMap proxy that captures the limits and airflow the controller used this step."""

    def __init__(self, amap: AirflowMap):
        self.amap, self.limits_seen, self.q = amap, None, None

    def limits(self, amb, t_f):
        self.limits_seen = self.amap.limits(amb, t_f)
        return self.limits_seen

    def actuate(self, amb, t_f, q):
        self.q = q
        return self.amap.actuate(amb, t_f, q)


class Recorder:
    def __init__(self, ctrl, rmap: RecordingMap):
        self.ctrl, self.rmap = ctrl, rmap
        self.cols = {k: [] for k in ("pit", "sp", "lid", "q_min", "q_max", "q")}

    def step(self, obs, dt):
        self.rmap.q = None
        # limits are an input in the Rust API: evaluate them every step even if the controller skips it
        q_min, q_max = self.rmap.amap.limits(obs.ambient, obs.pit_f)
        out = self.ctrl.step(obs, dt)
        for k, v in zip(self.cols, (obs.pit_f, obs.setpoint_f, obs.lid, q_min, q_max, self.rmap.q or 0.0)):
            self.cols[k].append(v)
        return out


def model_fixture() -> dict:
    ambients = [(20.0, 0.0), (0.0, 3.0)]
    top = TOP_VENT["slider 1/4"]
    flows = []
    for amb_c, wind in ambients:
        amb = Ambient(c_to_k(amb_c), wind)
        for pit_f in (70.0, 150.0, 225.0, 325.0):
            for damper, s in ((0.0, 0.0), (0.3, 0.0), (1.0, 0.0), (1.0, 0.1), (1.0, 0.5), (1.0, 1.0)):
                q = air_flow(DEFAULT_EGG, DEFAULT_INTAKE, FAN, amb, s, damper, f_to_k(pit_f), top)
                flows.append({"amb_c": amb_c, "wind": wind, "pit_f": pit_f, "damper": damper, "s": s, "cfm": q / CFM})
    steady = []
    for amb_c, wind in ambients:
        amb = Ambient(c_to_k(amb_c), wind)
        for pit_f, food in ((150.0, 0.0), (225.0, 0.0), (225.0, 150.0), (325.0, 0.0)):
            ss = thermal.steady_state(DEFAULT_EGG, amb, f_to_k(pit_f), food)
            steady.append({"amb_c": amb_c, "wind": wind, "pit_f": pit_f, "food_w": food,
                           "m_air": ss.m_air_kg_s, "t_wi": ss.t_wall_in_k, "t_wo": ss.t_wall_out_k})
    maps = []
    for amb_c, wind in ambients:
        amb = Ambient(c_to_k(amb_c), wind)
        amap = AirflowMap(DEFAULT_EGG, DEFAULT_INTAKE, FAN)
        for pit_f in (100.0, 225.0, 287.0, 325.0):
            q_min, q_max = amap.limits(amb, pit_f)
            for frac in (0.1, 0.3, 0.6, 0.9):
                q = q_min + frac * (q_max - q_min)
                damper, s = amap.actuate(amb, pit_f, q)
                maps.append({"amb_c": amb_c, "wind": wind, "pit_f": pit_f, "q_min": q_min, "q_max": q_max,
                             "q": q, "damper": damper, "fan": s})
    return {"air_flow": flows, "steady_state": steady, "airflow_map": maps}


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--out", type=Path, default=Path("fw/minimeat-core/tests/fixtures/control.json"))
    args = ap.parse_args()
    model_out = args.out.with_name("model.json")
    model_out.parent.mkdir(parents=True, exist_ok=True)
    model_out.write_text(json.dumps(model_fixture(), indent=1))
    print(f"wrote {model_out}")
    schedule: Schedule = autotune(DEFAULT_EGG)
    ff_real = FeedForward(DEFAULT_EGG)
    ff_q = np.array([ff_real.q_cfm(AMB, t) for t in FF_TEMPS_F])
    out = {
        "dt": 10.0,
        "ff": {"t0": FF_TEMPS_F[0], "step": 1.0, "q": [round(q, 6) for q in ff_q]},
        "schedule": [
            {"sp": m.setpoint_f, "k": m.k_f_per_cfm, "tau": m.tau_s, "theta": m.theta_s} for m in schedule.models
        ],
        "trajectories": [],
    }
    for name, sc in SCENARIOS.items():
        for kind in ("FfPi", "Mpc"):
            rmap = RecordingMap(AirflowMap(DEFAULT_EGG, DEFAULT_INTAKE, FAN))
            ff = TableFeedForward(ff_q)
            ctrl = (FfPi if kind == "FfPi" else Mpc)(rmap, ff, schedule)
            rec = Recorder(ctrl, rmap)
            simulate(
                DEFAULT_EGG, DEFAULT_INTAKE, FAN, AMB, TOP_VENT[sc["top"]], sc["events"], sc["duration_s"], rec,
                setpoint_f=sc["setpoint_f"], init_f=sc["init_f"], noise_f=0.3,
            )
            c = rec.cols
            out["trajectories"].append({
                "scenario": name,
                "controller": kind,
                "pit": [round(v, 4) for v in c["pit"]],
                "sp": c["sp"],
                "lid": [int(v) for v in c["lid"]],
                "q_min": [round(v, 6) for v in c["q_min"]],
                "q_max": [round(v, 6) for v in c["q_max"]],
                "q": [round(v, 6) for v in c["q"]],
            })
            print(f"{name}/{kind}: {len(c['q'])} steps")
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(json.dumps(out, separators=(",", ":")))
    print(f"wrote {args.out} ({args.out.stat().st_size // 1024} KB)")
