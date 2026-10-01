# Task #437: a live target-dir gate reused across checkouts judged the wrong tree

Date: 2026-10-01. Task: #437 "Pin per-worktree `CARGO_TARGET_DIR` so a stale
foreign artifact cannot be run by cargo test" (`allowProtectedChanges: false`,
so no protected path was touched). Capabilities used: ordinary build/test only
— no `CS_GAME_DIR`, GPU, audio or network capability was read or claimed, so no
`acceptance.json` is produced (`docs/contracts/CLI-EVIDENCE.md`, evidence-bound
tasks only). The failure was reproduced on this machine and involved no original
game data.

## The failure

While implementing #434 in this worktree, without #434 causing it:

```
the effective cargo target directory /…/bunny-2/target
is not private to this worktree /…/bunny-2-rev87/tools/cs_xtask/../..
```

`CARGO_TARGET_DIR` was this worktree's own `target/`, which is correct. The
*compiled-in* root was foreign: `accept_t383_per_worktree_target_dir.rs` derived
the worktree with `Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")`, which
rustc bakes in at compile time. A test binary built in one checkout and run in
another therefore names the checkout that produced it, and the gate compared
`…/bunny-2-rev87/target` — a directory nobody had wrong — against
`…/bunny-2`, and failed.

`touch tools/cs_xtask/src/*.rs tools/cs_xtask/tests/accept_t383_…rs` makes it
pass, which is the tell: the binary was stale, not the environment. It
reproduced identically on unmodified `origin/main` in this checkout.

## Why a shared or reused directory produces one

Cargo keys an artifact by package id and metadata fingerprint, never by source
path (the rule behind [#383] and [#433]). So a `CARGO_TARGET_DIR` shared between
checkouts — or one left behind by a checkout that was replaced — lets
`cargo test` run a foreign artifact here without compiling anything. There is
nothing in the environment under test that is wrong, so the failure has no cause
the agent can act on, and `cargo test --workspace` is unusable until someone
deletes a `target/` directory by hand.

The related exposure is catalogued in
`2026-09-30-t441-renamed-worktree-stale-test-binaries.md`: about 25 tests across
`crates/` and `tools/` reach their own sources through `env!("CARGO_MANIFEST_DIR")`,
which is the same staleness seen from the reading side.

[#383]: 2026-09-29-t383-shared-cargo-target-dir.md
[#433]: 2026-09-30-t433-cargo-reuses-artifacts-of-a-removed-worktree.md

## The resolution

`cs_xtask::target_dir` grew `running_workspace_root()` and the seam behind it,
`workspace_root_from(start)`: walk up from the process working directory to the
nearest `Cargo.toml` carrying a `[workspace]` table. That is the same
nearest-ancestor rule cargo itself applies to resolve a package's workspace, so
it cannot name a different root than the one cargo built against. Cargo runs
test binaries with the package root as the working directory, so the walk starts
inside the checkout under test whichever checkout compiled the binary.

Three suites ran the gate against this very checkout and all three derived the
root the old way — `accept_t383_` (this task), plus `accept_t433_` and the
`accept_t440_` suite that landed on `main` while this task was in review, which
had the same bake copied from `accept_t383_`. All three now call the one
function, so they cannot drift apart again.

`accept_t437_runtime_workspace_root.rs` pins the derivation:

* the nearest `[workspace]` table wins when manifests nest, and is found from a
  package directory, the workspace root, and below both;
* only the table header counts — `# [workspace]`, a `workspace = …` key and
  `[workspace.dependencies]` are all passed over;
* a tree with no workspace manifest above it reports `None` rather than a guess,
  and the gates turn that into a loud failure naming the checkout they could
  not identify;
* `running_workspace_root` follows the *working directory*, not the compile-time
  bake: the suite re-executes its own binary from a fixture workspace it was
  never built in and requires the child to name that workspace. This is the
  reported defect reduced to one process start, so it is caught on a machine
  with no stale `target/` at all;
* the gate accepts a directory private to the named checkout and still refuses,
  loudly and by name, one two live checkouts could write to — asserted in both
  directions, so which of the two is being judged cannot change the verdict;
* `verify-target-dir` — whose own default is `--workspace-root .` — agrees with
  the derived root, so the live gates and the command an agent runs by hand
  cannot judge different trees.

## Verification, on this machine

Both directions of the acceptance criteria, with real cargo:

* A binary compiled in this checkout, run from `/tmp/t437-sim/fresh` (a
  workspace that had never been built in), with
  `CARGO_TARGET_DIR=/tmp/t437-sim/fresh/target`: `accept_t383_` 12/12,
  `accept_t433_` 7/7, `accept_t440_` 4/4, `accept_t437_` 8/8 pass. Before the
  fix the same binaries fail, naming `…/bunny-alpha-2` as the worktree for a
  directory inside `/tmp/t437-sim/fresh`.
* The same binaries from the same cwd with
  `CARGO_TARGET_DIR=/tmp/t437-sim/shared-fleet-target`: all three live gates
  still fail with the `Shared` verdict naming the runtime worktree and the fix.
  Nothing was traded away to get the first half.
* Removing the implementation fails the suite: `workspace_root_from` stubbed to
  `None` fails four of the `accept_t437_` tests, and `running_workspace_root`
  put back to the `env!` bake — unnormalized and canonicalized alike — fails
  the re-execution test, which catches exactly the shape `accept_t433_` and
  `accept_t440_` still had.

Checks: `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets
--all-features --locked -- -D warnings`, `cargo test --workspace --locked` (163
suites, 0 failures), and `cargo test --workspace --locked -- accept_t437_
--include-ignored` (8 run, 8 passed).