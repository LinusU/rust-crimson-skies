# F58-A: threat cases and session identity rules — what is defined and what stays open

Date: 2026-10-02. Task: F58-A "Define threat cases and session identity rules"
(`specs/F58-network-abuse-resistance-reconnect-and-match-recovery.md`), stage
`### F58-A`. Capabilities: ordinary build/test (`retail` was not needed and
none of this uses the installation). Test prefix: `accept_f58_a_`.

## Files and the observable failure

- `crates/cs_net/src/validation.rs` (new): the F58 threat model
  ([`ThreatCase`], [`ThreatDisposition`], [`SessionViolation`]), the live
  session epoch ([`SessionIdentity`]), the per-peer replay window
  ([`SequenceWindow`]), the server-owned peer-to-actor table
  ([`ActorOwnership`]) and the host gate ([`SessionGate::admit`]). Also the
  synthetic fixture ([`synthetic_fire_message`]) and the typed weapon-acceptance
  output ([`FireRequest`], [`fire_requests`]).
- `crates/cs_net/src/recovery.rs` (new): fresh-epoch allocation
  ([`SessionGenerations`]), the reconnect/late-join decision
  ([`decide_recovery`]) that refuses every [`ClientClaim`] and resumes only
  from `ResumeState::FullAuthoritativeSnapshot`, the one-pilot-per-aircraft
  table ([`PilotBindings`]) and the award-once ledger ([`RewardLedger`]).
- `crates/cs_app/src/network/recovery.rs` (new): the host app receive boundary
  ([`SessionReceiver`], [`Inbound`]) that admits a decoded client packet and
  turns an admitted fire packet into an F27 intent ([`fire_intent`]).
- Wiring edits: `pub mod validation;`/`pub mod recovery;` plus a doc paragraph
  in `crates/cs_net/src/lib.rs`; `pub mod recovery;` plus a doc paragraph in
  `crates/cs_app/src/network/mod.rs`.
- Tests: `crates/cs_net/tests/accept_f58_a_session_identity.rs` (7),
  `crates/cs_net/tests/accept_f58_a_recovery_rules.rs` (7),
  `crates/cs_app/tests/accept_f58_a_replay_fire_packet.rs` (4).
- Observable failure without the implementation: a client replays a valid fire
  packet captured from a previous session epoch. With no session identity rule
  the server has nothing to refuse it by; whichever layer trusts it would turn
  the edge into a shot. With F58-A the packet is refused as stale before any
  fire request exists (`accept_f58_a_replaying_a_prior_session_fire_packet_spawns_no_projectile`),
  and the F27 resolver independently refuses a foreign-session intent, so no
  projectile spawns even if the gate were bypassed.

## The rules this stage defines

1. One live `SessionId` epoch; a packet naming any other value is stale and
   changes nothing. A refused stale packet does **not** advance the replay
   window, so the first live packet may reuse the stale sequence.
2. Within an epoch a peer's packet sequence is strictly increasing; a sequence
   at or below the admitted one is a replay and is absorbed. State is one
   integer per peer, so a flood of replays cannot grow it
   (`accept_f58_a_the_replay_window_is_bounded_by_peers_not_packets`).
3. A peer acts only for the actor the server bound to it; binding another
   pilot's aircraft is refused and names the owner.
4. Reconnect issues a **fresh** epoch (`SessionGenerations::issue`, monotonic,
   never reused), so every packet from the prior connection is stale by rule 1.
5. A resume never applies client-authored state: score, health, faction,
   outcome, aircraft and rewards are all listed as refused and the only state
   source is the full authoritative snapshot (F57-A).
6. A reconnect reclaims the aircraft its own prior peer flew and is refused any
   other pilot's; the `RewardLedger` keys on a match-stable `AwardId`, so a
   replayed pickup/capture after the epoch change is `AlreadyAwarded`.
7. A finished match or a match with no authoritative state cannot be resumed; a
   late join is refused when the mode closed it while a reconnect is not.

Every value is newly authored engine design; no original network cap, rate,
abuse or reconnect behavior was measured, and none is asserted.

## Not known / not done here (gates the later stages and fidelity claims)

1. **Rate and resource caps are declared, not implemented.** `ImpossibleRate`
   and `ResourceExhaustion` are `ThreatCase`s with a `Disconnect` disposition,
   but no window or budget exists yet. They are F58-B's deliverable; the F58-D
   adversarial matrix (capability `network_real`, which this machine does not
   have) is what would give them measured evidence. F58-D is therefore blocked
   on a real provisioned network and cannot be satisfied synthetically.
2. **`DisconnectReason` has no abuse arm.** The wire enum
   (`cs_net::message::DisconnectReason`) is `Voluntary | Timeout |
   SessionEnded`; F58-A's `SessionViolation` carries the bounded reason at the
   identity layer, but a real disconnect needs a wire reason. `message.rs` is
   not an F58-A owner path, so mapping (or extending) it is left to F58-B/C and
   must not silently reuse "session ended" for abuse.
3. **`cs_app::network::recovery::fire_intent` narrows `ActorId::serial` into the
   F27 `FireIntentId`.** It uses the peer number as `producer` (a `u16`, so it
   cannot truncate) and the packet sequence as `sequence`; the F27 identity
   contract is otherwise unchanged. F58-B should confirm the intent identity is
   what it wants when it adds ownership validation and rates.
4. **No transport is pinned.** No socket, framing or reconnect handshake exists
   (F54-B/C); this stage is the pure rule set and its app boundary. Nothing here
   claims interoperability or original behavior.
5. **`cs_sim::damage::ActorId` and `cs_types::net::ActorId` remain two shapes.**
   `cs_net`/`cs_app` use the shared `cs_types::net` type; the F27 resolver takes
   the `cs_sim::damage` twin, and `fire_intent` converts between them. The
   migration is already recorded as follow-up in the F29-A findings; it is not
   an F58-A owner path.
