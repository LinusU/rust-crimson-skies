# #691: why a backtrace lost `file:line`, and what the test now reports about it

Date: 2026-10-06. Task: #691 "cs_xtask T430 backtrace test fails on macOS
hosts: frames print no file:line". Capabilities used: ordinary build/test
only, so no `acceptance.json` is produced (`docs/contracts/CLI-EVIDENCE.md`).
Machine: macOS aarch64, rustc 1.98.1 (LLVM 22.1.8), the shared multi-agent
build host described in `2026-10-04-t617-cargo-test-enoent-is-external.md`.

This is a finding about the project's own test gate on one host. It says
nothing about the original game.

The reported failure is reproduced by an environment variable,
`CARGO_PROFILE_DEV_DEBUG=0`, which overrides the committed
`[profile.dev] debug = "line-tables-only"` and reproduces the reporter's five
frames exactly ("The reported cause, reproduced" below). The macOS mechanism
the implementer first investigated — a binary whose debug-map objects are
removed from the target directory after the link — is a real second condition
but does not explain the report, because it leaves `std` frames located. Both
are covered: the test fails under either, and the failure now says which one it
is.

## Where a macOS test binary keeps its line tables

rustc's default on macOS is `-C split-debuginfo=unpacked`, and cargo passes it
explicitly (`cargo test -v` shows `-C split-debuginfo=unpacked` on every
macOS rustc line). With it, the linked executable carries **no DWARF**. It
has a debug map instead: one `N_OSO` stab per object file, naming

* the `*.rcgu.o` objects of the crate itself, by absolute path, next to the
  binary in `target/debug/deps` (129 `N_OSO` entries in
  `accept_t430_ci_disk_budget`, measured with `dsymutil -s`), and
* the members of the toolchain's prebuilt rlibs under `~/.rustup`, for
  `std`, `core`, `alloc` and friends.

`std`'s backtrace printer opens those paths only when it prints a backtrace.
If a path is gone by then, the frames it would have located print their
symbol name and nothing else. Nothing reports the missing file.

## Reproduction

On an `origin/main` build of `accept_t430_ci_disk_budget` (unpacked), the
crate's own `*.rcgu.o` files were moved aside and the panic helper run with
`RUST_BACKTRACE=1`:

```
   1: core::panicking::panic_fmt
             at /rustc/48a229ce…/library/core/src/panicking.rs:80:14
   2: accept_t430_ci_disk_budget::cs_xtask_t430_panic_helper
   3: accept_t430_ci_disk_budget::cs_xtask_t430_panic_helper::{closure#0}
   4: <accept_t430_ci_disk_budget::cs_xtask_t430_panic_helper::{closure#0} as core::ops::function::FnOnce<()>>::call_once
```

Frames 2 to 4 are exactly the reported ones: symbol names, no location. With
the objects put back, the same binary prints
`at ./tools/cs_xtask/tests/accept_t430_ci_disk_budget.rs:194:5`.

On this host, the owner's `disk-prune` (`prune-stale-bins.sh --delete …/target/debug/deps`,
captured in the #617 finding) removes files from agents' target directories
after they are built. A long-lived target directory whose objects were pruned
after the link gives the frames 2 to 4 above. That is a real and independent
condition; whether it has occurred here is **unknown** — a sweep of every test
binary in one target directory found no binary missing an object (see the
change section), so it is not established that it ever has.

Note what this account does *not* explain about the report: frames 0 and 1
(`std`, `core`) carried no location either, and a binary that lost only its own
objects still resolves those from the rustup rlibs, as the reproduction above
shows. The next section reproduces all five frames, and attributes the report to
the environment instead.

The test passed on this host before the change only because frames 0 and 1
resolved from the rustup rlibs. Those frames say nothing about this
workspace's profile: they would resolve with the workspace's line tables gone
entirely.

## Option measured and rejected: `-C split-debuginfo=packed` on macOS

