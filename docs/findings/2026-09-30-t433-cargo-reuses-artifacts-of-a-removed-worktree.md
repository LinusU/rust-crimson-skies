# Task #433: a worktree-private `CARGO_TARGET_DIR` still served a worktree that was deleted

Date: 2026-09-30. Task: #433 "Cargo reuses artifacts built by a removed sibling
worktree, so a test can read a path that no longer exists" (`LOCAL-TARGET-STALE-ABS-PATH`,
`allowProtectedChanges: false`, so `.github/` was not touched). Capabilities used:
ordinary build/test only — no `CS_GAME_DIR`, GPU, audio or network capability was
read or claimed, so no `acceptance.json` is produced
(`docs/contracts/CLI-EVIDENCE.md`, evidence-bound tasks only). All commands were run
from the workspace root
(`/Users/linus/coding/rust-crimson-skies/bunny-alpha-2`, macOS aarch64, rustc
1.98.1).

## The failure

Found while verifying task #430. The review worktree
`bunny-alpha-1-rev98` was built into `bunny-alpha-1/target` and then deleted.
Afterwards, in `bunny-alpha-1`, with a clean tree:

```
thread 'pe_resources_tests::accept_f12_g_no_engine_path_reads_the_type_255_payload'
panicked at crates/cs_formats/src/pe_resources.rs:1922:33:
reading /…/bunny-alpha-1-rev98/crates/cs_formats/src/pe_resources.rs:
No such file or directory (os error 2)
```

and, in the same worktree, `cargo test -p cs_app --doc` failed differently:

```
error[E0432]: unresolved import cs_content::world::MissionOverlay
```

Both are the same cause. `cs_formats` reaches its own source through
`env!("CARGO_MANIFEST_DIR")`, so the `rlib` cargo reused was compiled in
`bunny-alpha-1-rev98`, with that path baked in. The doctest failure is the
reuse of a `libcs_content-*.rlib` that predates the commit which added
`MissionOverlay`: cargo decided it was fresh because the tree it was asked
about matched a fingerprint computed from a *different* tree's content.

`bunny-alpha-1/target` is private to `bunny-alpha-1` by name, so task #383's
guard passed it. That guard answers "can another checkout write here?", which
is a question about the future. This defect is about the past: a finished
checkout wrote here, and its artifacts outlived it.

## Why cargo cannot see it

Cargo keys an artifact by package id and metadata fingerprint. For a path
package the fingerprint is a function of the *content* and the compiler
settings, not of where the content lives, so two checkouts of one tree produce
interchangeable artifacts and cargo reuses them silently, reporting
`Finished in 0.00s`.

## The record cargo does leave, and where

Every `.d` dep-info file lists the environment variables its unit read, one
per line, at the end:

```
# env-dep:CARGO_BIN_EXE_cs_xtask=placeholder:cs_xtask
# env-dep:CARGO_MANIFEST_DIR=/…/bunny-alpha-1-rev98/crates/cs_formats
```

That is the checkout which produced the artifact, written verbatim. On this
workspace 80 of 1154 dep-info files in `target/debug/deps` carry a
`CARGO_MANIFEST_DIR` line, and every one of them names a crate of this
worktree (`crates/cs_app`, `crates/cs_assets`, `crates/cs_content`,
`crates/cs_formats`, `tools/cs_inspect`, `tools/cs_xtask`). A crate's manifest
directory is a directory for as long as its checkout is there, so a recorded
value that is not a directory is positive evidence that the checkout is gone —
no hashing to reproduce, no guess about cargo internals.

## Reproduction, with none of the owner's leftovers

Two checkouts of one tree, one target directory, then delete one of them. This
is the fixture in `tools/cs_xtask/tests/accept_t433_stale_target_dir.rs`
(`accept_t433_a_removed_worktree_leaves_a_live_checkout_running_its_binary`),
which runs real cargo:

1. create `wt-a` and an identical `survivor` from the same sources — the
   fixture binary reads `env!("CARGO_MANIFEST_DIR")/src/main.rs` and prints
   which checkout it came from, the shape of the `cs_formats` failure;
2. `cargo run` in `wt-a` with `CARGO_TARGET_DIR=survivor/target`;
3. `cargo run` in `survivor` with the same target dir: cargo prints
   `Finished dev profile [unoptimized + debuginfo] target(s) in 0.00s` and runs
   `wt-a`'s binary;
4. `rm -rf wt-a`, then `cargo run` in `survivor` again: still `Finished in
   0.00s`, and the program exits 1 with
   `cannot read /…/wt-a/src/main.rs: No such file or directory (os error 2)`.

