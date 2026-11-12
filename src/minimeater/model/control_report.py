"""Controller bake-off on the BGE Large model -> docs/modeling/control.md.  Run: just model::control"""

import argparse
from concurrent.futures import ProcessPoolExecutor
from dataclasses import dataclass
from pathlib import Path

import matplotlib

matplotlib.use("Agg")
import matplotlib.pyplot as plt  # noqa: E402
import numpy as np  # noqa: E402

from .control import AirflowMap, FeedForward, FfPi, Mpc, PercentPid, Schedule, identify  # noqa: E402
from .fans import FANS  # noqa: E402
from .params import DEFAULT_EGG, DEFAULT_INTAKE, TOP_VENT, Ambient, uncertainty_band  # noqa: E402
from .report import md_table  # noqa: E402
from .sim import Event, SimResult, simulate  # noqa: E402
from .units import c_to_k  # noqa: E402

FAN = FANS[1]  # NF-A6x25 5V PWM, the provisional pick
ID_SETPOINTS = (150.0, 225.0, 275.0, 325.0)
H = 3600.0


@dataclass(frozen=True)
class Scenario:
    key: str
    title: str
    setpoint_f: float
    top: str
    ambient: Ambient
    duration_s: float
    events: tuple
    init_f: float | None = None
    holds: tuple = ()  # (start_s, end_s, setpoint) windows for RMS error
    lids: tuple = ()  # lid close times for recovery


SCENARIOS = [
    Scenario(
        "chili", "Chili drying 150F (cold start, lid 1 min at 4 h)", 150.0, "petals open", Ambient(c_to_k(20), 0),
        6 * H, (Event(4 * H, lid_open_s=60),), holds=((2 * H, 4 * H, 150.0), (4.5 * H, 6 * H, 150.0)), lids=(4 * H + 60,),
    ),
    Scenario(
        "brisket", "Brisket 250F (cold start; lid 2 min @2.5 h; stall load @3 h; cold front 5C/4 m/s @4 h)",
        250.0, "slider 1/4", Ambient(c_to_k(20), 0), 7 * H,
        (
            Event(2.5 * H, lid_open_s=120),
            Event(3 * H, food_load_w=150.0),
            Event(4 * H, ambient=Ambient(c_to_k(5), 4.0)),
        ),
        holds=((1.5 * H, 2.5 * H, 250.0), (3 * H, 4 * H, 250.0), (4 * H, 7 * H, 250.0)), lids=(2.5 * H + 120,),
    ),
    Scenario(
        "hotfast", "Hot & fast 325F (cold start, 0C/3 m/s, lid 2 min at 2 h)", 325.0, "slider 1/2",
        Ambient(c_to_k(0), 3.0), 4 * H, (Event(2 * H, lid_open_s=120),),
        holds=((1.25 * H, 2 * H, 325.0), (2.5 * H, 4 * H, 325.0)), lids=(2 * H + 120,),
    ),
    Scenario(
        "steps", "Setpoint steps from steady 225F: 275F @0.5 h, 200F @2.5 h", 225.0, "slider 1/4",
        Ambient(c_to_k(20), 0), 4.5 * H, (Event(0.5 * H, setpoint_f=275.0), Event(2.5 * H, setpoint_f=200.0)),
        init_f=225.0, holds=((1.5 * H, 2.5 * H, 275.0), (3.5 * H, 4.5 * H, 200.0)),
    ),
]

CONTROLLERS = ["PercentPid", "FfPi", "Mpc"]


def build_controller(name: str, schedule: Schedule):
    if name == "PercentPid":
        return PercentPid()
    amap = AirflowMap(DEFAULT_EGG, DEFAULT_INTAKE, FAN)  # firmware-side model: nominal, assumes top 1/4
    ff = FeedForward(DEFAULT_EGG)
    return FfPi(amap, ff, schedule) if name == "FfPi" else Mpc(amap, ff, schedule)


