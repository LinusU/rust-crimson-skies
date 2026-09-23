# F11: Scene hierarchy, aircraft parts, sockets, and LOD

**Target:** 2000 PC original-data reimplementation. **Initial status:** not implemented; not original-verified.
**Prerequisite features:** F10, F14.
**Owner paths:** `crates/cs_content/src/scene.rs`; `crates/cs_app/src/scene.rs`; `crates/cs_app/src/airframe_visual.rs`; `crates/cs_content/tests/scene.rs`.
**Shared contract:** [IDENTITY-CONTENT](../docs/contracts/IDENTITY-CONTENT.md).

## Deliverable and interfaces

Convert parsed nodes into stable SceneNodeId records with parent, local transform, render mesh, visibility/LOD state, collision role, animation bindings and semantic sockets. Airframe definitions reference roots in PLANES.ZBD, not models selected by array position.

## Non-negotiable behavior

1. Reject cycles, dangling parents and ambiguous roots. Preserve authored transforms and all LOD variants; do not flatten hierarchies irreversibly.
2. Part-to-gameplay bindings identify guns, rocket mounts, engines, control surfaces, camera anchors and damage zones using evidence-backed mappings.
3. A model name is not proof that a plane is player-selectable in every mode. Discover roster availability and forced mission assignments separately.
4. LOD changes only presentation; collision, weapon origin and damage identity cannot jump with distance.
5. Scene reload uses session-generation ownership; no hidden old roots or stale material handles.

## Acceptance tests

These are minimum discriminating tests. Add regression cases for every discovered variant. Tests call production code, not parallel test-only implementations. They must be selected with a nonempty task-specific test prefix and must fail when the relevant behavior is removed.

**AC01:** Nested transforms and negative scale preserve visual/collision alignment after canonical conversion.
**AC02:** A destroyed wing and its gun remain disabled across an LOD transition.
**AC03:** Load and unload the same airframe 100 times without increasing live entity count.
**AC04:** Private roster audit maps every root, part, mount and cockpit binding or flags a blocker.

## Bounded implementation slices

### F11-A: Define node hierarchy and semantic binding records

Dependencies: F10-A, F14-A. Required capabilities: ordinary build/test.

Work only on this stage. Define typed inputs/outputs and a minimal synthetic fixture first; do not jump ahead to a whole runtime.

Required task-test prefix: `accept_f11_a_`. Minimum scenario: Nested transforms and negative scale preserve visual/collision alignment after canonical conversion.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F11-B: Implement hierarchy import and LOD selection

Dependencies: F11-A, F10-C, F14-C. Required capabilities: ordinary build/test.

Work only on this stage. Implement the smallest production path that exercises the declared behavior; keep unrelated systems unchanged.

Required task-test prefix: `accept_f11_b_`. Minimum scenario: A destroyed wing and its gun remain disabled across an LOD transition.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F11-C: Bind aircraft parts, sockets and damage visuals

Dependencies: F11-B. Required capabilities: ordinary build/test.

Work only on this stage. Wire the implemented path into its actual producer and consumer; include teardown/retry and error propagation.

Required task-test prefix: `accept_f11_c_`. Minimum scenario: Load and unload the same airframe 100 times without increasing live entity count.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F11-D: Validate every discovered airframe including mission-only types

Dependencies: F11-C. Required capabilities: retail.

Work only on this stage. Collect actual integration/reference evidence; repair discovered regressions without weakening the specification. Missing capabilities or original data block this stage.

Required task-test prefix: `accept_f11_d_`. Minimum scenario: Private roster audit maps every root, part, mount and cockpit binding or flags a blocker.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.

## Evidence and completion

Record the actual input fingerprint and consumer trace. Synthetic fixtures alone cannot certify original-data behavior.

A code/test pass awards at most **checked**. The reviewer must inspect runtime wiring, error paths, stale-state handling and test sensitivity. Data-dependent claims require source span, installation hash and an independent reference/probe. Visual, audio and ordinary-play claims require their respective capabilities. Developer placeholders never satisfy a retail acceptance case.

## Research boundary

No unrecorded compatibility assumptions are allowed.

## References

[S03](../docs/research/SOURCES.md); [S08](../docs/research/SOURCES.md); [S10](../docs/research/SOURCES.md)

The requirements and tests are independently authored engineering designs. Source labels distinguish observed leads from measured original semantics; consult the evidence ledger before changing gameplay values.
