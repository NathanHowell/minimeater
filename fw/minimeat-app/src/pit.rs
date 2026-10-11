//! The control loop (`.4.14`): one call per control period turns the pit reading into an actuation,
//! with the mode/phase state machine, the supervisors (lid `.4.6`, fire `.4.19`), the learner
//! (`.4.4`) and the probe fail-safe around the pit controller.
//!
//! Plain synchronous code over plain values, so the host tests drive it against the plant model
//! exactly as the firmware does. The embassy task around it reads the probe pipeline (`.4.7`), calls
//! [`Pit::step`] every [`PERIOD_S`], hands the actuation to the drivers (`.4.8`), publishes the
//! status and [`Event`]s, and saves [`Pit::persist`] through the config store. Two jobs are too slow
//! for the control period and go to a low-priority task: the autotune fit ([`Pit::take_fit`] ->
//! [`Identifier::fit`] -> [`Pit::fitted`], ~1-3 s on the C6) and the airflow-map rebuild when the
//! ambient moves ([`Pit::map_wanted`] -> [`AirflowMap::build`] -> [`Pit::set_map`]).
//!
//! **Never lose the cook.** Everything that defines the cook (mode, setpoint, controller, autotune
//! dither, the learned schedule, the lid settings) is in [`Settings`], saved as it changes, and a
//! board that boots mid-cook carries on from it: the controller starts fresh on a hot egg the way it
//! approaches any setpoint (reference shaping from where the pit is now, so no overshoot), seeded
//! with the airflow that holds the pit where it is. A missing pit reading is ridden through for
//! [`PROBE_GRACE_S`] on the last command; after that the flap closes and the fan stops (a fire with
//! no air cannot run away), and control resumes the same way when the probe comes back.

use heapless::Vec;
use minimeat_core::control::autotune::{Autotune, Due, Fit, Identifier, scheduling_point};
use minimeat_core::control::{Feedforward, FfPi, Fire, FireObs, FireWatch, Fopdt, Mpc, Obs, PitController, Schedule};
use minimeat_core::model::Ambient;
use minimeat_core::model::actuator::{flap_angle_deg, open_frac};
use minimeat_core::model::airflow::{Actuation, AirPath, AirflowMap, ModelFeedforward};

use crate::alarms::Limit;
use crate::config::{ControllerKind, Cook, Lid, Mode, Settings};
use crate::hal::PROBE_JACKS;
use crate::probes::Check;

/// Control period, s.
pub const PERIOD_S: f64 = 10.0;
/// A pit reading missing for longer than this fails safe, s.
pub const PROBE_GRACE_S: f64 = 20.0;
/// Airflow at hold is averaged over this long (s) before it is trusted and saved as [`Cook::hold`];
/// the saved value moves only when the average drifts by more than `HOLD_MOVE` (relative, or
/// `HOLD_MOVE_CFM`), to spare the flash.
const HOLD_S: f64 = 600.0;
const HOLD_MOVE: f64 = 0.05;
const HOLD_MOVE_CFM: f64 = 0.03;
/// Setpoint limits (docs/protocol.md).
pub const SP_MIN_F: f64 = 150.0;
pub const SP_MAX_F: f64 = 325.0;

/// `phase` in the status (docs/protocol.md).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Idle,
    Startup,
    Hold,
    Lid,
    Recover,
    Manual,
    Fault,
}

/// `fault`/`fault_clear` codes this loop raises.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FaultCode {
    /// The selected pit probe isn't `ok` (raised by the pit loop).
    PitProbe,
    /// ADC die above 80C: the device keeps running and warns.
    BoardHot,
    /// Fan driven but the tach shows it not turning.
    FanStall,
    /// The probe ADC is not answering.
    Adc,
    /// The ambient sensor is not answering: feedforward falls back to 20C.
    Ambient,
}

impl FaultCode {
    /// Wire name (`faults` and the `fault` event's `code`, docs/protocol.md).
    pub fn name(self) -> &'static str {
        match self {
            FaultCode::PitProbe => "pit_probe",
            FaultCode::BoardHot => "board_hot",
            FaultCode::FanStall => "fan_stall",
            FaultCode::Adc => "adc",
            FaultCode::Ambient => "ambient",
        }
    }

    pub const ALL: [FaultCode; 5] = [
        FaultCode::PitProbe,
        FaultCode::BoardHot,
        FaultCode::FanStall,
        FaultCode::Adc,
        FaultCode::Ambient,
    ];
}

