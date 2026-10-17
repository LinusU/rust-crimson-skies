# F53-FU6: the reopen verdict and the block-or-proceed policy

Date: 2026-10-17. Task: #768 / F53-FU6 "F53-D follow-up: feed
`save_dependency` into the host reopen path and decide block-vs-proceed".
Spec: `specs/F53-mod-mounts-custom-content-and-compatibility-signatures.md`,
AC04 and stage `### F53-D`. Shared contract:
`docs/contracts/IDENTITY-CONTENT.md`. Capabilities used: ordinary build/test
only (no `CS_GAME_DIR` read, no evidence report required). The input this
builds on is
`docs/findings/2026-10-08-f53-d-source-protection-mod-isolation-and-reproducible-load-order.md`.

## Files and the one observable failure (listed before editing)

- `crates/cs_app/src/ui/mods.rs` (F53's `cs_app` owner path): the host half of
  the reopen check — `SaveReopenDecision`, `SaveReopenVerdict`,
  `save_reopen_verdict` and the `ModsNotice::SaveDependency` projection.
- `crates/cs_app/tests/accept_f53_fu6_dependent_save_reopen.rs` (new): the
  AC04 scenario through `cs_app::profile::ProfileSession` plus the policy's
  other two arms.
- This file.

**One observable failure:** a save written while the blueprint mod is enabled
records `fingerprint.content` (the announced signature, folded) and owns
`blueprint/synthetic.zephyr`; disabling the mod and reopening the save leaves
the host with no opinion at all — it either serves bytes the save was not
written against or treats the path as a fresh stock profile. Before this
stage no code in `crates/cs_app/src` called
`cs_content::mods::{save_dependency, provided_ids, content_signature}` on a
reopened document (`grep` finds none). After it, the host runs the check and
the dependency is *refused and named*.

Test count: 2 `accept_f53_fu6_*` tests in
`crates/cs_app/tests/accept_f53_fu6_dependent_save_reopen.rs`, selected by
`cargo test --workspace --locked -- accept_f53_fu6_ --include-ignored`
(exit 0, 2 executed, both passing). `crates/cs_app/tests/` is this task's
`tests/` owner entry under the precedent F53-D recorded (F48-D and FU3 landed
`crates/cs_app/tests/accept_*` files under the same entry). No test is
ignored: nothing here needs original data.

## The decision: refuse on missing content, warn-and-proceed on a differing mark

`save_dependency` answers three things (F53-D): the recorded mark versus the
announced signature (`Matches` / `Unrecorded` / `Differs { recorded,
announced }`), and the blueprints the save owns that this session cannot
serve. The host's policy turns that answer into one decision,
[`SaveReopenDecision`]:

| report | decision | stable code |
| --- | --- | --- |
| any unprovided owned blueprint | `Refuse` | `save_content_unprovided` |
| `Differs`, everything provided | `OpenMarked` | `save_content_differs` |
| `Matches` / `Unrecorded`, everything provided | `Open` | `save_content_matches` |

The decision is a function of the report alone — the same save under the same
content always decides the same way — and the report's own
`diagnostic_lines()` are carried in every arm, so the operator sees exactly
what `save_dependency` found. `SaveReopenVerdict::notice()` projects a
non-empty verdict into `ModsNotice::SaveDependency { decision, lines }`; the
stable code is `decision.code()` and `lines` is the report verbatim.

**Why missing content refuses.** A blueprint the save owns that this session
cannot serve is a *named hard dependency*: the campaign state it belongs to
will reference content that does not exist. Serving the save anyway is a
downgrade in fact, whatever warning precedes it, and the failure would
surface later, far from the decision. Refusing keeps the failure at the point
where the operator can act on it, and costs nothing: the document is never
touched (F53-D established that this module has no write path into the save),
so re-enabling the mod — or opening the save in the session it was written
under — is always still possible. This is the same fail-closed stance F53
takes everywhere else (non-negotiable 2 refuses a mount rather than guessing;
F53-B refuses a symlink or an unmeasured format rather than resolving it).

**Why a differing mark alone warns and proceeds.** The recorded mark is a
designed 8-byte fold of the announced signature, and the signature covers the
plan and the measured payload bytes — including *cosmetic* mods, which
deliberately do not mark sessions (`MountedMods::marks_sessions` is false for
a cosmetic-only plan). A save written under a cosmetic-only set is
`ProfileKind::Modded`-marked and reads `Differs` under stock content, yet
every blueprint it owns is served. Refusing that would make a purely cosmetic
toggle a load failure the content cannot justify, and AC04 does not ask for
it: what it asks for is a *visible report*, which the warning is. Proceeding
is therefore "not silent", and the operator can still stop.

**Why not the single hard rule "any unsatisfied report refuses".** That rule
would refuse the cosmetic case above for no content reason. **Why not "always
warn and proceed".** That would serve a save whose blueprint set is
incomplete and call it a warning — the exact silent downgrade AC04 forbids.

## What shipped

- `crates/cs_app/src/ui/mods.rs`: `save_reopen_verdict(document, request,
  base_fingerprint, mounted)` runs `save_dependency` with
  `content_signature(mounted, base_fingerprint)` and
  `provided_ids(request, mounted)` — the *same* triple `lobby_compatibility`
  announces — and returns the decided `SaveReopenVerdict`.
  `SaveReopenVerdict::notice()` is the screen projection. Nothing writes.
- `crates/cs_app/tests/accept_f53_fu6_dependent_save_reopen.rs`: test one is
  the AC04 scenario — the blueprint mod is mounted through
  `ui::mods::mount_selection`, a marked save owning `synthetic.zephyr` is
  written through `ProfileSession::commit_with`, the mod is disabled, the save
  is reopened through `ProfileSession::open` (which auto-selects it from the
  active pointer, reads the whole revision and reports no recovery), and the
  host verdict is `Refuse` naming both folds and the unprovided blueprint,
  with the population byte-for-byte what the write left and the re-enabled
  mount reproducing the recorded signature exactly. Test two pins the other
  two arms: `Differs` with everything provided warns and proceeds; a matching
  mark opens silently; and a matching mark with an unprovided blueprint still
  refuses — the check keys on content the session can serve, not on the mark
  alone.

## The caller that is not wired (owner-path question), and what blocks

The reopen path itself still does not call the check. F53's owner paths are
`crates/cs_content/src/mods/`, `crates/cs_assets/src/mods.rs`,
`crates/cs_app/src/ui/mods.rs` and `tests/` (spec F53 line 5). The host code
that actually opens a save is:

- `crates/cs_app/src/profile.rs` — `ProfileSession::open` (which selects the
  active profile through `load_selected`), `ProfileSession::select` and the
  session's `commit_with`; F48's owner path, not F53's;
- `crates/cs_app/src/ui/front_end/flow.rs::open_profile`, the only consumer
  of those, at `FlowSetup`'s declared base/origin/kind; F45's owner path
  (`crates/cs_app/src/ui/front_end/`), not F53's.

There is no save-reopen/load screen in `crates/cs_app/src/ui/` (the front-end
`Loading` screen is the mission-load screen and never opens a profile
document); the mods module opens no save. `ProfileSession` also carries no
mount or selection today, so wiring it needs a place for the announced
content configuration as well as the call — a second `profile.rs` change.

The task's owner-path note says to record this and `block_task` for an owner
path decision rather than editing a caller outside F53's owner paths, so that
is what the implementing session did (see the task's blocked reason; nothing
was edited outside the owner paths above). Affected content: a dependent save
reopened through the shipped game is not yet refused or warned by the reopen
path itself — the decision and the report are verified at the host-module
boundary, driven by the real `ProfileSession` reopen. **No fidelity or release
claim may rest on the reopen path blocking until a caller is wired.** Resolving
task: F53-FU6 (#768) itself once the owner names the path; the sibling
F53-FU5-SAVE-MARK (#767) already holds `crates/cs_app/src/profile.rs` for the
mark half, and the two must be sequenced.

## Unknowns and limitations

- The eight-byte fold, the fingerprint name and the population choice are the
  designed vocabulary F53-D recorded; the decision inherits all of F53-D's
  unknowns and adds none. In particular the check still covers only
  `document.blueprints`, the one owned-content list the F48-A schema records.
- The base installation fingerprint is still caller-supplied
  (`MountEnvironment::base_fingerprint`, F53-C's limitation); the tests use a
  designed constant.
- `ModsNotice` carries no widget layout or input — as with `ModsView`, the
  front-end stage owns presentation; the notice carries the stable code and
  the designed English fallback only.

## Checks run

- `cargo fmt --all -- --check` — exit 0.
- `cargo clippy --workspace --all-targets --all-features --locked -- -D
  warnings` — exit 0.
- `cargo test --workspace --locked` — exit 0, 4967 passed, 0 failed, 969
  ignored.
- `cargo test --workspace --locked -- accept_f53_fu6_ --include-ignored` —
  exit 0, 2 tests executed, both passing.

## Sensitivity probes (applied, run and reverted; tree left byte-identical)

- Replacing the body of `save_reopen_verdict` with a fixed
  `SaveReopenVerdict { Open, no lines }` (the wiring removed, the signatures
  kept) fails **both** `accept_f53_fu6_*` tests (`Open` where `Refuse` and
  `OpenMarked` are asserted). The tests are sensitive to the
  `save_dependency` call itself, not to the types around it.
- The file was restored with `cp` from a pre-edit copy; `git diff` showed only
  the intended change afterwards and the selection was rerun green.
