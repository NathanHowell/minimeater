//! The stall policies (`.4.23`) on the brisket cook: `notify` and `wrap` leave the setpoint alone,
//! `bump` raises it for the stall and puts it back when it breaks, without overshooting the new
//! setpoint, and the brisket is done sooner. Each run is its own process (the twin is one run per
//! process); the events are asserted by the suite (`suite::Expect`), the trajectory here.

use std::path::PathBuf;
use std::process::Command;

struct Row {
    t_h: f64,
    pit: f64,
    sp: f64,
    core: f64,
    stalled: bool,
}

fn run(name: &str) -> Vec<Row> {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("stall_policy");
    std::fs::create_dir_all(&dir).unwrap();
    let csv = dir.join(format!("{name}.csv"));
    let out = Command::new(env!("CARGO_BIN_EXE_minimeater-twin"))
        .args(["--scenario", name, "--metrics", "--csv"])
        .arg(&csv)
        .output()
        .expect("twin binary runs");
    assert!(out.status.success(), "{name}: {}", String::from_utf8_lossy(&out.stderr));
    std::fs::read_to_string(&csv)
        .unwrap()
        .lines()
        .skip(1)
        .map(|l| {
            let f: Vec<_> = l.splitn(14, ',').collect();
            let n = |i: usize| f[i].parse::<f64>().unwrap_or(f64::NAN);
            Row {
                t_h: n(0) / 3600.0,
                pit: n(1),
                sp: n(3),
                core: n(9),
                stalled: f[13].starts_with("Some(In"),
            }
        })
        .collect()
}

fn done_h(rows: &[Row]) -> f64 {
    rows.iter().find(|r| r.core >= 203.0).expect("the brisket finished").t_h
}

#[test]
fn only_bump_moves_the_setpoint_and_it_comes_back() {
    let (notify, wrap, bump) = std::thread::scope(|s| {
        let a = s.spawn(|| run("stall-notify"));
        let b = s.spawn(|| run("stall-wrap"));
        let c = s.spawn(|| run("stall-bump"));
        (a.join().unwrap(), b.join().unwrap(), c.join().unwrap())
    });

    for rows in [&notify, &wrap] {
        assert!(rows.iter().skip(1).all(|r| r.sp == 250.0), "the setpoint never moves");
    }
    assert_eq!(
        done_h(&notify),
        done_h(&wrap),
        "a reminder changes nothing about the cook"
    );

    let first = bump.iter().position(|r| r.stalled).expect("a stall");
    let stall_start = bump[first].t_h;
    let at = bump.iter().position(|r| r.sp > 250.0).expect("the setpoint was bumped");
    assert!(bump[at].t_h - stall_start < 0.1, "bumped as the stall began");
    assert_eq!(bump[at].sp, 275.0, "by the default 25F");
    let back = bump[at..].iter().position(|r| r.sp == 250.0).expect("and put back") + at;
    let stall_end = bump[first..].iter().find(|r| !r.stalled).unwrap().t_h;
    assert!(
        (bump[back].t_h - stall_end).abs() < 0.1,
        "restored {:.2} h, the stall broke {stall_end:.2} h",
        bump[back].t_h
    );
    assert!(
        bump[at..back].iter().all(|r| r.sp == 275.0),
        "one bump, not a staircase"
    );

    // the pit climbs to the new setpoint the way it does for any setpoint change
    let over = bump[at..back].iter().map(|r| r.pit - r.sp).fold(f64::MIN, f64::max);
    assert!(over < 10.0, "{over:.1}F over the bumped setpoint");

    let (a, b) = (done_h(&notify), done_h(&bump));
    println!("done at {a:.2} h on notify, {b:.2} h bumped to 275F ({over:.1}F over)");
    assert!(b < a - 0.5, "the bump pays: {b:.2} h against {a:.2} h");
}
