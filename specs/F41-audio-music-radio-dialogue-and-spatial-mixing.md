# F41: Audio, music, radio dialogue, and spatial mixing

**Target:** 2000 PC original-data reimplementation. **Initial status:** not implemented; not original-verified.
**Prerequisite features:** F06, F14, F15, F16.
**Owner paths:** `crates/cs_content/src/audio.rs`; `crates/cs_app/src/audio/`; `crates/cs_sim/src/audio_events.rs`; `tests/`.
**Shared contract:** [IDENTITY-CONTENT](../docs/contracts/IDENTITY-CONTENT.md).

## Deliverable and interfaces

AudioCatalog maps original sample/music/dialogue ids to immutable decoded assets and playback metadata. Runtime audio consumes authoritative event ids and continuous emitter state. Separate buses for engine, weapons, impacts, environment, music, radio and UI support accessible mixing without changing game state.

## Non-negotiable behavior

1. Engine pitch/volume depend on measured throttle/engine state with stable smoothing, not render FPS. Doppler, attenuation and loops must be evidence-backed or designed options.
2. Radio dialogue has speaker, priority, interruptibility, subtitle and completion semantics. Mission progression cannot depend on an unavailable physical audio device.
3. Repeated simulation/network events cannot duplicate one-shot audio. Loop emitters stop on despawn, swap, pause policy or device loss.
4. Music transitions and authored cues are preserved; random background tracks are not a substitute. Inventory any disc or external media dependencies explicitly.
5. Record decoded PCM tests separately from audible device verification; a nonempty WAV is not proof the user heard sound.

## Acceptance tests

These are minimum discriminating tests. Add regression cases for every discovered variant. Tests call production code, not parallel test-only implementations. They must be selected with a nonempty task-specific test prefix and must fail when the relevant behavior is removed.

**AC01:** A weapon event replayed twice plays one accepted one-shot.
**AC02:** Destroy the player aircraft; engine loop ends and new aircraft loop binds correctly.
**AC03:** Lose audio device mid-mission; simulation and dialogue completion continue safely.
**AC04:** Reference radio ordering, loop seams, stereo/spatial orientation and complete media coverage.

## Bounded implementation slices

### F41-A: Define audio catalog, buses and event identity

Dependencies: F06-A, F14-A, F15-A, F16-A. Required capabilities: ordinary build/test.

Work only on this stage. Define typed inputs/outputs and a minimal synthetic fixture first; do not jump ahead to a whole runtime.

Required task-test prefix: `accept_f41_a_`. Minimum scenario: A weapon event replayed twice plays one accepted one-shot.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F41-B: Implement decoding, loops and spatial emitters

Dependencies: F41-A, F06-C, F14-C, F15-C, F16-C. Required capabilities: ordinary build/test.

Work only on this stage. Implement the smallest production path that exercises the declared behavior; keep unrelated systems unchanged.

Required task-test prefix: `accept_f41_b_`. Minimum scenario: Destroy the player aircraft; engine loop ends and new aircraft loop binds correctly.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F41-C: Wire radio queue, music transitions and subtitles

Dependencies: F41-B. Required capabilities: ordinary build/test.

Work only on this stage. Wire the implemented path into its actual producer and consumer; include teardown/retry and error propagation.

Required task-test prefix: `accept_f41_c_`. Minimum scenario: Lose audio device mid-mission; simulation and dialogue completion continue safely.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F41-D: Perform original-media audit plus real audible playback review

Dependencies: F41-C. Required capabilities: retail, audio.

Work only on this stage. Collect actual integration/reference evidence; repair discovered regressions without weakening the specification. Missing capabilities or original data block this stage.

Required task-test prefix: `accept_f41_d_`. Minimum scenario: Reference radio ordering, loop seams, stereo/spatial orientation and complete media coverage.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.

## Evidence and completion

Record the actual input fingerprint and consumer trace. Synthetic fixtures alone cannot certify original-data behavior.

A code/test pass awards at most **checked**. The reviewer must inspect runtime wiring, error paths, stale-state handling and test sensitivity. Data-dependent claims require source span, installation hash and an independent reference/probe. Visual, audio and ordinary-play claims require their respective capabilities. Developer placeholders never satisfy a retail acceptance case.

## Research boundary

No unrecorded compatibility assumptions are allowed.

## References

[S02](../docs/research/SOURCES.md); [S13](../docs/research/SOURCES.md)

The requirements and tests are independently authored engineering designs. Source labels distinguish observed leads from measured original semantics; consult the evidence ledger before changing gameplay values.
