# F58: Network abuse resistance, reconnect, and match recovery

**Target:** 2000 PC original-data reimplementation. **Initial status:** not implemented; not original-verified.
**Prerequisite features:** F29, F36, F54, F55, F56, F57.
**Owner paths:** `crates/cs_net/src/validation.rs`; `crates/cs_net/src/recovery.rs`; `crates/cs_app/src/network/recovery.rs`; `tests/`.
**Shared contract:** [UI-NETWORK](../docs/contracts/UI-NETWORK.md).

## Deliverable and interfaces

Validate all client intents against server-owned actors, loadouts, rates, tick windows and match state. Reconnect uses fresh authenticated session identity and a full authoritative snapshot; a client-provided score, health, faction or outcome is never accepted.

## Non-negotiable behavior

1. Prevent stale-session packets, invalid ownership, impossible rates, oversized messages and resource exhaustion. Disconnect abusive peers with a bounded reason, not a process panic.
2. No automatic port forwarding or credentials/service accounts created without explicit owner consent. Direct Internet hosting documents firewall/port exposure and privacy boundaries.
3. Host migration is optional designed scope; if absent, host departure ends the match cleanly and returns clients to menu. Do not claim seamless recovery without implementation.
4. Reconnect or late join cannot replay pickup/capture rewards or bind to another pilots aircraft.
5. Network tests run without private credentials; logs redact passwords and remote identifying data beyond needed diagnostics.

## Acceptance tests

These are minimum discriminating tests. Add regression cases for every discovered variant. Tests call production code, not parallel test-only implementations. They must be selected with a nonempty task-specific test prefix and must fail when the relevant behavior is removed.

**AC01:** Replay a prior-session fire packet and prove no projectile spawns.
**AC02:** Client requests damage/score directly; server rejects it.
**AC03:** Disconnect during docking or objective carry; authoritative state resolves once.
**AC04:** Flood invalid packets within designed limits without unbounded CPU/memory growth.

## Bounded implementation slices

### F58-A: Define threat cases and session identity rules

Dependencies: F29-A, F36-A, F54-A, F55-A, F56-A, F57-A. Required capabilities: ordinary build/test.

Work only on this stage. Define typed inputs/outputs and a minimal synthetic fixture first; do not jump ahead to a whole runtime.

Required task-test prefix: `accept_f58_a_`. Minimum scenario: Replay a prior-session fire packet and prove no projectile spawns.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F58-B: Implement intent validation and rate/resource caps

Dependencies: F58-A, F29-C, F36-C, F54-C, F55-C, F56-C, F57-C. Required capabilities: ordinary build/test.

Work only on this stage. Implement the smallest production path that exercises the declared behavior; keep unrelated systems unchanged.

Required task-test prefix: `accept_f58_b_`. Minimum scenario: Client requests damage/score directly; server rejects it.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F58-C: Add reconnect/disconnect and clean host-loss flows

Dependencies: F58-B. Required capabilities: ordinary build/test.

Work only on this stage. Wire the implemented path into its actual producer and consumer; include teardown/retry and error propagation.

Required task-test prefix: `accept_f58_c_`. Minimum scenario: Disconnect during docking or objective carry; authoritative state resolves once.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F58-D: Run adversarial packet and interrupted-match matrix

Dependencies: F58-C. Required capabilities: network_real.

Work only on this stage. Collect actual integration/reference evidence; repair discovered regressions without weakening the specification. Missing capabilities or original data block this stage.

Required task-test prefix: `accept_f58_d_`. Minimum scenario: Flood invalid packets within designed limits without unbounded CPU/memory growth.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.

## Evidence and completion

Record the actual input fingerprint and consumer trace. Synthetic fixtures alone cannot certify original-data behavior.

A code/test pass awards at most **checked**. The reviewer must inspect runtime wiring, error paths, stale-state handling and test sensitivity. Data-dependent claims require source span, installation hash and an independent reference/probe. Visual, audio and ordinary-play claims require their respective capabilities. Developer placeholders never satisfy a retail acceptance case.

## Research boundary

No unrecorded compatibility assumptions are allowed.

## References

Project-designed engineering contract; no original behavior is asserted by the design alone.

The requirements and tests are independently authored engineering designs. Source labels distinguish observed leads from measured original semantics; consult the evidence ledger before changing gameplay values.
