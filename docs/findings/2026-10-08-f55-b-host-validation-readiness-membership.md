# F55-B: host validation, readiness and membership — design notes and what stays open

Date: 2026-10-08. Task: F55-B "Implement host validation, readiness and
membership"
(`specs/F55-multiplayer-lobby-host-rules-readiness-and-ux.md`). Ordinary
build/test only. Everything is newly authored engine design: no original
multiplayer packet, option or rate is known or claimed.

## Files and the observable failure

Listed before editing, per the sheet.

- `crates/cs_net/src/lobby.rs`: the member-to-host wire envelope
  [`LobbyPacket`] (`Command`, `Launch`, `LaunchAck`, `CancelLaunch`) with its
  bounded codec ([`LobbyPacket::encode`] / [`LobbyPacket::decode`], private
  `PacketReader`/`PacketWriter`, [`PacketError`]), the host's single entry
  point [`Lobby::receive_bytes`] → [`Lobby::receive`] with the gate order
  epoch → replay → membership → dispatch, the outcome/rejection vocabulary
  ([`PacketOutcome`], [`PacketReject`]), the bounded replay guard
  (`Lobby::seen`, [`MAX_SEEN_LOBBY_PACKETS`]), and `Display`/`Error` for
  `LaunchError` (it had `Debug` only, and `PacketReject` reports it).
- `crates/cs_net/tests/accept_f55_b_host_validation.rs` (new): 10 tests, all
  named `accept_f55_b_*`.
- Wiring edits: none were needed — `lobby` is already declared in
  `crates/cs_net/src/lib.rs`.
- Observable failure without the implementation: there is no host-side path
  that a *packet* can take, so a launch request naming the pre-ban revision
  has nothing to refuse it at the wire boundary:
  `accept_f55_b_launch_packet_for_an_old_rules_revision_is_rejected` cannot
  even be written against production code.

## What the slice is

The smallest production path that exercises the declared behaviour: bytes in,
validated state change out.

```
member bytes ─▶ LobbyPacket::decode ─▶ Lobby::receive
                                          1. EventId.session == lobby session  (epoch)
                                          2. EventId already applied?          (replay, idempotent)
                                          3. sender is a member                (membership)
                                          4. apply / begin_launch / acknowledge_launch / cancel_launch
                                             (authority first, then phase, revision, digest, readiness)
```

- **Host validation.** Every decode bound is checked before a lobby field is
  read: packet cap (`MAX_PACKET_BYTES`), list caps (`MAX_LOADOUT_COMPONENTS`
  and the callsign/chat byte caps), unknown packet/command/action tags, zero
  ids, truncation, trailing bytes, UTF-8, and the constructors that own each
  text bound (`Callsign::new`, `ChatText::new`, `Revision::new`,
  `ContentId::parse`).
- **Readiness.** `SetReady`/`SetLoadout` travel the same path, so a stale
  revision, a banned component or a wrong team still refuse after decoding,
  and a host ban still revokes readiness with its reason
  (`PacketOutcome::Applied` carries `ReadyRevoked`).
- **Membership.** A stranger and a member who already left cleanly are both
  `PacketReject::NotAMember`, checked before dispatch rather than inferred
  from a later failure.
- **Authority.** There is no client packet that can express host authority;
  the authority check runs inside `Lobby::apply`/`begin_launch` *after*
  decoding, so a client that encodes one is refused, not trusted.

## Decisions

- **Gate order is epoch → replay → membership → dispatch.** A replayed id is
  answered `Replayed` even if its sender has since left: "nothing changed" is
  the correct and harmless answer, whereas re-reporting `NotAMember` invites a
  retry loop. Epoch comes first because a packet from another session must not
  be interpreted at all.
- **Only accepted packets join the replay guard.** A refused packet may be
  corrected and retried under the same id; recording failures would make the
  correction undeliverable. The guard is a bounded `VecDeque` that evicts the
  oldest id at `MAX_SEEN_LOBBY_PACKETS` (256), the same policy as the F54-C
  client seen-set. It is a freshness/idempotency bound, not a security
  boundary: a peer that fills it can replay a very old id.
- **`PacketOutcome::Replayed` is a success, not an error.** Reliable delivery
  can redeliver after a retry; the contract wants application idempotency, so
  the redelivery is acknowledged and broadcasts nothing.
- **The lobby has its own envelope instead of a `cs_net::message` payload
  variant.** `message.rs`, `codec.rs` and `lifecycle.rs` are outside this
  task's owner paths, so the envelope lives in `lobby.rs` with the same wire
  discipline the F54-B codec documents (little-endian integers, `u16` counts
  checked at decode, no floats, unknown tags and trailing bytes refused).
  Carrying it on the session wire is follow-up work (see below).