/// `~/event` (docs/protocol.md), the ones the control loop raises.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Event {
    LidOpen {
        pit_f: f64,
    },
    LidClosed {
        pit_f: f64,
    },
    FireLow {
        pit_f: f64,
        q: f64,
    },
    FireOut {
        pit_f: f64,
        q: f64,
    },
    SetpointReached {
        sp_f: f64,
    },
    /// A food probe's stall began or ended (`.4.22`; raised by [`crate::food::Foods`]).
    StallStart {
        probe: u8,
        t_f: f64,
    },
    StallEnd {
        probe: u8,
        t_f: f64,
    },
    /// The dither ended: the model at that setpoint now, and the fits taken during it.
    AutotuneDone {
        model: Fopdt,
        accepted: u32,
    },
    /// A probe crossed its limit, or came back inside it (`.4.12`).
    Alarm {
        probe: u8,
        which: Limit,
        t_f: f64,
    },
    AlarmClear {
        probe: u8,
        which: Limit,
        t_f: f64,
    },
    /// Every probe against ambient (power-up, or the `selfcheck` command).
    SelfCheck {
        results: [Check; PROBE_JACKS],
    },
    Fault(FaultCode),
    FaultClear(FaultCode),
}

impl Event {
    /// The `event_type` on the wire (docs/protocol.md).
    pub fn name(&self) -> &'static str {
        match self {
            Event::LidOpen { .. } => "lid_open",
            Event::LidClosed { .. } => "lid_closed",
            Event::FireLow { .. } => "fire_low",
            Event::FireOut { .. } => "fire_out",
            Event::SetpointReached { .. } => "setpoint_reached",
            Event::StallStart { .. } => "stall_start",
            Event::StallEnd { .. } => "stall_end",
            Event::AutotuneDone { .. } => "autotune_done",
            Event::Alarm { .. } => "alarm",
            Event::AlarmClear { .. } => "alarm_clear",
            Event::SelfCheck { .. } => "selfcheck",
            Event::Fault(_) => "fault",
            Event::FaultClear(_) => "fault_clear",
        }
    }
}

/// `~/set/...` and `~/cmd/...` that reach the control loop.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Command {
    Mode(Mode),
    Sp(f64),
    Controller(ControllerKind),
    LidDetect(bool),
    LidBoost(f64),
    /// Manual outputs, %.
    ManualFlap(f64),
    ManualFan(f64),
    Autotune,
    AutotuneStop,
    ModelReset,
    /// Compare the probes against ambient; the control task handles it, since it owns the probes.
    SelfCheck,
    /// Silence the LED for the alarms active now (the control task owns the alarms).
    AlarmAck,
}

/// Why a command was rejected (`rejected` event `error`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reject {
    OutOfRange,
    NotAllowed,
}

/// One period's result.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Output {
    pub phase: Phase,
    /// Commanded airflow, CFM (`out.q`).
    pub q: f64,
    /// Flap open-area fraction and fan speed fraction, for the drivers (`.4.8`).
    pub act: Actuation,
    /// Flap position, % of travel (`out.flap`).
    pub flap_pct: f64,
    /// The reference the controller tracks (`ref`): the shaped approach, plus the autotune dither.
    pub ref_f: f64,
    pub lid: bool,
    pub fire: Fire,
}

pub type Events = Vec<Event, 8>;

const CLOSED: Actuation = Actuation { damper: 0.0, fan: 0.0 };

pub struct Pit {
    cook: Cook,
    lid: Lid,
    ctrl: PitController,
    learn: Autotune,
    watch: FireWatch,
    path: AirPath,
    map: AirflowMap,
    ambient: Ambient,
    /// The controller needs a fresh start (and a seed) before it next steps.
    fresh: bool,
    out: Output,
    lost_s: f64,
    faulted: bool,
    was_open: bool,
    was_starting: bool,
    was_exciting: bool,
    accepted_at_start: u32,
    /// Time at hold, s, and the airflow averaged over it, CFM.
    hold_s: f64,
    hold_q: f64,
    due: Option<Due>,
    fitting: bool,
    events: Events,
}

