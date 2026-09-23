# F28: Rockets, special ordnance, counter-effects, and nitro

**Target:** 2000 PC original-data reimplementation. **Initial status:** not implemented; not original-verified.
**Prerequisite features:** F14, F23, F24, F27, F29.
**Owner paths:** `crates/cs_sim/src/weapons/ordnance.rs`; `crates/cs_content/src/ordnance.rs`; `crates/cs_app/src/ordnance.rs`; `tests/`.
**Shared contract:** [FLIGHT-PHYSICS](../docs/contracts/FLIGHT-PHYSICS.md).

## Deliverable and interfaces

Implement a typed ordnance behavior registry for every original hardpoint component. Discovered families may include direct explosive, proximity/flak, guided or tagged-target weapons, area-denial/engine effects, aerial torpedoes and nitro boosters. The actual catalog and original observations determine which are present and their rules.

## Non-negotiable behavior

1. Each definition declares launch geometry, stack capacity/weight, arming, fuse, guidance, lifetime, area effect, damage/status channels and media. Do not substitute every rocket with one homing missile.
2. Nitro is not excluded as an Xbox-only feature: the PC manual lists a nitro activation control. Its capacity, tradeoffs and duration still require original evidence.
3. Area effects have bounded lifetimes and stable recipient ids. Damage, choking/stall or marker effects are separate from their visual particles.
4. Guidance cannot track destroyed or invalid targets forever; lost-target behavior is specified. Proximity fuse uses swept relative distance and respects arming conditions.
5. Hardpoint firing order, ammo cycling and incompatible equipment are shared with loadout validation; no unsupported custom plane can bypass the shop through an import.

## Acceptance tests

These are minimum discriminating tests. Add regression cases for every discovered variant. Tests call production code, not parallel test-only implementations. They must be selected with a nonempty task-specific test prefix and must fail when the relevant behavior is removed.

**AC01:** Proximity fuse triggers for a fast near-pass but not before arming.
**AC02:** Guidance loses a target safely on destruction or session change.
**AC03:** A timed engine-status effect expires on the correct simulation tick and resets on restart.
**AC04:** Boost changes thrust/consumption but never directly teleports or scales render dt.

## Bounded implementation slices

### F28-A: Define exhaustive ordnance behavior and effect registry

Dependencies: F14-A, F23-A, F24-A, F27-A, F29-A. Required capabilities: ordinary build/test.

Work only on this stage. Define typed inputs/outputs and a minimal synthetic fixture first; do not jump ahead to a whole runtime.

Required task-test prefix: `accept_f28_a_`. Minimum scenario: Proximity fuse triggers for a fast near-pass but not before arming.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F28-B: Implement direct/proximity/guidance/status/boost mechanisms

Dependencies: F28-A, F14-C, F23-C, F24-C, F27-C, F29-C. Required capabilities: ordinary build/test.

Work only on this stage. Implement the smallest production path that exercises the declared behavior; keep unrelated systems unchanged.

Required task-test prefix: `accept_f28_b_`. Minimum scenario: Guidance loses a target safely on destruction or session change.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F28-C: Integrate hardpoints, UI, AI use and network events

Dependencies: F28-B. Required capabilities: ordinary build/test.

Work only on this stage. Wire the implemented path into its actual producer and consumer; include teardown/retry and error propagation.

Required task-test prefix: `accept_f28_c_`. Minimum scenario: A timed engine-status effect expires on the correct simulation tick and resets on restart.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F28-D: Close original ordnance catalog and test every discovered behavior

Dependencies: F28-C. Required capabilities: retail.

Work only on this stage. Collect actual integration/reference evidence; repair discovered regressions without weakening the specification. Missing capabilities or original data block this stage.

Required task-test prefix: `accept_f28_d_`. Minimum scenario: Boost changes thrust/consumption but never directly teleports or scales render dt.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.

## Evidence and completion

Record the actual input fingerprint and consumer trace. Synthetic fixtures alone cannot certify original-data behavior.

A code/test pass awards at most **checked**. The reviewer must inspect runtime wiring, error paths, stale-state handling and test sensitivity. Data-dependent claims require source span, installation hash and an independent reference/probe. Visual, audio and ordinary-play claims require their respective capabilities. Developer placeholders never satisfy a retail acceptance case.

## Research boundary

Do not import Xbox power-ups, instant special maneuvers or fictional balance values. Any unknown original ordnance behavior blocks its loadouts and the full-catalog gate.

## References

[S13](../docs/research/SOURCES.md)

The requirements and tests are independently authored engineering designs. Source labels distinguish observed leads from measured original semantics; consult the evidence ledger before changing gameplay values.
