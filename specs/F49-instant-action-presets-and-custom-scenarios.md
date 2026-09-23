# F49: Instant Action presets and custom scenarios

**Target:** 2000 PC original-data reimplementation. **Initial status:** not implemented; not original-verified.
**Prerequisite features:** F14, F18, F24, F27, F28, F32, F33, F37, F44, F45.
**Owner paths:** `crates/cs_content/src/instant_action.rs`; `crates/cs_sim/src/scenario.rs`; `crates/cs_app/src/ui/instant_action/`; `tests/`.
**Shared contract:** [UI-NETWORK](../docs/contracts/UI-NETWORK.md).

## Deliverable and interfaces

InstantActionCatalog includes every original preset and all supported custom-scenario dimensions discovered from data/UI. Custom scenarios select environment, allied/enemy roster, loadouts, skill and victory rules through the same validators and mission runtime used by campaign.

## Non-negotiable behavior

1. An IA mission is not just campaign launch with rewards disabled if its authored scenario differs. Preserve original preset identities and parameters.
2. Custom scenario validation prevents impossible factions, unknown planes, unsupported ordnance and invalid player count/roster configurations.
3. IA never modifies campaign progression or money; records and custom blueprints use their explicit profile scopes.
4. Deterministic scenario seed is displayed in developer tools and captured in replays; no randomization hidden from evidence.
5. Every visible customization option affects the actual spawned scenario, not only UI text.

## Acceptance tests

These are minimum discriminating tests. Add regression cases for every discovered variant. Tests call production code, not parallel test-only implementations. They must be selected with a nonempty task-specific test prefix and must fail when the relevant behavior is removed.

**AC01:** Launch each discovered preset and verify its expected actors, world and rules.
**AC02:** Change one custom roster slot and confirm only the intended actor changes.
**AC03:** Complete/retry IA and verify campaign cash/progression unchanged.
**AC04:** Empty/invalid rosters give actionable validation errors instead of endless sessions.

## Bounded implementation slices

### F49-A: Define preset/custom scenario schemas

Dependencies: F14-A, F18-A, F24-A, F27-A, F28-A, F32-A, F33-A, F37-A, F44-A, F45-A. Required capabilities: ordinary build/test.

Work only on this stage. Define typed inputs/outputs and a minimal synthetic fixture first; do not jump ahead to a whole runtime.

Required task-test prefix: `accept_f49_a_`. Minimum scenario: Launch each discovered preset and verify its expected actors, world and rules.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F49-B: Implement scenario normalization and isolated outcomes

Dependencies: F49-A, F14-C, F18-C, F24-C, F27-C, F28-C, F32-C, F33-C, F37-C, F44-C, F45-C. Required capabilities: ordinary build/test.

Work only on this stage. Implement the smallest production path that exercises the declared behavior; keep unrelated systems unchanged.

Required task-test prefix: `accept_f49_b_`. Minimum scenario: Change one custom roster slot and confirm only the intended actor changes.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F49-C: Build IA selection/customization/loadout UI

Dependencies: F49-B. Required capabilities: ordinary build/test.

Work only on this stage. Wire the implemented path into its actual producer and consumer; include teardown/retry and error propagation.

Required task-test prefix: `accept_f49_c_`. Minimum scenario: Complete/retry IA and verify campaign cash/progression unchanged.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F49-D: Verify the complete original Instant Action catalog

Dependencies: F49-C. Required capabilities: retail.

Work only on this stage. Collect actual integration/reference evidence; repair discovered regressions without weakening the specification. Missing capabilities or original data block this stage.

Required task-test prefix: `accept_f49_d_`. Minimum scenario: Empty/invalid rosters give actionable validation errors instead of endless sessions.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.

## Evidence and completion

Record the actual input fingerprint and consumer trace. Synthetic fixtures alone cannot certify original-data behavior.

A code/test pass awards at most **checked**. The reviewer must inspect runtime wiring, error paths, stale-state handling and test sensitivity. Data-dependent claims require source span, installation hash and an independent reference/probe. Visual, audio and ordinary-play claims require their respective capabilities. Developer placeholders never satisfy a retail acceptance case.

## Research boundary

No unrecorded compatibility assumptions are allowed.

## References

[S13](../docs/research/SOURCES.md)

The requirements and tests are independently authored engineering designs. Source labels distinguish observed leads from measured original semantics; consult the evidence ledger before changing gameplay values.
