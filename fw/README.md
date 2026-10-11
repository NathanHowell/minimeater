# fw

A Rust cargo workspace for the ESP32-C6-WROOM-1-N8 (ADR 0001).

| Crate | Target | What |
|---|---|---|
| `minimeat-core` | host and `riscv32imac-unknown-none-elf` | `no_std` logic: units, probe conversion, and later the controller, autotune and MQTT `proto`. The firmware and the host digital twin share it. |
| `minimeat-fw` | `riscv32imac-unknown-none-elf` | the embassy binary on esp-hal 1.2 and esp-rtos |

```bash
cargo test -p minimeat-core   # host tests; `cargo test` alone does the same (default-members)
cargo fw-build                # firmware, release
cargo fw                      # build, flash over USB-C, then monitor (needs `cargo install espflash --locked`)
```

- **Toolchain:** stable Rust only. `rust-toolchain.toml` pulls the RISC-V target and `rust-src`. There is no Xtensa fork.
- **Floating point:** `minimeat-core` uses f64 with `libm`, so host and target compute the same values. The C6 has no FPU, so everything is soft-float either way.
- **Ported from Python:** tests pin values produced by the reference in `src/minimeat/model/`. When a port changes, regenerate those values with `uv run python` and update the test constants.
