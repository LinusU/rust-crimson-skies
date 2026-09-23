# F60: Performance, memory, stability, and platforms

**Target:** 2000 PC original-data reimplementation. **Initial status:** not implemented; not original-verified.
**Prerequisite features:** F15, F17, F18, F23, F41, F50, F54, F59.
**Owner paths:** `crates/cs_app/src/diagnostics/`; `tools/cs_xtask/src/perf.rs`; `docs/findings/perf/`; `tests/`.
**Shared contract:** [CLI-EVIDENCE](../docs/contracts/CLI-EVIDENCE.md).

## Deliverable and interfaces

Target macOS Apple Silicon, Windows x86-64 and Linux x86-64 where the pinned stack supports them. Primary performance target is a playable original-content experience on the owners machine. Set budgets after measuring a fixed baseline; report p50/p95/p99 frame and simulation times, load time, memory and cache size.

## Non-negotiable behavior

1. Do not promise a universal FPS value without hardware/resolution/settings. Designed initial target is sustained 60 FPS presentation at 1080p on the declared reference machine, with 120 Hz simulation budget measured separately.
2. Benchmark worst-case campaign battle, dense world, smoke/transparent effects, many projectiles and full configured multiplayer load.
3. Do not remove collision, AI, audio or mission content to pass performance tests. Quality degradation is an explicit setting with correctness invariants.
4. Repeated mission/menu/locale/device switching must not leak entities, asset handles, audio loops or tasks.
5. Unsupported platform features are recorded before release; no Windows-only external extractor required for normal gameplay on macOS.

## Acceptance tests

These are minimum discriminating tests. Add regression cases for every discovered variant. Tests call production code, not parallel test-only implementations. They must be selected with a nonempty task-specific test prefix and must fail when the relevant behavior is removed.

**AC01:** Run a 60-minute designed soak across mission/IA/menu cycles with memory trend bounds.
**AC02:** Record cold/warm loads and worst-case frame timing with all required systems enabled.
**AC03:** Build and smoke on each declared platform in CI or attached manual evidence.
**AC04:** Audio device loss, focus loss, display resize and sleep/resume fail safely.

## Bounded implementation slices

### F60-A: Define benchmark scenarios and hardware-specific budgets

Dependencies: F15-A, F17-A, F18-A, F23-A, F41-A, F50-A, F54-A, F59-A. Required capabilities: ordinary build/test.

Work only on this stage. Define typed inputs/outputs and a minimal synthetic fixture first; do not jump ahead to a whole runtime.

Required task-test prefix: `accept_f60_a_`. Minimum scenario: Run a 60-minute designed soak across mission/IA/menu cycles with memory trend bounds.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F60-B: Instrument CPU/GPU/asset/simulation costs

Dependencies: F60-A, F15-C, F17-C, F18-C, F23-C, F41-C, F50-C, F54-C, F59-C. Required capabilities: ordinary build/test.

Work only on this stage. Implement the smallest production path that exercises the declared behavior; keep unrelated systems unchanged.

Required task-test prefix: `accept_f60_b_`. Minimum scenario: Record cold/warm loads and worst-case frame timing with all required systems enabled.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F60-C: Optimize only measured hotspots without content loss

Dependencies: F60-B. Required capabilities: ordinary build/test.

Work only on this stage. Wire the implemented path into its actual producer and consumer; include teardown/retry and error propagation.

Required task-test prefix: `accept_f60_c_`. Minimum scenario: Build and smoke on each declared platform in CI or attached manual evidence.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F60-D: Complete platform, soak and performance acceptance matrix

Dependencies: F60-C. Required capabilities: ordinary build/test.

Work only on this stage. Collect actual integration/reference evidence; repair discovered regressions without weakening the specification. Missing capabilities or original data block this stage.

Required task-test prefix: `accept_f60_d_`. Minimum scenario: Audio device loss, focus loss, display resize and sleep/resume fail safely.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.

## Evidence and completion

Record the actual input fingerprint and consumer trace. Synthetic fixtures alone cannot certify original-data behavior.

A code/test pass awards at most **checked**. The reviewer must inspect runtime wiring, error paths, stale-state handling and test sensitivity. Data-dependent claims require source span, installation hash and an independent reference/probe. Visual, audio and ordinary-play claims require their respective capabilities. Developer placeholders never satisfy a retail acceptance case.

## Research boundary

No unrecorded compatibility assumptions are allowed.

## References

Project-designed engineering contract; no original behavior is asserted by the design alone.

The requirements and tests are independently authored engineering designs. Source labels distinguish observed leads from measured original semantics; consult the evidence ledger before changing gameplay values.
