# fw

A Rust cargo workspace for the ESP32-C6-WROOM-1-N8 (ADR 0001).

| Crate | Target | What |
|---|---|---|
| `minimeat-core` | host and `riscv32imac-unknown-none-elf` | `no_std` and no-alloc logic: units, probe conversion and the pit controller (MPC, FF+PI, reference shaping, lid recovery); later autotune and the MQTT `proto`. The firmware and the host digital twin share it. |
| `minimeat-app` | host and target | the application, generic over the peripheral traits in `hal` (probe ADC, ambient, fan PWM, tach, servo, servo power, LED, button, config flash, USB-C source), plus the chip drivers (ADS1220, TMP117, USB-C CC) over `embedded-hal-async`. No esp-hal and no target `cfg`s. |
| `minimeat-sim` | host (and `no_std`) | the BGE Large plant model: the Python `sim.py` port plus fuel depletion, the fire going out, and a two-node food model with evaporative stall. `hal` implements the `minimeat-app` traits on the plant (`World`), for the digital twin and the scenario suite. |
| `minimeat-fw` | `riscv32imac-unknown-none-elf` | the embassy binary on esp-hal 1.2 and esp-rtos; `board.rs` binds the traits to esp-hal and holds the (provisional) pin map |

```bash
cargo test                    # host tests for minimeat-core, -app and -sim (default-members)
cargo fw-build                # firmware, release
cargo fw                      # build, flash over USB-C, then monitor (needs `cargo install espflash --locked`)
```

- **Peripheral boundary:** application code takes an `Io<B: Board>` and never names esp-hal types. Embassy tasks can't be generic, so each binary wraps the generic async functions in thin `#[task]`s for its concrete board. Time is `embassy-time` on both sides (the twin supplies its own driver).
- **Toolchain:** stable Rust only. `rust-toolchain.toml` pulls the RISC-V target and `rust-src`. There is no Xtensa fork.
- **Floating point:** `minimeat-core` uses f64 with `libm`, so host and target compute the same values. The C6 has no FPU, so everything is soft-float either way.
- **Ported from Python:** tests pin values produced by the reference in `src/minimeat/model/`.
  - Controller: `tests/control_replay.rs` replays closed-loop trajectories from `uv run minimeat-fixtures` (`tests/fixtures/control.json`). The Rust MPC and FF+PI reproduce the Python commands to within a few 1e-6 CFM. Regenerate the fixture whenever `control.py` changes.
  - Thermistor constants: regenerate with `uv run python`.
