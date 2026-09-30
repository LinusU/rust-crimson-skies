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
| 36729946195 | rally/420 | 2026-09-30 14:33, 14:55 | `cargo test` failed at this link |
| 36725414616 | rally/420 | 2026-09-30 14:08, 14:21 | `cargo test` failed at this link |
| 36723571849, 36728255759 | rally/417 | 2026-09-30 | `cargo test` failed at this link |
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
linker rustc invokes here is rust-lld (`cc … -fuse-ld=lld`, and the failure
was inside its writer threads), and lld writes its output through a
memory-mapped buffer it has already `ftruncate`d to the final size: when the
filesystem cannot back those pages the store faults, the kernel reports
`Bus error`, `collect2` prints exactly the line above, and `cargo test`
aborts at 101. That is the signature of running out of disk during the
largest write of the job, and the measured 1.91 GiB is what was left for it.
In the failing run the link died about three seconds in ("all doctests ran in
6.62s; merged doctests compilation took 3.59s"), i.e. while the output file
was being filled, not after a slow grind.

**Unknown, recorded as unknown:** what fills the other ~142 GB of the runner
disk. It is not this repository's build tree — the tree is ~10 GB (§
"Measured locally") — so it belongs to the image and the host, and nothing in
this repository can change it. That is why the fix below buys headroom
instead of reclaiming the disk, and why the owner's own options (§"Left to
the owner") are still worth considering.

## Why full DWARF is what spends the budget

Measured locally on this workspace (macOS aarch64, `target/debug` before this
change: `du -sk` = 10,164,944 KB, of which 2,456 MB rlibs, 1,122 MB rmeta,
6,123 MB test binaries and objects). Those rlibs carry **full** DWARF — they
contain `__debug_ranges`/`__debug_abbrev`, which a `line-tables-only` build
does not emit (see the three-way comparison below):

`libbevy_pbr-fa72f9e38f1c2949.rlib`, 169.5 MB, by section:

| section | MB | what it is |
|---|---|---|
| `__debug_str` | 77.9 | type, variable and function **names** |
| `__debug_info` | 13.3 | type and variable DIEs |
| `__apple_names` | 10.2 | name accelerator |
| `__debug_ranges` | 6.6 | variable location lists |
| `__debug_line` | 4.9 | the **line program** |
| `__text` | 7.1 | the code itself |

So 61% of that rlib is DWARF, and 98% of the DWARF is type/variable/name
data that only a debugger uses. Three-way build of the same crate
(`libcs_formats`, clean `CARGO_TARGET_DIR` per case, macOS):

| `[profile.dev] debug` | rlib bytes | `__text` | `__debug_str` | `__debug_info` |
|---|---|---|---|---|
| unset / `2` (rustc default) | 17,044,456 | 1.05 MB | 3.82 MB | 2.35 MB |
| `"line-tables-only"` | 12,115,056 | 0.90 MB | 2.31 MB | 0.35 MB |
| `false` | 8,327,248 | 0.90 MB | — | — |

`line-tables-only` keeps the line program (so a panic backtrace still prints
`file:line`) and drops the rest; `false` would also throw the line program
away, which is not worth trading for the extra few megabytes.

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

## Commands run

| command | exit | result |
|---|---|---|
| `cargo fmt --all -- --check` | 0 | clean |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | 0 | clean |
| `cargo test --workspace --locked` | 0 | all tests pass |
| `cargo test --workspace --locked -- accept_t430_ --include-ignored` | 0 | 5 selected, 5 passed |
| `cargo run -p cs_xtask -- verify-ci-budget` | 0 | pass with the fix, exit 1 with `debug = 2` |

Sensitivity was checked by reverting the one line: with `[profile.dev]
debug = 2`, `accept_t430_this_workspace_keeps_full_dwarf_out_of_the_ci_profiles`
and `accept_t430_the_workspace_keeps_backtrace_line_numbers` fail and
`verify-ci-budget` exits 1 with the message naming the setting.

## CI verification

CI runs on every push, and the acceptance criterion for this task is repeated
green runs. The runs of this branch are listed in the handover summary; each
one is a `cargo test` step that had to link the merged `cs_app` doctest.

## Left to the owner

These need `.github/` and were deliberately not done here:

* a `df -h` (or `cargo clean`-equivalent) step, or a step that removes
  `/home/runner/work/_temp` before `cargo test`, if the image's own disk
  pressure is the bigger term;
* `-C link-arg=-Wl,--threads=1` (or another `RUSTFLAGS`) for the linux
  target, which trades link time for a smaller linker footprint — note it
  helps only if memory, not disk, is the binding constraint;
* retrying the job on the SIGBUS. That hides the symptom and is not
  recommended while the disk headroom is this thin.