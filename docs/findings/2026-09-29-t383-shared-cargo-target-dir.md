# T383: a shared `CARGO_TARGET_DIR` silently invalidates agent check results

Date: 2026-09-29. Task: #383 "Give each agent worktree its own
`CARGO_TARGET_DIR` so concurrent builds cannot clobber each other".
Capabilities used: ordinary build/test only (no `retail` data touched).
Machine: macOS (Darwin 27), aarch64-apple-darwin, rustc/cargo 1.98.1.

## Symptom (reported in the task, reproduced here)

The task reports that every agent worktree under
`/Users/linus/coding/rust-crimson-skies/` exported one
`CARGO_TARGET_DIR=/Users/linus/coding/rust-crimson-skies/target`, and that a
`panic!` probe added in one worktree never ran under the shared directory
even though cargo reported `Compiling`/`Finished` — the binary under test
was another worktree's.

Reproduced with a minimal pair of packages (no game code involved):

```sh
# two packages, both named `probe`, differing only in what they print
T=$(mktemp -d)
mkdir -p "$T/wt-a/src" "$T/wt-b/src"
for w in a b; do
  printf '[package]\nname = "probe"\nversion = "0.0.0"\nedition = "2021"\n\n[workspace]\n' \
    > "$T/wt-$w/Cargo.toml"
done
echo 'fn main() { println!("MARKER-A"); }' > "$T/wt-a/src/main.rs"
echo 'fn main() { println!("MARKER-B"); }' > "$T/wt-b/src/main.rs"

export CARGO_TARGET_DIR="$T/shared"
(cd "$T/wt-a" && cargo run --quiet)   # prints MARKER-A
(cd "$T/wt-b" && cargo run --quiet)   # prints MARKER-A  <-- wt-b's code never ran
unset CARGO_TARGET_DIR
```

Measured on this machine: the second `cargo run` printed `MARKER-A`. Cargo
saw a fresh `probe` artifact in the shared directory (same package id, same
metadata fingerprint — the fingerprint does not include the source
directory) and reused it without compiling, then executed it. `wt-b`'s
mutation was invisible in `wt-b`'s own binary.

The automated version of this reproduction is
`accept_t383_a_shared_target_dir_runs_a_foreign_binary` in
`tools/cs_xtask/tests/accept_t383_per_worktree_target_dir.rs`; it builds
real fixture packages under `target/t383-target-dir-fixtures/` and asserts
exactly this output.

## Why every result from a shared directory is non-evidence

`cargo test`, `cargo clippy` and the `accept_*` prefix selections all run
binaries whose contents depend on which worktree wrote the artifact last —
or, as above, which wrote it *first* and was then treated as fresh. A green
run under the fleet's old environment proved nothing about the tree it was
invoked in. This invalidates the AGENTS.md "checks before every push"
contract on any machine with a shared `CARGO_TARGET_DIR`.

## What this change installs

* `cs_xtask::target_dir` — resolves the effective target directory by
  asking cargo itself (`cargo metadata --no-deps` reports
  `target_directory` after applying `CARGO_TARGET_DIR`, every applicable
  `.cargo/config.toml` and the worktree-local default, so the gate never
  re-implements precedence), then requires it to be private to the
  worktree: inside the workspace root, or containing a path component equal
  to the workspace directory's own name (the `<root>/<worktree>` /
  `<root>/target/<worktree>` layouts a per-worktree value is derived with).
* `cs_xtask verify-target-dir [--workspace-root <dir>]` — prints the
  effective directory and exits 1 when it is shared, with the fix spelled
  out in the error.
* `accept_t383_this_worktrees_effective_target_dir_is_per_worktree` runs
  inside `cargo test --workspace` — one of the checks AGENTS.md already
  requires before every push — so an agent whose environment still exports
  a shared directory gets a loud failure naming the fix instead of
  producing untrustworthy greens.
* `README.md` "Running agents" now states the rule. `AGENTS.md` is a
  protected path this task may not touch; see "Owner actions" below.

## The rule

An agent may trust its build/test results only when the effective cargo
target directory is private to its checkout:

* unset `CARGO_TARGET_DIR` (cargo uses `<checkout>/target/`), or
* `export CARGO_TARGET_DIR` to a directory that names the checkout, e.g.
  `"$PWD/target"` or `"$CARGO_TARGET_DIR_ROOT/$(basename "$PWD")"`.

## Reviewer procedure

1. `cargo run -p cs_xtask -- verify-target-dir` — reports the effective
   directory this checkout will really use; exits 1 if it is shared.
2. `cargo test --workspace --locked -- accept_t383_ --include-ignored` —
   runs the reproduction (shared dir runs a foreign binary), the isolation
   proof (per-worktree dirs run each own binary), the classification cases
   and the live gate against this checkout.
3. Optional manual spot-check of the enforcement path:
   `CARGO_TARGET_DIR=/tmp/t383-shared cargo test -p cs_xtask --test
   accept_t383_per_worktree_target_dir -- this_worktrees` must fail with
   the instructions message.
4. Optional manual reproduction of the defect on any machine: the shell
   snippet in "Symptom" above. Under a shared dir the second run prints
   `MARKER-A`; with `CARGO_TARGET_DIR` unset (or per-worktree values) it
   prints `MARKER-B`.

## Owner actions outside this repository

These are not doable from inside a task branch:

* Stop exporting a fleet-wide `CARGO_TARGET_DIR` in the agent launcher
  (`start-agent.sh` / `common.env` / shell profile), or derive it per
  worktree as the task suggests.
* Add the rule to `AGENTS.md` (protected path): agents must not trust check
  results from a `CARGO_TARGET_DIR` that is not private to their worktree;
  `verify-target-dir` and the `accept_t383_` suite enforce it during
  `cargo test --workspace`.

## Limitations

* "Names the worktree" is checked by path component equality with the
  workspace directory's basename. Two checkouts with the *same* basename
  under different parents (e.g. `/a/devin-1` and `/b/devin-1`) would still
  collide under `<root>/<basename>` derivation; using the full worktree
  path or an in-worktree `target/` avoids that.
* `cargo metadata` is spawned per `verify_workspace` call; inside the test
  suite that is one subprocess on `cargo test --workspace`.
* `target_dir_from_metadata` takes the first `"target_directory"` string
  field in the `cargo metadata` JSON, matching the hand-rolled extraction
  used elsewhere in `cs_xtask`. A workspace member that carried such a key
  under `[package.metadata]` would shadow the real field — the gate then
  rejects the stray value, failing loudly rather than silently accepting a
  shared directory.
* The gate runs where the agent runs checks. It cannot fix environments it
  never sees; it makes a wrong environment fail loudly instead of passing
  quietly.
