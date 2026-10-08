# F53-D: Source protection, mod isolation and reproducible load order

Date: 2026-10-08. Task: F53-D "Verify source protection, mod isolation and
reproducible load order"
(`specs/F53-mod-mounts-custom-content-and-compatibility-signatures.md`,
section `### F53-D`). Shared contract:
`docs/contracts/IDENTITY-CONTENT.md`.
Capabilities used: ordinary build/test only (no `CS_GAME_DIR` read, no
evidence report required).

## Files and the one observable failure (listed before editing)

- `crates/cs_content/src/mods/save.rs` (new): the save half of the mod
  system — `SAVE_CONTENT_FINGERPRINT`, `signature_fingerprint`,
  `save_fingerprint`, `mark_save_document`, `save_population`,
  `provided_ids`, `SaveSignatureStatus`, `SaveDependencyReport` and
  `save_dependency`, plus its `accept_f53_d_*` unit tests.
- `crates/cs_content/src/mods/mod.rs`: `mod save;`, the re-exports, the new
  `synthetic_blueprint_mod()` fixture and the module-header paragraph for
  the F53-D half.
- `crates/cs_content/tests/accept_f53_d_mod_isolation.rs` (new): the
  session/export-level verification — read-only sources, the export
  guard, opt-in isolation and reproducible load order.
- `crates/cs_app/tests/accept_f53_d_dependent_save.rs` (new): the AC04
  scenario through the consumer that actually reopens saves,
  `cs_app::profile::ProfileSession`.
- This file.

**One observable failure:** write a save while `synthetic.zephyr-blueprint`
is enabled — the save records `fingerprint.content` (the session's
announced content signature, folded) and owns `blueprint/synthetic.zephyr`
— then disable the mod and reopen the save under the stock signature. A
session that cannot name what changed either silently treats the save as a
stock save (the destructive fallback the acceptance criterion forbids: the
mod-added blueprint is dropped, or the whole save is refused as corrupt)
or serves bytes the save was not written against. The implemented check
reports the truth instead: `SaveSignatureStatus::Differs` naming the
recorded and announced folds, `unprovided_blueprints` naming
`blueprint/synthetic.zephyr`, the reopened document byte-identical to the
one written, and **nothing written back** — no fallback, no repair, no
new revision.

Test count: 5 `accept_f53_d_*` tests — 2 unit tests in
`crates/cs_content/src/mods/save.rs`, 2 in
`crates/cs_content/tests/accept_f53_d_mod_isolation.rs`, 1 in
`crates/cs_app/tests/accept_f53_d_dependent_save.rs` — selected by
`cargo test --workspace --locked -- accept_f53_d_ --include-ignored`
(exit 0, 5 executed, all passing). The crate `tests/` directories are
this task's `tests/` owner path under repository precedent (F48-D landed
`crates/cs_content/tests/accept_f48_d_crash_recovery_matrix.rs` and
`crates/cs_app/tests/accept_f48_d_crash_recovery_matrix.rs` under the
same owner entry); the save-half unit tests stay inside the owner file
as in F53-A/B/C. No test is ignored: nothing here needs `CS_GAME_DIR`.

## What this stage implements

- **The mark — `mark_save_document` / `save_fingerprint` /
  `signature_fingerprint`.** A session writes the content signature it
  announces (`content_signature`: the base fingerprint stock, the mounted
  signature modded) into the save's `fingerprint.content` entry. The F48-A
  field is eight bytes and the signature is thirty-two, so the entry
  carries the signature's first eight bytes big-endian — a designed fold,
  a record to check *against*, never a second hash of the content. The
  mark is idempotent (the schema refuses a repeated fingerprint name, so
  an earlier `content` entry is replaced), and a stock session marks the
  base fingerprint it announces, so "claims nothing" and "claims stock"
  are told apart on reopen.
- **The population — `save_population`.** Any mounted set, cosmetic or
  not, writes into `ProfileKind::Modded`, because a save recorded under
  bytes the installation does not provide is not a production save; a
  session with no mounted set writes `Production`. The F48 library
  enforces the wall (`LibraryError::ForeignDocument`) and the unit test
  exercises it both directions: a modded document refused on write into a
  synthetic library, and a modded slot planted under a synthetic library
  refused on read rather than adopted.
