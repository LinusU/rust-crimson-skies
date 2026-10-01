# F54-A: wire messages and authoritative ownership — design notes and what stays open

Date: 2026-10-01. Task: F54-A "Define wire messages and authoritative
ownership" (`specs/F54-modern-multiplayer-transport-and-authority-protocol.md`).
Capabilities used: ordinary build/test only. All values are newly authored
engine design; nothing derives from the original game or from a claimed
original network protocol (legacy DirectPlay/MSN/IPX interoperability is
explicitly out of scope per the sheet).

## Files and the observable failure

- `crates/cs_types/src/net.rs` (new): `SessionId` (the wire's session epoch),
  `PeerId`, the contract-shaped `ActorId { session, serial }` and
  `EventId { session, tick, producer, sequence }`, and the server-side
  `ActorAllocator` minting non-recycled serials. All ids are nonzero so a
  default or truncated field can never alias a live one.
- `crates/cs_net/src/compat.rs`: `ProtocolVersion`/`PROTOCOL_VERSION`,
  `Compatibility` (rules + content signatures + enabled mod set),
  `ClientHello`, `SessionParameters`, `HandshakeReject`, `HelloReply` /
  `SessionGrant`, and the pure `evaluate_hello` gate.
- `crates/cs_net/src/message.rs`: `MessageHeader` (epoch + sequence),
  `ClientPayload`/`ServerPayload`, `SessionMessage` with `verify_origin`,
  `expect_session` epoch checks, `Delivery` (Reliable vs Sequenced),
  `InputBatch`, `ReliableEvent`/`EventBody`, `SnapshotFrame` (bounded opaque
  payload pending F57-A) and `WireError` bounds validation.
- `crates/cs_net/src/authority.rs`: the UI-NETWORK ownership table as data
  (`AuthorityDomain::owner`).
- `crates/cs_net/src/bounds.rs`: every packet size/count cap in one place.
- `crates/cs_net/src/fixture.rs`: the minimal synthetic session fixture.
- `crates/cs_net/tests/accept_f54_a_wire_contract.rs`: 15 tests.
- Wiring edits: `pub mod net;` and a doc paragraph in
  `crates/cs_types/src/lib.rs`; `cs_net/src/lib.rs` rewritten (module
  declarations and crate docs only — it was a doc-only stub).
- Observable failure without the implementation: there is no handshake to
  evaluate — `evaluate_hello(&params, &hello)` does not exist, so an
  unsupported protocol or content hash cannot be rejected with a reason
  (`accept_f54_a_unsupported_protocol_is_rejected_with_reason`,
  `accept_f54_a_content_hash_mismatch_is_rejected_with_reason`).

## Decisions

- **Pre-session vs in-session split.** `ClientHello`/`HelloReply` are
  unsequenced (no session exists yet); everything after `Welcome` is a
  `SessionMessage` whose `MessageHeader` carries `session` + `sequence`
  (contract: "messages carry session epoch and sequence/tick").
- **`evaluate_hello` is pure.** It validates the offer, then compares
  protocol, rules hash, content hash and the exact mod set in that order;
  the first mismatch is the named `HandshakeReject`. Committing session
  state is the host runtime's job (F54-B/C), so a mismatch can never reach
  launch or leave half a session.
- **Authority is structural, then checked.** Server-owned payloads (spawn,
  snapshot, events, disconnect) only exist as `ServerPayload` variants — a
  `ClientPayload` cannot express them — and `SessionMessage::verify_origin`
  rejects a packet that claims the wrong side at the codec boundary. The
  exhaustive `match` in `accept_f54_a_client_payloads_are_requests_not_
  authority` fails to compile if a client verb is ever added without review.
- **Finite numerics are structural.** No wire field carries a float; input
  axes arrive as F22-A `i16`-quantized `AxisValue`s. Validation covers what
  is actually checkable: counts, tick order/span and the UI-action ban
  (`UiAction` is client-owned and never crosses the wire).
- **Events dedup on `EventId`, not on delivery.** `ReliableEvent.id` carries
  `(session, tick, producer, sequence)`; reconnect/retry replays collapse to
  the same id (contract: "Reliable delivery does not replace application
  idempotency").
- **`InputAck { through }`** is the application-level input acknowledgment
  the contract requires ("Inputs have sequence acknowledgment"); input
  packets are `Sequenced`, so loss drops stale frames while acks bound the
  client's resend window. Dedup key for input packets is the header
  `sequence`.
- **`SnapshotFrame.payload` is bounded opaque bytes.** The quantized actor
  record schema is F57-A's `snapshot.rs`; the envelope caps it at
  `MAX_SNAPSHOT_BYTES` and never interprets it. This is a declared layering
  boundary, not a guessed schema.
- **All bounds are designed values** (`bounds.rs` module doc): no original
  network budget is known or claimed.

## Open / not claimed (resolving stages)

- **Transport choice is unmade.** F54 non-negotiable 1 (one maintained Rust
  transport after a documented API/license evaluation, version frozen)
  belongs to F54-B "Implement one pinned transport and handshake". No
  transport crate was added.
- **No codec yet.** `MAX_PACKET_BYTES` is declared but only enforceable once
  F54-B/C encode packets; message `validate()` covers typed bounds.
- **`SessionId`/`PeerId` allocation** on the host is F54-B/C runtime work;
  `ActorAllocator` is the A-stage seed of server-side id ownership.
- **Snapshot payload schema** (`snapshot.rs`): F57-A. **Lobby vocabulary**
  (callsign, readiness, rules revisions, password): F55-A (`lobby.rs`).
  **Session threat/reconnect/identity rules**: F58-A. Gameplay-domain event
  kinds (hits, objectives, dialogue, score lines) are declared by their
  owning stages and travel through `ReliableEvent`.
- **`cs_app/src/network/` untouched**: there is no transport to wire into
  the app state machine yet; the Bevy-side session glue is F54-C's "wire
  the implemented path into its actual producer and consumer".
- **Shared-id migration**: `cs_sim::damage::ActorId` and
  `cs_script::ir::ActorId` predate `cs_types::net::ActorId` (recorded in the
  F29-A findings). Migrating them onto the shared type is follow-up work;
  `cs_sim`/`cs_script` are outside this task's owner paths.
- **`network_real` evidence** stays gated on F54-D; nothing here is marked
  more than *checked*.