- **Join stays on the handshake path.** `JoinRequest`/`Lobby::admit` are not
  re-expressed as a packet: admission already runs password → compatibility →
  phase/late join → capacity → callsign against the F54-B `ClientHello`, and a
  second join grammar would be a parallel implementation of that order.

## Open / not claimed (resolving tasks)

- **The envelope is not on the wire yet.** `ClientPayload`/`ServerPayload`
  (F54-A, not owned here) still carry no lobby variant, so a real client
  cannot send these bytes and `lifecycle::ServerSession` does not dispatch to
  `Lobby::receive_bytes`. Filed as a follow-up task: the lobby's *validated*
  path exists, the transport wiring does not.
- **`LoadoutValidator` still has no production implementation.** The hook is
  exercised by the tests through `AcceptAll`/`RejectAll`, as in F55-A. A
  content-side implementation cannot be honestly written yet: `cs_net`'s
  `Loadout` is a flat `{ blueprint, components }` id list, while
  `cs_content::construction::ConstructionRules::validate` needs an
  `AircraftBlueprint` with armor zones, weapon mounts and hardpoints, and no
  authored blueprint-id → `AircraftBlueprint` table exists in `cs_content`.
  Mapping ids onto fitments would be a guess. Filed as a follow-up task.
- **`crates/cs_content/src/lobby.rs` was not created** for that reason: there
  is no content-side lobby data this stage can use without guessing. Budget
  and fitment validation therefore remains unproven for lobby loadouts; the
  host's own bans, the shape checks and the shared-validator hook are the
  judgements that run today.
- **Chat rate limiting is not specified.** F55-A left it open; the spec's
  non-negotiable 5 asks for bounded, escaped text with mute controls, which
  `ChatText` (length/control-character bounds, `escaped()`) and
  `LobbyView` (mute) already provide. No original chat rate is known, and the
  planned rate/resource caps live in F58-B, so no rate was invented here.
- **LAN discovery, Internet-vs-discovery wording, pilot voice, ping display
  and the per-mode late-join table** are not in this stage (F55-C, F56-A).
- **Client-side display of these packets** (feeding decoded `LobbyEvent`s into
  `cs_app`'s `LobbyView`, the lobby screens) is F55-C.

## Tests

`crates/cs_net/tests/accept_f55_b_host_validation.rs` — 10 tests, each
encoding a packet with the production codec and handing the *bytes* to
`Lobby::receive_bytes`:

- `accept_f55_b_launch_packet_for_an_old_rules_revision_is_rejected` — the
  minimum scenario (AC02): after a host ban bumps the revision, the launch
  packet built for the old revision is `StaleRevision`, the lobby is still
  gathering, and the same host under the live revision launches.
- `accept_f55_b_a_launch_commits_only_after_every_member_acknowledges` — an
  acknowledgment naming another rules digest is `AckMismatch`; the last
  acknowledgment commits atomically.
- `accept_f55_b_a_replayed_packet_is_applied_once`,
  `accept_f55_b_a_replayed_launch_packet_does_not_recommit` — idempotency by
  `EventId`; two distinct ids with the same text both apply.
- `accept_f55_b_a_packet_from_another_session_epoch_is_refused` — epoch gate,
  for a launch and for a rules change.
- `accept_f55_b_a_non_member_packet_is_refused` — a stranger and a departed
  member.
- `accept_f55_b_a_client_packet_cannot_use_host_authority` — client launch and
  client rules change refused; the host's own packet is accepted.
- `accept_f55_b_malformed_bytes_are_refused_before_the_lobby_changes` —
  truncated, trailing, unknown packet tag, unknown command kind, zero
  session id; revision, phase and rules untouched after every refusal.
- `accept_f55_b_an_unacceptable_chat_packet_is_refused` — invalid UTF-8, a
  control character, 201 characters, and 801 wire bytes.
- `accept_f55_b_readiness_and_membership_travel_the_host_receive_path` —
  admit → ready → host ban → readiness revoked with the reason, all through
  `receive_bytes`.

Sensitivity was measured, not assumed (mutation run, reverted afterwards):

- With the stale-revision refusal in `begin_launch` disabled,
  `accept_f55_b_launch_packet_for_an_old_rules_revision_is_rejected` failed
  (`left: Launch(DigestMismatch)` vs `right: Launch(StaleRevision {...})`).
- With the replay guard in `Lobby::receive` disabled,
  `accept_f55_b_a_replayed_packet_is_applied_once` (chat applied twice) and
  `accept_f55_b_a_replayed_launch_packet_does_not_recommit`
  (`Launch(WrongPhase { phase: InMatch })`) failed: 8 passed, 2 failed.
- With the implementation removed entirely the tests do not compile, because
  they call `LobbyPacket`, `Lobby::receive_bytes`, `PacketOutcome` and
  `PacketReject` production API.

## Commands

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --locked
cargo test --workspace --locked -- accept_f55_b_ --include-ignored
```