- **The check — `save_dependency` → `SaveDependencyReport`.** On reopen,
  the recorded mark is compared against the fold of what this session
  announces (`Matches` / `Unrecorded` / `Differs { recorded, announced }`)
  and every blueprint the document owns is checked against
  `provided_ids` — the base ids plus the targets of *winning* payloads
  only, since a shadowed claim serves nothing. The answer is a report
  (`is_satisfied`, `diagnostic_lines` naming each unsatisfied need), and
  the module holds no write path back to the document: a dependent save
  under a disabled mod is diagnosed, never repaired into something it is
  not. `Unrecorded` satisfies — a save making no content claim cannot
  fail a content check — and the status stays visible in the report.
- **AC04 through the real consumer.** The `cs_app` test mounts the
  blueprint mod through the host's own `mount_selection` path, writes a
  marked save owning `blueprint/synthetic.zephyr` via
  `ProfileSession::commit_with`, disables the mod and reopens under the
  stock announcement: the document reads whole (no warnings, no recovery,
  no revision burned), the report is `Differs` naming both folds plus the
  unprovided blueprint, the stock population shows no trace of the save,
  the population directory is byte-for-byte unchanged afterwards, and
  re-enabling the mod reproduces the recorded signature exactly, so the
  untouched save reports `Matches`.
- **Source protection, verified end to end.** The session-level test
  mounts three mods beside an installation stand-in, resolves base bytes
  and mod bytes from their own mounts, then proves the only write path —
  the `ExportDirectory` — refuses a root inside the installation or
  inside a mod root (`ExportError::RootInsideMount` naming the protected
  mount). A private export outside every source writes each winning
  payload's own bytes (digest-identical to the mount's measurement),
  ships exactly the payloads plus `mod-export.txt`, names the
  base-provided `unresolved-original` dependencies it refuses to copy,
  and contains no installation bytes. After mount, reads and export,
  every mounted tree is byte-for-byte what it was.
- **Isolation and reproducible order.** A `ResolveContext` that never
  opted into the `ModStack` cannot resolve a mod key the opted-in session
  serves — the same mounts answer nothing without the stack. Offering the
  same manifests forward and backward plans identically: the order,
  signature and `mount_to_text` are functions of the set (beta's
  dependency on alpha forces the order, not the input sequence), and a
  contested payload path resolves to the mod later in the plan — served
  bytes are that mod's. Disabling a mod changes the signature and drops
  its added ids from `provided_ids`; re-enabling the exact set reproduces
  the original signature.

## Unknowns, limitations and where they are resolved

- The eight-byte fold is designed vocabulary (F53 "Research boundary":
  nothing is known about mod fields in the original save format). The
  check compares one save's record against this session's announcement —
  it never looks a signature up — so a fold collision is a theoretical
  64-bit birthday matter between *different* content configurations, not
  a lookup ambiguity. `Differs` reporting both folds keeps any such case
  diagnosable rather than silent.
- `unprovided_blueprints` covers `document.blueprints` — the only
  content-id list the F48-A schema records. If the save schema later
  records other owned content ids, this check must grow to cover them;
  there is no hidden fallback that would serve unlisted ids.
- The report is an answer to a caller; whether a load screen blocks or
  proceeds on an unsatisfied save is a host decision a later stage owns.
  This stage supplies the honest report and proves no write path exists
  to abuse.
