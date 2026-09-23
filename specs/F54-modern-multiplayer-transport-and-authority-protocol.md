# F54: Modern multiplayer transport and authority protocol

**Target:** 2000 PC original-data reimplementation. **Initial status:** not implemented; not original-verified.
**Prerequisite features:** F14, F16, F22, F37, F48.
**Owner paths:** `crates/cs_net/`; `crates/cs_types/src/net.rs`; `crates/cs_app/src/network/`; `tests/`.
**Shared contract:** [UI-NETWORK](../docs/contracts/UI-NETWORK.md).

## Deliverable and interfaces

Implement a new versioned multiplayer protocol for this reimplementation. Host/server owns simulation, spawning, mission rules, damage and score. Clients submit bounded tick-stamped inputs and receive snapshots/reliable semantic events. Legacy DirectPlay/MSN/IPX interoperability is explicitly outside this release scope.

## Non-negotiable behavior

1. Choose one maintained Rust transport after a documented API/license evaluation; do not invent a secure protocol or silently add multiple network libraries. Freeze the selected version.
2. Handshake includes protocol, engine rules, installation/canonical-content compatibility signatures and enabled mods. Mismatches fail before launch.
3. Packets have size/count/rate caps, validated ids and finite numeric fields. No arbitrary asset transfer, filenames or deserialization of executable behavior.
4. Use reliable delivery for lifecycle/rules and sequenced snapshots for motion; application-level ids deduplicate events.
5. Do not assume deterministic lockstep across hardware. Server authority with prediction/interpolation is the baseline.

## Acceptance tests

These are minimum discriminating tests. Add regression cases for every discovered variant. Tests call production code, not parallel test-only implementations. They must be selected with a nonempty task-specific test prefix and must fail when the relevant behavior is removed.

**AC01:** Unsupported protocol/content hash is rejected with a clear reason.
**AC02:** Duplicate/out-of-order inputs cannot duplicate fire or score.
**AC03:** Fuzz packet decoding with oversized counts, NaNs and invalid ids.
**AC04:** Run two separate local processes through connect/launch/finish/disconnect.

## Bounded implementation slices

### F54-A: Define wire messages and authoritative ownership

Dependencies: F14-A, F16-A, F22-A, F37-A, F48-A. Required capabilities: ordinary build/test.

Work only on this stage. Define typed inputs/outputs and a minimal synthetic fixture first; do not jump ahead to a whole runtime.

Required task-test prefix: `accept_f54_a_`. Minimum scenario: Unsupported protocol/content hash is rejected with a clear reason.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F54-B: Implement one pinned transport and handshake

Dependencies: F54-A, F14-C, F16-C, F22-C, F37-C, F48-C. Required capabilities: ordinary build/test.

Work only on this stage. Implement the smallest production path that exercises the declared behavior; keep unrelated systems unchanged.

Required task-test prefix: `accept_f54_b_`. Minimum scenario: Duplicate/out-of-order inputs cannot duplicate fire or score.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F54-C: Wire server/client lifecycle and bounded message processing

Dependencies: F54-B. Required capabilities: ordinary build/test.

Work only on this stage. Wire the implemented path into its actual producer and consumer; include teardown/retry and error propagation.

Required task-test prefix: `accept_f54_c_`. Minimum scenario: Fuzz packet decoding with oversized counts, NaNs and invalid ids.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F54-D: Verify loopback plus actual two-machine connectivity and protocol resilience

Dependencies: F54-C. Required capabilities: network_real.

Work only on this stage. Collect actual integration/reference evidence; repair discovered regressions without weakening the specification. Missing capabilities or original data block this stage.

Required task-test prefix: `accept_f54_d_`. Minimum scenario: Run two separate local processes through connect/launch/finish/disconnect.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.

## Evidence and completion

Network synthetic tests are not proof of original multiplayer content. Multiplayer content catalog parity is F56; real-network evidence is required for release.

A code/test pass awards at most **checked**. The reviewer must inspect runtime wiring, error paths, stale-state handling and test sensitivity. Data-dependent claims require source span, installation hash and an independent reference/probe. Visual, audio and ordinary-play claims require their respective capabilities. Developer placeholders never satisfy a retail acceptance case.

## Research boundary

No unrecorded compatibility assumptions are allowed.

## References

[S13](../docs/research/SOURCES.md)

The requirements and tests are independently authored engineering designs. Source labels distinguish observed leads from measured original semantics; consult the evidence ledger before changing gameplay values.
