//! Transcendental constants with fixed inputs, computed here with the same `libm` the firmware runs (pure Rust, the
//! same bits on the host and the C6) and written to `OUT_DIR/consts.rs` as exact bit patterns. `ln`/`exp` are not
//! `const fn`; plain arithmetic on these stays in ordinary consts. Tests in each module check every value against
//! the runtime call, bit for bit.

use std::fmt::Write;

/// `control::autotune`: the fit's tau bounds, s, and its coarse grid size (keep in step with autotune.rs).
const TAU_MIN: f64 = 60.0;
const TAU_MAX: f64 = 6000.0;
const TAU_GRID: usize = 24;
/// `thermistor`: the generic B3950 preset, 100k at 25 C.
const B3950_R25: f64 = 100e3;

fn main() {
    let mut out = String::new();
    // the inputs too, so the modules can assert at compile time that they still match
    writeln!(
        out,
        "pub const BUILT_TAU_MIN: f64 = {TAU_MIN:?};\npub const BUILT_TAU_MAX: f64 = {TAU_MAX:?};"
    )
    .unwrap();
    writeln!(
        out,
        "pub const BUILT_TAU_GRID: usize = {TAU_GRID};\npub const BUILT_B3950_R25: f64 = {B3950_R25:?};"
    )
    .unwrap();
    let mut put = |name: &str, doc: &str, v: f64| {
        writeln!(
            out,
            "/// {doc}: {v:e}\npub const {name}: f64 = f64::from_bits({:#018x});",
            v.to_bits()
        )
        .unwrap();
    };
    let (l0, l1) = (libm::log(TAU_MIN), libm::log(TAU_MAX));
    put("LN_TAU_MIN", "ln(TAU_MIN)", l0);
    put("LN_TAU_MAX", "ln(TAU_MAX)", l1);
    put("LN_B3950_R25", "ln(100e3)", libm::log(B3950_R25));
    let taus: Vec<String> = (0..=TAU_GRID)
        .map(|i| {
            // the same expression as autotune's coarse loop, so exp sees the same bits
            let lt = l0 + (l1 - l0) * i as f64 / TAU_GRID as f64;
            format!("f64::from_bits({:#018x})", libm::exp(lt).to_bits())
        })
        .collect();
    writeln!(
        out,
        "/// exp of each coarse-grid point: the fit's log-spaced taus, s.\npub const TAU_COARSE: [f64; {}] = [{}];",
        TAU_GRID + 1,
        taus.join(", ")
    )
    .unwrap();
    let dst = std::path::Path::new(&std::env::var("OUT_DIR").unwrap()).join("consts.rs");
    std::fs::write(dst, out).unwrap();
    println!("cargo:rerun-if-changed=build.rs");
}
