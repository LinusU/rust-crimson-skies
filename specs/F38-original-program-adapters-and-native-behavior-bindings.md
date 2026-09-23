# F38: Original program adapters and native behavior bindings

**Target:** 2000 PC original-data reimplementation. **Initial status:** not implemented; not original-verified.
**Prerequisite features:** F13, F37, F39.
**Owner paths:** `crates/cs_formats/src/script_raw/`; `crates/cs_content/src/script_adapter/`; `crates/cs_script/src/bindings/`; `docs/findings/scripts/`; `tests/`.
**Shared contract:** [SCRIPT-MISSION](../docs/contracts/SCRIPT-MISSION.md).

## Deliverable and interfaces

Implement the original source/bytecode/animation adapters approved by F13, compiling or interpreting them into F37 semantics without losing timing or side effects. HostBindingRegistry maps each observed call to a typed engine operation with argument validation and provenance.

## Non-negotiable behavior

1. No generic Lua/Rhai replacement for original programs unless a verified translator preserves semantics. Do not handwrite all missions based solely on a walkthrough.
2. Every reachable original instruction and native call is enumerated with coverage and behavior tests. A binding stub that returns success is forbidden.
3. Original numeric conversions, boolean semantics, random choices and timer boundary behavior are preserved where they affect outcomes; unsupported uncertainty remains visible.
4. Maintain a source-to-IR map so a runtime event can be traced to archive/member/offset or line.
5. Native calls may only access the simulation API, not arbitrary filesystem/network/OS execution. Original script data is untrusted input.

## Acceptance tests

These are minimum discriminating tests. Add regression cases for every discovered variant. Tests call production code, not parallel test-only implementations. They must be selected with a nonempty task-specific test prefix and must fail when the relevant behavior is removed.

**AC01:** A program referencing an unknown host call fails validation before flight.
**AC02:** Differential traces compare normalized original and recreated event ordering for a measured scenario.
**AC03:** Bad argument types/ranges report source location without a Rust panic.
**AC04:** Coverage audit refuses campaign-ready when one reachable original opcode is unimplemented.

## Bounded implementation slices

### F38-A: Implement the first verified original-program adapter

Dependencies: F13-A, F37-A, F39-A. Required capabilities: ordinary build/test.

Work only on this stage. Define typed inputs/outputs and a minimal synthetic fixture first; do not jump ahead to a whole runtime.

Required task-test prefix: `accept_f38_a_`. Minimum scenario: A program referencing an unknown host call fails validation before flight.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F38-B: Implement the observed host-binding families in bounded batches

Dependencies: F38-A, F13-C, F37-C, F39-C. Required capabilities: retail.

Work only on this stage. Implement the smallest production path that exercises the declared behavior; keep unrelated systems unchanged.

Required task-test prefix: `accept_f38_b_`. Minimum scenario: Differential traces compare normalized original and recreated event ordering for a measured scenario.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F38-C: Connect complete source maps and instruction coverage reports

Dependencies: F38-B. Required capabilities: retail.

Work only on this stage. Wire the implemented path into its actual producer and consumer; include teardown/retry and error propagation.

Required task-test prefix: `accept_f38_c_`. Minimum scenario: Bad argument types/ranges report source location without a Rust panic.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F38-D: Prove all campaign-reachable instructions and bindings against original evidence

Dependencies: F38-C. Required capabilities: retail.

Work only on this stage. Collect actual integration/reference evidence; repair discovered regressions without weakening the specification. Missing capabilities or original data block this stage.

Required task-test prefix: `accept_f38_d_`. Minimum scenario: Coverage audit refuses campaign-ready when one reachable original opcode is unimplemented.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.

## Evidence and completion

Record the actual input fingerprint and consumer trace. Synthetic fixtures alone cannot certify original-data behavior.

A code/test pass awards at most **checked**. The reviewer must inspect runtime wiring, error paths, stale-state handling and test sensitivity. Data-dependent claims require source span, installation hash and an independent reference/probe. Visual, audio and ordinary-play claims require their respective capabilities. Developer placeholders never satisfy a retail acceptance case.

## Research boundary

This feature must remain blocked where F13 has not resolved the format or semantics. A complete engine-facing IR alone does not unblock original mission compatibility.

## References

[S02](../docs/research/SOURCES.md); [S07](../docs/research/SOURCES.md); [S12](../docs/research/SOURCES.md)

The requirements and tests are independently authored engineering designs. Source labels distinguish observed leads from measured original semantics; consult the evidence ledger before changing gameplay values.
