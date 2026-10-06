# Task #696: the CI runner's disk after the toolchain cleanup, and what one more test file costs

Date: 2026-10-06. Task: #696 "CI runner disk is exhausted by the workspace test step"
(`CI-DISK-BUDGET`, `allowProtectedChanges: false`, so `.github/` was not touched).
Capabilities used: ordinary build/test and `gh` read access to CI logs. No
`CS_GAME_DIR`, GPU, audio or network capability was read or claimed, so no
`acceptance.json` is produced (`docs/contracts/CLI-EVIDENCE.md`). Local
measurements are from this workspace on macOS aarch64, rustc 1.98.1, `dev`
profile (full DWARF), which is stated wherever a local number is given, because
CI links with `line-tables-only` and the two are not the same byte count. CI
numbers come from the runs of `LinusU/rust-crimson-skies` quoted below, read
with `gh run view <id> --log`.

## The measurement the task asked for

The task's acceptance is `Disk after tests` on main showing a comfortable
margin, with the before/after numbers recorded. Both are in the logs:

| run | main commit | when (UTC) | free after `Free runner disk space` | free after `cargo test` |
|---|---|---|---|---|
| 37386273061 | `73f84b1c` | 2026-10-05 23:03 | 105 GB | **450 MB** (100% used) |
| 37394319899 | `385160c2` | 2026-10-06 00:29 | 124 GB | **20 GB** (87% used) |

Both `df` lines are verbatim from the logs (the first run's two steps are
unnamed in its own log, so only the numbers are quoted for it):

```
37386273061  rust  <unnamed>  /dev/root  145G   41G  105G  28% /   (before cargo test)
37386273061  rust  <unnamed>  /dev/root  145G  144G  450M 100% /   (after cargo test)
37394319899  rust  Free runner disk space  /dev/root  145G   21G  124G  15% /
37394319899  rust  Disk after tests        /dev/root  145G  125G   20G  87% /
```

`385160c2` ("Free the runner's remaining unused toolchains before the CI
build", pushed by the owner) is what moved it: it extended the cleanup step to
`/opt/hostedtoolcache`, `~/.ghcup`, boost, swift, powershell, chromium,
node_modules, `/opt/microsoft`, `/opt/google`, `/opt/az`, the JVMs and
miniconda, and pruned docker images. Its own `df` pair shows 87 GB free before
the cleanup and 124 GB after, so the extended removal is 37 GB — and 124 GB
minus the 105 GB the previous commit's cleanup left is the whole 19 GB
difference in the outcome. The job writes about **104 GB** in both runs:
`73f84b1c` had 105 GB to spend and finished with 450 MB, `385160c2` had 124 GB
and finished with 20 GB.

So the acceptance criterion is met with 20 GB free, an order of magnitude more
than the 450 MB that made #666 fail, and the run is green (`37394319899`
conclusion `success` on both jobs). The `pack` job, which has no Rust build, is
unaffected.

## What the ~104 GB is made of

Not the restored cache. `Swatinem/rust-cache` reported on run 37394319899:

```
Cache hit for: v0-rust-rust-Linux-x64-7804be49-84f1c5c6
Cache Size: ~1082 MB (1135019483 B)
```

1.08 GB, a full key match. The task asked whether stale artifacts inflate
`target/`; they do not — there is almost nothing cached to inflate it, and the
workflow sets none of the action's cache-scope options, so that ~1 GB is the
whole cache. The write is the build.

The build is dominated by the test binaries. The plan is countable from the
tree: **376 test binaries** before this task's own suite (365 integration test
files plus 11 enabled unit test harnesses; `tools/cs_xtask` sets
`test = false` on both of its targets per task #610, so they are out). Measured
locally with `cs_xtask report-test-disk` (this task's tool, see below), against
this workspace's own `target/`:

| member | planned | measured bytes |
|---|---|---|
| `crates/cs_app` | 138 | 15.74 GB |
| `crates/cs_content` | 69 | 271 MB |
| `crates/cs_sim` | 49 | 95 MB |
| `crates/cs_formats` | 27 | 52 MB |
| `crates/cs_assets` | 27 | 582 MB |
| `tools/cs_inspect` | 25 | 70 MB |
| `tools/cs_xtask` | 14 | 21 MB |
| `crates/cs_script` | 12 | 27 MB |
| `crates/cs_net` | 10 | 20 MB |
| `crates/cs_types` | 6 | 8 MB |
| **total** | **377** | **16.89 GB** |

(The total here includes the new `accept_t696` suite of this task; the 376 in
the plan count above is the tree before it. Local, `dev` profile, full DWARF:
CI's `line-tables-only` binaries are smaller, so **16.89 GB is a local figure,
not the CI write**, and the CI write is larger because it also builds every
dependency rlib, which is most of the rest.)

**138 of the 377 are engine-linked**, and the split is unambiguous on the
measured tree: the engine-linked binaries are 104.8 MB to 233.3 MB, everything
else is 1.0 MB to 20.8 MB (the largest of the small group is `cs_content`'s own
unit-test harness), and nothing lands between 20.8 MB and 104.8 MB. So:

* **one more `cs_app` test file costs about 107 MB** locally (the median of the
  engine-linked group; the largest is `crates/cs_app/tests/world/main.rs` at
  233 MB). That is the number that decides whether the next file fits, and with
  20 GB of CI headroom it is roughly 190 more files before the runner is full.
* `crates/cs_app` alone is 138 of the 377 binaries and 15.7 GB of the 16.9 GB
  measured. It is the member that decides this budget.

`cs_xtask report-test-disk` prints exactly this, per member, from a real build —
see "The measurement is a tool" below. Two limits are stated in its output
rather than papered over: a target with no binary in the target directory is
reported as *unmeasured*, never as zero bytes; and doc-test binaries are not
counted at all (rustdoc links one per doc code block), so the total is a floor
for the tree, not a bound on what `cargo test` writes.

## Why the runner is the tight one and this checkout is not

The local `target/` for the whole workspace is 22 GB. The CI job writes ~104 GB
for the same tree. The difference is not the test binaries alone: CI builds every
dependency from scratch each run (the cache holds ~1 GB), and a Bevy + Avian +
wgpu graph with line tables is tens of gigabytes of rlibs before a single test
binary is linked. That part is `.github/`'s to configure (bigger runner, longer
lived cache, split job), not the workspace's.

