# F10: GameZ mesh topology and material records

**Target:** 2000 PC original-data reimplementation. **Initial status:** not implemented; not original-verified.
**Prerequisite features:** F03, F06, F08.
**Owner paths:** `crates/cs_formats/src/gamez/`; `crates/cs_content/src/mesh.rs`; `crates/cs_app/src/mesh.rs`; `crates/cs_formats/tests/gamez/`.
**Shared contract:** [IDENTITY-CONTENT](../docs/contracts/IDENTITY-CONTENT.md).

## Deliverable and interfaces

Parse the CS-specific GameZ data that supplies world geometry and PLANES.ZBD meshes. Preserve raw mesh indices, polygon flags, per-corner material/UV/color data and node associations. Create a canonical mesh IR independent of Bevy.

## Non-negotiable behavior

1. Pin the legacy CS-capable reference revision. Current mech3ax main is not a drop-in geometry oracle.
2. Triangle strips alternate winding; degenerate triangles still advance strip parity. Ngons need validated triangulation, not a triangle fan for all concave polygons.
3. Split render vertices where UV, color, normal or material differs even if positions share an index. Preserve source-corner maps for diagnostics.
4. Bounds and topology validate before upload. Do not silently drop broken faces like a visual exporter; enumerate unsupported geometry and its visible consumers.
5. Do not assume a field called specular in an older API is material specularity: newer source reclassified one such field as soil. Keep raw fields until verified.

## Acceptance tests

These are minimum discriminating tests. Add regression cases for every discovered variant. Tests call production code, not parallel test-only implementations. They must be selected with a nonempty task-specific test prefix and must fail when the relevant behavior is removed.

**AC01:** Strip [0,1,2,3] yields consistent orientation, with a degenerate insertion still correct.
**AC02:** A concave polygon triangulates without triangles outside its boundary.
**AC03:** A shared position with different per-corner UVs remains a visible seam as authored.
**AC04:** Report exact missing/invalid face counts for every private world and airframe.

## Bounded implementation slices

### F10-A: Define lossless mesh IR and topology test fixtures

Dependencies: F03-A, F06-A, F08-A. Required capabilities: ordinary build/test.

Work only on this stage. Define typed inputs/outputs and a minimal synthetic fixture first; do not jump ahead to a whole runtime.

Required task-test prefix: `accept_f10_a_`. Minimum scenario: Strip [0,1,2,3] yields consistent orientation, with a degenerate insertion still correct.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F10-B: Implement the verified CS GameZ mesh subset

Dependencies: F10-A, F03-C, F06-C, F08-C. Required capabilities: ordinary build/test.

Work only on this stage. Implement the smallest production path that exercises the declared behavior; keep unrelated systems unchanged.

Required task-test prefix: `accept_f10_b_`. Minimum scenario: A concave polygon triangulates without triangles outside its boundary.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F10-C: Upload mesh IR and audit material/texture dependencies

Dependencies: F10-B. Required capabilities: ordinary build/test.

Work only on this stage. Wire the implemented path into its actual producer and consumer; include teardown/retry and error propagation.

Required task-test prefix: `accept_f10_c_`. Minimum scenario: A shared position with different per-corner UVs remains a visible seam as authored.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F10-D: Close every render-critical unknown on the private geometry corpus

Dependencies: F10-C. Required capabilities: retail.

Work only on this stage. Collect actual integration/reference evidence; repair discovered regressions without weakening the specification. Missing capabilities or original data block this stage.

Required task-test prefix: `accept_f10_d_`. Minimum scenario: Report exact missing/invalid face counts for every private world and airframe.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.

## Evidence and completion

Record the actual input fingerprint and consumer trace. Synthetic fixtures alone cannot certify original-data behavior.

A code/test pass awards at most **checked**. The reviewer must inspect runtime wiring, error paths, stale-state handling and test sensitivity. Data-dependent claims require source span, installation hash and an independent reference/probe. Visual, audio and ordinary-play claims require their respective capabilities. Developer placeholders never satisfy a retail acceptance case.

## Research boundary

The pinned reference parses only its supported corpus. Direct binary field layouts beyond those explicitly documented in research must be established before coding, never guessed from JSON property names.

## References

[S02](../docs/research/SOURCES.md); [S03](../docs/research/SOURCES.md); [S08](../docs/research/SOURCES.md)

The requirements and tests are independently authored engineering designs. Source labels distinguish observed leads from measured original semantics; consult the evidence ledger before changing gameplay values.