- **No production caller yet (reviewer-confirmed).** Nothing in this
  build calls `mark_save_document`, `save_population`, `provided_ids` or
  `save_dependency` outside the acceptance tests: the host save write
  path (`crates/cs_app/src/profile.rs`) is not an F53 owner path, no
  load/reopen screen exists yet, and no gameplay run opens a modded
  profile session today (the mod system's own consumers are
  `mount_selection` and `lobby_compatibility`, both from F53-C, and
  neither writes a save).
  Affected content: a save written while a mod set is enabled carries no
  `fingerprint.content` entry in the shipped game, and a dependent save
  reopened through the host is not yet shown this report — the behaviour
  above is verified at the library/consumer boundary, not in a wired
  host path. Resolving tasks: `F53-FU5-SAVE-MARK` (#767, mark the save
  the session announces when a modded run commits a revision) and
  `F53-FU6-SAVE-REOPEN` (#768, feed `save_dependency` into the reopen
  path and decide block-vs-proceed). Until both land, no fidelity or
  release claim may rest on the mark actually being written by the game.
- `MountEnvironment::base_fingerprint` remains caller-supplied (the F53-C
  limitation is unchanged; the tests use a designed constant).

## Checks run

- `cargo fmt --all -- --check` — exit 0.
- `cargo clippy --workspace --all-targets --all-features --locked --
  -D warnings` — exit 0.
- `cargo test --workspace --locked` — exit 0.
- `cargo test --workspace --locked -- accept_f53_d_ --include-ignored` —
  exit 0, 5 tests run, all passing.

### Reviewer's runs (detached review worktree, `bunny-2`)

At `265e9607` (the submitted head, before this review's docs commit):

- `cargo fmt --all -- --check` — exit 0.
- `cargo clippy --workspace --all-targets --all-features --locked --
  -D warnings` — exit 0.
- `cargo test --workspace --locked` — exit 0, 4083 passed, 0 failed.
- `cargo test --workspace --locked -- accept_f53_d_ --include-ignored`
  — exit 0, 5 tests executed (`1 + 2 + 2`), all passing.

Run of record, on the final head this review pushed (full four
checks, because this review added commits; those commits change only
`docs/findings/`, which cargo does not compile):

- `cargo fmt --all -- --check` — exit 0.
- `cargo clippy --workspace --all-targets --all-features --locked --
  -D warnings` — exit 0.
- `cargo test --workspace --locked` — exit 0, 4095 passed, 0 failed.
- `cargo test --workspace --locked -- accept_f53_d_ --include-ignored`
  — exit 0, 5 tests executed, all passing.

- Caveat for the next agent: this session exports
  `CARGO_TARGET_DIR=<bunny-2>/target`, which makes
  `accept_t383_this_worktrees_effective_target_dir_is_per_worktree`
  fail (T383's own message) from a second worktree. Run the suite with
  `CARGO_TARGET_DIR="$PWD/target"` per that test's advice; with the
  shared directory the suite reported 4004 passed / that one
  environment failure, and with a private one it was fully green.

## Sensitivity probes (none committed)

- Removing `save.rs` removes the unit tests' call targets and breaks the
  two integration files' imports, so the save half cannot silently
  regress to a stub.
- If `mark_save_document` wrote nothing, the reopened document is
  `Unrecorded` — satisfied under the stock announcement — and the AC04
  test's `Differs` assertion fails.
- If `provided_ids` ignored mounted winners, the disabled reopen's
  `unprovided_blueprints` is empty and three of the five tests fail.
- If `save_population` answered `Production` for a mounted set, the
  marked save lands in the production population and the interactive
  session's empty-`live` assertion fails.
- If the resolver ignored `context.mods`, the unopted session resolves
  the contested key and the isolation assertion fails; if the plan order
  followed offer order, the forward/backward plan and signature equality
  fails.

### Reviewer's independent sensitivity probes (mutations applied, run and reverted; tree left byte-identical)

- `mark_save_document` replaced by a no-op body: the reopened document
  records nothing, the verdict is `Unrecorded` instead of `Differs`, and
  `accept_f53_d_disabling_a_mod_reopens_a_dependent_save_without_destructive_fallback`
  fails (selection exit 101).
- `provided_ids` made to ignore the mounted winners: the mod-added
  blueprint is unprovided even under its own mod, and the same AC04 test
  fails (selection exit 101).
- Both mutations were reverted with `cp` from a pre-edit copy;
  `git diff --stat` and `git status --short` were empty afterwards and
  the selection was rerun green.