The workspace's own lever is the count and the size of the test binaries, which
is what `report-test-disk` measures. Two levers are available in owner paths and
neither is taken here:

1. `CARGO_PROFILE_TEST_DEBUG` is already `line-tables-only` in the workflow, and
   `[profile.dev] debug = "line-tables-only"` is already in the workspace
   manifest and gated by `cs_xtask verify-ci-budget` (task #430). Nothing there
   is full DWARF.
2. Consolidating `cs_app`'s 138 test files into fewer binaries would cut both
   count and total. It is a large, invasive refactor of 138 files that belongs
   in its own task, not as a side effect of a measurement task, and at 20 GB of
   headroom it is not needed now.

## The measurement is a tool, not a number in a document

`tools/cs_xtask/src/footprint.rs`, reached as `cs_xtask report-test-disk`,
prints the plan and the measurements above:

```
$ cargo run -q -p cs_xtask -- report-test-disk
report-test-disk: crates/cs_app       138 planned,  138 measured,  15742281488 measured
…
report-test-disk: 377 test binaries in the plan (366 integration, 11 unit harnesses); 377 measured here
report-test-disk: 138 engine-linked (>= 50000000 bytes) and 239 smaller; 16889287368 measured in total (the largest binary of each source; a bin target cargo links twice is counted once)
report-test-disk: one more engine-linked test file costs about 107113240 bytes (median); the largest is crates/cs_app/tests/world/main.rs at 233258952 bytes
report-test-disk: doc-test binaries are not counted (rustdoc links one per doc code block), so the measured total is a floor
```

Design points that are load-bearing, each pinned by an `accept_t696_` test in
`tools/cs_xtask/tests/accept_t696_test_disk_footprint.rs`:

* The plan is read from the manifests and the filesystem, honouring `test = false`
  on `[lib]` / `[[bin]` the way `tools/cs_xtask/Cargo.toml` uses it. Both test
  layouts count: `tests/<name>.rs` and `tests/<name>/main.rs`.
* A measurement is taken from the binary whose cargo `.d` sidecar names that
  target's *root* source file, so two members with equally named test files
  (`crates/cs_app`, `crates/cs_content`, `crates/cs_formats`, `crates/cs_sim` all
  have `accept_doclib_conflict.rs`) stay apart. The *root* dependency is what
  identifies a target: matching any dependency measures `tools/cs_inspect`'s lib
  harness (14.9 MB, whose rule line also names `src/main.rs`) when asked for the
  bin's.
* A `src/main.rs` bin target is linked twice by `cargo test` — the plain binary
  an integration test can exec, and its own harness — so the measurement reports
  the binary count as well as the larger size. For `cs_app` that is 228 MB and
  104 MB from one source, not one 228 MB binary.
* A target with no binary measures as *unknown*. With nothing measured the tool
  prints that the marginal cost is "unknown rather than zero" and exits 0: not
  built here is not a finding about the workspace, and the report is not a gate.

## Cross-checks run over this finding (review pass, 2026-10-06)

Everything above was re-derived from the same commits by someone reading the
logs and the tool's output rather than the numbers above (the review pass ran
under the same agent identity as the implementation, so this is a second pass,
not an independent one).

* The two acceptance numbers, read again from the runs: 450 MB after tests on
  `73f84b1c` (run 37386273061) and 20 GB on `385160c2` (run 37394319899, green),
  plus 6.8 GB / 6.1 GB / 3.4 GB / 1.9 GB for the four earlier sampled runs. All
  reproduce exactly.
