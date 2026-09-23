# F20: Object animation and authored destruction states

**Target:** 2000 PC original-data reimplementation. **Initial status:** not implemented; not original-verified.
**Prerequisite features:** F11, F14, F16, F17.
**Owner paths:** `crates/cs_content/src/animation.rs`; `crates/cs_app/src/animation/`; `crates/cs_sim/src/animated_object.rs`; `tests/`.
**Shared contract:** [IDENTITY-CONTENT](../docs/contracts/IDENTITY-CONTENT.md).

## Deliverable and interfaces

Animation IR describes verified node transforms, material changes, visibility swaps, attachments and event markers. It must support original propellers, moving control surfaces, doors, turrets, capital-ship mechanisms and breakable-state transitions without baking gameplay into render frame callbacks.

## Non-negotiable behavior

1. Gameplay markers fire in fixed-tick simulation with event ids; interpolation only changes presentation.
2. Animation unknowns must retain source locator and block affected gameplay transitions. Do not assume MechWarrior animation event semantics apply to CS.
3. State transitions are idempotent; destroyed nodes cannot respawn due to animation looping or LOD replacement.
4. Parent changes preserve the required world/local pose explicitly. Release attachments before despawning parents.
5. Reversing or skipping cinematics must not duplicate pickups, ammo or mission events.

## Acceptance tests

These are minimum discriminating tests. Add regression cases for every discovered variant. Tests call production code, not parallel test-only implementations. They must be selected with a nonempty task-specific test prefix and must fail when the relevant behavior is removed.

**AC01:** A door opening at a fixed tick changes collider and mesh state coherently.
**AC02:** A looping propeller never emits repeated one-shot gameplay events.
**AC03:** Detach cargo from a moving parent with correct inherited velocity.
**AC04:** Skip an animation containing a mission marker; final semantic state is reached exactly once.

## Bounded implementation slices

### F20-A: Define animation channels and event markers

Dependencies: F11-A, F14-A, F16-A, F17-A. Required capabilities: ordinary build/test.

Work only on this stage. Define typed inputs/outputs and a minimal synthetic fixture first; do not jump ahead to a whole runtime.

Required task-test prefix: `accept_f20_a_`. Minimum scenario: A door opening at a fixed tick changes collider and mesh state coherently.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F20-B: Implement verified transform/material/attachment tracks

Dependencies: F20-A, F11-C, F14-C, F16-C, F17-C. Required capabilities: ordinary build/test.

Work only on this stage. Implement the smallest production path that exercises the declared behavior; keep unrelated systems unchanged.

Required task-test prefix: `accept_f20_b_`. Minimum scenario: A looping propeller never emits repeated one-shot gameplay events.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F20-C: Wire stateful animated props and destruction transitions

Dependencies: F20-B. Required capabilities: ordinary build/test.

Work only on this stage. Wire the implemented path into its actual producer and consumer; include teardown/retry and error propagation.

Required task-test prefix: `accept_f20_c_`. Minimum scenario: Detach cargo from a moving parent with correct inherited velocity.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F20-D: Validate all mission-critical original animation families

Dependencies: F20-C. Required capabilities: gpu, retail.

Work only on this stage. Collect actual integration/reference evidence; repair discovered regressions without weakening the specification. Missing capabilities or original data block this stage.

Required task-test prefix: `accept_f20_d_`. Minimum scenario: Skip an animation containing a mission marker; final semantic state is reached exactly once.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.

## Evidence and completion

Record the actual input fingerprint and consumer trace. Synthetic fixtures alone cannot certify original-data behavior.

A code/test pass awards at most **checked**. The reviewer must inspect runtime wiring, error paths, stale-state handling and test sensitivity. Data-dependent claims require source span, installation hash and an independent reference/probe. Visual, audio and ordinary-play claims require their respective capabilities. Developer placeholders never satisfy a retail acceptance case.

## Research boundary

No unrecorded compatibility assumptions are allowed.

## References

[S02](../docs/research/SOURCES.md); [S08](../docs/research/SOURCES.md); [S12](../docs/research/SOURCES.md)

The requirements and tests are independently authored engineering designs. Source labels distinguish observed leads from measured original semantics; consult the evidence ledger before changing gameplay values.
