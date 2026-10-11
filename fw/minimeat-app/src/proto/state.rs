//! `~/state` (docs/protocol.md): the status as published every control period.

use core::fmt::Write;

use minimeat_core::units::k_to_f;

use super::json::Json;
use super::names;
use crate::config::Mode;
use crate::tasks::Status;

/// Writes the `~/state` object for `s`. `rssi` is the WiFi signal in dBm, `None` while there is no
/// link (or no WiFi, as in the twin).
pub fn state<W: Write>(j: &mut Json<W>, s: &Status, rssi: Option<i32>) {
    let c = &s.cook;
    j.begin_obj();
    j.key("v").int(1);
    j.key("up").int(s.up_s as i64);
    j.key("mode").str(names::mode(c.mode));
    j.key("phase").str(names::phase(s.out.phase));
    j.key("sp").num(c.sp_f, 1);
    j.key("ref").num(s.out.ref_f, 1);
    j.key("pit")
        .opt_num(s.temps.get(c.pit_probe as usize).copied().flatten(), 1);
    // the ambient sensor reads in C; the wire is F
    j.key("amb").opt_num(s.ambient_c.map(|c| k_to_f(c + 273.15)), 1);
    j.key("probes").begin_arr();
    for n in 0..s.temps.len() {
        j.begin_obj();
        j.key("t").opt_num(s.temps[n], 1);
        j.key("st").str(s.probe_st[n].name());
        j.key("alarm").opt_str(s.alarms[n].map(|a| a.name()));
        j.key("food");
        match s.foods[n] {
            Some(f) => {
                j.begin_obj();
                j.key("rate").opt_num(f.rate_f_h, 1);
                j.key("eta").opt_num(f.eta_s.map(libm::round), 0);
                j.key("stall").str(names::stall(f.stall));
                j.end_obj();
            }
            None => {
                j.null();
            }
        }
        j.end_obj();
    }
    j.end_arr();
    j.key("tc").null();
    j.key("out").begin_obj();
    j.key("q").num(s.out.q, 2);
    j.key("flap").num(s.out.flap_pct, 1);
    j.key("fan").num(s.out.act.fan * 100.0, 1);
    j.key("rpm").opt_num(s.fan_rpm.map(libm::round), 0);
    j.end_obj();
    j.key("lid").bool(s.out.lid);
    j.key("fire").str(names::fire(s.out.fire));
    j.key("autotune");
    // the controller learns whenever it is running the cook
    if c.mode == Mode::Run {
        j.begin_obj();
        j.key("exciting").bool(s.autotune.exciting);
        j.key("accepted").int(i64::from(s.autotune.accepted));
        j.key("window").num(s.autotune.window, 2);
        j.end_obj();
    } else {
        j.null();
    }
    j.key("faults").begin_arr();
    for f in s.faults.iter() {
        j.str(f.name());
    }
    j.end_arr();
    j.key("diag").begin_obj();
    j.key("rssi").opt_num(rssi.map(f64::from), 0);
    j.key("board").opt_num(s.adc_die_c, 1);
    j.end_obj();
    j.end_obj();
}
