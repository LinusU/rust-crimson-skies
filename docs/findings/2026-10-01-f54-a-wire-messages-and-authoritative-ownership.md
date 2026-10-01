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
  `SessionGrant`, the pure `evaluate_hello` gate, the host-side
  `PeerAllocator` (bounded by `MAX_SESSION_PEERS`, non-recycled) and
  `admit_hello`, which turns the gate into the `HelloReply` the host sends.
- `crates/cs_net/src/message.rs`: `MessageHeader` (epoch + sequence),
  `ClientPayload`/`ServerPayload`, `SessionMessage` with `verify_origin`,
  `expect_session` epoch checks, `Delivery` (Reliable vs Sequenced),
  `InputBatch`, `ReliableEvent`/`EventBody`, `SnapshotFrame` (bounded opaque
  payload pending F57-A) and `WireError` bounds validation.
- `crates/cs_net/src/authority.rs`: the UI-NETWORK ownership table as data
  (`AuthorityDomain::owner`).
- `crates/cs_net/src/bounds.rs`: every packet size/count cap in one place.
- `crates/cs_net/src/fixture.rs`: the minimal synthetic session fixture.
- `crates/cs_net/tests/accept_f54_a_wire_contract.rs`: 19 tests.
- Wiring edits: `pub mod net;` and a doc paragraph in
  `crates/cs_types/src/lib.rs`; `cs_net/src/lib.rs` rewritten (module
  declarations and crate docs only — it was a doc-only stub).
- Observable failure without the implementation: there is no handshake to
  evaluate — `evaluate_hello(&params, &hello)` does not exist, so an
  unsupported protocol or content hash cannot be rejected with a reason
  (`accept_f54_a_unsupported_protocol_is_rejected_with_reason`,
  `accept_f54_a_content_hash_mismatch_is_rejected_with_reason`).

## Review pass (bunny-alpha-1, independent reviewer, fresh context)

The implementation on the branch was authored by an earlier agent session. The
review found no functional bug in the handshake gate, epoch rejection,
directional authority or bounds; it found three gaps, all now fixed on the
branch:

1. **A test that asserted nothing.** `accept_f54_a_client_payloads_are_requests_
   not_authority` matched on the two `ClientPayload` variants with empty arms
   and no assertion. It now states each verb's contract class in the
   exhaustive match *and* asserts it, through both `ClientPayload::delivery`
   and the `ClientMessage` envelope's forwarding, and checks the packet's
   origin at the codec boundary.
2. **Untested, unconsumed declared bounds.** `MAX_SESSION_PEERS` had no
   consumer at all, and the `CompatError::TooManyMods` variant no test. The
   host side of "authoritative ownership" now has the same seed the actor side
   had: `PeerAllocator` mints bounded, non-recycled, nonzero `PeerId`s, and
   `admit_hello` is the host's admission decision — `evaluate_hello` first and
   pure, so **a mismatch never consumes a peer id**, and a session at
   `MAX_SESSION_PEERS` is itself a named rejection
   (`HandshakeReject::SessionFull`). A full cap and the order-insensitivity of
   the mod-set comparison are both tested, plus an internal-consistency test
   (`MAX_SNAPSHOT_BYTES <= MAX_PACKET_BYTES`, a batch at the frame cap fits
   inside the span cap, the peer cap fits the nonzero `u16` peer space).
3. **A wrong rejection reason.** `HandshakeReject::MalformedHello` was also
   used when the *host's own* `SessionParameters` were malformed, so a
   rejection sent to the client read "invalid hello: ..." and blamed a peer for
   a defect that may be the host's. The variant is now
   `MalformedSignature` and reads "malformed compatibility signature: ...".
   `HelloReply`/`SessionGrant` and `MAX_PACKET_BYTES` now also have tests or a
   stated consumer.
4. **An overstated invariant in `cs_types/src/net.rs`.** The module doc claimed
   "every id is an integer newtype that is never zero", but `ActorId::serial`
   and `EventId::{producer, sequence}` are plain public fields — and the
   synthetic fixture itself uses `producer: 0`. The guarantee is real but
   narrower: the zero sentinel covers the `SessionId`/`PeerId` newtypes, while
   for the compound ids it is `ActorAllocator` issuing serials from 1 that
   keeps serial 0 (and no live event) unreachable. The docs now say exactly
   that instead of claiming more than the types enforce.

`MAX_PACKET_BYTES` remains unenforced by production code: there is no codec at
this stage, which is stated below and belongs to F54-B/C.

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
- **The gate is pure, so admission is ordered.** `admit_hello` runs
  `evaluate_hello` first and only then allocates a peer id, so a turned-away
  client cannot spend a session's membership; the host's `SessionId` is passed
  in because the source of session generations is F54-B/C runtime work.
  Peer ids are not recycled inside a session (a departed peer's id must never
  alias a later arrival), so a long session with churn is bounded by
  `MAX_SESSION_PEERS` and refuses further joins with a named reason.
- **The mod set is compared as a set**, not as a list: two peers agree when
  they enable the same mods whatever order the client listed them in.
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
  F54-B/C encode packets; message `validate()` covers typed bounds. The test
  `accept_f54_a_declared_bounds_are_internally_consistent` pins the invariant
  that must hold when the codec lands (`MAX_SNAPSHOT_BYTES <= MAX_PACKET_BYTES`).
- **Exhaustion of actor serials is unreachable through the public API.**
  `cs_types::net::AllocError::Exhausted` guards `ActorAllocator::allocate`
  against wrapping a `u64` serial, but no construction path can put the
  allocator within `u64::MAX` allocations of the end, so the variant is
  defensive and untested. Peer exhaustion *is* reachable and tested
  (`MAX_SESSION_PEERS`). A caller that restores a persisted allocator (a
  future save/profile feature, F48) would need a constructor that takes a known
  next serial; that is not invented here.
- **`SessionId` allocation** on the host is F54-B/C runtime work;
  `ActorAllocator` and `PeerAllocator` are the A-stage seeds of server-side id
  ownership, and `admit_hello` takes the host's already-allocated session.
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
