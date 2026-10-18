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

Source of the policy: the owner's decision on #768 (Linus / Claude M3,
2026-10-17 11:15 local) — `save_content_unprovided` refuses,
`save_content_differs` warns and proceeds, a match opens silently — and the
owner extended the owner paths by exactly `crates/cs_app/src/profile.rs`,
sequenced after #767.

- `crates/cs_app/src/ui/mods.rs`: `save_reopen_verdict_for(document,
  announced, provided)` runs `save_dependency` and returns the decided
  `SaveReopenVerdict`; `save_reopen_verdict(document, request,
  base_fingerprint, mounted)` computes the same triple `lobby_compatibility`
  announces and delegates. `SaveReopenVerdict::notice()` is the screen
  projection. Nothing writes.
- `crates/cs_app/src/profile.rs`: `SessionContent { signature, provided }` is
  the one declaration (the seam #767 started with `with_announced_signature`;
  there is no second parallel configuration). `ProfileSession::
  open_with_content` takes it, and both `open` (the active profile) and
  `select` run the verdict through one private `load_checked`. A refused save
  is not selected: `open` succeeds with nothing selected and names the refusal
  in `warnings()`; `select` returns `SessionError::SaveRefused { profile,
  code, lines }` and leaves the selection as it was. A warned save is selected
  and the verdict stays in `reopen_verdict()`. A session that declares
  nothing (`open`, `open_sandbox`) behaves exactly as before and
  `reopen_verdict()` is `None` — the mark stays `Unrecorded`, never forged.
- `crates/cs_app/tests/accept_f53_fu6_dependent_save_reopen.rs` (in
  `suite_a`): the AC04 scenario through `ProfileSession::open_with_content`
  (mod disabled; refusal naming both folds and `synthetic.zephyr`; `select`
  refuses by name; the population is byte-for-byte what the write left; the
  re-enabled mount reproduces the signature and opens silently); the warn arm
  through the session (selected, warning kept, `open` writes nothing, the
  document is never rewritten); and the free-function arms.

## The caller that is still not wired

`FrontEndFlow::open_profile` (`crates/cs_app/src/ui/front_end/flow.rs`, F45)
still calls `ProfileSession::open` with no content, because `FlowSetup`
declares no mount. The owner ruled not to edit it here; wiring the declared
mount into `FlowSetup` is filed as a follow-up. Affected content: a save
reopened through the shipped front end is not yet checked. **No fidelity or
release claim may rest on the shipped reopen path refusing until that task
lands.**

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
- `cargo test --locked -p cs_app` — exit 0 (all binaries ok).
- `cargo test --locked -p cs_app -- accept_f53_fu6_ --include-ignored` —
  exit 0, 3 tests executed, all passing.

## Sensitivity probe (applied, run and reverted)

Changing `ProfileSession::open`'s `load_checked` call back to
`load_selected` (removing the open-time check) fails two of the three
`accept_f53_fu6_*` tests (the AC04 scenario and the warn arm); the third
exercises the free function. Restored afterwards.
