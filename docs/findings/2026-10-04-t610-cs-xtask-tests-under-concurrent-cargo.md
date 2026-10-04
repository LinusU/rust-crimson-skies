# Task #610: `cs_xtask` checks flake under a concurrent cargo writing the target dir

Date: 2026-10-04. Task: #610 "cs_xtask T430 profile checks flake with 'No such
file or directory' when cargo is writing the target dir" (CS-XTASK-FLAKE).
Capabilities used: ordinary build/test only — no `CS_GAME_DIR`, GPU, audio or
network capability was read or claimed, so no `acceptance.json` is produced
(`docs/contracts/CLI-EVIDENCE.md`, evidence-bound tasks only). Commands were
run from the workspace root
(`/Users/linus/coding/rust-crimson-skies/swe2-max-1`, macOS aarch64,
rustc 1.98.x) with `CARGO_TARGET_DIR` pointing at the checkout's `target/`.

## The reported failure

`cargo test --workspace` intermittently died with

```
error: test failed, to rerun pass `cs_xtask --lib`
No such file or directory (os error 2)
```

while a second `cargo test --workspace` (another agent or a local run)
overlapped it on the same target directory.

## Reproduction and before/after counts

Two reproduction commands, one per mechanism.

**1. Concurrent test runs on one target dir (the reported scenario).**

```sh
# two shells (or one script), each looped 6 times:
for i in 1 2 3 4 5 6; do
  cargo test -p cs_xtask --locked --test accept_t433_stale_target_dir
done
```

with the second loop started while the first was still running. Before the
fix: **11 of 12 suite invocations failed** (5 of 6 on one side, 6 of 6 on the
other). The failure signatures were exactly the reported ones —
`error: could not execute process target/.../probe (never executed) /
No such file or directory (os error 2)` — plus linker failures where one
run's `remove_dir_all` deleted object files the other run's `rustc` needed.
Both runs share `target/t433-target-dir-fixtures`, so each `remove_dir_all`
in one process deleted the fixture tree the other process was mid-build on.

After the fix: **0 of 12 suite invocations failed** (same command, same
machine — see below).

**2. Exec of a binary a concurrent cargo is relinking.**

```sh
# shell A: rebuild the unit-test harnesses in a loop
while :; do cargo test -p cs_xtask --lib --bin cs_xtask --no-run; done
# shell B: run them in a loop
for i in $(seq 30); do cargo test -p cs_xtask --lib --bin cs_xtask; done
```

Before the fix the victim loop intermittently lost the binary it was about
to exec — on Linux that surfaces as `No such file or directory (os error
2)` (the reported signature), on this macOS box the already-running binary
was SIGKILLed instead (its adhoc code signature invalidates mid-relink).
After the fix `cargo test -p cs_xtask` schedules **no** `--lib`/`--bin`
harnesses at all, so there is no binary to lose: the `test` build plan for
`cs_xtask` lists only the twelve `tests/` integration suites.

## Root cause

Three mechanisms, all from a second cargo writing the same target dir:

1. **Empty harness binaries.** `src/lib.rs` and `src/main.rs` carry zero
   `#[test]`s, yet cargo still builds and execs a unit-test harness for each
   in the default `cargo test` plan. A concurrent rebuild replaces
   `target/debug/deps/cs_xtask-<hash>` between the first run's discovery and
   exec — the reported `os error 2`.
2. **Transient `NotFound` reads.** Every gate read a file once
   (`fs::read_to_string` on manifests, `fs::read`/`fs::read_dir` on dep-info
   under `target/`, `Path::is_file`/`is_dir`). A writer that replaces a file
   leaves a remove-and-recreate window in which the path names nothing.
3. **Shared fixture roots.** The `accept_*` suites created fixtures at fixed
   paths like `target/t433-target-dir-fixtures`; two overlapping runs
   `remove_dir_all`'d each other's trees mid-build. (This is also why the
   measured repro above is the t433 suite: it is the heaviest
   fixture-writer.)

## What changed

- `tools/cs_xtask/Cargo.toml`: `test = false` on `[lib]` and the
  `[[bin]] cs_xtask` target. The empty harnesses leave the default test
  plan; the integration suites in `tests/` are unaffected. Explicit
  `cargo test -p cs_xtask --lib` still builds a zero-test harness (cargo's
  documented behavior for explicit target selection), but `cargo test
  --workspace` — the reported failure path — no longer schedules or execs
  them.
- `tools/cs_xtask/src/transient.rs` (new): retries an operation that fails
  with `ErrorKind::NotFound` — and only that kind — under a `Policy`.
  `PATIENT` (60 × 50 ms) spans a remove-and-relink window for files that
  must exist; `SCAN` (5 × 20 ms) covers a rename pair inside a listing walk
  where absence is a normal answer. Every other error kind is returned on
  first sight; a file that stays absent keeps its `NotFound` — the retry
  buys the writer's window, it never invents the file. `command_output`
  applies the same retry to `Command::output` for binaries the tests exec
  out of `target/`.
- `budget.rs`, `ci.rs`, `pins.rs`, `package.rs`, `bootstrap.rs`,
  `target_dir.rs`, `main.rs`: manifest/workflow/dep-info reads and
  existence probes routed through `transient` (`PATIENT` where the file
  must exist, `SCAN` inside directory walks). The T430 assertions are
  unchanged — `verify_manifest` still takes the manifest text it is given
  and rejects full DWARF the same way; only the read that fetches that
  text retries a transient absence.
- `tests/accept_*.rs`: every suite's fixture root gains a
  `std::process::id()` component (`target/<suite>-fixtures/<pid>`), so two
  `cargo test` processes on the target dir cannot delete each other's
  trees; per-test names inside each pid root keep parallel threads apart
  exactly as before. Spawns of `env!("CARGO_BIN_EXE_cs_xtask")` and
  `current_exe()` — binaries that live in `target/` and can be mid-relink —
  go through `transient::command_output`. Spawns of `cargo`/`git` (toolchain
  paths outside the target dir) were left alone.
- `tests/accept_t610_transient_target_dir.rs` (new, task prefix
  `accept_t610_`): a read racing a real concurrent remove-and-rename writer
  must never surface `NotFound`; a permanently absent file still errors;
  `command_output` execs a stable binary and reports a missing one; the
  `read_dir`/`is_dir`/`is_file` probes behave the way the dep-info walk
  consumes them; and the `test = false` manifest configuration plus the
  no-`#[test]` invariant in `src/lib.rs`/`src/main.rs` are pinned.

## Checks

- `cargo test -p cs_xtask --locked --test accept_t610_transient_target_dir`
  — 5/5 pass.
- `cargo test -p cs_xtask --locked --test accept_t433_stale_target_dir` run
  in two overlapping 6-iteration loops — 12/12 invocations pass (was 1/12).
- `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets
  --all-features --locked -- -D warnings`, `cargo test --workspace
  --locked` — results recorded in the Rally handover.

## Known limits

- Probes whose expected answer may be "absent" (`removed_manifest_dirs`,
  `foreign_manifest_dirs_with_home`) use `SCAN`, so a missing directory
  costs ~100 ms instead of `PATIENT`'s 3 s — the rename-pair window a walk
  realistically has to span, not the whole link. Probes for files that must
  exist (workspace and member manifests, the `--workspace-root` argument)
  keep `PATIENT`.
- The fixture roots remain under the shared `target/`; a process is now
  protected only from *other processes'* deletion, and stale `<pid>` trees
  accumulate until `cargo clean` reaps them, same as before.
