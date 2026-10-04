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

> **Correction (task #620).** This finding originally attributed mechanism 1
> solely to a second cargo. Task #617
> (`2026-10-04-t617-cargo-test-enoent-is-external.md`, measurement in #613/#617)
> captured the owner's `prune-stale-bins.sh --delete …/target/debug/deps`
> unlinking harness executables with no second cargo present. Both writers
> produce the same `os error 2`, and this task's reproductions only exercise
> the cargo one. `test = false` removes the two empty harnesses from the plan;
> it does not stop the pruner and does not prevent the flake for crates that
> have real unit tests.

Three mechanisms; mechanisms 2 and 3 come from a second cargo writing the same
target dir, mechanism 1 from either that or the external prune above:

1. **Empty harness binaries.** `src/lib.rs` and `src/main.rs` carry zero
   `#[test]`s, yet cargo still builds and execs a unit-test harness for each
   in the default `cargo test` plan. A concurrent rebuild (or a prune of old
   executables) replaces or removes `target/debug/deps/cs_xtask-<hash>` between the first run's discovery and
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
  `PATIENT` (60 × 50 ms) spans a remove-and-relink window for a file the
  caller requires to exist; `SCAN` (4 attempts, no sleep) is for an entry
  inside a walk, where absence is a normal answer. Every other error kind is
  returned on first sight; a file that stays absent keeps its `NotFound` — the
  retry buys the writer's window, it never invents the file. `command_output`
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
  consumes them; the walk still records its one profile through a target
  directory full of non-profile directories; the walk budget never sleeps;
  and the `test = false` manifest configuration plus the no-`#[test]`
  invariant in `src/lib.rs`/`src/main.rs` are pinned.

## A sleeping walk budget does not scale: review correction

The first version of this work gave `SCAN` five 20 ms retries, reasoning that
a walk entry should be skipped "quickly". That reasoning was wrong about the
price, because the walks that use `SCAN` are priced per *entry*, not per
call. `target_dir::dep_info_files` probes `<entry>/deps` under **every**
directory a target directory holds, and a checkout that has run its test
suites has thousands of them: measured in this workspace's `bunny-alpha-2`
checkout after the `tests/accept_*` pid-keyed fixture roots landed, `target/`
held **3194 top-level directories, of which exactly one (`debug`) had a
`deps/` child**. One `dep_info_files` scan therefore took 3193 absent probes
x 100 ms = ~319 s of `nanosleep`, and `accept_t433_this_worktrees_target_dir_holds_no_removed_worktree`
— which scans three times — did not finish in 25 minutes (`sample` showed the
thread parked in `std::thread::sleep` inside `transient::retry` called from
`transient::read_dir`). `cargo test -p cs_xtask` was unusable in that state.

`SCAN` now retries **without sleeping** (4 immediate attempts). An atomic
rename never produces a `NotFound` at all, so the only window a listing probe
can hit is a remove-then-recreate, and for a directory listing that is rare
and short: a few extra `open`/`stat` calls ride it out, and a genuinely
absent entry stays absent for microseconds. `PATIENT` keeps the sleep for the
one case where patience is right — a file the caller *requires*, whose writer
removes it and rebuilds it at the end of a compile. After the correction the
same suite runs in 6.6 s and the whole `cs_xtask` package (81 tests) in 42 s.
`accept_t610_the_walk_policy_never_sleeps` pins the split, and
`accept_t610_a_walk_over_non_profile_directories_still_records_its_profile`
pins the walk's answer through the same junk.

The same correction reaches `DirEntry::file_type`: `dep_info_files` classified
each listing entry with one unretried `file_type()`, and an entry that vanished
mid-walk classified as "not a directory" — which skips a whole profile
directory and reads none of its dep-info, i.e. a *false pass* on the #433
gate. It now goes through `transient::file_type(entry, transient::SCAN)`,
which keeps the same answer and retries the classification.

## Checks

- `cargo test -p cs_xtask --locked --test accept_t610_transient_target_dir`
  — 7/7 pass (5 from the implementation, 2 added by review).
- `cargo test -p cs_xtask --locked --test accept_t433_stale_target_dir` run
  in two overlapping 6-iteration loops — 12/12 invocations pass (was 1/12).
- `cargo test -p cs_xtask --locked` — 81/81 pass in 42 s.
- `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets
  --all-features --locked -- -D warnings`, `cargo test --workspace
  --locked` — results recorded in the Rally handover.

## Known limits

- Probes whose expected answer may be "absent" (`removed_manifest_dirs`,
  `foreign_manifest_dirs_with_home`, the `<entry>/deps` probe) use `SCAN`,
  which buys immediacy rather than patience: a transient removal inside such a
  walk longer than a few syscalls is answered as absence, which for
  `removed_manifest_dirs` means the walk can under-report a removed worktree
  while a cargo is mid-relink. That is the deliberate trade — the alternative
  is a sleep priced once per directory in `target/`, measured above at ~319 s
  per scan. Probes for files that must exist (workspace and member manifests,
  the `--workspace-root` argument) keep `PATIENT`, so a genuinely missing file
  is reported ~3 s late rather than instantly.
- `corpus.rs` still reads the source tree, the committed fixtures and the
  private install with unretried `fs::` calls. That is deliberate: none of
  those paths is written by a concurrent cargo, and a retry there would be
  paid once per file of an install-sized tree.
- The fixture roots remain under the shared `target/`; a process is now
  protected only from *other processes'* deletion, and stale `<pid>` trees
  accumulate until `cargo clean` reaps them, same as before. This is also why
  `target/` holds thousands of directories, which is what priced the walk
  budget above.
