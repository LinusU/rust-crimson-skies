# F36: Docking, passenger pickups, boarding, and plane swaps

**Target:** 2000 PC original-data reimplementation. **Initial status:** not implemented; not original-verified.
**Prerequisite features:** F22, F23, F29, F31, F33, F34, F35.
**Owner paths:** `crates/cs_sim/src/interaction/`; `crates/cs_content/src/interaction.rs`; `crates/cs_app/src/interaction.rs`; `tests/`.
**Shared contract:** [STATE-TRANSACTIONS](../docs/contracts/STATE-TRANSACTIONS.md).

## Deliverable and interfaces

Interaction transitions are explicit state machines: Available, Approaching, Eligible, Latching, Attached/Transferring, Released, Completed or Aborted. The transaction binds stable actor ids and a mission/session generation. Docking, passenger collection, boarding and changing player aircraft share infrastructure but retain distinct eligibility and effects.

## Non-negotiable behavior

1. Eligibility uses swept position, orientation, relative velocity and mission authorization. A single radius test is insufficient for moving docking hooks.
2. Exactly one system owns pose/control during latch and release. Transfer velocity, pilot, inventory and camera bindings through a declared per-transition policy.
3. Do not complete a mission merely because any zeppelin was approached. The active objective specifies which interaction and semantic completion event is required.
4. A destroyed target, canceled cinematic, pause, retry or disconnect aborts cleanly without duplicate pilots or cargo.
5. Airframe swaps can occur during a mission. Input, HUD, spyglass, sounds, weapons, damage and networking all rebind to the new actor in one authoritative transition.

## Acceptance tests

These are minimum discriminating tests. Add regression cases for every discovered variant. Tests call production code, not parallel test-only implementations. They must be selected with a nonempty task-specific test prefix and must fail when the relevant behavior is removed.

**AC01:** Fly past the hook too fast or from the wrong direction; interaction does not latch.
**AC02:** Dock on a moving carrier across an origin rebase without false speed.
**AC03:** Destroy the carrier during latch; player control/lifecycle resolves safely.
**AC04:** Transfer to a captured plane exactly once, then retry mission; original starting actor returns.

## Bounded implementation slices

### F36-A: Define interaction state machines and transfer policy

Dependencies: F22-A, F23-A, F29-A, F31-A, F33-A, F34-A, F35-A. Required capabilities: ordinary build/test.

Work only on this stage. Define typed inputs/outputs and a minimal synthetic fixture first; do not jump ahead to a whole runtime.

Required task-test prefix: `accept_f36_a_`. Minimum scenario: Fly past the hook too fast or from the wrong direction; interaction does not latch.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F36-B: Implement moving-frame eligibility and atomic transfer

Dependencies: F36-A, F22-C, F23-C, F29-C, F31-C, F33-C, F34-C, F35-C. Required capabilities: ordinary build/test.

Work only on this stage. Implement the smallest production path that exercises the declared behavior; keep unrelated systems unchanged.

Required task-test prefix: `accept_f36_b_`. Minimum scenario: Dock on a moving carrier across an origin rebase without false speed.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F36-C: Connect docking/pickup/boarding/plane-swap consumers

Dependencies: F36-B. Required capabilities: ordinary build/test.

Work only on this stage. Wire the implemented path into its actual producer and consumer; include teardown/retry and error propagation.

Required task-test prefix: `accept_f36_c_`. Minimum scenario: Destroy the carrier during latch; player control/lifecycle resolves safely.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F36-D: Verify all distinct original interaction patterns with reference runs

Dependencies: F36-C. Required capabilities: retail.

Work only on this stage. Collect actual integration/reference evidence; repair discovered regressions without weakening the specification. Missing capabilities or original data block this stage.

Required task-test prefix: `accept_f36_d_`. Minimum scenario: Transfer to a captured plane exactly once, then retry mission; original starting actor returns.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.

## Evidence and completion

Record the actual input fingerprint and consumer trace. Synthetic fixtures alone cannot certify original-data behavior.

A code/test pass awards at most **checked**. The reviewer must inspect runtime wiring, error paths, stale-state handling and test sensitivity. Data-dependent claims require source span, installation hash and an independent reference/probe. Visual, audio and ordinary-play claims require their respective capabilities. Developer placeholders never satisfy a retail acceptance case.

## Research boundary

No unrecorded compatibility assumptions are allowed.

## References

[S13](../docs/research/SOURCES.md); [S14](../docs/research/SOURCES.md); [S15](../docs/research/SOURCES.md)

The requirements and tests are independently authored engineering designs. Source labels distinguish observed leads from measured original semantics; consult the evidence ledger before changing gameplay values.
