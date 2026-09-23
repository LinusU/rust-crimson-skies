# F18: World geometry, terrain, water, and traversable interiors

**Target:** 2000 PC original-data reimplementation. **Initial status:** not implemented; not original-verified.
**Prerequisite features:** F10, F11, F16, F17, F23.
**Owner paths:** `crates/cs_content/src/world.rs`; `crates/cs_app/src/world/`; `crates/cs_app/tests/world/`.
**Shared contract:** [IDENTITY-CONTENT](../docs/contracts/IDENTITY-CONTENT.md).

## Deliverable and interfaces

Load each discovered world group as authored geometry with collision, surface roles, object instances, sector/visibility metadata and mission-local overlays. Preserve tunnels, arches, building openings, hangars and stunt passages; a heightfield substitute is not acceptable where original geometry matters.

## Non-negotiable behavior

1. Visual and collision meshes share provenance and coordinate conversion but may use different verified simplifications. Never close a traversable opening through convex-hull simplification.
2. Water and ground contacts follow explicit gameplay surface rules. Water rendering must not invent an infinite collision plane over legitimate low-flight areas.
3. Object identity survives sector streaming. Gameplay-required objects remain simulated or in a correctly summarized state outside render visibility.
4. World boundaries and ceiling/floor rules are data-driven or documented design fallbacks; no arbitrary invisible wall in fidelity mode.
5. Loading a reused world for another mission applies its authored variant, damage initial state and object population, not leftovers from the last run.

## Acceptance tests

These are minimum discriminating tests. Add regression cases for every discovered variant. Tests call production code, not parallel test-only implementations. They must be selected with a nonempty task-specific test prefix and must fail when the relevant behavior is removed.

**AC01:** Fly a swept body through a narrow synthetic arch at high speed without collision mismatch.
**AC02:** Unload and reload a sector containing a damaged objective; state persists correctly.
**AC03:** Open an authored door and verify both render and collision update once.
**AC04:** Visit every discovered world group and compare representative geometry and traversal routes.

## Bounded implementation slices

### F18-A: Define world instances, sectors and collision roles

Dependencies: F10-A, F11-A, F16-A, F17-A, F23-A. Required capabilities: ordinary build/test.

Work only on this stage. Define typed inputs/outputs and a minimal synthetic fixture first; do not jump ahead to a whole runtime.

Required task-test prefix: `accept_f18_a_`. Minimum scenario: Fly a swept body through a narrow synthetic arch at high speed without collision mismatch.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F18-B: Implement world import and static collision generation

Dependencies: F18-A, F10-C, F11-C, F16-C, F17-C, F23-C. Required capabilities: ordinary build/test.

Work only on this stage. Implement the smallest production path that exercises the declared behavior; keep unrelated systems unchanged.

Required task-test prefix: `accept_f18_b_`. Minimum scenario: Unload and reload a sector containing a damaged objective; state persists correctly.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F18-C: Add mission overlays and safe visibility/streaming

Dependencies: F18-B. Required capabilities: ordinary build/test.

Work only on this stage. Wire the implemented path into its actual producer and consumer; include teardown/retry and error propagation.

Required task-test prefix: `accept_f18_c_`. Minimum scenario: Open an authored door and verify both render and collision update once.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F18-D: Audit all original world variants and stunt-critical openings

Dependencies: F18-C. Required capabilities: gpu, retail.

Work only on this stage. Collect actual integration/reference evidence; repair discovered regressions without weakening the specification. Missing capabilities or original data block this stage.

Required task-test prefix: `accept_f18_d_`. Minimum scenario: Visit every discovered world group and compare representative geometry and traversal routes.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.

## Evidence and completion

Record the actual input fingerprint and consumer trace. Synthetic fixtures alone cannot certify original-data behavior.

A code/test pass awards at most **checked**. The reviewer must inspect runtime wiring, error paths, stale-state handling and test sensitivity. Data-dependent claims require source span, installation hash and an independent reference/probe. Visual, audio and ordinary-play claims require their respective capabilities. Developer placeholders never satisfy a retail acceptance case.

## Research boundary

No unrecorded compatibility assumptions are allowed.

## References

[S03](../docs/research/SOURCES.md); [S08](../docs/research/SOURCES.md)

The requirements and tests are independently authored engineering designs. Source labels distinguish observed leads from measured original semantics; consult the evidence ledger before changing gameplay values.