impl Pit {
    /// Boot, from what the store loaded. Builds the airflow map (~1700 root solves): call it before
    /// the control loop starts.
    pub fn new(s: &Settings, path: AirPath, ambient: Ambient) -> Self {
        let mut learn = Autotune::default();
        if s.cook.autotune && s.cook.mode == Mode::Run {
            learn.start();
        }
        let mut p = Self {
            cook: s.cook,
            lid: s.lid,
            ctrl: controller(s.cook.controller, s.schedule.clone(), s.lid),
            learn,
            watch: FireWatch::default(),
            path,
            map: AirflowMap::build(&path, ambient),
            ambient,
            fresh: true,
            out: Output {
                phase: Phase::Idle,
                q: 0.0,
                act: CLOSED,
                flap_pct: 0.0,
                ref_f: s.cook.sp_f,
                lid: false,
                fire: Fire::Unknown,
            },
            lost_s: 0.0,
            faulted: false,
            was_open: false,
            was_starting: false,
            was_exciting: false,
            accepted_at_start: 0,
            hold_s: 0.0,
            hold_q: 0.0,
            due: None,
            fitting: false,
            events: Vec::new(),
        };
        p.was_exciting = p.learn.status().exciting;
        p
    }

    pub fn cook(&self) -> &Cook {
        &self.cook
    }

    pub fn schedule(&mut self) -> &Schedule {
        self.ctrl.schedule_mut()
    }

    pub fn autotune(&self) -> &Autotune {
        &self.learn
    }

    pub fn output(&self) -> &Output {
        &self.out
    }

    /// Copies what must survive a power cut into `s`, for the config store to save (it writes only
    /// what changed).
    pub fn persist(&mut self, s: &mut Settings) {
        s.cook = self.cook;
        s.lid = self.lid;
        if s.schedule != *self.ctrl.schedule_mut() {
            s.schedule = self.ctrl.schedule_mut().clone();
        }
    }

    pub fn command(&mut self, c: Command) -> Result<(), Reject> {
        match c {
            Command::Mode(m) => {
                if m != Mode::Run {
                    self.learn.stop();
                }
                if self.cook.mode == Mode::Off && m != Mode::Off {
                    // a cook starts from scratch; manual -> run is bumpless (the controller tracked)
                    self.fresh = true;
                    self.cook.hold = None;
                }
                self.cook.mode = m;
            }
            Command::Sp(v) => {
                if !(SP_MIN_F..=SP_MAX_F).contains(&v) {
                    return Err(Reject::OutOfRange);
                }
                self.cook.sp_f = v;
            }
            Command::Controller(k) => {
                if k != self.cook.controller {
                    self.cook.controller = k;
                    let schedule = self.ctrl.schedule_mut().clone();
                    self.ctrl = controller(k, schedule, self.lid);
                    self.fresh = true;
                }
            }
            Command::LidDetect(on) => {
                self.lid.detect = on;
                apply_lid(&mut self.ctrl, self.lid);
            }
            Command::LidBoost(b) => {
                if !(1.0..=3.0).contains(&b) {
                    return Err(Reject::OutOfRange);
                }
                self.lid.boost = b;
                apply_lid(&mut self.ctrl, self.lid);
            }
            Command::ManualFlap(v) | Command::ManualFan(v) if !(0.0..=100.0).contains(&v) => {
                return Err(Reject::OutOfRange);
            }
            Command::ManualFlap(v) => self.cook.manual_flap = v,
            Command::ManualFan(v) => self.cook.manual_fan = v,
            Command::Autotune => {
                if self.cook.mode != Mode::Run {
                    return Err(Reject::NotAllowed);
                }
                self.learn.start();
            }
            Command::AutotuneStop => self.learn.stop(),
            Command::ModelReset => {
                *self.ctrl.schedule_mut() = Schedule::default();
            }
            // the control task owns the probes and alarms and answers these
            Command::SelfCheck | Command::AlarmAck => {}
        }
        Ok(())
    }

    /// The ambient has moved enough that the airflow map should be rebuilt (off the control path).
    pub fn map_wanted(&self) -> Option<(AirPath, Ambient)> {
        self.map.stale(&self.ambient).then_some((self.path, self.ambient))
    }

    pub fn set_map(&mut self, map: AirflowMap) {
        self.map = map;
    }

    /// A window due for an autotune fit: copy the [`Identifier`] now (it moves on next period), fit
    /// it off the control path, and hand the result to [`Self::fitted`]. One fit at a time; a window
    /// that comes due while one is out is skipped (the next one is 5 min later).
    pub fn take_fit(&mut self) -> Option<(Due, &Identifier)> {
        let due = self.due.take()?;
        self.fitting = true;
        Some((due, self.learn.window()))
    }

    /// The result of a [`Self::take_fit`] job. Returns the schedule entry it changed, if any; the
    /// next [`Self::persist`] picks it up.
    pub fn fitted(&mut self, due: Due, fit: Option<Fit>) -> Option<Fopdt> {
        self.fitting = false;
        self.learn.apply(due, fit, self.ctrl.schedule_mut())
    }

