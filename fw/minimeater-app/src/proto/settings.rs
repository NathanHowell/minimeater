//! `~/config` (docs/protocol.md): every setting, as stored in flash, plus the learned model.

use core::fmt::Write;

use minimeater_core::control::Schedule;

use super::json::Json;
use super::names;
use crate::config::{Probe, ProbeKind, Settings};

/// Writes the `~/config` object.
pub fn config<W: Write>(j: &mut Json<W>, s: &Settings) {
    let c = &s.cook;
    j.begin_obj();
    j.key("v").int(1);
    j.key("mode").str(names::mode(c.mode));
    j.key("sp").num(c.sp_f, 1);
    j.key("pit_probe").int(i64::from(c.pit_probe));
    j.key("controller").str(names::controller(c.controller));
    j.key("lid_detect").bool(s.lid.detect);
    j.key("lid_boost").num(s.lid.boost, 1);
    j.key("manual_flap").num(c.manual_flap, 1);
    j.key("manual_fan").num(c.manual_fan, 1);
    j.key("probes").begin_arr();
    for p in &s.probes {
        probe(j, p);
    }
    j.end_arr();
    j.key("model").begin_arr();
    // empty until something has been learned: the nominal schedule is not a model of this egg
    if s.schedule != Schedule::default() {
        for m in s.schedule.models() {
            j.begin_obj();
            j.key("sp").num(m.sp_f, 1);
            j.key("k").num(m.k, 1);
            j.key("tau").num(m.tau_s, 0);
            j.key("dead").num(m.theta_s, 0);
            j.end_obj();
        }
    }
    j.end_arr();
    j.end_obj();
}

fn probe<W: Write>(j: &mut Json<W>, p: &Probe) {
    j.begin_obj();
    j.key("name").str(&p.name);
    match p.kind {
        ProbeKind::Preset(preset) => {
            j.key("type").str(preset.name());
            j.key("coeffs").null();
        }
        ProbeKind::Custom(t) => {
            j.key("type").str("custom");
            j.key("coeffs").begin_obj();
            j.key("a").sci(t.a);
            j.key("b").sci(t.b);
            j.key("c").sci(t.c);
            j.end_obj();
        }
    }
    j.key("offset").num(p.offset_f, 1);
    j.key("lo").opt_num(p.lo_f, 1);
    j.key("hi").opt_num(p.hi_f, 1);
    j.key("cut").str(p.cut.name());
    j.key("stall").str(p.stall.name());
    j.key("bump").num(p.bump_f, 1);
    j.end_obj();
}
