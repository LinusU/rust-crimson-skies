# T336: the `__eh_frame` linker warning is unwind corruption, not noise

Date: 2026-09-23. Task: #336 "Investigate the `__eh_frame` linker warning when
linking the cs binary" (follow-up of
`docs/findings/2026-09-23-f00-d-platform-bootstrap-evidence-and-toolchain-freeze.md`,
"Why a static gate is required"). Capabilities used: ordinary build/test only.
Machine: macOS (Darwin 27), aarch64-apple-darwin, rustc/cargo 1.98.1,
`ld` 27037.1.

## Symptom

`cargo test --workspace` / `cargo build -p cs_app` emit, surfaced through
rustc's default `#[warn(linker_messages)]`:

```
ld: __eh_frame section too large (max 16MB) to encode dwarf unwind offsets in
compact unwind table, performance of exception handling might be affected
```

## Measurement (dev profile, `target/debug/cs`, before the fix)

| item | value | how |
|---|---|---|
| `__TEXT,__eh_frame` | **17,061,116 B** (16.27 MiB) | `size -m -l` / `otool -l` |
| `__TEXT,__unwind_info` | 3,452,880 B | same |
| FDEs in `__eh_frame` | 351,004 | `xcrun llvm-dwarfdump --debug-frame` |
| first FDE past the cap | offset `0x01000000`, FDE #345,274, `pc=102e426c8` | same |
| compact-unwind entries | 351,006 dwarf-mode, 349,013 second-level total | `xcrun unwinddump` |
| entries with `dwarf offset 0x00000000` | **5,733** | same |

## Root cause

Compact-unwind entries that reference DWARF unwind info store the FDE offset
in a 24-bit field, so offsets ≥ `0x01000000` (16 MiB) are unencodable. The
linker still emits a second-level entry for each such function, but with the
dwarf offset field wrapped to `0x00000000` — and offset 0 in `__eh_frame` is
the CIE, never an FDE. The 5,733 affected entries are exactly the FDEs at
`__eh_frame` offsets ≥ 16 MiB (first victim `funcOffset=0x02E426C8` =
`pc=102e426c8`, the boundary FDE). They are mostly `parry3d`/`hashbrown`/`glam`
code — the physics path.

This is **correctness, not performance**: on this platform the unwinder has no
working `__eh_frame` fallback. Verified by experiment — a probe binary linked
with `-Wl,-no_compact_unwind` keeps its `__eh_frame` section yet any panic
dies immediately:

```
fatal runtime error: failed to initiate panic, error 5, aborting
```

(error 5 = `_URC_END_OF_STACK`: no usable unwind info at the panic site.) So a
panic unwinding through any of the 5,733 corrupt-entry frames reads the CIE as
an FDE and aborts mid-unwind — destructors skipped, `catch_unwind` bypassed,
test binaries SIGABRT instead of reporting a failure.

## Options weighed

| option | verdict |
|---|---|
| `panic = "abort"` | rejected: changes behaviour (no unwinding at all) and `cargo test` requires unwind-capable targets |
| `-Wl,-no_compact_unwind` via build script | rejected: removes `__unwind_info`; measured above — unwinding then aborts at the first frame |
| `-Wl,-no_warn_eh_frame_too_large` | rejected as the *only* change: it hides the warning while leaving 5,733 broken unwind entries; remains a documented fallback if size ever cannot be kept down |
| `strip` / `debug = 0` | useless: `__eh_frame` is a loadable `__TEXT` section, not debug info |
| `-Cforce-unwind-tables=no` | needs `RUSTFLAGS`/`.cargo/config.toml` (outside owner paths) and tables are still required under `panic = "unwind"` |
| **`[profile.dev.package."*"] opt-level = 3`** | **chosen**: shrinks dependency code — and therefore `__eh_frame` — back under the cap; also the upstream-recommended dev setting for Bevy (opt-level-0 engine code is too slow to run) |

## Result (dev profile, `target/debug/cs`, after the fix)

| item | before | after |
|---|---|---|
| `__eh_frame` | 17,061,116 B | **6,215,304 B** (5.9 MiB, 63% headroom) |
| `__unwind_info` | 3,452,880 B | 1,233,576 B |
| second-level unwind entries | 349,013 | 119,454 |
| dwarf-mode entries | 351,006 | 93,487 |
| `dwarf offset 0` entries | 5,733 | **0** |
| max encoded dwarf offset | 0xFFFFC8 (saturated) | 0x5ED644 |
| linker warning | emitted | **gone** |

`cargo build -p cs_app --bin cs` and `cargo test --workspace` link cleanly.
Release was measured too: `cargo build --release -p cs_app --bin cs` produces
`__eh_frame` = 5,627,420 B, `__unwind_info` = 1,140,248 B, zero `dwarf offset
0` entries, no warning (release deps are already `opt-level = 3`, so the
override mainly matters for dev/test).

The `test` profile inherits the `dev` package override (the workspace test
binaries reuse the opt-level-3 dependency build and their `__eh_frame` is
under the cap — asserted by the tests below).

The warning is intentionally **not suppressed**: if dependency growth pushes
`__eh_frame` back over 16 MiB it will reappear as a tripwire instead of
silently corrupting unwind entries again.

## Gates added (`accept_t336_*`, `crates/cs_app/tests/`)

| test | fails when |
|---|---|
| `accept_t336_dev_profile_keeps_dependencies_optimized` | the `[profile.dev.package."*"] opt-level = 3` override is removed from the workspace `Cargo.toml` (portable static gate) |
| `accept_t336_linked_images_keep_eh_frame_under_the_encoding_limit` | either the `cs` binary (`CARGO_BIN_EXE_cs`) or the test binary itself carries ≥ 16 MiB of `__eh_frame`, or `__unwind_info` is missing — Mach-O load commands are parsed directly, no fixtures (macOS only; on Linux the other two tests still run) |
| `accept_t336_panic_unwinds_and_runs_destructors` | a panic unwound through three frames fails to reach `catch_unwind` or skips destructors — the property the corrupt entries would break |

## Residual risk

- Only `cs_app` links bevy/avian today. If another crate gains a large dep
  graph its binaries inherit the same workspace override automatically.
- The cap is a property of Apple's compact-unwind format; Linux/ELF builds
  have no equivalent limit (`.eh_frame` is used directly).

## Sources

`man ld` (`-warn_compact_unwind`, `-no_warn_eh_frame_too_large`),
`xcrun unwinddump`, `xcrun llvm-dwarfdump --debug-frame`, `otool -l`,
`size -m -l`, probe binaries under `/tmp` (not committed). No original data
was read; `CS_GAME_DIR` was not used.