    /// One control period. `pit_f`: the selected pit probe, `None` unless its status is `ok`;
    /// `ambient`: the outside-air reading (or the 20C fallback). Returns this period's output and
    /// the events it raised.
    pub fn step(&mut self, pit_f: Option<f64>, ambient: Ambient, dt: f64) -> (Output, &Events) {
        self.events.clear();
        self.ambient = ambient;
        let pit = pit_f.filter(|y| y.is_finite());
        self.lost_s = if pit.is_some() { 0.0 } else { self.lost_s + dt };
        let faulted = self.lost_s > PROBE_GRACE_S;
        if faulted != self.faulted {
            self.faulted = faulted;
            self.emit(if faulted {
                Event::Fault(FaultCode::PitProbe)
            } else {
                Event::FaultClear(FaultCode::PitProbe)
            });
        }

        let sp = self.cook.sp_f;
        // Ran: closed loop; Held: riding through a missing reading; otherwise not controlling
        let (mut ran, mut held) = (false, false);
        match (self.cook.mode, pit) {
            (Mode::Off, _) => {
                self.fresh = true;
                self.set(Phase::Idle, 0.0, CLOSED, sp);
            }
            (Mode::Manual, _) => {
                let act = Actuation {
                    damper: open_frac(self.cook.manual_flap / 100.0 * 90.0),
                    fan: self.cook.manual_fan / 100.0,
                };
                let y = pit.unwrap_or(sp);
                let q = self.map.airflow(y, act);
                match pit {
                    // keep the controller's estimate current, so the return to run is bumpless
                    Some(y) if !self.fresh => {
                        let (q_min, q_max) = self.map.limits(y);
                        let obs = Obs {
                            pit_f: y,
                            sp_f: sp,
                            lid: false,
                            q_min,
                            q_max,
                        };
                        let ff = ModelFeedforward {
                            path: &self.path,
                            ambient: self.ambient,
                        };
                        self.ctrl.track(&obs, &ff, q, dt);
                    }
                    Some(y) => self.start_controller(y, Some(q)),
                    None => self.fresh = true,
                }
                self.set(Phase::Manual, q, act, sp);
            }
            (Mode::Run, None) if faulted => {
                // fail safe: no air, no runaway
                self.fresh = true;
                self.set(Phase::Fault, 0.0, CLOSED, sp);
            }
            // a reading or two missing: ride through on the last command
            (Mode::Run, None) => held = true,
            (Mode::Run, Some(y)) => {
                if self.fresh {
                    self.start_controller(y, None);
                }
                self.run(y, dt);
                ran = true;
            }
        }

        if !ran {
            // not closed-loop plant data: restart the learning window, keep the fire watch's clock
            let obs = Obs {
                pit_f: f64::NAN,
                sp_f: sp,
                lid: false,
                q_min: 0.0,
                q_max: 0.0,
            };
            self.learn.sample(&obs, sp, self.out.q, false, dt);
            self.watch.update(
                &FireObs {
                    pit_f: f64::NAN,
                    sp_f: sp,
                    q: 0.0,
                    q_ff: 0.0,
                    q_max: 0.0,
                    expected_low: true,
                },
                dt,
            );
        }
        if !ran && !held {
            if self.out.lid {
                self.emit(Event::LidClosed {
                    pit_f: pit.unwrap_or(f64::NAN),
                });
            }
            self.out.lid = false;
            self.out.fire = Fire::Unknown;
            self.was_open = false;
            self.was_starting = false;
        }

        let exciting = self.learn.status().exciting;
        if exciting && !self.was_exciting {
            self.accepted_at_start = self.learn.status().accepted;
        }
        if self.was_exciting && !exciting {
            let at = scheduling_point(sp);
            let model = self.ctrl.schedule_mut().at(at);
            let accepted = self.learn.status().accepted - self.accepted_at_start;
            self.emit(Event::AutotuneDone { model, accepted });
        }
        self.was_exciting = exciting;
        self.cook.autotune = exciting;
        (self.out, &self.events)
    }