Both checkouts are written before the build, so the survivor's sources are
older than the artifact — the state in which cargo calls a foreign artifact
fresh, on any filesystem whose timestamps resolve in order. A hand-typed
version of the same experiment, run during this task:

```
$ BASE=$PWD/target/t433-probe; mk_pkg $BASE/wt-a; cp -R $BASE/wt-a $BASE/survivor
$ cd $BASE/wt-a     && CARGO_TARGET_DIR=$BASE/survivor/target cargo run --quiet
…/t433-probe/wt-a
$ cd $BASE/survivor && CARGO_TARGET_DIR=$BASE/survivor/target cargo run
    Finished `dev` profile … in 0.00s
…/t433-probe/wt-a
$ rm -rf $BASE/wt-a
$ cd $BASE/survivor && CARGO_TARGET_DIR=$BASE/survivor/target cargo run
    Finished `dev` profile … in 0.00s
    Running `target/debug/probe`
cannot read …/t433-probe/wt-a/src/main.rs: No such file or directory (os error 2)
```

## The resolution

`cs_xtask::target_dir` grew the second half of the gate (task #383 owns the
first). `recorded_manifest_dirs` reads the `# env-dep:CARGO_MANIFEST_DIR=`
lines out of `<target>/<profile>/deps/*.d` and `<target>/<profile>/*.d`;
`removed_manifest_dirs` keeps the ones that are no longer directories;
`require_live` turns a non-empty result into `TargetDirError::Stale`, which
names the stale directory, the recorded path that is gone and the fix
(`cargo clean --target-dir …`). `verify_workspace` calls it after the #383
check, because a directory another live worktree can write to is the earlier
defect whatever it contains.

`cs_xtask verify-target-dir` therefore fails instead of reporting a stale
directory as private, and `cargo test --workspace` fails too: the
`accept_t433_this_worktrees_target_dir_holds_no_removed_worktree` test runs
the gate against this very checkout. On the reported environment it says:

```
cs-xtask: the cargo target directory /…/bunny-alpha-1/target is private to
worktree /…/bunny-alpha-1, but it still holds artifacts whose recorded
CARGO_MANIFEST_DIR names /…/bunny-alpha-1-rev98/crates/cs_formats: that
checkout is gone, and cargo still calls those artifacts fresh because it keys
them by fingerprint rather than by source path, so `cargo test` can run a
binary that reads a path outside this worktree (task #433). The directory name
cannot express this, so delete /…/bunny-alpha-1/target and let the next build
recreate it, e.g. cargo clean --target-dir /…/bunny-alpha-1/target.
```

## What is deliberately *not* in the gate

* **A recorded `CARGO_MANIFEST_DIR` that exists but belongs to another live
  checkout.** That is a real hazard, but the recorded values include
  `/…/.cargo/registry/src/…` for any registry crate that reads the variable, so
  "not inside this worktree" would fail on ordinary dependencies. Only a
  directory that is *gone* is evidence, and the task is about a gone one.
* **Artifacts that record no `CARGO_MANIFEST_DIR` at all.** Such a unit did not
  read the variable, so it has no absolute path of this checkout compiled into
  it; `OUT_DIR` is inside the target directory and `file!()` is relative to the
  crate. Reusing them is not what broke anything here.
* **A recorded path under `$CARGO_HOME` that cargo's own cache pruned.** A
  registry crate that reads `CARGO_MANIFEST_DIR` records
  `/…/.cargo/registry/src/…`, and `cargo cache --autoclean` or a hand-removed
  `registry/src` takes that directory away. That is cargo's cache lifecycle, not
  a removed worktree, and the gate reports the two the same way; the advice it
  gives (`cargo clean --target-dir`) is then only expensive, not wrong. No
  dependency in this workspace's graph records such a path today — every
  recorded value above is a crate of this worktree — so this is a known
  limitation of the rule, not something observed here.
* **Deleting anything.** The gate reports; `cargo clean --target-dir` and
  removing a worktree directory belong to whoever owns the machine.

## Operational note for the owner

The gate now says what cargo cannot, but it cannot undo the artifacts: a
worktree that is removed still has to have its `target/` directory deleted, and
with it the reuse stops. The gate exists so that forgetting is loud instead of
silent. The same stale-binary symptom also arrives through a *renamed* rather
than removed checkout, and the steps to clear it by hand are in
`docs/findings/2026-09-30-t441-renamed-worktree-stale-test-binaries.md`; the
gate here is the automated half of that note.
