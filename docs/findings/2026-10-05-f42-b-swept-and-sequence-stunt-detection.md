# F42-B: swept and sequence stunt detection

Date: 2026-10-05. Task: #177 (F42-B). Sheet: `specs/F42-stunts-fame-photos-and-optional-achievement-events.md`.
Capabilities: ordinary build/test. Nothing here is `verified_original`.

## What was built

* `cs_content::stunts`: `StuntDraft::follow_on_gates` / `StuntDefinition::follow_on_gates()`:
  the gates flown after the first, validated like the first (usable normal,
  positive extents, clearance achievable in each hole).
* `cs_app::stunts::lower_stunt` lowers them into `StuntRuleDraft::follow_on`; each shares the stunt's
  direction and clearance thresholds.
* `cs_sim::stunts`: `StuntRule::gate_count` / `gate_rule`; `StuntBook` keeps one cursor per rule.
  An in-order gate gives `TraversalOutcome::Advanced` and pays nothing; the last gate takes the existing
  reward-identity path, so one-time/repeatable policy and retry seeding apply unchanged. `StuntSampler`
  / `PoseChange` turn per-tick world positions into the swept/rebased/teleport movement the book judges;
  the first sample has no path and cannot earn.

## Designed rules (unmeasured, not original behaviour)

* Cursor resets on a teleport/discontinuous sample and after a completion.
* Flying the **first** gate again mid-sequence restarts at 1 rather than being refused.
* No time limit between gates: none is measured. Not invented.
* The original's spelling of a gate sequence, and whether it shares one thresholds pair, is unknown;
  F42-D must measure it. All fixtures are `SyntheticFixture` / `Reconstructed`.

## Tests

`crates/cs_app/tests/accept_f42_b_sequence_detection.rs` (6 tests). Mutation: removing the cursor reset
on discontinuity fails the teleport test.
