# F15: Asynchronous asset loading and private cache

**Target:** 2000 PC original-data reimplementation. **Initial status:** not implemented; not original-verified.
**Prerequisite features:** F04, F14.
**Owner paths:** `crates/cs_assets/src/cache/`; `crates/cs_app/src/loading.rs`; `crates/cs_app/src/assets.rs`; `crates/cs_assets/tests/`.
**Shared contract:** [IDENTITY-CONTENT](../docs/contracts/IDENTITY-CONTENT.md).

## Deliverable and interfaces

A content load is a cancellable transaction from Requested through Loading, Validating, Ready or Failed. No world becomes interactive until its gameplay-critical closure is ready. Cache entries identify installation hash, source span hash, decoder/IR version and conversion options.

## Non-negotiable behavior

1. Only cs_app converts canonical assets into Bevy assets. Cache stores are private, bounded and atomic; no writes to source installation.
2. Cancellation drains or safely detaches work and discards stale session results. Never attach an old mission texture to a new mission because async completion arrived late.
3. A partially written cache entry fails integrity validation and is rebuilt. Cache corruption cannot change campaign state.
4. Background loading cannot mutate simulation mid-tick. The simulation receives a versioned ready bundle at a controlled boundary.
5. Loading progress is based on measured work units; errors show the missing dependency and recovery path, not a progress bar stuck at 99 percent.

## Acceptance tests

These are minimum discriminating tests. Add regression cases for every discovered variant. Tests call production code, not parallel test-only implementations. They must be selected with a nonempty task-specific test prefix and must fail when the relevant behavior is removed.

**AC01:** Cancel a mission load, switch world, then complete the old IO future; no old entities appear.
**AC02:** Kill the process during cache write; next startup recovers cleanly.
**AC03:** Changing one livery source invalidates only affected derived assets.
**AC04:** Warm and cold loads produce equal content hashes and gameplay state.

## Bounded implementation slices

### F15-A: Define load transaction and cache key contracts

Dependencies: F04-A, F14-A. Required capabilities: ordinary build/test.

Work only on this stage. Define typed inputs/outputs and a minimal synthetic fixture first; do not jump ahead to a whole runtime.

Required task-test prefix: `accept_f15_a_`. Minimum scenario: Cancel a mission load, switch world, then complete the old IO future; no old entities appear.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F15-B: Implement bounded asynchronous reads and atomic cache

Dependencies: F15-A, F04-C, F14-C. Required capabilities: ordinary build/test.

Work only on this stage. Implement the smallest production path that exercises the declared behavior; keep unrelated systems unchanged.

Required task-test prefix: `accept_f15_b_`. Minimum scenario: Kill the process during cache write; next startup recovers cleanly.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F15-C: Wire loading UI, cancellation and simulation handoff

Dependencies: F15-B. Required capabilities: ordinary build/test.

Work only on this stage. Wire the implemented path into its actual producer and consumer; include teardown/retry and error propagation.

Required task-test prefix: `accept_f15_c_`. Minimum scenario: Changing one livery source invalidates only affected derived assets.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F15-D: Run cold/warm/restart tests on full private content

Dependencies: F15-C. Required capabilities: retail.

Work only on this stage. Collect actual integration/reference evidence; repair discovered regressions without weakening the specification. Missing capabilities or original data block this stage.

Required task-test prefix: `accept_f15_d_`. Minimum scenario: Warm and cold loads produce equal content hashes and gameplay state.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.

## Evidence and completion

Record the actual input fingerprint and consumer trace. Synthetic fixtures alone cannot certify original-data behavior.

A code/test pass awards at most **checked**. The reviewer must inspect runtime wiring, error paths, stale-state handling and test sensitivity. Data-dependent claims require source span, installation hash and an independent reference/probe. Visual, audio and ordinary-play claims require their respective capabilities. Developer placeholders never satisfy a retail acceptance case.

## Research boundary

No unrecorded compatibility assumptions are allowed.

## References

Project-designed engineering contract; no original behavior is asserted by the design alone.

The requirements and tests are independently authored engineering designs. Source labels distinguish observed leads from measured original semantics; consult the evidence ledger before changing gameplay values.
