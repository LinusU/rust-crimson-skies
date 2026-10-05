# F42-C: fame, AI observations and scrapbook rewards

Date: 2026-10-05. Task: #178 (F42-C). Sheet: `specs/F42-stunts-fame-photos-and-optional-achievement-events.md`.
Capabilities: ordinary build/test. Nothing here is `verified_original`.

## What was built

`cs_app::stunts::consume_stunt_outcomes` takes the `TraversalOutcome`s a `StuntBook` produced and applies
each `Completed` one to its consumers:

* **Fame:** `FameTally` adds the completion's fame. Application is exactly-once per
  `(session, reward key, tick)`, so a replayed hand-over moves nothing; a mission retry is a new session.
* **Scrapbook:** `ui::scrapbook::record_stunt` notes the stunt fact; the report lists the reward media
  the catalog now shows as unlocked. Media absent from the catalog refuses the whole batch.
* **AI:** each newly applied completion yields a `StuntSighting` (stunt, actor, tick, crossing).

An advance or a refusal applies nothing. The batch is validated before any state changes.
Mission eligibility stays in `lower_mission_stunts` (the book only holds the mission's rules), so two
missions in one world see different stunts.

## Unknown, not guessed

* **AI reaction to a stunt is unmeasured** (see t465: whether an AI may even earn one is runtime
  behaviour no file records). The sighting carries the observation and decides no reaction; no pursuit,
  morale or targeting change is applied. Resolving task: F42-D (needs an original run).
* **Cash** (`reward.cash_minor`) is not applied; it belongs to the F43 economy.
* Fame total, per-completion identity and the atomic-batch policy are designed, not original rules.
  All fixtures are `SyntheticFixture`/reconstructed.

## Tests

`crates/cs_app/tests/accept_f42_c_fame_ai_scrapbook.rs` (5 tests): same world / other mission with a
different eligible set, replayed hand-over, retry (one-time vs repeatable), missing media atomicity,
refusals pay nothing.
