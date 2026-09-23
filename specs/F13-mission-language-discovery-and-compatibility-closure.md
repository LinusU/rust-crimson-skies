# F13: Mission-language discovery and compatibility closure

**Target:** 2000 PC original-data reimplementation. **Initial status:** not implemented; not original-verified.
**Prerequisite features:** F01, F02, F06, F07, F12.
**Owner paths:** `docs/findings/scripts/`; `crates/cs_formats/src/script_raw/`; `tools/cs_inspect/src/script_discovery.rs`; `crates/cs_formats/tests/script_raw/`.
**Shared contract:** [SCRIPT-MISSION](../docs/contracts/SCRIPT-MISSION.md).

## Deliverable and interfaces

Locate the actual campaign mission programs, animation event records, native binding tables and loading manifests in the private installation. Produce ScriptInventory and OpcodeLedger before selecting a VM architecture. INTERP loading scripts, reader files, camera animation and mission animation are distinct until evidence proves relationships.

## Non-negotiable behavior

1. For each candidate record identify byte range, format discriminator, version, references, likely role and evidence confidence. Scan for headers and strings only as leads; a scan is not a decoder.
2. Use disassembly or runtime observations of an owner-supplied original only as a separately documented research method. Do not incorporate lifted copyrighted game code into the project.
3. Choose among lossless source parser, bytecode interpreter, or evidenced declarative translation only after the corpus supports the choice. F37 defines the engine-facing IR, not an assertion about the original format.
4. Every reachable opcode, command and native call has signature, effects, timing, error behavior and source evidence. Unknown reachable instructions cause UnsupportedMission with a trace.
5. A demo substitute, mission-number switch or hand-authored kill-all script cannot satisfy original campaign support. No completion until each mission dependency closure is resolved.

## Acceptance tests

These are minimum discriminating tests. Add regression cases for every discovered variant. Tests call production code, not parallel test-only implementations. They must be selected with a nonempty task-specific test prefix and must fail when the relevant behavior is removed.

**AC01:** Inventory all candidate script containers without pretending unknown records are instructions.
**AC02:** Unknown opcode at a reached program counter fails with mission and source location.
**AC03:** An unused unknown record remains visible in the report, with reachability evidence.
**AC04:** Compare two original runs differing in one controlled event to distinguish timer from kill-count triggers.

## Bounded implementation slices

### F13-A: Build script inventory and disassembly-neutral evidence schema

Dependencies: F01-A, F02-A, F06-A, F07-A, F12-A. Required capabilities: ordinary build/test.

Work only on this stage. Define typed inputs/outputs and a minimal synthetic fixture first; do not jump ahead to a whole runtime.

Required task-test prefix: `accept_f13_a_`. Minimum scenario: Inventory all candidate script containers without pretending unknown records are instructions.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F13-B: Locate and classify loading, mission and animation programs

Dependencies: F13-A, F01-C, F02-C, F06-C, F07-C, F12-C. Required capabilities: retail.

Work only on this stage. Implement the smallest production path that exercises the declared behavior; keep unrelated systems unchanged.

Required task-test prefix: `accept_f13_b_`. Minimum scenario: Unknown opcode at a reached program counter fails with mission and source location.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F13-C: Resolve instruction/native signatures with isolated probes

Dependencies: F13-B. Required capabilities: retail.

Work only on this stage. Wire the implemented path into its actual producer and consumer; include teardown/retry and error propagation.

Required task-test prefix: `accept_f13_c_`. Minimum scenario: An unused unknown record remains visible in the report, with reachability evidence.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F13-D: Approve a complete mission-language implementation plan from measured evidence

Dependencies: F13-C. Required capabilities: retail.

Work only on this stage. Collect actual integration/reference evidence; repair discovered regressions without weakening the specification. Missing capabilities or original data block this stage.

Required task-test prefix: `accept_f13_d_`. Minimum scenario: Compare two original runs differing in one controlled event to distinguish timer from kill-count triggers.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.

## Evidence and completion

Private input is mandatory for discovery. This feature cannot be marked verified_original from public source research alone.

A code/test pass awards at most **checked**. The reviewer must inspect runtime wiring, error paths, stale-state handling and test sensitivity. Data-dependent claims require source span, installation hash and an independent reference/probe. Visual, audio and ordinary-play claims require their respective capabilities. Developer placeholders never satisfy a retail acceptance case.

## Research boundary

The public research did not establish a complete Crimson Skies mission VM, opcode table or native ABI. This is a first-class blocking research workstream, not a solved format.

## References

[S02](../docs/research/SOURCES.md); [S07](../docs/research/SOURCES.md); [S12](../docs/research/SOURCES.md)

The requirements and tests are independently authored engineering designs. Source labels distinguish observed leads from measured original semantics; consult the evidence ledger before changing gameplay values.
