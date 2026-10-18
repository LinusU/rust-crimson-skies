# Task #1265: a fleet scan that no target directory holds another checkout's artifacts

Date: 2026-10-18. Task: #1265 "A second checkout builds into bunny-alpha-2's
target directory and fails the T383 gate" (`T383-FOREIGN-TARGET`,
`allowProtectedChanges: false`). Capabilities used: ordinary build/test only —
no `CS_GAME_DIR`, GPU, audio or network capability was read or claimed, so no
`acceptance.json` is produced (`docs/contracts/CLI-EVIDENCE.md`,
evidence-bound tasks only). Machine: macOS (Darwin 25), aarch64-apple-darwin.

## The incident, and what the owner already fixed

On 2026-10-10 a `cargo test --workspace --locked` in `bunny-alpha-2` failed
the T383 live gate: its `target/` held dep-info recording
`CARGO_MANIFEST_DIR=…/bunny-2/crates/cs_app`, so a cargo whose working
directory was `bunny-2` had built into `bunny-alpha-2`'s target directory.

The owner traced the main cause to manual APFS clones between agents'
targets (`cp -c` of `deps`/`build`/`.fingerprint`), stopped cloning, and
deleted the contaminated target directories. This task keeps only the guard
the acceptance asks for: a *cheap scan* that no checkout's `target/` holds
artifacts naming a different checkout, plus the provisioning half — that the
`CARGO_TARGET_DIR` each session inherits resolves inside its own checkout.

## What the scan adds that the per-worktree gate cannot see

`verify-target-dir` (tasks #383/#433/#440) runs *inside* one checkout and
judges the target directory that checkout's cargo resolves. It cannot notice
a foreign checkout's build writing into a sibling's `target/` while the
sibling itself is idle — the records sit in a directory no running gate is
watching. The fleet layout gives the missing vantage point: all checkouts
are siblings under one parent (`/Users/linus/coding/rust-crimson-skies/`),
so one scan over that parent sees every agent's directory.

`cs_xtask::target_dir::scan_fleet` enumerates a checkout as every direct
child of the fleet root whose own `Cargo.toml` declares a `[workspace]`
table (the same line rule `workspace_root_from` climbs with). For each
checkout it does two things:

* **Provisions.** The session launcher sources `<checkout>/.env` before the
  agent loop starts, so the `CARGO_TARGET_DIR` assignment found there is the
  value every session in that checkout inherits. `env_assignment` reads the
  last `CARGO_TARGET_DIR=` line (with `export`, quotes and a trailing
  `#` comment handled), `resolve_declared` expands `$PWD`/`${PWD}`, `$HOME`/
  `${HOME}` and a leading `~/`, and `is_per_worktree` classifies the result
  as `Private` or `Shared`. A value still carrying an expansion the scan
  cannot see is `Unverifiable` — reported as a note, never guessed and never
  silently passed (the per-session live gate remains the assertion that
  covers it).
* **Artifacts.** Every target directory the checkout could build into —
  `<root>/target` plus a provisioned directory that resolves elsewhere — is
  read for dep-info `# env-dep:CARGO_MANIFEST_DIR=` records. A record that is
  outside the checkout's root and outside cargo's home names a different
  checkout; `ForeignRecord::live` says whether it is still present (the
  `bunny-2` shape) or gone (the removed-checkout shape task #433 already
  refuses per-worktree). A record inside the checkout itself is that
  checkout's own business — `verify_workspace` gates it — even when the
  recorded path is gone, so test fixtures under `target/` can never produce
  a fleet finding.

`cs_xtask verify-target-dirs [--workspace-root <dir>] [--fleet-root <dir>]`
prints one line per checkout, then the findings; exit 1 when any exist, 2 on
a bad option. The default fleet root is the workspace root's parent — the
layout the owner actually runs — so agents run it bare.

## The live gate

`accept_t1265_this_fleets_target_dirs_hold_no_foreign_records` runs the scan
in `cargo test --workspace`, so the check is automatic wherever an agent
tests rather than a manual sweep someone remembers to run.

Fleet state when this gate first ran (2026-10-18): 13 checkouts under
`/Users/linus/coding/rust-crimson-skies/`; the eight agent `.env` files all
provision `<own checkout>/target` (Private); five `target/` directories hold
dep-info, and every record names the checkout's own crates, fixtures under
its own `target/`, or `~/.cargo/registry` — zero foreign records. That is
consistent with the owner's cleanup and does not reproduce the pid-62637
observation: no foreign write has recurred since the clones stopped.

```
verify-target-dirs: 13 checkout(s) under /Users/linus/coding/rust-crimson-skies,
5 target dir(s) scanned — no target directory holds artifacts another checkout built
```

## The tests

`tools/cs_xtask/tests/accept_t1265_fleet_target_dirs.rs`:

* clean fleet: two checkouts building their own targets scan clean, a
  sibling without a workspace manifest is not enumerated;
* foreign build: a checkout building into a sibling's `target/` is named —
  target directory, foreign checkout, `cargo clean --target-dir` fix —
  while it is still present;
* classification edges: a gone foreign record is still a finding, a record
  inside the checkout or under cargo's home is not;
* provisioning: `Shared`/`Private`/`Unverifiable` outcomes, including
  `export`, quotes, comments, last-assignment-wins and `$PWD` expansion;
* a private provisioned directory *outside* the checkout is scanned too;
* the subcommand end to end: 0 on clean, 1 with named findings on dirty, 1
  on an unusable workspace root, 2 on a bad option;
* the live fleet gate above.

Fixture packages are named `probe-<checkout>` rather than sharing one
`probe`: an identical package id already fresh in the victim's target
directory is reused *without compiling* (task #383's defect) and leaves no
dep-info of its own, so the same-name shape cannot produce the record a
foreign build leaves.

## Limits

* The provisioning check reads `.env` — this fleet's provisioning shape. A
  fleet that sets `CARGO_TARGET_DIR` elsewhere gets the artifact scan but a
  `InCheckoutDefault` declaration; the per-session live gate still covers
  the running session's real environment. An expression the scan cannot
  expand is a printed note, not a verdict.
* `--target-dir` passed per-invocation and a session whose environment
  drifted after provisioning (the `session_move` case the task suspects)
  are not visible to a static scan — they are exactly what the artifact
  records catch on the next scan anywhere in the fleet.
* A sibling checkout whose `target/` already holds foreign records makes
  every agent's `cargo test --workspace` fail until it is cleaned. That is
  the point of the gate; the finding names the directory and the fix.

## Reviewer procedure

1. `cargo run -p cs_xtask -- verify-target-dirs` — the live fleet report.
2. `cargo test -p cs_xtask --locked --test cs_xtask_tests -- accept_t1265_ --include-ignored`
   — seven tests, all exercising `scan_fleet`/`verify-target-dirs` or the
   real fleet; without the scan they do not compile.
3. Manual spot-check: build any sibling workspace with
   `CARGO_TARGET_DIR=<this checkout>/target`, then rerun
   `verify-target-dirs` — exit 1 naming both checkouts.