* The plan was checked against **cargo's own view** rather than against the
  tool: `cargo metadata --no-deps --locked` reports **366** targets of kind
  `test` across the ten members, which is exactly the plan's 366 integration
  targets; adding the 11 enabled unit-test harnesses gives the 377 the tool
  prints. Per member it agrees everywhere (`cs_app` 136+2, `cs_content` 68+1,
  `cs_formats` 26+1, `cs_net` 9+1, `cs_script` 11+1, `cs_sim` 48+1,
  `cs_types` 5+1, `cs_inspect` 23+2, `cs_xtask` 14+0).
* The size split was re-measured with an independent script that reads cargo's
  `.d` sidecars itself and does not call the tool. It attributes **379** sources
  where the plan holds 377: the two extra ones are `tools/cs_xtask/src/main.rs`
  (the `cs_xtask` binary itself, which `test = false` keeps out of the plan) and
  `crates/cs_app/tests/accept_f29_c_propulsion_gate.rs`, whose `.d` and binary are
  still in the local target directory although the test file is gone — a stale
  artifact for a deleted file is not in the plan, which is right, because
  `cargo test` links nothing for it. With those two in, the script measures
  104.8 MB–233.3 MB engine-linked, 1.0 MB–20.8 MB small, median 107,113,240 and
  17.02 GB in total, against the tool's 138/239, median 107,113,240 and 16.89 GB:
  the same split, the extra 0.13 GB being the stale 129.5 MB `cs_app` binary and
  the `cs_xtask` binary itself.
* Every `src/lib.rs` in the tree has exactly **one** binary here whose `.d`
  sidecar names it, and each of the two `src/main.rs` bins has two (cs_app
  104.7 MB and 228.7 MB, cs_inspect 1.0 MB and 13.4 MB). A doc-test binary
  leaves no such sidecar, so the report's "doc-test binaries are not counted"
  and the "linked twice" note both hold as stated on this tree.
* Two `accept_t696_` tests failed under a member-scoped
  `cargo test -p cs_xtask` in a fresh per-worktree target directory (task
  #383), because they demanded a workspace-wide build's coverage and its
  marginal cost. Both now say what holds under either invocation: whatever the
  directory holds is measured and non-zero, a target it holds nothing for stays
  unknown, full coverage is asserted when and only when every member has a
  binary, and the unknown-marginal-cost path is now asserted against a
  directory that provably holds nothing, which is deterministic. Both mutants
  were confirmed to fail the fixed suite.

## What is left to the owner

`.github/` stays protected and untouched by this task. If the margin keeps
shrinking — the five main runs sampled on 2026-10-05 went 6.8 GB (08:32Z), 6.1 GB
(17:14Z), 3.4 GB (19:00Z), 1.9 GB (21:20Z), 450 MB (23:03Z) — the options that
are *not* the workspace's to take, in the order I would measure them:

1. A larger runner or a split job, so the dependency rlibs and the test binaries
   are not written to the same filesystem in one process's lifetime.
2. Failing loudly instead of mysteriously: make `Disk after tests` exit non-zero
   below a threshold, so the next exhaustion is a named disk-budget failure
   rather than a `No space left on device` in whatever link happens to be
   running. Task #637 already recommended this; it is one line in the same step
   that prints the `df`.
3. `CARGO_PROFILE_TEST_DEBUG: 0` instead of `line-tables-only` for the test
   profile. That drops line tables too, so a panic's `file:line` goes away. Not
   proposed: it trades diagnosability for a few GB this task has not measured a
   need for.

## Commands run

```
git fetch origin && git checkout --no-track -B rally/696-ci-runner-disk-is-exhausted-by-the-works origin/main
gh run list --branch main --limit 10 --json ...
gh api repos/LinusU/rust-crimson-skies/actions/runs/37394319899/jobs --jq ...
gh run view 37394319899 --log          # df lines, cache size, test-plan count
gh run view 37386273061 --log          # df lines, cache size
cargo fmt -p cs_xtask
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --locked
cargo test --workspace --locked -- accept_t696_ --include-ignored
```

The review pass added, on top of the four checks:

```
gh run view <run id> --log | grep '/dev/root 145G'   # the acceptance numbers, re-read
cargo metadata --no-deps --locked --format-version 1 # cargo's own test-target list
cargo run -q --locked -p cs_xtask -- report-test-disk # the report's output
python3 <independent .d-sidecar measurement>          # the size split, without the tool
CARGO_TARGET_DIR=target/review-t696-partial \
  cargo test -p cs_xtask --test accept_t696_test_disk_footprint --locked
```

Exit codes are in the handover and the review notes on task #696.

## Related findings

* `docs/findings/2026-09-30-t430-rust-lld-sigbus-in-ci.md` — the same disk
  exhaustion seen as a linker SIGBUS; the `debug = "line-tables-only"` fix.
* `docs/findings/2026-10-05-t637-ci-doctest-linker-sigbus-was-disk-exhaustion.md`
  — the same exhaustion measured across runs, with the shrinking margin this
  task's numbers continue.
* `docs/findings/2026-10-04-t617-cargo-test-enoent-is-external.md` — the
  unrelated `cargo test` exec failure, kept distinct.