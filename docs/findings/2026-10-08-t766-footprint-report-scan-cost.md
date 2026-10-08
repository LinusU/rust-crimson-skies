# Task #766: what `report-test-disk` waits on when the target directory is large and contended

Date: 2026-10-08. Task: #766 "cs_xtask accept_t696 footprint tests block >60 min
under shared-host load" (`T696-SLOW-ON-LOADED-HOST`,
`allowProtectedChanges: false`, so no protected path was touched). Capabilities
used: ordinary build/test. No `CS_GAME_DIR`, GPU, audio or network capability
was read or claimed, so no `acceptance.json` is produced
(`docs/contracts/CLI-EVIDENCE.md`). All local numbers are from this worktree on
macOS aarch64, rustc 1.98.1, `dev` profile (`line-tables-only`), and are stated
as local numbers.

## The incident, and the question

While `cargo test --workspace` ran on 2026-10-08 (agent bunny-1, task #752),
the two cs_xtask tests `accept_t696_the_report_command_prints_the_measured_footprint`
and `accept_t696_the_workspaces_test_binaries_are_measured_not_guessed` sat in
uninterruptible sleep (`ps` state `UN`) for over 60 minutes — 5-8 min CPU in
64 min elapsed — while sibling worktrees ran their own `cargo test --workspace`.
The process was `target/debug/cs_xtask report-test-disk` walking a 63 GB target
directory. The same suite with `-- --skip accept_t696` finished green.

The acceptance asks for three things: what the process waits on (walk, stat or
lock), a fix that bounds the tests or makes them explicit about contention, and
no weakening of what the suite pins about the measurement.

## What it waits on: the walk, billed per target — not a lock

**Not a lock.** Nothing in the path opens a file for writing or takes a lock:
`tools/cs_xtask/src/footprint.rs` and `transient.rs` call only `read_dir`,
`metadata` and `read_to_string` on manifests, the `tests/` directories, the
`deps` directory and cargo's `.d` sidecars. The only `lock` anywhere in
`cs_xtask` is `Cargo.lock`, read as text by `pins.rs`. The process never sees
cargo's own lock, and `report-test-disk` writes nothing. The `UN` state in the
incident is I/O wait on the volume, not contention on a lock.

**The walk, multiplied by the plan.** `measure` listed `debug/deps` again for
*every target in the plan*. The plan is 437 targets here and the directory held
7186 entries (1704 `.d` sidecars, of which 441 have a binary beside them — the
rest are `lib*.d` sidecars whose `.rlib` is not a plain file name). So one
report cost:

| call | per target | per report (437 targets) |
|---|---|---|
| `read_dir` of `deps` | 1 | 437 |
| directory entries visited | 7186 | 3 140 282 |
| `metadata` on the binary beside a sidecar | 1704 (+1 when it matched) | 745 086 |
| `read_to_string` of a `.d` sidecar | 441 | 192 717 |

Measured, not estimated: a temporary instrumented build (counters and `Instant`
phase timers around each call, reverted; it is not in the tree) reported, on one
run of `./target/debug/cs_xtask report-test-disk`:

```
PROBE total=6928.9ms walk=1631.2ms (dirs=437, entries=3140282) \
  read=2235.8ms (n=192717) stat=2041.4ms (n=745086) accounted=5908.4ms rest=1020.5ms
```

and `time` on the same run gave 1.88 s user, **5.11 s system**, 7.9 s wall: the
cost is syscalls, not bytes (the sidecars are 6.5 MB in total). The counts
reproduce arithmetically — 437 x 7186, 437 x 1704, 437 x 441 — so they are a
property of the algorithm, not of this disk.

**Under contention the same run degrades 2.9x, and the stat phase degrades
worst.** Six concurrent instances of the same binary (started with `&`, waited
in the same shell, nothing left running):

| | idle (1 run) | 6 concurrent, each |
|---|---|---|
| wall time | 6.9 s | **20.3 s** |
| walk (`read_dir` calls) | 1.63 s | 3.64 s |
| entry iteration (rest) | 1.02 s | 1.43 s |
| reads (192 717) | 2.24 s | 4.66 s (2.1x) |
| stats (745 086) | 2.04 s | **10.54 s (5.1x)** |

So the wait is spread over the three phases of the walk, with `metadata` the
most latency-sensitive — which is exactly what a loaded host does to a syscall
that must reach the filesystem. Extrapolating linearly from 6.9 s to the
incident's 64 min needs a further ~550x, which is what a 63 GB directory (more
entries per listing), several `cargo test --workspace` runs writing gigabytes
and evicting the page cache, and an `UN`-state disk supply; nothing in the path
grows super-linearly, because the growth is the multiplication in the table
above. The point of the fix is that the multiplier is removed entirely, so the
remaining cost is one directory's worth of calls, whatever the plan holds.

## The fix: one pass, and the pass prints its own bill

`footprint::measure` (one target, one listing) is replaced by `scan_deps`: the
`deps` directory is listed **once**, each entry costs one `metadata` (which
both answers "is it built" and is the measurement) and one sidecar read, and
the sidecar's *root* dependency is matched against the plan through a hash
index keyed by source path — one lookup per path component of the root instead
of one comparison per target. The measurement's semantics are unchanged: the
match is still on the `.d` sidecar's root dependency, the largest of a
source's binaries is still the one kept, a target with no binary here is still
*unknown* rather than zero.

What the pass cost is returned with what it found, as `DepsScan { listings,
entries, stats, dep_files, elapsed }`, and `report-test-disk` opens with it:

