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
| 36723571849 | rally/417 | 2026-09-30, earlier | `cargo test` failed at this link |
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
  (`2`, `true`, `"full"`), because those overrides would undo the setting for
  exactly the crates CI links. `true` is cargo's boolean spelling of the same
  setting, not a reduced one: the same scratch crate built with `debug = true`
  and with `debug = 2` produced a byte-identical 555,480-byte binary, so it is
  rejected with `2`. A trailing comment is stripped before the level is read,
  so `debug = 2 # full type information` cannot smuggle the level past the
  gate.

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

### The cache key does not follow the profile, so the saving is not collected yet

Added in review. `Swatinem/rust-cache` keys on the toolchain and `Cargo.lock`,
not on `Cargo.toml`, so the post-job step of a run on the fix reports
`Cache up-to-date` and the *pre-fix* full-DWARF tree stays in the cache. Run
36750916306 shows the whole sequence: `Cache Size: ~2577 MB
(2702354412 B)`, `Cache restored successfully`, then 395 crates recompiled
into the same `target/`, then `Cache up-to-date`.

What cargo does with the artifacts of the profile it just replaced was
measured on a scratch crate, same machine, same flags, no `cargo clean`
between the two builds:

| step | `target/debug/deps/` | `du -sk target` |
|---|---|---|
| `[profile.dev] debug = 2` | `libscratch430-43b14ecffeb4fd14.rlib` (315,840 B) | 2,160 KB |
| then `debug = "line-tables-only"` | that rlib **plus** `libscratch430-6c560f4a94786fe5.rlib` (192,616 B) | 3,056 KB |

Cargo does not delete the previous profile's artifacts: the unit's metadata
hash changes, so the rebuild writes new files and leaves the old ones. So
today a CI run on this branch carries the restored full-DWARF tree *and*
builds the line-tables-only tree on top of it, and the 4.8 GiB of headroom in
the paragraph above is not what the job actually has. The green runs show it
fits on the runners that ran them, not that it buys headroom on a tight one.

This is an inference from a measured retention behaviour, not a measurement of
a runner's peak: nothing in a run that passes reports its own disk high-water
mark, and the probe commit that could have measured it is not on this branch.
What is measured is the retention above and the cache messages quoted. The
consequence for the owner is that the cache-key line in "Left to the owner" is
a prerequisite for this fix paying off, not only a speed-up: with it, the
first run after the change stores the smaller tree and every later run starts
from it.

### `CARGO_PROFILE_DEV_DEBUG` overrides the manifest, and is set in some shells

Also added in review, because it makes a local check of this fix meaningless
if it is left in place. Cargo reads `CARGO_PROFILE_*` from the environment
*before* the manifest, so a shell that exports
`CARGO_PROFILE_DEV_DEBUG=line-tables-only` makes the committed
`[profile.dev] debug` line inert for every local build — `cargo build -v`
prints `debuginfo=line-tables-only` for the workspace's own crate, taken from
the environment, whichever value the manifest states. The A/B numbers in
"Measured effect" were taken with the variable unset
(`env -u CARGO_PROFILE_DEV_DEBUG`), and the `accept_t430_` gate is unaffected
either way because it reads the manifest text. CI is unaffected: the failing
job exports no `CARGO_PROFILE_*` at all.

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
green runs. Run **36739807340** (push, head `de07a45`, attempts 1 and 2 both
green) and run **36747597757** (push, head `8bc12bc`, green after the rebase
onto the then-current `main`) are green on `ubuntu24`, including the step that
was failing:

* the `cargo test` step recompiled **395 crates** (`bevy_pbr`, `bevy_render`,
  `wgpu`, `naga`, …), so the failing link really did run against
  `line-tables-only` rlibs instead of the cached full-DWARF ones;
* the merged doctest linked and passed: `Doc-tests cs_app` →
  `test crates/cs_app/src/livery.rs - livery (line 49) ... ok`
  (`merged doctests compilation took 2.03s` in 36739807340, `1.85s` in
  36747597757 — the same link that died after 3.03s in the failing runs);
* the five `accept_t430_` tests that existed at that head pass there; the
  review pass adds a sixth, so a run on a later head carries six;
* the restored cache was the **old** one (`Cache Size: ~2577 MB`) and the
  post-job step reported `Cache up-to-date`, so these runs started from the
  full-DWARF tree — the harder direction, not the easier one.

The runs are listed in the handover summary; every one of them is a
`cargo test` step that had to link the merged `cs_app` doctest.