    fn ff(&self) -> ModelFeedforward<'_> {
        ModelFeedforward {
            path: &self.path,
            ambient: self.ambient,
        }
    }

    /// A fresh controller for a cook that may already be under way (boot, a probe back, a new
    /// controller): it approaches the setpoint from the pit's current temperature, starting from
    /// airflow `q` if given (manual mode), else from what [`Cook::hold`] says this egg needs.
    fn start_controller(&mut self, y: f64, q: Option<f64>) {
        let schedule = self.ctrl.schedule_mut().clone();
        self.ctrl = controller(self.cook.controller, schedule, self.lid);
        let ff = self.ff();
        let trim = match (q, self.cook.hold) {
            (Some(q), _) => q - ff.q_cfm(y.min(self.cook.sp_f)),
            (None, Some((sp, q))) => q - ff.q_cfm(sp),
            (None, None) => 0.0,
        };
        self.ctrl.resume(trim);
        self.fresh = false;
        self.hold_s = 0.0;
    }

    /// Averages the airflow while holding, and updates [`Cook::hold`] when it has moved.
    fn track_hold(&mut self, phase: Phase, q: f64, dt: f64) {
        if phase != Phase::Hold {
            self.hold_s = 0.0;
            return;
        }
        self.hold_q = if self.hold_s == 0.0 {
            q
        } else {
            self.hold_q + (q - self.hold_q) * (dt / HOLD_S).min(1.0)
        };
        self.hold_s += dt;
        if self.hold_s < HOLD_S {
            return;
        }
        let sp = self.cook.sp_f;
        let moved = match self.cook.hold {
            Some((at, q0)) => at != sp || (self.hold_q - q0).abs() > (HOLD_MOVE * q0).max(HOLD_MOVE_CFM),
            None => true,
        };
        if moved {
            self.cook.hold = Some((sp, self.hold_q));
        }
    }

    fn run(&mut self, y: f64, dt: f64) {
        let sp = self.cook.sp_f;
        let ref_sp = self.learn.reference(sp, dt);
        let (q_min, q_max) = self.map.limits(y);
        let obs = Obs {
            pit_f: y,
            sp_f: ref_sp,
            lid: false,
            q_min,
            q_max,
        };
        let ff = ModelFeedforward {
            path: &self.path,
            ambient: self.ambient,
        };
        let q = self.ctrl.step(&obs, &ff, dt);
        let q_ff = ff.q_cfm(sp);
        let act = self.map.actuate(y, q);

        let lid = self.ctrl.lid();
        let (open, recovering, starting) = (lid.open(), lid.active(), self.ctrl.starting());
        let phase = if open {
            Phase::Lid
        } else if recovering {
            Phase::Recover
        } else if starting {
            Phase::Startup
        } else {
            Phase::Hold
        };
        let fire = self.watch.update(
            &FireObs {
                pit_f: y,
                sp_f: sp,
                q,
                q_ff,
                q_max,
                expected_low: starting || recovering,
            },
            dt,
        );
        if let Some(due) = self.learn.sample(&obs, sp, q, !recovering, dt)
            && !self.fitting
        {
            self.due = Some(due);
        }

        if open != self.was_open {
            self.emit(if open {
                Event::LidOpen { pit_f: y }
            } else {
                Event::LidClosed { pit_f: y }
            });
        }
        if self.was_starting && !starting && !recovering {
            self.emit(Event::SetpointReached { sp_f: sp });
        }
        if fire != self.out.fire {
            match fire {
                Fire::Low => self.emit(Event::FireLow { pit_f: y, q }),
                Fire::Out => self.emit(Event::FireOut { pit_f: y, q }),
                _ => {}
            }
        }
        self.was_open = open;
        self.was_starting = starting;
        self.track_hold(phase, q, dt);
        self.set(phase, q, act, ref_sp);
        self.out.lid = open;
        self.out.fire = fire;
    }

    fn set(&mut self, phase: Phase, q: f64, act: Actuation, ref_f: f64) {
        self.out.phase = phase;
        self.out.q = q;
        self.out.act = act;
        self.out.flap_pct = flap_angle_deg(act.damper) / 90.0 * 100.0;
        self.out.ref_f = ref_f;
    }

    fn emit(&mut self, e: Event) {
        // eight per period is more than any period raises; a full queue drops the newest
        let _ = self.events.push(e);
    }
}

fn controller(kind: ControllerKind, schedule: Schedule, lid: Lid) -> PitController {
    let mut c = match kind {
        ControllerKind::Mpc => PitController::Mpc(Mpc::new(schedule)),
        ControllerKind::FfPi => PitController::FfPi(FfPi::new(schedule)),
    };
    apply_lid(&mut c, lid);
    c
}

fn apply_lid(c: &mut PitController, lid: Lid) {
    let r = c.lid_mut();
    r.boost = lid.boost;
    match (lid.detect, r.detector.is_some()) {
        (true, false) => r.detector = Some(Default::default()),
        (false, true) => r.detector = None,
        _ => {}
    }
}
