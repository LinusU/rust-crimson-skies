# F46: HUD, instruments, mission map, and pause

**Target:** 2000 PC original-data reimplementation. **Initial status:** not implemented; not original-verified.
**Prerequisite features:** F21, F22, F24, F27, F28, F29, F30, F39, F45.
**Owner paths:** `crates/cs_app/src/ui/hud/`; `crates/cs_content/src/hud.rs`; `crates/cs_app/tests/hud/`.
**Shared contract:** [UI-NETWORK](../docs/contracts/UI-NETWORK.md).

## Deliverable and interfaces

HUD data is a read-only projection of authoritative aircraft and mission state: speed, altitude, attitude, compass, selected weapons/ammo, damage, target/spyglass and objectives. Mission map shows authored geography, objectives/recon and valid pause commands. Original instruments and modern accessibility overlays coexist as explicit settings.

## Non-negotiable behavior

1. Show original display units where verified while simulation remains SI. Airspeed and ground speed, altitude datum and low-altitude warnings require explicit definitions.
2. Do not expose hidden objectives or enemies through a new map overlay in fidelity mode.
3. UI scaling preserves aspect and safe margins. Cockpit gauges and text remain legible at declared resolutions; no stretch-to-fill artwork.
4. Single-player pause freezes simulation; menu/audio/media policies remain explicit. Network pause is not server pause.
5. No stale ammo, damage or target on plane swap. UI derives from current session generation and actor binding.

## Acceptance tests

These are minimum discriminating tests. Add regression cases for every discovered variant. Tests call production code, not parallel test-only implementations. They must be selected with a nonempty task-specific test prefix and must fail when the relevant behavior is removed.

**AC01:** Known attitude quaternion produces the expected horizon and heading.
**AC02:** Zero ammo changes gauge state immediately without changing weapon selection unexpectedly.
**AC03:** Swap aircraft with different weapons and cockpit; all instruments rebind.
**AC04:** Compare original cockpit/map views and review accessibility at multiple resolutions.

## Bounded implementation slices

### F46-A: Define instrument values and display-unit policies

Dependencies: F21-A, F22-A, F24-A, F27-A, F28-A, F29-A, F30-A, F39-A, F45-A. Required capabilities: ordinary build/test.

Work only on this stage. Define typed inputs/outputs and a minimal synthetic fixture first; do not jump ahead to a whole runtime.

Required task-test prefix: `accept_f46_a_`. Minimum scenario: Known attitude quaternion produces the expected horizon and heading.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F46-B: Implement HUD, original gauges and target display

Dependencies: F46-A, F21-C, F22-C, F24-C, F27-C, F28-C, F29-C, F30-C, F39-C, F45-C. Required capabilities: ordinary build/test.

Work only on this stage. Implement the smallest production path that exercises the declared behavior; keep unrelated systems unchanged.

Required task-test prefix: `accept_f46_b_`. Minimum scenario: Zero ammo changes gauge state immediately without changing weapon selection unexpectedly.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F46-C: Wire map, objective/recon pages and pause behavior

Dependencies: F46-B. Required capabilities: ordinary build/test.

Work only on this stage. Wire the implemented path into its actual producer and consumer; include teardown/retry and error propagation.

Required task-test prefix: `accept_f46_c_`. Minimum scenario: Swap aircraft with different weapons and cockpit; all instruments rebind.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F46-D: Validate full HUD correctness and multi-resolution readability

Dependencies: F46-C. Required capabilities: gpu, retail.

Work only on this stage. Collect actual integration/reference evidence; repair discovered regressions without weakening the specification. Missing capabilities or original data block this stage.

Required task-test prefix: `accept_f46_d_`. Minimum scenario: Compare original cockpit/map views and review accessibility at multiple resolutions.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.

## Evidence and completion

Record the actual input fingerprint and consumer trace. Synthetic fixtures alone cannot certify original-data behavior.

A code/test pass awards at most **checked**. The reviewer must inspect runtime wiring, error paths, stale-state handling and test sensitivity. Data-dependent claims require source span, installation hash and an independent reference/probe. Visual, audio and ordinary-play claims require their respective capabilities. Developer placeholders never satisfy a retail acceptance case.

## Research boundary

No unrecorded compatibility assumptions are allowed.

## References

[S13](../docs/research/SOURCES.md)

The requirements and tests are independently authored engineering designs. Source labels distinguish observed leads from measured original semantics; consult the evidence ledger before changing gameplay values.
