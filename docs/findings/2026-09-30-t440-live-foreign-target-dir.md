# Task #440: a worktree-private `CARGO_TARGET_DIR` still served a *live* sibling worktree

Date: 2026-09-30. Task: #440 "verify-target-dir cannot see a live sibling
worktree's artifacts in a worktree-private target directory"
(`LOCAL-TARGET-LIVE-FOREIGN-ARTIFACTS`, `allowProtectedChanges: false`, so
`.github/` was not touched). Capabilities used: ordinary build/test only — no
`CS_GAME_DIR`, GPU, audio or network capability was read or claimed, so no
`acceptance.json` is produced (`docs/contracts/CLI-EVIDENCE.md`, evidence-bound
tasks only). Commands were run from the workspace root
(`/Users/linus/coding/rust-crimson-skies/deepseek-1`, macOS aarch64, rustc
1.98.1). This task builds on #433's rule in `cs_xtask::target_dir`; its branch
was based on #433's branch because the new rule replaces none of #433's
behaviour and its acceptance criteria require #433's `accept_t433_` suite.

## The remaining hazard

Task #383 answers "can another checkout write into this target directory?" and
#433 answers "does it still serve a checkout that was deleted?". A third case
is unanswered: a **live** sibling worktree that built into this directory and
is still there. The shape the owner hit is `…/bunny-alpha-1-rev98` sharing
`…/bunny-alpha-1/target`; while the sibling exists, neither rule fires, and a
`cargo test` in one tree can still run the other tree's binary. Task #430 was
created from exactly that confusion.

## Reproduced live, with no leftovers

Two checks of one tree, one target directory, **neither deleted**. The fixture
is the shape of `tools/cs_xtask/tests/accept_t440_live_foreign_target_dir.rs`:

```
$ BASE=$PWD/target/t440-probe; mk_pkg $BASE/wt-a; mk_pkg $BASE/survivor
$ cd $BASE/wt-a     && CARGO_TARGET_DIR=$BASE/survivor/target cargo run --quiet
…/t440-probe/wt-a
$ cd $BASE/survivor && CARGO_TARGET_DIR=$BASE/survivor/target cargo run
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.00s
    Running `target/debug/probe`
…/t440-probe/wt-a
```

`survivor` runs `wt-a`'s binary, prints `wt-a`'s own path and exits 0 — no
error at all, because the artifact is fresh by fingerprint and the path it was
compiled with still exists. #433 could not see this: its rule only fires on a
recorded directory that is *gone*.

## The record cargo leaves, and why the location is the rule

For every unit whose crate read `CARGO_MANIFEST_DIR`, cargo writes one
`# env-dep:CARGO_MANIFEST_DIR=<absolute path>` line into that unit's `.d`
dep-info file. In the reproduction the survivor's target records exactly:

```
# env-dep:CARGO_MANIFEST_DIR=/…/t440-probe/wt-a
```

"Recorded and outside this worktree" cannot be the rule: any registry crate
that reads the variable records `$CARGO_HOME/registry/src/index.crates.io-…/<crate>-<version>`,
so that rule would fail on ordinary dependencies (#433 noted this and left the
live case out). The distinction this task draws is by **location**, not
existence:

* a recorded manifest directory **inside this workspace root** is this
  checkout's own;
* one **under cargo's home** (`$CARGO_HOME` when set and non-empty, else
  `~/.cargo`) is cargo's own registry cache;
* anything else that still exists is a **foreign checkout**, alive or not.

`foreign_manifest_dirs` / `foreign_manifest_dirs_with_home` implement exactly
that and `require_live` turns a non-empty result into the new
`TargetDirError::Foreign`, whose message names the target directory, this
worktree, the recorded foreign paths and the fix (`cargo clean --target-dir …`).
A record that is gone is still `#433`'s `TargetDirError::Stale`, and it is
reported first, so #433's verdicts and messages are unchanged.

## What a foreign *live* record looks like on this workspace today

Nothing outside the worktree is recorded today, so the hazard is latent here.
On this checkout (`target/debug/deps`, 1155 `.d` files) six carry a
`CARGO_MANIFEST_DIR` line, and all six name a crate of this checkout:

```
/…/deepseek-1/crates/cs_app
/…/deepseek-1/crates/cs_assets
/…/deepseek-1/crates/cs_content
/…/deepseek-1/crates/cs_formats
/…/deepseek-1/tools/cs_inspect
/…/deepseek-1/tools/cs_xtask
```

No `/…/.cargo/registry/src/…` value is recorded, and no foreign live value is
recorded. A **foreign live record**, when one exists, looks like
`<other-checkout>/crates/<crate>` (or `<other-checkout>` for a root package) —
an existing directory that is neither under this workspace root nor under
`~/.cargo`. The reproduction above produces exactly
`/…/t440-probe/wt-a`.

## How the rule is inferred, and its limits

The rule is inferred from cargo's **dep-info file format** (the
`# env-dep:<name>=<value>` lines cargo writes and the `.d` layout cargo uses:
`<target>/<profile>/deps/*.d` plus the uplifted `<target>/<profile>/*.d`), not
from cargo's internals. It does not reproduce cargo's fingerprint hashing, and
it deliberately does not try to: it reads a value cargo wrote and judges its
location. Known limitations, all of them honest limits rather than things
observed here:

* **Cargo's home cannot be derived when neither `CARGO_HOME` nor a home
  directory can be read.** `cargo_home()` is then `None` and nothing is treated
  as cargo's cache, so a recorded registry path could be reported as foreign.
  That is a broken environment, and the advice the gate gives (`cargo clean
  --target-dir`) is then only expensive, not wrong.
* **A recorded path that is removed and outside both roots** is reported by
  #433's `Stale`, not by this rule — the intended order. A registry path under
  an autocleaned `$CARGO_HOME/registry/src` is therefore still reported the way
  #433 already reported it.
* **A foreign checkout that happens to live inside this workspace root** (for
  example a nested checkout under this tree) is classified as this checkout's
  own. That is what "inside the workspace" means, and the workspace's own
  `target/` default already treats such a layout as one tree.
* **Deleted records and deleted checkouts are not this rule's job.** The gate
  reports; reclaiming the directory belongs to whoever owns the machine.

## Resolution

`tools/cs_xtask/src/target_dir.rs` grew `cargo_home`, `foreign_manifest_dirs`
and `foreign_manifest_dirs_with_home`, and `require_live` now checks the
foreign set after the removed set. `tools/cs_xtask/tests/accept_t440_live_foreign_target_dir.rs`
pins it: a real-cargo live pair is refused by `verify_workspace_with_env` and by
`cs_xtask verify-target-dir`; a hand-written target directory proves the
cargo-home exclusion (the same registry record is foreign when the home is
pointed elsewhere), that this checkout's own record is not foreign, and that a
gone record stays `#433`'s; and this checkout's own target directory is checked
as part of `cargo test --workspace`. With the foreign check removed, the two
production-path tests fail and the other two still pass, so the check is what
they exercise.
