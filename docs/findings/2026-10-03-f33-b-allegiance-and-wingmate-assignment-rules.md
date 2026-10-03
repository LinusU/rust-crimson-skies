# F33-B: Allegiance and wingmate assignment rules

Date: 2026-10-03. Task: F33-B "Implement allegiance and wingmate assignment
rules" (`specs/F33-wingmates-factions-neutral-traffic-and-pilot-identity.md`,
section `### F33-B`). Shared contract:
`docs/contracts/STATE-TRANSACTIONS.md`. Capabilities used: ordinary
build/test only (no `CS_GAME_DIR` read, no evidence report required).

## Files and the one observable failure (listed before editing)

- `crates/cs_sim/src/allies.rs`: the briefing/assignment **rules** that write
  through the F33-A store — `WingmateChoice` (the selected airframe and
  loadout for one slot), `BriefingPlan` (one choice per slot),
  `BriefingError`, the pure `briefed_wingmates` rule, and the new
  `AlliesRoster` transactions: `set_player_faction`/`player_faction`,
  `reset_wingmates` (the retry/reset transaction), `set_wingmate_loadout`
  (the in-session rearm) and `register_wingmate` (the allegiance rule that
  commits a spawned wingmate to the mission's player faction with the
  assigned pilot, geometry, voice and survivability).
- `crates/cs_app/src/roster.rs`: `open_roster`, the production entry that
  opens a session's `AlliesRoster` from a lowered mission roster and the
  player's briefing plan — committing the player faction and deriving the
  wingmate set from the **authored** records every time, so a retry rebuilds
  rather than carrying the failed world's state.
- `crates/cs_app/tests/accept_f33_b_briefing_and_wingmates.rs` (new): the
  cross-crate AC02 scenario.
- In-source `#[cfg(test)]` tests in both owner source files, named
  `accept_f33_b_*` and selected by the same filter.
- `crates/cs_sim/src/lib.rs`, `crates/cs_app/src/lib.rs` (wiring only):
  the module documentation paragraphs.
- This file.

**One observable failure:** before this stage `AlliesRoster` had no rule that
turns a briefing selection into assignments. `assign_wingmates` only stores a
caller-supplied set, so nothing connects the player's per-slot briefing choice
to a slot's loadout, and — because a caller could hand a retry the assignments
of the just-failed world — the failed mission's rearm or capture survived the
retry. The AC02 scenario
`accept_f33_b_a_retry_restores_the_briefing_loadout_and_allegiance` drives
`open_roster` on a new session generation after an in-session rearm and a
capture: if the rule reads a stored, mutated assignment instead of the
authored base plus the plan, the retry's loadout is the failed rearm and the
test fails. The design makes the assignment a **pure** function of the
authored set and the plan, so the mutated copy is unreachable by construction.

## Semantics defined at this stage

- **The briefing selects equipment, not identity.** `WingmateChoice` carries
  the airframe and the loadout. The rule copies pilot, voice and
  survivability from the authored `WingmateAssignment` unchanged, so a
  selected plane never re-paints an enemy/friendly status (non-negotiable 1)
  and a briefing choice never replaces an authored voice (non-negotiable 5).
- **A selection for an unauthored slot is refused.** `briefed_wingmates`
  returns `BriefingError::UnknownWingmateSlot` rather than inventing a
  wingmate; there is no authored pilot, voice or survivability to carry.
- **Allegiance is the player-faction commitment.** `register_wingmate`
  commits a spawned wingmate actor to the session's player faction while its
  geometry stays the assigned airframe's own field, so a wingmate's side and
  its airframe remain separate as in F33-A. A session that declares no player
  faction refuses (`BriefingError::NoPlayerFaction`) rather than guessing one.
  Faction *relations* (hostility/alliance/neutral protection) remain F30's
  versioned `AllegianceTable`; this stage commits the side an actor is on and
  leaves relation changes to the capture transaction (`AlliesRoster::capture`)
  and the F30 scripted-declare path.
- **Retry restores the authored state.** `reset_wingmates` re-derives the
  whole set from the authored assignments and the plan, discarding an
  in-session `set_wingmate_loadout`; `open_roster` is what a new session
  generation runs, so a retry restores the briefing loadout and the authored
  allegiance rather than a mutated copy of the just-failed world
  (`STATE-TRANSACTIONS`). The briefing *selection* is a session input and is
  re-applied identically across retries.

## Unknowns recorded (not guessed)

- Whether the original 2000 PC game lets the player change a wingmate's
  aircraft or loadout at the briefing/flight check, what options it offers,
  whether the selection persists across a retry and whether a wingmate's
  equipment can change mid-mission are **unmeasured**. F13/F14 recover no
  briefing or loadout records; the retail stage is F33-D and the briefing
  screens are F45. Every type, rule and fixture here is newly authored project
  design carrying designed/synthetic provenance.
- The option set a selection must be drawn from (which aircraft and loadouts
  the flight check may offer) belongs to the campaign/profile side
  (F43-C, F45-C), not to this stage: this stage's rule validates that the
  selected **slot** was authored, and treats the aircraft and loadout as
  already-namespaced content ids.

## Not claimed

No original-data verification, no ECS/AI wiring, no briefing UI, no dialogue
playback and no retail roster. The task awards at most **checked** status.

## Test counts

9 `accept_f33_b_*` tests, all passing:
`cargo test --workspace --locked -- accept_f33_b_ --include-ignored`

- 5 unit tests in `crates/cs_sim/src/allies.rs` (the rules and the roster
  transactions).
- 4 integration tests in
  `crates/cs_app/tests/accept_f33_b_briefing_and_wingmates.rs` (the AC02
  scenario end to end through `cs_content` → `cs_app::roster::open_roster` →
  `cs_sim::allies`).

## Sensitivity probe (run and reverted; no probe committed)

`briefed_wingmates` was changed to ignore the plan (return the authored set
unchanged) while the rest stayed intact:
`cargo test -p cs_sim --locked -- accept_f33_b_` then failed
`accept_f33_b_briefing_selects_equipment_and_keeps_authored_identity` and
`accept_f33_b_reset_restores_the_briefing_after_an_in_session_rearm` (3
passed, 2 failed), and the integration AC02 tests read the wrong loadout. The
probe was reverted; the committed tree is the green one.

## Commands run (all exit 0)

- `cargo fmt --all -- --check` → `FMT_OK`
- `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`
  → 0 warnings
- `cargo test --workspace --locked` → all targets pass, 0 failures
- `cargo test --workspace --locked -- accept_f33_b_ --include-ignored`
  → 9 passed, 0 failed (5 in `cs_sim`'s lib, 4 in the `cs_app` integration
  target)