def autotune(plant) -> Schedule:
    """Simulated per-egg autotune: identify an FOPDT at each scheduling setpoint on the real plant."""
    amap = AirflowMap(DEFAULT_EGG, DEFAULT_INTAKE, FAN)
    amb = Ambient(c_to_k(20), 0)
    return Schedule([identify(plant, DEFAULT_INTAKE, FAN, amb, TOP_VENT["slider 1/4"], amap, sp) for sp in ID_SETPOINTS])


def run_one(args) -> tuple:
    sc_key, plant_name, ctrl_name = args
    sc = next(s for s in SCENARIOS if s.key == sc_key)
    plant = uncertainty_band(DEFAULT_EGG)[plant_name]
    sched = autotune(plant)
    r = simulate(
        plant, DEFAULT_INTAKE, FAN, sc.ambient, TOP_VENT[sc.top], list(sc.events), sc.duration_s,
        build_controller(ctrl_name, sched), setpoint_f=sc.setpoint_f, noise_f=0.3, init_f=sc.init_f,
    )
    return sc_key, plant_name, ctrl_name, r, sched


def metrics(sc: Scenario, r: SimResult) -> dict:
    t, y = r.t_s, r.pit_f
    dt = t[1] - t[0]
    out = {}
    # approach: first segment (start or first step)
    seg_start = 0.0 if sc.init_f is None else sc.events[0].t_s
    sp0 = sc.setpoint_f if sc.init_f is None else sc.events[0].setpoint_f
    seg = t >= seg_start
    within = np.abs(y[seg] - sp0) < 5.0
    out["reach min"] = (np.argmax(within) * dt) / 60 if within.any() else float("nan")
    seg_end = sc.holds[0][1]
    first = (t >= seg_start) & (t < seg_end)
    out["overshoot F"] = max(0.0, float(np.max(y[first] - sp0)))
    errs = np.concatenate([y[(t >= a) & (t < b)] - sp for a, b, sp in sc.holds])
    out["hold RMS F"] = float(np.sqrt(np.mean(errs**2)))
    out["hold max |e| F"] = float(np.max(np.abs(errs)))
    rec = []
    for tc in sc.lids:
        after = t >= tc
        ok = np.abs(y[after] - r.setpoint_f[after]) < 5.0
        rec.append(np.argmax(ok) * dt / 60 if ok.any() else float("nan"))
    out["lid recovery min"] = rec[0] if rec else float("nan")
    lid_os = [float(np.max(y[(t >= tc) & (t < tc + 1800)] - r.setpoint_f[(t >= tc) & (t < tc + 1800)])) for tc in sc.lids]
    out["lid overshoot F"] = max(0.0, lid_os[0]) if lid_os else float("nan")
    hours = (t[-1] - t[0]) / H
    out["servo strokes/h"] = float(np.sum(np.abs(np.diff(r.damper)))) / hours
    out["fan on %"] = 100.0 * float(np.mean(r.fan > 0))
    return out


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--out", type=Path, default=Path("docs/modeling"))
    ap.add_argument("--jobs", type=int, default=None)
    args = ap.parse_args()
    img_dir = args.out / "control"
    img_dir.mkdir(parents=True, exist_ok=True)

    plants = list(uncertainty_band(DEFAULT_EGG).keys())
    jobs = [(sc.key, p, c) for sc in SCENARIOS for p in plants for c in CONTROLLERS]
    with ProcessPoolExecutor(max_workers=args.jobs) as ex:
        results = {(k, p, c): (r, s) for k, p, c, r, s in ex.map(run_one, jobs)}

    md: list[str] = []
    w = md.append
    w("# Controller bake-off: BGE Large, 150-325F\n")
    w("_Generated by `just model::control` from `src/minimeater/model/control*.py`. Do not edit by hand._\n")
    w(
        f"Plant: the BGE Large model at three parameter sets (low loss / nominal / high loss), fan {FAN.name}, "
        "0.3F probe noise, 10 s control period. The controllers only see the noisy probe, the setpoint, a "
        "lid-open flag and the on-board ambient sensor. FfPi and Mpc use a *firmware-side* model (nominal "
        "parameters, top vent assumed 1/4 open) for feedforward and actuator inversion, plus a per-egg FOPDT "
        "model from a simulated autotune (a 30% airflow step at each of "
        f"{', '.join(f'{s:.0f}' for s in ID_SETPOINTS)}F). The real top vent differs per scenario, so model "
        "mismatch is part of every run.\n"
    )
    w("Controllers:\n")
    w("- **PercentPid**: PI on 0-100% output (P=2 %/F, I=0.005 %/F/s), damper-first mapping. HeaterMeter-style baseline.")
    w(
        "- **FfPi**: commands airflow, not %. Steady-state feedforward from the model and ambient sensor, actuator "
        "inversion (airflow to damper/fan), tethered reference shaping, SIMC gains from the autotuned model, "
        "gain-scheduled by setpoint, conditional-integration anti-windup."
    )
    w(
        "- **Mpc**: offset-free linear MPC on the same model and inversion. Kalman disturbance estimate, 30 min "
        "horizon in 6 blocks, overshoot weighted 8x undershoot (a kamado cannot be actively cooled), move "
        "penalty, actuator limits.\n"
    )

    w("## Identified models (simulated autotune)\n")
    rows = []
    for p in plants:
        _, sched = results[(SCENARIOS[0].key, p, "FfPi")]
        for m in sched.models:
            rows.append([p, f"{m.setpoint_f:.0f}", f"{m.k_f_per_cfm:.0f}", f"{m.tau_s / 60:.1f}", f"{m.theta_s:.0f}"])
    w(md_table(["plant", "setpoint F", "gain F/CFM", "tau min", "dead time s"], rows))
    w("")

    keys = [
        "reach min", "overshoot F", "hold RMS F", "hold max |e| F", "lid recovery min", "lid overshoot F",
        "servo strokes/h", "fan on %",
    ]
    summary = {c: {k: [] for k in keys} for c in CONTROLLERS}
    for sc in SCENARIOS:
        w(f"## {sc.title}\n")
        w(f"Top vent: {sc.top}.\n")
        rows = []
        for c in CONTROLLERS:
            for p in plants:
                m = metrics(sc, results[(sc.key, p, c)][0])
                for k in keys:
                    summary[c][k].append(m[k])
                rows.append([c, p] + [f"{m[k]:.1f}" for k in keys])
        w(md_table(["controller", "plant"] + keys, rows))
        w("")
        fig, (ax1, ax2) = plt.subplots(2, 1, figsize=(8, 5.5), sharex=True)
        for i, c in enumerate(CONTROLLERS):
            r = results[(sc.key, "nominal", c)][0]
            ax1.plot(r.t_s / H, r.pit_f, color=f"C{i}", lw=0.8, label=c)
            ax2.plot(r.t_s / H, r.air_cfm, color=f"C{i}", lw=0.8)
        r0 = results[(sc.key, "nominal", CONTROLLERS[0])][0]
        ax1.plot(r0.t_s / H, r0.setpoint_f, color="k", lw=0.6, ls="--", label="setpoint")
        ax1.set(ylabel="pit F", title=f"{sc.title} - nominal plant")
        ax1.legend(fontsize=7)
        ax1.grid(alpha=0.3)
        ax2.set(xlabel="hours", ylabel="air CFM")
        ax2.grid(alpha=0.3)
        fig.tight_layout()
        fig.savefig(img_dir / f"{sc.key}.png", dpi=120)
        plt.close(fig)
        w(f"![{sc.key}](control/{sc.key}.png)\n")

    w("## Summary across all scenarios and plants\n")
    rows = []
    for c in CONTROLLERS:
        row = [c]
        for k in keys:
            v = np.array(summary[c][k], dtype=float)
            v = v[~np.isnan(v)]
            row.append(f"{np.mean(v):.1f} / {np.max(v):.1f}" if len(v) else "-")
        rows.append(row)
    w("mean / worst:\n")
    w(md_table(["controller"] + keys, rows))
    w("")
    (args.out / "control.md").write_text("\n".join(md))
    print(f"wrote {args.out / 'control.md'}")


