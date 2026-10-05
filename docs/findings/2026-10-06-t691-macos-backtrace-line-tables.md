# #691: why macOS backtraces lost `file:line`, and the setting that keeps it

Date: 2026-10-06. Task: #691 "cs_xtask T430 backtrace test fails on macOS
hosts: frames print no file:line". Capabilities used: ordinary build/test
only, so no `acceptance.json` is produced (`docs/contracts/CLI-EVIDENCE.md`).
Machine: macOS aarch64, rustc 1.98.1 (LLVM 22.1.8), the shared multi-agent
build host described in `2026-10-04-t617-cargo-test-enoent-is-external.md`.

This is a finding about the project's own test gate on one host. It says
nothing about the original game.

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
after the link gives exactly this output. The report also shows frames 0
and 1 (`std`, `core`) without a location, which means the rustup rlib paths
were unreadable from that process too; which deletion or move caused that in
the reporter's checkout (`bunny-alpha-2`, own `CARGO_TARGET_DIR`) was not
observed here and stays **unknown**. Both halves have the same cause: line
tables the binary only points at.

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

## The change

* The tests keep rustc's default. `accept_t430_a_panic_backtrace_names_the_file_and_line`
  first reads its own binary's Mach-O debug map (`N_OSO` entries of the
  `LC_SYMTAB` symbol table). If any object of its own crate that the map names
  no longer exists, it prints `… NOT RUN on this host …` with the missing paths
  and this finding's name, and returns: no backtrace from that binary can name
  a line whatever the profile says. The check is compiled for macOS only; on
  every other host, and on macOS when the objects are there, the test runs in
  full and fails when the line tables are missing.
* The test now requires a frame located in its own file, not any `.rs:`
  location, so `std` frames can no longer satisfy it.
* `accept_t430_debug_map_reader_lists_only_object_paths` checks the reader on
  a synthetic image; `accept_t430_macos_guard_reads_this_binarys_debug_map`
  checks that on macOS it finds this binary's objects, so the guard cannot
  silently read nothing.

Verified on this host: with the binary's own objects moved aside, the test
prints the `NOT RUN` reason and passes; with them restored it runs the
backtrace and passes; an unpacked build with the objects moved aside fails
the tightened assertion when the guard is not in the way (measured before the
guard was added).

## Not covered

* Linux behaviour is unchanged; nothing here was run on Linux except by CI.
* On a pruned macOS host the backtrace check does not run. It says so in the
  test output, and running it there needs a rebuild of that binary.
* What removed the rustup rlib paths in the reporter's process stays unknown.
