"""Pit probe auto-detection (minimeat-seq.4.36): which jack is in the pit, and what to do until it is known.

Reference for the firmware port. Three parts:

  PitFinder   - looks at every jack's reading each control period and names the pit probe. It needs
                evidence that a probe is measuring the fire's gas: clearly above ambient, and clearly
                above every other probe (or alone and hot). Once it names one the choice is locked
                for the cook; a locked probe that goes missing is a fault, never a reason to look again.
  PitMismatch - watches the selected probe for not behaving like a pit probe (meat selected by hand,
                or named by the finder with no pit probe plugged in); a trip warns or stops per `pit_mismatch`.
  AutoPit     - the controller around it. Until the finder has a name, the wrapped controller runs on the
                hottest probe, which is the pit probe once the fire is lit (shutting the flap would starve
                the fire, and feedforward airflow alone lights it too slowly to be seen: the twin took 94 min
                to reach setpoint against 30). With no probe reading at all it gets the model's feedforward
                airflow for the setpoint. After, it runs on the locked jack.
"""

from dataclasses import dataclass, field, replace

from .control import AirflowMap, FeedForward
from .sim import Controller, Obs
from .units import k_to_f


@dataclass
class PitFinder:
    """Names the pit probe from the jack readings; `None` until there is evidence.

    A candidate is named when, for `hold_s`, all of these hold:
      hot   - it is `hot_f` above the ambient: the fire is lit and the probe is in its gas, not in a
              cold roast;
      gap   - it is `gap_f` above every other reading (vacuous for a lone probe);
      live  - it looks like a probe in gas, not in meat: it rises at `rise_f_min` or more over the
              last `window_s`, or it is within `near_f` of the setpoint (a cook already at
              temperature, as after a factory reset).
    The rise test only rules out a probe that is flat (a cook already at temperature takes the `near_f`
    way in). It cannot tell a pit probe from meat: thin meat in a hot pit climbs as fast as a slow
    light-off, so with no pit probe plugged in a food probe can be named.
    The choice is then locked: a locked probe that goes missing is the
    controller's `pit_probe` fault, never a reason to look again.
    """

    hot_f: float = 40.0
    gap_f: float = 40.0
    rise_f_min: float = 1.0
    window_s: float = 180.0
    near_f: float = 25.0
    hold_s: float = 120.0
    locked: int | None = None
    _hist: list = field(default_factory=list)  # (t_s, readings)
    _cand: int | None = None
    _since: float = 0.0

    def _slope_f_min(self, j: int) -> float:
        pts = [(t, r[j]) for t, r in self._hist if r[j] is not None]
        if len(pts) < 3:
            return 0.0
        n = len(pts)
        tm = sum(t for t, _ in pts) / n
        ym = sum(y for _, y in pts) / n
        var = sum((t - tm) ** 2 for t, _ in pts)
        return 60.0 * sum((t - tm) * (y - ym) for t, y in pts) / var if var else 0.0

    def step(self, t_s: float, ambient_f: float, setpoint_f: float, jacks: tuple[float | None, ...]) -> int | None:
        if self.locked is not None:
            return self.locked
        self._hist.append((t_s, jacks))
        self._hist = [(t, r) for t, r in self._hist if t > t_s - self.window_s]
        live = [(r, j) for j, r in enumerate(jacks) if r is not None]
        cand = None
        if live:
            top, j = max(live)
            rest = [r for r, k in live if k != j]
            gas = self._slope_f_min(j) >= self.rise_f_min or abs(top - setpoint_f) <= self.near_f
            if top >= ambient_f + self.hot_f and gas and all(top - r >= self.gap_f for r in rest):
                cand = j
        if cand is None or cand != self._cand:
            self._cand, self._since = cand, t_s
        elif t_s - self._since >= self.hold_s:
            self.locked = cand
        return self.locked


@dataclass
class PitMismatch:
    """Watches the selected pit probe for not behaving like one; `policy` says what a trip does.

    The selected probe may be meat (a manual choice, or an auto choice with no pit probe plugged in).
    A food probe lags the gas by tens of minutes, so the controller calls for full air and the real pit
    runs away unseen. Two signatures, each held continuously:
      slow - the fan is running and the probe is `short_f` or more below the setpoint for `slow_s`.
             A pit probe lights off to within `short_f` in under 40 min in every plant of the band
             (the slowest is a high-loss kamado at 325 F in cold wind); meat takes longer, or never.
             A cook that cannot reach its setpoint (out of fuel) trips it too, which is also worth knowing.
      hot  - the probe is `over_f` or more above the setpoint for `over_s`. A pit probe in the band
             overshoots by 34 F at most; a food probe in a pit left on full air overshoots by 100 F.
    `tripped` latches the reason until `reset`; the caller applies the policy: warn raises the fault
    advisory and stop shuts the flap and fan as `pit_probe` does.
    """

    policy: str = "warn"  # off | warn | stop
    short_f: float = 40.0
    slow_s: float = 3600.0
    over_f: float = 50.0
    over_s: float = 120.0
    tripped: str | None = None
    _slow_since: float | None = None
    _hot_since: float | None = None

    def step(self, t_s: float, probe_f: float, setpoint_f: float, fan_running: bool) -> str | None:
        if self.policy == "off" or self.tripped is not None:
            return self.tripped
        slow = fan_running and probe_f <= setpoint_f - self.short_f
        self._slow_since = (self._slow_since if self._slow_since is not None else t_s) if slow else None
        hot = probe_f >= setpoint_f + self.over_f
        self._hot_since = (self._hot_since if self._hot_since is not None else t_s) if hot else None
        if self._slow_since is not None and t_s - self._slow_since >= self.slow_s:
            self.tripped = "slow"
        elif self._hot_since is not None and t_s - self._hot_since >= self.over_s:
            self.tripped = "hot"
        return self.tripped

    def reset(self) -> None:
        self.tripped = self._slow_since = self._hot_since = None


@dataclass
class AutoPit:
    """Wraps a pit controller so it can start with no idea which jack is the pit."""

    inner: Controller
    amap: AirflowMap
    ff: FeedForward
    finder: PitFinder = field(default_factory=PitFinder)
    mismatch: PitMismatch = field(default_factory=PitMismatch)
    fan: float = 0.0  # last fan command, for the mismatch watch

    @property
    def pit_jack(self) -> int | None:
        return self.finder.locked

    def step(self, obs: Obs, dt: float) -> tuple[float, float]:
        amb_f = k_to_f(obs.ambient.t_k)
        jack = self.finder.step(obs.t_s, amb_f, obs.setpoint_f, obs.jacks)
        if jack is None:
            live = [(r, j) for j, r in enumerate(obs.jacks) if r is not None]
            jack = max(live)[1] if live else None
        if jack is not None and obs.jacks[jack] is not None:
            if self.mismatch.step(obs.t_s, obs.jacks[jack], obs.setpoint_f, self.fan > 0.0) and self.mismatch.policy == "stop":
                self.fan = 0.0
                return 0.0, 0.0
            out = self.inner.step(replace(obs, pit_f=obs.jacks[jack]), dt)
            self.fan = out[1]
            return out
        q = self.ff.q_cfm(obs.ambient, obs.setpoint_f)
        return self.amap.actuate(obs.ambient, amb_f, q)