A `.cargo/config.toml` entry
`[target.'cfg(target_os = "macos")'] rustflags = ["-C", "split-debuginfo=packed"]`
makes rustc run `dsymutil` at link time, write `<binary>.dSYM` next to each
binary and delete the objects itself. It works: the rustflag wins over cargo's
explicit `unpacked` (it comes later on rustc's command line), `dwarfdump
--debug-line` on the `.dSYM` lists both `library/core/src/panicking.rs` and
`accept_t430_ci_disk_budget.rs`, and the helper's frames keep their locations
with no object files beside the binary at all. The full workspace suite passed
under it (one unrelated `cs_net` loopback flake on the first run, green on
the rerun).

Its cost decided against it. Each `.dSYM` copies the line tables of the
binary's whole dependency graph, where unpacked binaries share the rlibs'
copy: measured after one full `cargo test --workspace` build, **377 `.dSYM`
bundles totalling 41.5 GiB** in one target directory, about 700 MB for each
test binary that links Bevy (`world`, `playtest_retail_launch`,
`playtest_full_aircraft`, `cs`), against 5 MB for `cs_xtask`. On a host
shared by several agents, each with its own target directory, and in a
workspace whose T430 work exists to cut the debug-info footprint, that is
not an acceptable price for one test's signal. The build also took 22 min 38 s
from a cold rustflags change (every macOS target directory is invalidated by
it once). This is recorded so the option is not re-tried blind; the owner may
still prefer it (or a prune that removes a binary and its objects together).

## The reported cause, reproduced: an overriding `CARGO_PROFILE_DEV_DEBUG`

The owner's note of 2026-10-06 is the one that reproduces the report, and it
reproduces it frame for frame. Measured here on an `origin/main` tree with
`CARGO_PROFILE_DEV_DEBUG=0` exported and its own target directory:

```
$ CARGO_PROFILE_DEV_DEBUG=0 cargo test -p cs_xtask --test accept_t430_ci_disk_budget
stack backtrace:
   0: __rustc::rust_begin_unwind
   1: core::panicking::panic_fmt
   2: accept_t430_ci_disk_budget::cs_xtask_t430_panic_helper
   3: accept_t430_ci_disk_budget::cs_xtask_t430_panic_helper::{closure#0}
   4: <…::{closure#0} as core::ops::FnOnce<()>>::call_once
```

That is the reporter's five frames exactly, including frames 0 and 1 without a
location, which the debug-map account above could not explain and this does:
with `debug = 0` rustc emits no line program at all, so the binary has no
debug map to point anywhere and `std` cannot locate anything. Cargo's
environment overrides `[profile.dev] debug` from `Cargo.toml`, so the committed
setting was in force in the repository and not in the build. The shared agent
environment on this Mac exported `CARGO_PROFILE_DEV_DEBUG=0` between
2026-10-05 10:29 and 23:31 CEST, which covers the report.

Two independent conditions therefore existed, and the report had both:

| Condition | Frames affected | Caused by |
| --- | --- | --- |
| `CARGO_PROFILE_DEV_DEBUG=0` | every frame, `std` included | an environment override, invisible in the manifest |
| objects removed after the link | this crate's frames only | a deletion in the target directory (#617) |

The first is the reported failure. The second is real — moving the objects
aside produces it — but it is not what was reported: a binary that loses only
its own objects still prints `at …/panicking.rs` for frames 0 and 1, which the
report did not.

## The change

* **The test no longer passes vacuously on a `std` frame.** It requires a
  frame located in `accept_t430_ci_disk_budget.rs` itself. Those frames resolve
  from the toolchain's prebuilt rlibs whatever this workspace's profile says,
  so before this change the test passed on a build with no line tables of its
  own at all. The tightened assertion fails under both conditions above, which
  is what makes it worth having.
* **A failure now names its cause instead of guessing.** The message reports
  the environment that produced the binary (`CARGO_PROFILE_DEV_DEBUG`,
  `CARGO_PROFILE_TEST_DEBUG`, `RUSTFLAGS`, `CARGO_ENCODED_RUSTFLAGS`, and
  their absence), and on macOS reads the binary's own `N_OSO` debug map to say
  whether it keeps its line tables in the object files beside it, how many it
  names, and which of them are gone. Measured under the reported condition:

  ```
  no backtrace frame named a line of accept_t430_ci_disk_budget.rs, …
  How the binary that just failed was built:
  CARGO_PROFILE_DEV_DEBUG="0" is set and cargo's environment overrides the committed [profile.dev] debug, so the binaries carry that level
  this binary's debug map names no object of its own crate, so it can locate a frame only through DWARF inside the binary itself: …
  ```

  and with the objects removed instead:

  ```
  CARGO_PROFILE_DEV_DEBUG="line-tables-only" is set and cargo's environment overrides the committed [profile.dev] debug, so the binaries carry that level
  this binary keeps the line tables of its own crate outside itself …: its debug map names 2 object(s) of this crate, of which 2 no longer exist
    gone: …/accept_t430_ci_disk_budget-cfddf8c8….accept_t430_ci_disk_budget.….rcgu.o
    gone: …/accept_t430_ci_disk_budget-cfddf8c8….accept_t430_ci_disk_budget.….rcgu.o
  ```

  This is deliberately a failure message and not a skip. The implementer's
  first attempt at this task excused the check on macOS when its own objects
  were missing, on the reading that a host file deletion is not the profile's
  fault. Review rejected that: it turns a real defect into a green result on
  the platform where it occurs, and no such deletion was observed on this
  host. A sweep of every test binary in one target directory — reading each
  one's debug map and checking each loose object it names — found 0 binaries
  with a missing object, out of 1269 loose objects named. AGENTS.md rule 6
  forbids skipping a test to reach green, and there is no measured occurrence
  to excuse. The diagnosis instead tells whoever hits it that the files are
  gone and that rebuilding the binary brings them back.
* **The object selector does not use the binary's name.** rustc truncates a
  long crate name in an object file name, so
  `accept_f02_b_one_byte_edit_fingerprint_and_cache-d009daec0120588f` is named
  by `f2a9df64c40d13f7-yte_edit_fingerprint….rcgu.o`, which shares no prefix
  with it: a `starts_with(<binary name>)` rule matches 0 of that binary's 2
  objects. `own_crate_objects` selects on the directory and the `.rcgu.o`
  suffix, and excludes the `archive(member)` references into a dependency's
  rlib, which are not paths at all.

New tests: `accept_t430_only_this_files_frames_count_as_located` (the matcher),
`accept_t430_debug_map_reader_lists_only_object_paths` (the reader on a
synthetic image, and that it reports a truncated or foreign image instead of
quietly returning nothing),
`accept_t430_own_objects_exclude_dependency_members_and_other_directories`,
`accept_t430_line_table_report_names_the_objects_that_are_gone`,
`accept_t430_line_table_diagnosis_names_the_environment`, and on macOS
`accept_t430_macos_report_accounts_for_every_object_it_finds`.

Verified on this host: with the objects restored the backtrace check runs and
passes; with them moved aside it fails and names them; with
`CARGO_PROFILE_DEV_DEBUG=0` in a separate target directory it fails and names
the override.

## Not covered

* Linux behaviour is unchanged; nothing here was run on Linux except by CI.
* The diagnosis is a message, not a repair. Nothing here rebuilds a binary
  whose objects were removed, and nothing unsets an environment variable for a
  parent process.
* What removed the rustup rlib paths in the reporter's process stays unknown.
  With `CARGO_PROFILE_DEV_DEBUG=0` the report is fully accounted for, but that
  path is only opened by the rustup tree being unreadable from that process.
