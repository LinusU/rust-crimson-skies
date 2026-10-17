# #1543: `cs_app` as one integration-test binary — and why macOS cannot link it under 16 MiB of `__eh_frame`

Date: 2026-10-17. Task: **#1543** "One integration-test binary for cs_app (249
test files)", following the convention of
`docs/findings/2026-10-17-t1542-one-test-binary-per-crate.md`. Capabilities:
`retail,gpu,audio`, `CS_GAME_DIR` set (see the ignored-set run in section 4).

## 1. What changed

- `crates/cs_app/Cargo.toml`: `autotests = false`, one `[[test]]`
  (`cs_app_tests`, `tests/main.rs`), and `cs_xtask` as a dev-dependency (the
  acceptance test calls the guard's library function). `Cargo.lock` gains the
  matching dependency edge.
- `crates/cs_app/tests/main.rs` declares the former top-level files and the
  12 former directory targets (`accessibility camera campaign environment
  flight hud physics render scrapbook text ui world`) as modules. Each former
  `tests/<dir>/main.rs` became `tests/<dir>/mod.rs`. No test body changed.
- Inside those 12 directories the sibling files reached the old binary root
  with `use crate::common::…`; the "root" is now the directory module, so
  `crate::` became `crate::<dir>::` (module scoping, mechanical, 136 files).
  `tests/damage`, `tests/debris` and `tests/playtest_retail` are still reached
  by their single consumer's `#[path]`; `f64_c_support`/`f64_d_support` (and
  `world/import_retail.rs`) were `#[path]`-included by several files, which
  clippy's `duplicate_mod` forbids, so they are declared once and the former
  includers `use` them.
- `tests/accept_f48_d_crash_recovery_matrix.rs`: the crash-child re-exec used
  `--exact <bare name>`; libtest's exact match is on the full path, so the
  constant is now `accept_f48_d_crash_recovery_matrix::f48_d_…`. This was the
  only test that relied on being its own process image by name.
- `tools/cs_xtask/src/test_layout.rs`: **guard bug fixed.** The guard resolved a
  `#[path = "…"]` on a file module against `<dir>/<file stem>` for every plain
  (non-`mod.rs`) file. rustc resolves it against the *directory of the declaring
  file* (the stem directory is only for nested `mod x;` without `#[path]` and for
  inline blocks), so the guard called `tests/damage/*.rs`, `tests/debris/*`,
  `tests/playtest_retail/*` and `tests/f64_d_support/mod.rs` stray although
  rustc compiles and runs them. Regression test
  `accept_test_bin_guard_resolves_path_attributes_beside_a_plain_file`. This is
  outside the task's owner paths; it is the minimum change that lets the guard
  pass on a correct layout.
- `tests/accept_test_bin_cs_app.rs`: two tests (`accept_test_bin_cs_app_…`) ask
  `cs_xtask::test_layout` that cs_app is migrated, has one target, and reaches
  every `.rs` file under `tests/`.

## 2. Test names, before and after

`cargo test -p cs_app --locked -- --list --include-ignored` on the first
version of the migration listed **2729** names; the sources of `HEAD~` hold
**2719** distinct `#[test]` functions. The 10 extra were `tests/world/import_retail.rs`
(9 tests) and one `accept_f18_…` test that
`tests/campaign/vs_m01_geometry_verdict.rs` loaded a second time by `#[path]`
(they ran in both the `world` and the `campaign` binary). clippy's
`duplicate_mod` (`-D warnings`) refuses a file loaded as two modules, so the
final layout declares `f64_c_support`/`f64_d_support` once in `main.rs` and
`world::import_retail` as `pub(crate)`, and the includers `use` them. Each of
those tests is therefore listed and run once in the single binary where it ran
twice across the old ones; no name from before is missing. Listing the old
layout itself (249 fresh binaries, ~1 min of macOS scan each) was not done.

## 3. The blocker: `__eh_frame` over 16 MiB on macOS

`docs/findings/2026-09-23-t336-eh-frame-unwind-table.md` records that a linked
macOS image whose `__eh_frame` passes 16 MiB gets corrupt compact-unwind
entries, and a panic through them aborts the process. `accept_t336_linked_
images_keep_eh_frame_under_the_encoding_limit` asserts it for the test binary
itself. The single `cs_app_tests` binary measures:

| `[profile.dev.package.cs_app] opt-level` | `__eh_frame` of `cs_app_tests` |
| --- | --- |
| 0 (default; workspace deps already at 3) | 24,411,288 B |
| 1 | 17,622,852 B |
| 3 | 17,504,988 B |
| "s" | 17,437,968 B |

(`cs` is 5.3-5.5 MiB, the `cs_app` lib unit binary 6.5-7.1 MiB, so the test
modules add ~11 MiB at any optimisation level.) The limit is 16,777,216 B. A
package opt-level override therefore does **not** fix it, so none is committed.
With the default profile the t336 gate fails on macOS and the linker prints
`__eh_frame section too large`; on Linux/ELF there is no such limit and that
gate is `cfg(target_os = "macos")`. The first full run of the single binary
(2037 passed) shows exactly this: the only failures were that gate, and the
f48_d re-exec fixed in section 1.

What this means for the owner: one binary per crate is incompatible with this
crate on macOS unless (a) the test code itself is reduced, (b) `cs_app`'s tests
are split into a small fixed number of binaries (which needs the guard's
"exactly one `[[test]]`" rule relaxed for this crate), or (c) the t336 gate is
changed. None of these is mine to decide.

## 4. Runs (host: macOS aarch64, `CS_GAME_DIR` set, `CS_ENGINE_IMAGE` and
`CS_EVIDENCE_DIR` unset)

- Whole binary, ignored tests skipped (`cargo test -p cs_app --test
  cs_app_tests`): 2037 passed, 3 failed, 689 ignored in 1093 s (the first
  launch pays the security scan and the slow retail-free composed tests; a
  second launch of the rebuilt binary finished in 82 s). The 3 failures were the
  two f48_d crash-child tests (fixed, section 1) and the t336 `__eh_frame` gate
  (section 3).
- Whole binary with the ignored set (`-- --include-ignored`): **2575 passed,
  156 failed**, 4204 s, exit 101. Failure causes, from the panic messages:
  ~140 `evidence_report_*` harnesses say `CS_EVIDENCE_DIR is not set` (they run
  only in the owner's evidence sequence), ~14 say `CS_ENGINE_IMAGE` is unset,
  2 want `CS_REGENERATE_*`, the t336 gate (section 3), one windowed capture
  (`accept_m01_play_world_windowed_mission_capture_draws_world_pixels`, "the
  mission capture must pass") and one audio-device assertion
  (`accept_vs_m01_runtime_evidence_audio_…engine_loop…`). I did not run the old
  249-binary layout to compare, so I cannot say the last two predate the
  migration: they are the ones a reviewer with the engine image, a window and an
  audio device should rerun alone and in the old layout. Everything else,
  including the process-global-state tests (env vars, current dir, Bevy `App`s,
  the f48_d re-exec), passed in one process.
- Compile time and wall time: the old layout was not rebuilt (249 fresh Bevy
  links with a ~1 min scan each is the cost this task removes). The single
  binary builds in ~1m40s incrementally and ~35-55 min on the first cold build
  on this busy host; test wall time is ~82 s once warm.
- `cargo test -p cs_xtask -- accept_test_bin_guard_`: 7 passed (including the
  new `#[path]` test), and `cs_xtask test-layout` now reports cs_app clean.
