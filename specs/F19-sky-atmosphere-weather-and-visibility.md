# F19: Sky, atmosphere, weather, and visibility

**Target:** 2000 PC original-data reimplementation. **Initial status:** not implemented; not original-verified.
**Prerequisite features:** F14, F16, F17, F18.
**Owner paths:** `crates/cs_content/src/environment.rs`; `crates/cs_app/src/environment/`; `crates/cs_sim/src/visibility.rs`; `tests/`.
**Shared contract:** [IDENTITY-CONTENT](../docs/contracts/IDENTITY-CONTENT.md).

## Deliverable and interfaces

EnvironmentDefinition separates sky art, sky orientation, fog, lighting, cloud layers, precipitation, wind and gameplay visibility. Load only effects actually authored for a mission; a visually attractive global weather randomizer is not a faithful default.

## Non-negotiable behavior

1. Unknown weather tuning remains unknown; a renderer default is tagged designed. Atmospheric visibility affecting AI is not inferred from arbitrary screen fog density.
2. Wind used by flight and projectiles is the same authoritative field. Purely decorative particles use a separate cosmetic RNG.
3. Sky assets stay centered on camera translation but honor world orientation; world rebasing cannot rotate or pop the sky.
4. Weather changes are deterministic timeline events when gameplay-relevant. Pause and replay respect their time domains.
5. Missing sky texture must be diagnostic; a generated sky can appear only in an explicitly labeled synthetic/developer profile.

## Acceptance tests

These are minimum discriminating tests. Add regression cases for every discovered variant. Tests call production code, not parallel test-only implementations. They must be selected with a nonempty task-specific test prefix and must fail when the relevant behavior is removed.

**AC01:** Rebase world under a fixed horizon; sky and sun direction stay stable.
**AC02:** Wind changes affect aircraft airspeed and projectile-relative velocity consistently.
**AC03:** Weather seeds do not change mission AI RNG sequences.
**AC04:** Capture the original environment states actually present in each world/scenario.

## Bounded implementation slices

### F19-A: Define environment data and time domains

Dependencies: F14-A, F16-A, F17-A, F18-A. Required capabilities: ordinary build/test.

Work only on this stage. Define typed inputs/outputs and a minimal synthetic fixture first; do not jump ahead to a whole runtime.

Required task-test prefix: `accept_f19_a_`. Minimum scenario: Rebase world under a fixed horizon; sky and sun direction stay stable.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F19-B: Implement sky/fog/light and discovered weather effects

Dependencies: F19-A, F14-C, F16-C, F17-C, F18-C. Required capabilities: ordinary build/test.

Work only on this stage. Implement the smallest production path that exercises the declared behavior; keep unrelated systems unchanged.

Required task-test prefix: `accept_f19_b_`. Minimum scenario: Wind changes affect aircraft airspeed and projectile-relative velocity consistently.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F19-C: Integrate authoritative wind and visibility policies

Dependencies: F19-B. Required capabilities: ordinary build/test.

Work only on this stage. Wire the implemented path into its actual producer and consumer; include teardown/retry and error propagation.

Required task-test prefix: `accept_f19_c_`. Minimum scenario: Weather seeds do not change mission AI RNG sequences.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F19-D: Verify environment variants against private mission captures

Dependencies: F19-C. Required capabilities: gpu, retail.

Work only on this stage. Collect actual integration/reference evidence; repair discovered regressions without weakening the specification. Missing capabilities or original data block this stage.

Required task-test prefix: `accept_f19_d_`. Minimum scenario: Capture the original environment states actually present in each world/scenario.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.

## Evidence and completion

Record the actual input fingerprint and consumer trace. Synthetic fixtures alone cannot certify original-data behavior.

A code/test pass awards at most **checked**. The reviewer must inspect runtime wiring, error paths, stale-state handling and test sensitivity. Data-dependent claims require source span, installation hash and an independent reference/probe. Visual, audio and ordinary-play claims require their respective capabilities. Developer placeholders never satisfy a retail acceptance case.

## Research boundary

No unrecorded compatibility assumptions are allowed.

## References

[S03](../docs/research/SOURCES.md); [S08](../docs/research/SOURCES.md)

The requirements and tests are independently authored engineering designs. Source labels distinguish observed leads from measured original semantics; consult the evidence ledger before changing gameplay values.
