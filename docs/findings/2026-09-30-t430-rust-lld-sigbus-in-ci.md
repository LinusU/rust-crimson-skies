# Task #430: the `rust` CI job died at the `cs_app` doctest link, and the disk was the reason

Date: 2026-09-30. Task: #430 "Fix intermittent rust-lld SIGBUS crash at the
cs_app doctest link in CI" (`allowProtectedChanges: false`, so `.github/` was
not touched). Capabilities used: ordinary build/test only — no `CS_GAME_DIR`,
GPU, audio or network capability was read or claimed, so no `acceptance.json`
is produced (`docs/contracts/CLI-EVIDENCE.md`, evidence-bound tasks only).
Commands were run from the workspace root
(`/Users/linus/coding/rust-crimson-skies/bunny-alpha-1`, macOS aarch64,
rustc 1.98.1); the CI numbers come from the runs of
`LinusU/rust-crimson-skies` quoted below.

## The failure

Every failing run died at the same step, `cargo test`, exit 101, with the
same line:

```
error: linking with `cc` failed: exit status: 1
  = note: "cc" "-m64" … "<2 object files omitted>" … -Wl,-Bstatic
      target/debug/deps/{libcs_app-…,libavian3d-…,libbevy_pbr-…, … 400 rlibs …}.rlib
      … -B<sysroot>/lib/rustlib/x86_64-unknown-linux-gnu/bin/gcc-ld -fuse-ld=lld
      … -o /tmp/rustdoctestDp2Vgw/rust_out -Wl,--gc-sections -pie …
  = note: collect2: fatal error: ld terminated with signal 7 [Bus error], core dumped
error: doctest failed, to rerun pass `-p cs_app --doc`
```

| run | branch | when | outcome |
|---|---|---|---|
| 36728255759 | rally/417 | 2026-09-30 14:55 | `cargo test` failed at this link |
| 36729946195 | rally/420 | 2026-09-30 14:33 and 14:55 | `cargo test` failed at this link |
| 36725414616 | rally/420 | 2026-09-30 14:08 and 14:21 | `cargo test` failed at this link |
| 36723571849 | rally/417 | 2026-09-30 14:0x | `cargo test` failed at this link |
| 36728100091 | main | 2026-09-30 14:18 | passed, same restored cache |
| 36722210693 | rally/420 | 2026-09-30 13:31 | passed, same tree |

That it is the *same* tree, the *same* cache key and the *same* step on
runners that both pass and fail is what a resource limit looks like, not a
content problem: the diff is not in the doctest.

## What was measured, from inside the job

The workflow is owner-maintained, so it was not edited to print anything.
Instead a temporary test was pushed on the task branch that reads the
runner's own state and fails, so the numbers reach the log
(`cargo test` swallows the stdout of a passing test). Run **36736183776**,
`rust` job, `ImageOS=ubuntu24`:

```
CARGO_TARGET_DIR=target
GITHUB_WORKSPACE=/home/runner/work/rust-crimson-skies/rust-crimson-skies
TMPDIR=Err(NotPresent)   RUNNER_TEMP=/home/runner/work/_temp
CARGO_INCREMENTAL=Ok("0")  RUSTFLAGS=Err(NotPresent)
nproc=4
Filesystem     1K-blocks      Used Available Use% Mounted on
/dev/root      151263856 149241340   2006132  99% /
MemTotal: 16373452 kB / MemAvailable: 15375080 kB
```

* The root filesystem held **1.91 GiB free, 99% used** while `cargo test`
  ran. The doctest binary is linked into `/tmp` (`TMPDIR` is unset, and
  `-o /tmp/rustdoctestDp2Vgw/rust_out` in the failure confirms it), which is
  on that same `/dev/root`.
* Memory was not the constraint: 15.4 GB available out of 16.4 GB. A memory
  shortage would have killed the process, not raised `SIGBUS`.
* `CARGO_INCREMENTAL=0` is already exported by the runner, so incremental
  artifacts are not this task's lever (and the fix does not set it).

