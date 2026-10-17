# F55-BU1-WIRE: the lobby packet envelope on the F54 session wire

Date: 2026-10-17. Task: F55-BU1-WIRE (#763), follow-up of F55-B (#221). Ordinary
build/test only; newly authored engine design, no original packet is known or
claimed.

## What crosses the wire now

```
client                                   host
LobbyPacket::encode ─ ClientPayload::Lobby(bytes) ──▶ session gate (epoch, sequence)
  (ClientSession::send_lobby)                         rate ladder (same as input)
                                                      ServerSession → Lobby::receive_bytes
                                                         (peer → lobby member map)
client ◀── EventBody::Lobby(LobbyEvent bytes) ◀────── ServerSession::publish_lobby
client ◀── EventBody::LobbyRefused{packet, class} ◀── only the sender, on a refusal
```

- `ClientPayload::Lobby` is `Delivery::Reliable`; `EventBody::Lobby` and
  `EventBody::LobbyRefused` ride the existing reliable server event channel and
  therefore the client's `EventId` seen-set.
- Both carry the lobby record as an opaque bounded blob
  (`MAX_LOBBY_RECORD_BYTES`, `MAX_PACKET_BYTES` minus envelope headroom). The
  lobby's own caps still run inside `LobbyPacket::decode`/`LobbyEvent::decode`.
- `LobbyEvent::encode`/`decode` (in `lobby.rs`) is the host-to-member half of
  the envelope; it follows the `LobbyPacket` discipline (little-endian, bounded
  text re-validated by its owning constructor, unknown tags and trailing bytes
  refused). This is the one change in `lobby.rs`; no validation semantics moved.
- Idempotency on the wire: each `send_lobby` takes a fresh session sequence, so
  the session gate admits a redelivered packet and the lobby's `(sender,
  EventId)` guard answers `PacketOutcome::Replayed` — nothing is applied or
  broadcast twice. A *verbatim* repeat of a session sequence is still refused by
  the gate.
- A lobby packet spends the sender's rate budget exactly like an input packet
  (`ServerSession::charge_rate`, extracted from `enqueue`); the shared work-queue
  bound still applies to input only.
- A launch naming an old rules revision is refused end to end: the host's lobby
  answers `PacketReject::Launch(StaleRevision)`, the host sees
  `ServerNotice::LobbyRefused`, and the sender receives
  `ClientNotice::LobbyRefused { refusal: StaleRevision }`.

## Decisions

- **Session peers and lobby members are different numberings.** The transport
  allocates peers from 1 for clients; the lobby allocates its own ids with the
  host as 1. `ServerSession::bind_lobby_member(peer, member)` records who on the
  wire is who in the lobby; an unbound peer is `NotAMember`. The host's own
  member may be bound (a host whose player connects to its own session as a
  client), which is how a launch packet reaches the host member over the wire.
- **Join stays a host call.** `Lobby::admit` runs on the host from the
  `ClientHello`/`JoinRequest`; this task does not move admission onto the wire.
  The application admits, then binds.
- **Departures.** A bound peer that leaves or is cut off is removed from the lobby
  and its `MemberLeft` events are published.
- **Epoch.** `reopen` drops the attached lobby: it belongs to the spent epoch.
- The validator handed to `attach_lobby` must be `Send + Sync` (the cs_app host
  resource crosses threads).

## Not claimed / open

- Remote **joining** from the join form (resolving an address, admitting a
  `ClientHello` as a lobby member without a host call) and LAN discovery remain
  F55-C/F56 work; this task makes packets and events cross the wire only.
- `LoadoutValidator` still has no production implementation (#764,
  F55-BU2-VALIDATOR); the wire tests use an accepting stub.
- Everything was exercised on loopback (`network_local`), never `network_real`.

## Wiring edits outside the named owner paths

- `crates/cs_net/src/transport.rs`: `HostEvent::PeerLobby` and the receive arm
  that emits it (the transport is where a gated `ClientPayload` becomes a host
  event; `lifecycle` cannot see the payload otherwise).
- `crates/cs_net/src/lobby.rs`: `LobbyEvent::encode`/`decode` only.
- `crates/cs_app/src/network/recovery.rs`: one match arm (`ClientPayload::Lobby`
  is not an input).
- `crates/cs_net/tests/accept_f54_a_wire_contract.rs`: the exhaustive
  client-payload test restates the new variant's class (reliable); no assertion
  was weakened.
- `crates/cs_app/src/ui/lobby/{mod,link}.rs`: `LobbyView::observe_session`,
  `Notice::PacketRefused`, and the link's stale "not carried" paragraph.

## Tests

`accept_f55_bu1_` in `crates/cs_net/tests/accept_f55_bu1_lobby_wire.rs` (8, real
loopback sessions) and `crates/cs_app/tests/accept_f55_bu1_lobby_view_feed.rs`
(1: events and a refusal reach `LobbyView`).
