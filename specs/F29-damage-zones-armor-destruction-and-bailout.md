# F29: Damage zones, armor, destruction, and bailout

**Target:** 2000 PC original-data reimplementation. **Initial status:** not implemented; not original-verified.
**Prerequisite features:** F11, F14, F20, F23.
**Owner paths:** `crates/cs_sim/src/damage/`; `crates/cs_content/src/damage.rs`; `crates/cs_app/src/damage.rs`; `tests/`.
**Shared contract:** [STATE-TRANSACTIONS](../docs/contracts/STATE-TRANSACTIONS.md).

## Deliverable and interfaces

DamageResolver accepts immutable HitEvents and produces ordered DamageEvents, part/system transitions, destruction and scoring events. Aircraft armor zones, internal structure, engines and weapon mounts are distinct where the original supports them. World and capital-ship damage graphs use the same identity discipline but their own rules.

## Non-negotiable behavior

1. Never infer gameplay damage solely from a particle or material. Damaged visuals consume authoritative state.
2. Resolve all hits deterministically within a tick, with an explicit simultaneous-lethal policy. Destroyed actors emit destruction/scoring once.
3. Separate actor death, pilot bailout, captured ownership, despawn and mission removal. These are not interchangeable objective events.
4. Bailout has a data/evidence-backed mission result policy and input confirmation appropriate to modern bindings; do not grant survival or success just because a parachute renders.
5. Restart and aircraft swap remove prior status effects, per-part damage and deferred events unless the authored transition explicitly transfers them.

## Acceptance tests

These are minimum discriminating tests. Add regression cases for every discovered variant. Tests call production code, not parallel test-only implementations. They must be selected with a nonempty task-specific test prefix and must fail when the relevant behavior is removed.

**AC01:** Two same-tick lethal hits award a single kill according to a declared attribution rule.
**AC02:** Armor and internal channels produce distinguishable results without invented multipliers.
**AC03:** Destroying a mount disables its firing and updates its visual state.
**AC04:** Bailout and ordinary death trigger the correct distinct mission transitions.

## Bounded implementation slices

### F29-A: Define damage graphs, hit ordering and lifecycle events

Dependencies: F11-A, F14-A, F20-A, F23-A. Required capabilities: ordinary build/test.

Work only on this stage. Define typed inputs/outputs and a minimal synthetic fixture first; do not jump ahead to a whole runtime.

Required task-test prefix: `accept_f29_a_`. Minimum scenario: Two same-tick lethal hits award a single kill according to a declared attribution rule.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F29-B: Implement zones, armor and system disablement

Dependencies: F29-A, F11-C, F14-C, F20-C, F23-C. Required capabilities: ordinary build/test.

Work only on this stage. Implement the smallest production path that exercises the declared behavior; keep unrelated systems unchanged.

Required task-test prefix: `accept_f29_b_`. Minimum scenario: Armor and internal channels produce distinguishable results without invented multipliers.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F29-C: Wire destruction visuals, debris, scoring and bailout

Dependencies: F29-B. Required capabilities: ordinary build/test.

Work only on this stage. Wire the implemented path into its actual producer and consumer; include teardown/retry and error propagation.

Required task-test prefix: `accept_f29_c_`. Minimum scenario: Destroying a mount disables its firing and updates its visual state.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F29-D: Validate original damage behaviors for planes and mission-critical objects

Dependencies: F29-C. Required capabilities: retail.

Work only on this stage. Collect actual integration/reference evidence; repair discovered regressions without weakening the specification. Missing capabilities or original data block this stage.

Required task-test prefix: `accept_f29_d_`. Minimum scenario: Bailout and ordinary death trigger the correct distinct mission transitions.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.

## Evidence and completion

Record the actual input fingerprint and consumer trace. Synthetic fixtures alone cannot certify original-data behavior.

A code/test pass awards at most **checked**. The reviewer must inspect runtime wiring, error paths, stale-state handling and test sensitivity. Data-dependent claims require source span, installation hash and an independent reference/probe. Visual, audio and ordinary-play claims require their respective capabilities. Developer placeholders never satisfy a retail acceptance case.

## Research boundary

No unrecorded compatibility assumptions are allowed.

## References

[S13](../docs/research/SOURCES.md)

The requirements and tests are independently authored engineering designs. Source labels distinguish observed leads from measured original semantics; consult the evidence ledger before changing gameplay values.
