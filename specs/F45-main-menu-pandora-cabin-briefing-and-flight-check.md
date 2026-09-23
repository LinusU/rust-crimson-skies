# F45: Main menu, Pandora cabin, briefing, and flight check

**Target:** 2000 PC original-data reimplementation. **Initial status:** not implemented; not original-verified.
**Prerequisite features:** F14, F15, F21, F22, F40, F41, F43, F44, F48, F51.
**Owner paths:** `crates/cs_app/src/ui/front_end/`; `crates/cs_content/src/ui_layout.rs`; `crates/cs_app/tests/ui/`.
**Shared contract:** [UI-NETWORK](../docs/contracts/UI-NETWORK.md).

## Deliverable and interfaces

Implement the complete application state machine from boot/install selection through main menu, profile, campaign cabin, briefing/recon, construction, flight check, loading, flight, pause/map, results and return. Original artwork/hotspots/voice are loaded where available; modern navigation is an accessible wrapper, not missing-screen placeholders.

## Non-negotiable behavior

1. Every screen has valid Back/Cancel behavior and keyboard/controller focus. A visible button must have a functioning state transition.
2. Briefing replay and recon images do not start the mission prematurely. Flight Check atomically selects player/wingmate aircraft and ammunition.
3. Do not silently replace the cabin/scrapbook/construction flow with a bare debug mission picker for release.
4. Loading failure returns to a coherent selection state without losing the draft or corrupting profile.
5. Screen transitions explicitly acquire/release input, audio and world resources. Returning to menu cannot leave the old world simulating.

## Acceptance tests

These are minimum discriminating tests. Add regression cases for every discovered variant. Tests call production code, not parallel test-only implementations. They must be selected with a nonempty task-specific test prefix and must fail when the relevant behavior is removed.

**AC01:** Run a UI automation path from fresh profile to first flight and back without CLI shortcuts.
**AC02:** Cancel at every preflight screen and verify state/currency unchanged.
**AC03:** Retry a failed load after repairing its dependency without restarting the app.
**AC04:** Capture and review all original front-end screens and navigation paths.

## Bounded implementation slices

### F45-A: Define frontend states and transition table

Dependencies: F14-A, F15-A, F21-A, F22-A, F40-A, F41-A, F43-A, F44-A, F48-A, F51-A. Required capabilities: ordinary build/test.

Work only on this stage. Define typed inputs/outputs and a minimal synthetic fixture first; do not jump ahead to a whole runtime.

Required task-test prefix: `accept_f45_a_`. Minimum scenario: Run a UI automation path from fresh profile to first flight and back without CLI shortcuts.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F45-B: Implement original-asset menu/cabin/briefing screens

Dependencies: F45-A, F14-C, F15-C, F21-C, F22-C, F40-C, F41-C, F43-C, F44-C, F48-C, F51-C. Required capabilities: ordinary build/test.

Work only on this stage. Implement the smallest production path that exercises the declared behavior; keep unrelated systems unchanged.

Required task-test prefix: `accept_f45_b_`. Minimum scenario: Cancel at every preflight screen and verify state/currency unchanged.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F45-C: Wire construction, flight check, loading and return flows

Dependencies: F45-B. Required capabilities: ordinary build/test.

Work only on this stage. Wire the implemented path into its actual producer and consumer; include teardown/retry and error propagation.

Required task-test prefix: `accept_f45_c_`. Minimum scenario: Retry a failed load after repairing its dependency without restarting the app.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F45-D: Run end-to-end keyboard/controller campaign navigation review

Dependencies: F45-C. Required capabilities: gpu, retail.

Work only on this stage. Collect actual integration/reference evidence; repair discovered regressions without weakening the specification. Missing capabilities or original data block this stage.

Required task-test prefix: `accept_f45_d_`. Minimum scenario: Capture and review all original front-end screens and navigation paths.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.

## Evidence and completion

Record the actual input fingerprint and consumer trace. Synthetic fixtures alone cannot certify original-data behavior.

A code/test pass awards at most **checked**. The reviewer must inspect runtime wiring, error paths, stale-state handling and test sensitivity. Data-dependent claims require source span, installation hash and an independent reference/probe. Visual, audio and ordinary-play claims require their respective capabilities. Developer placeholders never satisfy a retail acceptance case.

## Research boundary

No unrecorded compatibility assumptions are allowed.

## References

[S13](../docs/research/SOURCES.md)

The requirements and tests are independently authored engineering designs. Source labels distinguish observed leads from measured original semantics; consult the evidence ledger before changing gameplay values.