**Cost, stated plainly:** `Swatinem/rust-cache`'s key covers the toolchain
and `Cargo.lock`, not the profile, so the post-job step does not re-upload
and every later run restores the *older, larger* tree and recompiles those
395 crates. The `cargo test` step went from ~7 min to ~19 min in run
36739807340. The owner can make the cache follow the profile with
`shared-key: ${{ hashFiles('**/Cargo.toml') }}` on the cache step; that is a
`.github/` change and was not made here. What that costs in *disk* is measured
in "The cache key does not follow the profile" below, and it is the reason the
same change is a prerequisite rather than only a speed-up.

## Left to the owner

These need `.github/` and were deliberately not done here:

* the cache key above, so a profile change does not leave every run
  recompiling the graph — and, until it lands, so that the footprint saving
  this branch makes is actually collected (see "The cache key does not follow
  the profile");
* `debug = "none"` (or `[profile.test] debug = false`) for another ~31% of
  the object bytes, at the cost of `file:line` in test failure output. The
  review pass made that cost enforced rather than merely documented:
  `accept_t430_a_panic_backtrace_names_the_file_and_line` reads the backtrace
  a real panic produces, and it fails under `CARGO_PROFILE_DEV_DEBUG=none`
  (measured), so taking this option means deleting or amending that test
  deliberately.
* a `df -h` step, or a step that clears `/home/runner/work/_temp` before
  `cargo test`, if the image's own disk usage is the bigger term — the
  ~142 GB that is already there is not this repository's;
* `-C link-arg=-Wl,--threads=1` (or another `RUSTFLAGS`) for the linux
  target, which trades link time for a smaller linker footprint — note it
  helps only if memory, not disk, is the binding constraint, and the
  measurement above says 15.4 GB of 16.4 GB was available;
* retrying the job on the SIGBUS. That hides the symptom and is not
  recommended while the disk headroom is this thin.

## Review pass

Reviewer: `bunny-alpha-1` — the same agent instance that implemented the
change, with no independent review. It is recorded as a checked branch, not as
independent evidence about the original game (AGENTS.md, "Reviewing").

Two problems were found in the delivered work and fixed here rather than sent
back:

1. **The gate accepted the full-DWARF spelling `debug = true`.** Cargo maps
   `true` to `2`, and the two produce a byte-identical binary (measured on a
   scratch crate, 555,480 B each), so a manifest edited to `debug = true` would
   have restored the exact footprint this task exists to remove while
   `verify-ci-budget` and every `accept_t430_` test still passed. `true` is now
   in `budget::FULL_DWARF_LEVELS`, with test cases for it, for the quoted
   `"2"`, and for a level followed by a trailing comment — the comment is
   stripped before the level is read, which the first version of the scanner
   did not do, so `debug = 2 # ...` was accepted too.
2. **The `file:line` claim was only asserted as a manifest string.** The new
   `accept_t430_a_panic_backtrace_names_the_file_and_line` runs the panicking
   helper in a child process with `RUST_BACKTRACE=1` and requires a backtrace
   *frame* to name a source file and line, reading only the text after
   `stack backtrace:` so the panic header's own compile-time location cannot
   stand in for it. It passes on the committed profile and fails under
   `CARGO_PROFILE_DEV_DEBUG=none` (measured: 6 of 7 pass, this one fails, with
   frames that carry names but no locations), which is what makes the
   `debug = "line-tables-only"` choice and its 2.85 GiB worth something.

The review also measured two facts the change depends on and had not recorded:
cargo does not delete the previous profile's artifacts, and a
`CARGO_PROFILE_DEV_DEBUG` in the environment overrides the manifest. Both are
in the sections above, with the measurements and the limits of what they prove.

Commands run by the review pass on its own head, all with
`env -u CARGO_PROFILE_DEV_DEBUG`:

| command | exit | result |
|---|---|---|
| `cargo fmt --all -- --check` | 0 | clean |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | 0 | clean |
| `cargo test --workspace --locked` | 0 | REVIEW_WORKSPACE_TEST_RESULT |
| `cargo test --workspace --locked -- accept_t430_ --include-ignored` | 0 | REVIEW_SELECTION_RESULT |
| `CARGO_PROFILE_DEV_DEBUG=none cargo test -p cs_xtask --test accept_t430_ci_disk_budget` | 101 | the backtrace test fails, 6 others pass — its sensitivity check |