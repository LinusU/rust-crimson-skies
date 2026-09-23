# F14: Canonical content catalog and dependency closure

**Target:** 2000 PC original-data reimplementation. **Initial status:** not implemented; not original-verified.
**Prerequisite features:** F01, F02, F04, F12.
**Owner paths:** `crates/cs_types/src/content.rs`; `crates/cs_content/src/catalog/`; `tools/cs_inspect/src/catalog.rs`; `crates/cs_content/tests/`.
**Shared contract:** [IDENTITY-CONTENT](../docs/contracts/IDENTITY-CONTENT.md).

## Deliverable and interfaces

Build Catalog with stable ids for worlds, missions, airframes, loadouts, factions, weapons, sounds, dialogue, media, stunts, scrapbook items, IA scenarios and multiplayer rules. Every normalized field stores provenance or a designed-default marker. Stable ids derive from a semantic source key plus namespace, not enumeration order.

## Non-negotiable behavior

1. Parsing, normalization, dependency validation and runtime readiness are separate states. A catalog row may be visible but unavailable, with a reason.
2. Compute the transitive closure of every launchable mission/scenario. Verify resources, parsers, instructions, native handlers, sockets, strings, media and gameplay consumers.
3. All referenced quantities have units and permitted ranges. A missing critical value is an error, not Default::default().
4. Never filter unsupported missions out and divide successes by the smaller list. The declared baseline inventory fixes the denominator.
5. Duplicate identities, orphaned references and contradictory patch variants must be resolved explicitly. Export deterministic JSON for private comparison.

## Acceptance tests

These are minimum discriminating tests. Add regression cases for every discovered variant. Tests call production code, not parallel test-only implementations. They must be selected with a nonempty task-specific test prefix and must fail when the relevant behavior is removed.

**AC01:** Adding an unsupported mission increases the unsupported count and prevents full readiness.
**AC02:** Randomize input enumeration; ids and serialized order remain stable.
**AC03:** Delete a texture several edges deep and report the mission-to-texture dependency chain.
**AC04:** A synthetic launchable row is never mistaken for a retail catalog entry.

## Bounded implementation slices

### F14-A: Define stable content ids and provenance-bearing schema

Dependencies: F01-A, F02-A, F04-A, F12-A. Required capabilities: ordinary build/test.

Work only on this stage. Define typed inputs/outputs and a minimal synthetic fixture first; do not jump ahead to a whole runtime.

Required task-test prefix: `accept_f14_a_`. Minimum scenario: Adding an unsupported mission increases the unsupported count and prevents full readiness.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F14-B: Implement normalization and graph validation

Dependencies: F14-A, F01-C, F02-C, F04-C, F12-C. Required capabilities: ordinary build/test.

Work only on this stage. Implement the smallest production path that exercises the declared behavior; keep unrelated systems unchanged.

Required task-test prefix: `accept_f14_b_`. Minimum scenario: Randomize input enumeration; ids and serialized order remain stable.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F14-C: Expose catalog, closure and readiness inspection commands

Dependencies: F14-B. Required capabilities: ordinary build/test.

Work only on this stage. Wire the implemented path into its actual producer and consumer; include teardown/retry and error propagation.

Required task-test prefix: `accept_f14_c_`. Minimum scenario: Delete a texture several edges deep and report the mission-to-texture dependency chain.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F14-D: Generate the complete private baseline inventory and coverage denominator

Dependencies: F14-C. Required capabilities: retail.

Work only on this stage. Collect actual integration/reference evidence; repair discovered regressions without weakening the specification. Missing capabilities or original data block this stage.

Required task-test prefix: `accept_f14_d_`. Minimum scenario: A synthetic launchable row is never mistaken for a retail catalog entry.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.

## Evidence and completion

Record the actual input fingerprint and consumer trace. Synthetic fixtures alone cannot certify original-data behavior.

A code/test pass awards at most **checked**. The reviewer must inspect runtime wiring, error paths, stale-state handling and test sensitivity. Data-dependent claims require source span, installation hash and an independent reference/probe. Visual, audio and ordinary-play claims require their respective capabilities. Developer placeholders never satisfy a retail acceptance case.

## Research boundary

No unrecorded compatibility assumptions are allowed.

## References

[S01](../docs/research/SOURCES.md); [S03](../docs/research/SOURCES.md); [S04](../docs/research/SOURCES.md)

The requirements and tests are independently authored engineering designs. Source labels distinguish observed leads from measured original semantics; consult the evidence ledger before changing gameplay values.