`SIGBUS` on Linux is a fault on a mapped region the file no longer backs. The
linker rustc invokes here is rust-lld (`cc … -fuse-ld=lld` in the failing
log), and lld writes its output through a memory-mapped buffer it has
already `ftruncate`d to the final size: when the filesystem cannot back
those pages the store faults, the kernel reports `Bus error`, `collect2`
prints exactly the line above, and `cargo test` aborts at 101. That is the
signature of running out of disk during the largest write of the job, and the
measured 1.91 GiB is what was left for it. In run 36728255759 the doctest
harness reported the failure after 3.03 s with "all doctests ran in 6.62s;
merged doctests compilation took 3.59s" — the link did not run to
completion, it died while the output file was being filled.

**Reported, not reproduced:** the task report names
`llvm::parallelFor` inside rust-lld as the faulting frame. The CI log carries
only the `collect2` line above (no core dump is uploaded), so the frame is
taken from the report, not measured here. It does not change the
explanation: every mapped page lld touches in that phase is either an input
rlib that is already on disk or the output file it cannot extend.

**Unknown, recorded as unknown:** what fills the other ~142 GB of the runner
disk. It is not this repository's build tree — the build tree is measured
below at a few GB — so it belongs to the image and the host, and nothing in
this repository can change it. That is why the fix below buys headroom
instead of reclaiming the disk, and why the owner's own options
(§"Left to the owner") are still worth considering.

## Why full DWARF is what spends the budget

CI exports no `CARGO_PROFILE_*` override, so the profile it builds under is
rustc's default: `debug = 2`, full type, variable and name DWARF for every
one of the ~400 objects in the Bevy/Avian graph. A dependency rlib from that
build, `libbevy_math` (Bevy 0.19.1, 1.69 MB of `__text`), by section:

| section | full DWARF | `"line-tables-only"` |
|---|---|---|
| `__debug_str` (names) | 10.25 MB | 8.53 MB |
| `__debug_info` (type/variable DIEs) | 8.14 MB | 2.32 MB |
| `__debug_loc` (expression locations) | 5.05 MB | — |
| `__debug_ranges` | 1.55 MB | 1.22 MB |
| `__debug_line` (the line program) | 1.11 MB | 1.04 MB |
| `__apple_types`/`__apple_names` | 2.65 MB | 2.01 MB |
| `__text` (the code) | 1.69 MB | 1.69 MB |
| **rlib on disk** | **52,750,600 B** | **38,114,840 B** (−27.7%) |

The same A/B for a workspace crate (`libcs_formats`, clean
`CARGO_TARGET_DIR` per case): 17,044,456 B with `debug = 2`,
12,115,056 B with `"line-tables-only"`, 8,327,248 B with `debug = false`.

So `"line-tables-only"` is a 28% cut of the object bytes, not the 90% a
first reading of the section table suggests: the line program and much of
`__debug_str` survive, because the line table still names files and the
surviving DIEs still reference names. What it drops is `__debug_loc`, the
bulk of `__debug_info` and `__apple_types` — exactly the part only a
debugger's expression evaluation needs, and exactly the part a
`cargo test` run never reads. `debug = false` would cut a further 31% but
also throw away the line program, so a panic in a failing test would print
an address instead of `file:line`. That trade is the owner's to make; the
change below takes only the part that costs no debuggability.

## The change

`.github/` is protected, and the owner decides what the workflow does, so the
part of the job this repository owns is the profile it builds under. One
line in the workspace `Cargo.toml`:

```toml
[profile.dev]
debug = "line-tables-only"
```

`test` and `bench` inherit `dev`, so this covers the profile the failing
link ran under, and it applies to `cargo clippy` and `cargo test` alike. The
effect is on both sides of the failure: the build tree that has to fit on the
runner shrinks, and the linked doctest binary — the last and largest write of
the job — shrinks with it.

`tools/cs_xtask/src/budget.rs` is the gate that keeps it there, exposed as
`cs_xtask verify-ci-budget` and covered by
`tools/cs_xtask/tests/accept_t430_ci_disk_budget.rs`:

* `[profile.dev]` must state `debug` explicitly — not stating it *is*
  `debug = 2`, which is the state CI crashed in;
* `debug` in `profile.dev`, `profile.test`, `profile.bench` and their
  `package."*"` / `package.<name>` tables must not be a full-DWARF level
  (`2`, `"full"`), because those overrides would undo the setting for exactly
  the crates CI links.

