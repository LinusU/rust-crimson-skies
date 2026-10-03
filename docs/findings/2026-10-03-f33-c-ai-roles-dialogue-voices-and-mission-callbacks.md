# F33-C: AI roles, dialogue voices and mission callbacks

Date: 2026-10-03. Task: F33-C "Connect AI roles, dialogue voices and mission
callbacks" (`specs/F33-wingmates-factions-neutral-traffic-and-pilot-identity.md`,
section `### F33-C`). Shared contract:
`docs/contracts/STATE-TRANSACTIONS.md`. Capabilities used: ordinary
build/test only (no `CS_GAME_DIR` read, no `gpu`/`audio`, no evidence report
required).

## Files and the one observable failure (listed before editing)

- `crates/cs_sim/src/allies.rs`: the runtime **identity lifecycle**. New
  `AllyStatus` (the five F29 `LifecycleKind`s seen from the identity record),
  `AllyEventKind` and `AllyEvent` (the mission callback, carrying the pilot's
  authored voice), `AllyRole` (the authored AI role: wingmate slot, neutral
  index or unassigned), `AlliesError::{DuplicateLifecycle, ActorClosed}`,
  `classify_event` and the new `AlliesRoster` transactions
  `record_lifecycle`, `register_with_role`, and the gates `may_fire` /
  `is_acting` / `status` / `is_terminal` / `role_of` / `wingmate_slot_of`.
- `crates/cs_app/src/roster.rs`: the **consumer seam** —
  `apply_ally_lifecycle` (one actor) and `apply_roster_lifecycle` (the whole
  roster) feed the authoritative F29 `DamageResolver` lifecycle into the
  identity record, emit an `AllyEvent` mission callback plus an
  authored-voice `DialogueCue`, and take a grounded actor's mounts out of the
  `FireResolver` firing gate. Every refusal is named in `AllyConsumerLog`.
- `crates/cs_app/tests/accept_f33_c_ally_lifecycle.rs` (new): the cross-crate
  AC03 scenario and the role/callback/retry cases.
- In-source `#[cfg(test)]` tests in `crates/cs_sim/src/allies.rs`, named
  `accept_f33_c_*` and selected by the same filter.
- `crates/cs_sim/src/lib.rs`, `crates/cs_app/src/lib.rs` (wiring only): the
  module documentation paragraphs.
- This file.

**One observable failure:** before this stage an allied actor's destruction
lived only in the F29 damage ledger and (for weapon mounts) in the F29-C
per-part gate. A lethal hit on a non-mount node — the engine, the hull, or a
scripted destruction — left every gun enabled, so an ally destroyed while a
cutscene had the simulation paused could still fire from its now-stale actor.
`accept_f33_c_an_ally_killed_during_a_cutscene_cannot_later_fire` drives the
production path end to end (declared roster → `open_roster` → registered
wingmate → lowered `DamageResolver` records the death → `apply_roster_lifecycle`
→ watched `FireResolver`): if the consumer seam does not ground the actor's
guns, the later `FireIntent` is accepted instead of refused with
`MountDisabled` and the test fails at the shot.

## Semantics defined at this stage

- **The mission callback is typed, not a bare kill.** `record_lifecycle`
  turns one authoritative F29 `LifecycleKind` into an `AllyEvent` whose
  `AllyEventKind` separates a lost wingmate, a protected-neutral loss (a
  mission event, never a kill), an ordinary ally loss, a capture and a
  bailout — the distinctions `docs/contracts/SCRIPT-MISSION.md` requires.
- **Only destruction, despawn and mission removal end action.** A captured or
  bailed-out actor is still a physical object and may still act
  (`AllyStatus::ends_action`), mirroring `cs_sim::targeting`'s
  `ends_targeting` split. The firing-gate half of the consumer seam grounds
  every mount only for the ending states.
- **Once-per-kind and terminal closure.** A replayed transition is refused
  (`DuplicateLifecycle`) rather than re-emitted; a despawn or mission removal
  closes the record (`ActorClosed`) so a late event of a previous generation
  can never resurrect the actor, exactly as the F29 resolver does.
