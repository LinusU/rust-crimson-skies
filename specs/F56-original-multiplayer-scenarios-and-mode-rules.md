# F56: Original multiplayer scenarios and mode rules

**Target:** 2000 PC original-data reimplementation. **Initial status:** not implemented; not original-verified.
**Prerequisite features:** F14, F29, F35, F37, F39, F54, F55.
**Owner paths:** `crates/cs_content/src/multiplayer.rs`; `crates/cs_sim/src/multiplayer/`; `crates/cs_net/src/rules.rs`; `tests/`.
**Shared contract:** [UI-NETWORK](../docs/contracts/UI-NETWORK.md).

## Deliverable and interfaces

Inventory all original PC multiplayer scenario/mode ids and parameters. Required behavior families include combat scoring, team rules, objective possession/delivery and capital-ship combat where present. Implement every discovered original mode; do not use the Xbox sequel mode list or assume a count from a website.

## Non-negotiable behavior

1. Each mode defines spawn/respawn, lives, time/score limits, teams, friendly fire, victory/draw conditions and disconnect policy. Unknown values remain blocked.
2. Objective state is authoritative and singular: possession, dropped, returned and scored cannot occur twice due to packet retransmission.
3. Original maps and rule scripts resolve through the same catalog and program adapter, not an unrelated procedural arena.
4. Result attribution is session and participant scoped. A reconnect or recycled entity id cannot inherit previous-match score.
5. Human-count scaling, allowed component limits and custom planes are validated before start.

## Acceptance tests

These are minimum discriminating tests. Add regression cases for every discovered variant. Tests call production code, not parallel test-only implementations. They must be selected with a nonempty task-specific test prefix and must fail when the relevant behavior is removed.

**AC01:** Simultaneous lethal events and limit expiry produce one documented final result.
**AC02:** Two clients claim the same objective; only server-accepted ownership succeeds.
**AC03:** Restart match and ensure no score, pickup or timer leaks.
**AC04:** Play every discovered original mode/scenario with real clients and original assets.

## Bounded implementation slices

### F56-A: Discover the complete original multiplayer catalog

Dependencies: F14-A, F29-A, F35-A, F37-A, F39-A, F54-A, F55-A. Required capabilities: retail.

Work only on this stage. Define typed inputs/outputs and a minimal synthetic fixture first; do not jump ahead to a whole runtime.

Required task-test prefix: `accept_f56_a_`. Minimum scenario: Simultaneous lethal events and limit expiry produce one documented final result.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F56-B: Implement verified mode state machines and objective ownership

Dependencies: F56-A, F14-C, F29-C, F35-C, F37-C, F39-C, F54-C, F55-C. Required capabilities: retail.

Work only on this stage. Implement the smallest production path that exercises the declared behavior; keep unrelated systems unchanged.

Required task-test prefix: `accept_f56_b_`. Minimum scenario: Two clients claim the same objective; only server-accepted ownership succeeds.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F56-C: Wire maps, host options, scoring and end-of-match UI

Dependencies: F56-B. Required capabilities: retail.

Work only on this stage. Wire the implemented path into its actual producer and consumer; include teardown/retry and error propagation.

Required task-test prefix: `accept_f56_c_`. Minimum scenario: Restart match and ensure no score, pickup or timer leaks.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F56-D: Verify every original multiplayer rule variant and scenario

Dependencies: F56-C. Required capabilities: network_real, retail.

Work only on this stage. Collect actual integration/reference evidence; repair discovered regressions without weakening the specification. Missing capabilities or original data block this stage.

Required task-test prefix: `accept_f56_d_`. Minimum scenario: Play every discovered original mode/scenario with real clients and original assets.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.

## Evidence and completion

Record the actual input fingerprint and consumer trace. Synthetic fixtures alone cannot certify original-data behavior.

A code/test pass awards at most **checked**. The reviewer must inspect runtime wiring, error paths, stale-state handling and test sensitivity. Data-dependent claims require source span, installation hash and an independent reference/probe. Visual, audio and ordinary-play claims require their respective capabilities. Developer placeholders never satisfy a retail acceptance case.

## Research boundary

The exact original mode count, names, rules and caps were not recovered from supplied retail data. This task is not satisfied by three invented generic modes; catalog completeness is required.

## References

[S13](../docs/research/SOURCES.md)

The requirements and tests are independently authored engineering designs. Source labels distinguish observed leads from measured original semantics; consult the evidence ledger before changing gameplay values.