The gate follows `crates/cs_app/tests/accept_t336_eh_frame_unwind_table.rs`,
which guards `[profile.dev.package."*"] opt-level = 3` the same way: a
profile setting that keeps a CI job alive has to be checked by something that
runs in `cargo test --workspace`, because the workflow that would otherwise
notice is the one that cannot be edited from here.

## Measured effect

Same machine, same command sequence (`cargo fmt --all -- --check`,
`cargo clippy --workspace --all-targets --all-features --locked -- -D
warnings`, `cargo test --workspace --locked`), `cargo clean` first, macOS
aarch64:

| `[profile.dev] debug` | `du -sk target` afterwards |
|---|---|
| unset / `2` — what CI builds | 12,782,944 KB (12.19 GiB) |
| `"line-tables-only"` — this change | 9,790,708 KB (9.34 GiB) |

−2.85 GiB, −23.4%, and the linked binaries shrink with the rlibs. Applied to
the runner's measured 1.91 GiB of free space that would be roughly 4.8 GiB
of headroom for the largest write of the job instead of 1.9 GiB — an
estimate from the local ratio, **not** a measurement: the runner's own
`target/` size was not measured (the probe's `du` used the relative
`CARGO_TARGET_DIR` CI exports and found no `target/debug`).

## Commands run

| command | exit | result |
|---|---|---|
| `cargo fmt --all -- --check` | 0 | clean |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | 0 | clean |
| `cargo test --workspace --locked` | 0 | all tests pass (312 test binaries green) |
| `cargo test --workspace --locked -- accept_t430_ --include-ignored` | 0 | 5 selected, 5 passed |
| `cargo run -p cs_xtask -- verify-ci-budget` | 0 | pass with the fix, exit 1 with `debug = 2` |

Sensitivity was checked by reverting the one line: with `[profile.dev]
debug = 2`, `accept_t430_this_workspace_keeps_full_dwarf_out_of_the_ci_profiles`
and `accept_t430_the_workspace_keeps_backtrace_line_numbers` fail and
`verify-ci-budget` exits 1 with the message naming the setting.

## CI verification

CI runs on every push, and the acceptance criterion for this task is repeated
green runs. Run **36739807340** (push, head `de07a45`) is green on
`ubuntu24`, including the step that was failing:

* the `cargo test` step recompiled **395 crates** (`bevy_pbr`, `bevy_render`,
  `wgpu`, `naga`, …), so the failing link really did run against
  `line-tables-only` rlibs instead of the cached full-DWARF ones;
* the merged doctest linked and passed: `Doc-tests cs_app` →
  `test crates/cs_app/src/livery.rs - livery (line 49) ... ok`,
  `merged doctests compilation took 2.03s`;
* the five `accept_t430_` tests pass there;
* the restored cache was the **old** one (`Cache Size: ~2577 MB`) and the
  post-job step reported `Cache up-to-date`, so this run started from the
  full-DWARF tree — the harder direction, not the easier one.

The runs are listed in the handover summary; every one of them is a
`cargo test` step that had to link the merged `cs_app` doctest.

**Cost, stated plainly:** `Swatinem/rust-cache`'s key covers the toolchain
and `Cargo.lock`, not the profile, so the post-job step does not re-upload
and every later run restores the *older, larger* tree and recompiles those
395 crates. The `cargo test` step went from ~7 min to ~19 min in run
36739807340. The owner can make the cache follow the profile with
`shared-key: ${{ hashFiles('**/Cargo.toml') }}` on the cache step; that is a
`.github/` change and was not made here.

## Left to the owner

These need `.github/` and were deliberately not done here:

* the cache key above, so a profile change does not leave every run
  recompiling the graph;
* `debug = "none"` (or `[profile.test] debug = false`) for another ~31% of
  the object bytes, at the cost of `file:line` in test failure output;
* a `df -h` step, or a step that clears `/home/runner/work/_temp` before
  `cargo test`, if the image's own disk usage is the bigger term — the
  ~142 GB that is already there is not this repository's;
* `-C link-arg=-Wl,--threads=1` (or another `RUSTFLAGS`) for the linux
  target, which trades link time for a smaller linker footprint — note it
  helps only if memory, not disk, is the binding constraint, and the
  measurement above says 15.4 GB of 16.4 GB was available;
* retrying the job on the SIGBUS. That hides the symptom and is not
  recommended while the disk headroom is this thin.