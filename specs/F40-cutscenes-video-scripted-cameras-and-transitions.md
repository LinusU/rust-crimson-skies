# F40: Cutscenes, video, scripted cameras, and transitions

**Target:** 2000 PC original-data reimplementation. **Initial status:** not implemented; not original-verified.
**Prerequisite features:** F15, F20, F21, F36, F37, F39, F41.
**Owner paths:** `crates/cs_content/src/cinematics.rs`; `crates/cs_app/src/cinematics/`; `crates/cs_sim/src/cinematic_state.rs`; `tests/`.
**Shared contract:** [IDENTITY-CONTENT](../docs/contracts/IDENTITY-CONTENT.md).

## Deliverable and interfaces

Inventory original prerendered media and in-engine camera/action sequences. CinematicPlayer has explicit start, playing, skip-requested, completed, failed and canceled states. Decode discovered conventional formats through an approved dependency or documented private transcoding cache; no fabricated codec assumption.

## Non-negotiable behavior

1. Visual playback failure cannot silently skip a story-critical gameplay action. Separate semantic actions from media presentation and define recovery.
2. Skipping reaches the intended semantic end state exactly once, while preserving required objective/capture events. It is not equivalent to aborting the mission.
3. Use an audio/master media clock for lip/dialogue/video sync where present. Simulation pause policy is explicit per scene.
4. Maintain correct aspect ratio and letterboxing; high-resolution presentation must not stretch original frames.
5. Missing copyrighted media is reported and remains an incomplete-content condition, not replaced by a black screen and a PASS.

## Acceptance tests

These are minimum discriminating tests. Add regression cases for every discovered variant. Tests call production code, not parallel test-only implementations. They must be selected with a nonempty task-specific test prefix and must fail when the relevant behavior is removed.

**AC01:** Skip at start, midpoint and final frame; resulting mission state is identical where designed.
**AC02:** Pause/resume video and ensure audio/video drift remains within an approved tolerance.
**AC03:** Change aircraft during a scene and return player control to the correct actor.
**AC04:** A missing decoder or media file produces a useful error and no false completion.

## Bounded implementation slices

### F40-A: Define cinematic/media inventory and semantic boundaries

Dependencies: F15-A, F20-A, F21-A, F36-A, F37-A, F39-A, F41-A. Required capabilities: ordinary build/test.

Work only on this stage. Define typed inputs/outputs and a minimal synthetic fixture first; do not jump ahead to a whole runtime.

Required task-test prefix: `accept_f40_a_`. Minimum scenario: Skip at start, midpoint and final frame; resulting mission state is identical where designed.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F40-B: Implement decoded playback and authored camera timelines

Dependencies: F40-A, F15-C, F20-C, F21-C, F36-C, F37-C, F39-C, F41-C. Required capabilities: ordinary build/test.

Work only on this stage. Implement the smallest production path that exercises the declared behavior; keep unrelated systems unchanged.

Required task-test prefix: `accept_f40_b_`. Minimum scenario: Pause/resume video and ensure audio/video drift remains within an approved tolerance.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F40-C: Wire skip, pause, state transitions and failure recovery

Dependencies: F40-B. Required capabilities: ordinary build/test.

Work only on this stage. Wire the implemented path into its actual producer and consumer; include teardown/retry and error propagation.

Required task-test prefix: `accept_f40_c_`. Minimum scenario: Change aircraft during a scene and return player control to the correct actor.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F40-D: Verify every original story-critical cinematic and ending sequence

Dependencies: F40-C. Required capabilities: retail, gpu, audio.

Work only on this stage. Collect actual integration/reference evidence; repair discovered regressions without weakening the specification. Missing capabilities or original data block this stage.

Required task-test prefix: `accept_f40_d_`. Minimum scenario: A missing decoder or media file produces a useful error and no false completion.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.

## Evidence and completion

Record the actual input fingerprint and consumer trace. Synthetic fixtures alone cannot certify original-data behavior.

A code/test pass awards at most **checked**. The reviewer must inspect runtime wiring, error paths, stale-state handling and test sensitivity. Data-dependent claims require source span, installation hash and an independent reference/probe. Visual, audio and ordinary-play claims require their respective capabilities. Developer placeholders never satisfy a retail acceptance case.

## Research boundary

No unrecorded compatibility assumptions are allowed.

## References

[S12](../docs/research/SOURCES.md); [S13](../docs/research/SOURCES.md)

The requirements and tests are independently authored engineering designs. Source labels distinguish observed leads from measured original semantics; consult the evidence ledger before changing gameplay values.