- **The voice is the authored catalog id.** `AllyEvent.voice` is the record's
  `voice`, and the app seam emits a `DialogueCue` only when one was authored;
  a pilot with no authored voice produces no cue rather than a random line
  (non-negotiable 5).
- **The pass is state-driven and convergent.** Like
  `cs_app::damage::apply_damage_state`, the seam reads the resolver's
  lifecycle set and records only the kinds the roster has not seen, so
  running it during a paused cutscene, after the scene resumes, or twice in a
  row is convergent and a death that happened while the simulation was paused
  cannot be missed.
- **Retry is a new generation.** The roster is per session; a retry opened
  with a new session carries no death, role or gate state from the failed
  world (`STATE-TRANSACTIONS`).
- **AI roles are authored data.** `register_with_role` records the role the
  mission spawned the actor under (wingmate slot, neutral index, none), never
  inferred from the mesh or paint; the role decides whether a loss is a
  wingmate loss or an ordinary/neutral loss.

## Unknowns recorded (not guessed)

- **No measured mapping from the session `ActorId` to the script actor id.**
  `cs_script::runtime::MissionFacts` is keyed by `cs_script::ir::ActorId`
  (a bare `u32`), while this stage's callbacks are keyed by the
  session-qualified `cs_types::net`-backed `cs_sim::damage::ActorId`
  (`{ session, serial }`). No measured correspondence between the two exists
  on this branch, so no `MissionFacts` key was fabricated; the callback is the
  typed `AllyEvent` and the script binding stays a recorded unknown.
- Whether the original 2000 PC game postponed or discarded a death that
  happened during a cutscene, whether it played a wingmate/neutral loss line
  in response, which lines, and whether a captured or bailed-out aircraft
  could still fire are **unmeasured**. F40's cinematic inventory and F33-D's
  retail roster are where those are recovered; the retail stage is F33-D.
- The **neutral-role producer** is not wired to a spawn path here:
  `LoweredRoster.neutral_traffic` is lowered and `register_with_role` can
  record the role, but no F33 spawn system places neutral actors in the
  world yet (AC04's runtime half is F33-D). This stage does not invent one.

## Not claimed

No original-data verification, no ECS schedule/plugin, no AI command UI, no
dialogue playback/audio and no retail roster. The task awards at most
**checked** status.

## Test counts

7 `accept_f33_c_*` tests, all passing:
`cargo test --workspace --locked -- accept_f33_c_ --include-ignored`

- 3 unit tests in `crates/cs_sim/src/allies.rs` (classification and gating,
  terminal closure, role recording).
- 4 integration tests in
  `crates/cs_app/tests/accept_f33_c_ally_lifecycle.rs` (the AC03 cutscene
  scenario, capture/bailout not grounding, foreign/unknown refusals, retry).

## Sensitivity probe (run and reverted; no probe committed)

1. The firing-gate half of `apply_ally_lifecycle` was disabled
   (`if false && !roster.is_acting(&actor)`):
   `cargo test -p cs_app --test accept_f33_c_ally_lifecycle --locked`
   failed `accept_f33_c_an_ally_killed_during_a_cutscene_cannot_later_fire`
   (3 passed, 1 failed) at the `mounts_disabled` assertion — the later shot
   would have been accepted.
2. The wingmate branch of `classify_event` was disabled
   (`if false && matches!(role, AllyRole::Wingmate(_))`):
   `cargo test -p cs_sim --lib --locked -- accept_f33_c_` failed
   `accept_f33_c_record_lifecycle_classifies_and_gates_action` (2 passed, 1
   failed) — the loss was misclassified as an ordinary ally loss.

Both probes were reverted; the committed tree is the green one.

## Commands run (all exit 0)

- `cargo fmt --all -- --check` → `FMT_OK`
- `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`
  → 0 warnings
- `cargo test --workspace --locked` → all targets pass, 0 failures
- `cargo test --workspace --locked -- accept_f33_c_ --include-ignored`
  → 7 passed, 0 failed (3 in `cs_sim`'s lib, 4 in the `cs_app` integration
  target)
