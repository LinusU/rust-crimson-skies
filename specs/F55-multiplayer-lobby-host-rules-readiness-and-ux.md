# F55: Multiplayer lobby, host rules, readiness, and UX

**Target:** 2000 PC original-data reimplementation. **Initial status:** not implemented; not original-verified.
**Prerequisite features:** F44, F45, F51, F53, F54.
**Owner paths:** `crates/cs_app/src/ui/lobby/`; `crates/cs_net/src/lobby.rs`; `crates/cs_content/src/lobby.rs`; `tests/`.
**Shared contract:** [UI-NETWORK](../docs/contracts/UI-NETWORK.md).

## Deliverable and interfaces

Lobby supports host/join by direct address and LAN discovery when enabled, callsign, pilot voice, team, ping, password/access policy, host options, loadout validation, readiness and launch. Original available component/ammunition restrictions are preserved through shared validators.

## Non-negotiable behavior

1. Host changes to scenario, banned components or teams increment a rules revision and invalidate affected readiness/loadouts.
2. No client can launch, grant itself host authority or submit an invalid blueprint. Launch is an atomic acknowledged transition bound to a rules hash.
3. Discovery and Internet connectivity are distinct. Do not label a LAN broadcast test as Internet matchmaking; no mandatory proprietary service.
4. Disconnected clients leave cleanly; late join rules are explicit per mode. A dead host ends or pauses according to a designed policy, not accidental undefined state.
5. Chat is bounded, escaped text with mute controls; do not render markup or share secrets in logs.

## Acceptance tests

These are minimum discriminating tests. Add regression cases for every discovered variant. Tests call production code, not parallel test-only implementations. They must be selected with a nonempty task-specific test prefix and must fail when the relevant behavior is removed.

**AC01:** Host bans a selected weapon after clients ready; ready state is revoked and reason displayed.
**AC02:** Launch packet for an old rules revision is rejected.
**AC03:** Wrong password, full lobby and content mismatch return distinct errors.
**AC04:** Two computers join, ready, launch, finish and return to lobby without restarting.

## Bounded implementation slices

### F55-A: Define lobby state and revision protocol

Dependencies: F44-A, F45-A, F51-A, F53-A, F54-A. Required capabilities: ordinary build/test.

Work only on this stage. Define typed inputs/outputs and a minimal synthetic fixture first; do not jump ahead to a whole runtime.

Required task-test prefix: `accept_f55_a_`. Minimum scenario: Host bans a selected weapon after clients ready; ready state is revoked and reason displayed.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F55-B: Implement host validation, readiness and membership

Dependencies: F55-A, F44-C, F45-C, F51-C, F53-C, F54-C. Required capabilities: ordinary build/test.

Work only on this stage. Implement the smallest production path that exercises the declared behavior; keep unrelated systems unchanged.

Required task-test prefix: `accept_f55_b_`. Minimum scenario: Launch packet for an old rules revision is rejected.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F55-C: Build join/host/team/loadout/chat UI

Dependencies: F55-B. Required capabilities: ordinary build/test.

Work only on this stage. Wire the implemented path into its actual producer and consumer; include teardown/retry and error propagation.

Required task-test prefix: `accept_f55_c_`. Minimum scenario: Wrong password, full lobby and content mismatch return distinct errors.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F55-D: Verify original lobby options and real-network user flows

Dependencies: F55-C. Required capabilities: network_real.

Work only on this stage. Collect actual integration/reference evidence; repair discovered regressions without weakening the specification. Missing capabilities or original data block this stage.

Required task-test prefix: `accept_f55_d_`. Minimum scenario: Two computers join, ready, launch, finish and return to lobby without restarting.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.

## Evidence and completion

Record the actual input fingerprint and consumer trace. Synthetic fixtures alone cannot certify original-data behavior.

A code/test pass awards at most **checked**. The reviewer must inspect runtime wiring, error paths, stale-state handling and test sensitivity. Data-dependent claims require source span, installation hash and an independent reference/probe. Visual, audio and ordinary-play claims require their respective capabilities. Developer placeholders never satisfy a retail acceptance case.

## Research boundary

No unrecorded compatibility assumptions are allowed.

## References

[S13](../docs/research/SOURCES.md)

The requirements and tests are independently authored engineering designs. Source labels distinguish observed leads from measured original semantics; consult the evidence ledger before changing gameplay values.