```
$ ./target/debug/cs_xtask report-test-disk
report-test-disk: scan: 1 listing of …/target/debug/deps: 7453 entries, 452 dep files read, 1756 binaries stat'ed, 19 ms
report-test-disk: crates/cs_app       171 planned,  171 measured, 19668551744 measured
…
```

Before/after on this worktree:

| | before | after |
|---|---|---|
| `report-test-disk` wall time, idle | 6.9 s | **0.023 s** |
| same, 6 concurrent | 20.3 s each | (not rerun; the calls it was waiting on are gone) |
| calls per report | 437 listings, 3 140 282 entries, 745 086 stats, 192 717 reads | **1 listing, 7453 entries, 1756 stats, 452 reads** |
| `cargo test -p cs_xtask --test accept_t696_test_disk_footprint` | 15.4 s (8.6 s of it inside one `measure_workspace`) | **0.22-1.25 s**, 9 passed |

The "after" column's per-listing figures come from a directory that had grown
since the "before" probe was taken (7186 → 7453 entries, 1704 → 1756 sidecars,
441 → 452 with a binary beside them: this tree had built more targets in
between). Both columns are measured on this worktree; what the change removes
is the 437x multiplier, not the directory's own size.

## What the suite now pins (and how it was checked)

Three `accept_t696_` tests hold the bound, on the count of filesystem calls
rather than on a timer — deliberately: a wall-clock assertion would fail on
exactly the loaded host these tests are meant to survive, and the counts fail on
the multiplication directly.

* `accept_t696_the_deps_directory_is_listed_once_for_the_whole_plan` (new): a
  fixture whose plan (11 targets) is deliberately larger than the directory it
  is measured against (9 entries) — `listings == 1`, `entries == 9`,
  `stats == 4`, `dep_files == 3` — and the two plan binaries still measured by
  their roots, the ghost sidecar and the binary-less sidecar still unknown.
* `accept_t696_the_workspaces_test_binaries_are_measured_not_guessed`: the same
  bounds asserted on the **real** target directory (`listings == 1`,
  `stats <= entries`, `dep_files <= entries`), where a per-target listing is
  437 x 1756 stats over 7453 entries and cannot pass.
* `accept_t696_the_report_command_prints_the_measured_footprint`: reads the
  bill back out of the printed `scan:` line (every count is parsed from the
  word that names it) and requires `listings == 1`, for the real directory,
  and `(1, 0, 0, 0)` for one that does not exist — which still costs one
  listing and says 0 entries, 0 dep files, 0 stats.

Mutation check, to confirm the tests fail when the implementation is removed: a
temporary mutant restored the listing-per-target loop (same results, calls
accumulated per target). Three tests failed —

```
accept_t696_the_deps_directory_is_listed_once_for_the_whole_plan: left: 11, right: 1
accept_t696_the_workspaces_test_binaries_are_measured_not_guessed: left: 437, right: 1
accept_t696_the_report_command_prints_the_measured_footprint: left: 437, right: 1
   report-test-disk: scan: 437 listing of …/deps: 3256961 entries, 197524 dep files read, 767372 binaries stat'ed, 0 ms
test result: FAILED. 6 passed; 3 failed … finished in 28.76s
```

— and the mutant's numbers are the arithmetic above (437 x 7453 entries,
437 x 1756 sidecars, 437 x 452 reads), which cross-checks both halves of this
finding. The mutant was reverted; the suite is green again.

## One trap for whoever runs the suite next

Before this worktree had built the current `main`, `cargo test -p cs_xtask
--test accept_t696_test_disk_footprint` failed at `a workspace-wide build must
measure every target in the plan` (one unmeasured target:
`crates/cs_net/tests/accept_f55_b_host_validation.rs`, a test file this
checkout had but this `target/` did not). That is the suite working as
designed — it demands a build of *this* commit — and `cargo build --tests -p
cs_net` (or a full `cargo test --workspace`) cleared it. It is not a regression
of this change, and the failure message names the missing target.

## Commands run

```
git fetch origin && git checkout --no-track -B rally/766-… origin/main
cargo build --locked -p cs_xtask                                   # 0 (twice: probe, then real)
./target/debug/cs_xtask report-test-disk                           # before: 6.9 s wall, PROBE line above
( x6 ./target/debug/cs_xtask report-test-disk ) & wait             # contention: 20.3 s each, nothing left running
cargo test --locked -p cs_xtask --test accept_t696_test_disk_footprint   # before 15.4 s (1 failed: stale target dir)
cargo build --tests --locked -p cs_net                             # 0 — built the missing test binary
cargo test --locked -p cs_xtask --test accept_t696_test_disk_footprint   # after: 9 passed, 0.33 s
cargo test --locked -p cs_xtask --test accept_t696_test_disk_footprint   # mutant: 3 failed, 28.76 s
cargo fmt -p cs_xtask && cargo fmt --all -- --check                # 0
cargo clippy -p cs_xtask --all-targets --all-features --locked -- -D warnings   # 0
cargo test --workspace --locked                                    # see the handover for the exit codes
```

The instrumented probe used for the "before" numbers was written into
`footprint.rs`, run, read and reverted with `git checkout` before the fix was
written; it is not in the tree.

## Related findings

* `docs/findings/2026-10-06-t696-ci-runner-disk-budget.md` — the measurement
  this report exists for, and the design points the suite pins.
* `docs/findings/2026-10-04-f54-x7-missing-test-harness-binary.md` — a
  different way a test run says nothing; kept distinct from this one, which
  ran and waited.
