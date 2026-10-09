# F58-B: intent validation and rate/resource caps — what landed and what stays open

Date: 2026-10-09. Task: F58-B "Implement intent validation and rate/resource
caps" (`specs/F58-network-abuse-resistance-reconnect-and-match-recovery.md`),
stage `### F58-B`. Capabilities: ordinary build/test only (`retail` was not
needed; nothing here reads the installation). Test prefix: `accept_f58_b_`.

## Files and the observable failure

- `crates/cs_net/src/validation.rs` (extended, owner path): the intent layer.
  [`ClientIntent`] is every ask a client may make (`Input`, `EquipLoadout` and
  the `Claim*` family that authors server-owned truth), [`IntentValidator`]
  judges each ask in a fixed order against server-owned truth, the per-peer
  [`RateBudget`], server-owned actors ([`ActorOwnership`]), match state
  ([`Phase`]), the tick window and the host's loadout rules, and
  [`IntentRefusal`] names the bounded reason plus the disposition (absorb or
  disconnect). [`RateLimits`] and [`MatchStage`] are the two new inputs the
  host supplies; `MAX_INPUT_TICKS_BEHIND`/`MAX_INPUT_TICKS_AHEAD` bound the
  window.
- `crates/cs_net/src/recovery.rs` (extended, owner path): `ClientClaim` gained
  the `Damage { target, amount }` arm. AC02 names *damage* as well as score,
  and the reconnect claim vocabulary could not express it, so "a
  client-provided damage claim is never accepted" was not demonstrable at the
  recovery boundary. `decide_recovery` already refuses every claim, so no rule
  changed — the vocabulary now covers what the rule already says.
- `crates/cs_app/src/network/recovery.rs` (extended, owner path): the app
  boundary gained [`SessionReceiver::validate_intent`] (one ask at a time) and
  [`SessionReceiver::receive_validated`] (the packet path plus the intent
  layer, returning [`ValidatedInbound`]); the receiver now owns an
  `IntentValidator` whose budget is reset by `reopen`. `receive`, `Inbound`
  and `fire_intent` are unchanged, so the F58-A path and its tests stand.
- Wiring-only edits: doc paragraphs in `crates/cs_net/src/lib.rs` and
  `crates/cs_app/src/network/mod.rs` that named F58-B caps as "not here yet".
- Tests: `crates/cs_net/tests/accept_f58_b_intent_validation.rs` (4),
  `crates/cs_net/tests/accept_f58_b_rate_and_resource_caps.rs` (3),
  `crates/cs_net/tests/accept_f58_b_recovery_claims.rs` (2),
  `crates/cs_app/tests/accept_f58_b_refused_ask_spawns_nothing.rs` (1).
- Observable failure without the implementation: a client that asks the
  server to apply damage to an aircraft or to add points to its own score has
  nothing to be refused by — there is no intent vocabulary and no validator,
  so whatever consumed such an ask would have to trust it. With F58-B the ask
  is refused before any other check runs, with
  `IntentRefusal::ClientAuthoredTruth { domain }` naming the authority domain
  (F54-A `AuthorityDomain::{HitDamage, ScoreResult}`) the client tried to
  write, and no state moves in either direction
  (`accept_f58_b_client_requested_damage_and_score_are_refused`,
  `accept_f58_b_a_refused_ask_authorizes_no_projectile`).
  The second observable failure is the rate: with no budget, a peer could send
  as fast as the socket delivered and every packet was individually valid; now
  the (97th) intent past the design in one server-tick window is refused as
  `RateExceeded` and the peer is disconnected by disposition
  (`accept_f58_b_a_flooding_peer_is_cut_off_at_the_designed_rate`).

## The rules this stage adds

1. An ask that authors server-owned truth (`ClaimDamage`, `ClaimScore`,
   `ClaimHealth`, `ClaimFaction`, `ClaimOutcome`) is refused first, is not
   even charged to the rate budget, and leaves the ownership table and the
   budget untouched.
2. Every other ask is charged once against a per-peer counter keyed on the
   **server's** tick, never on anything the client sends, so a client cannot
   refill its own budget by naming new ticks. The budget tracks at most
   `MAX_SESSION_PEERS` counters and refuses to grow past that.
3. An ask naming an aircraft must name one the server bound to that peer.
4. Match state decides what may be asked: input needs `InMatch`; a loadout
   change needs `Gathering` or `InMatch` (a launch locks loadouts; a mid-match
   swap is the F36 docking path).
5. Input ticks sit inside `[server_tick - 64, server_tick + 64]`; outside is
   refused and absorbed.
6. A loadout ask is judged for shape (cap, duplicates, blueprint kind,
   component kinds) and against the host's banned components, exactly as the
   lobby's own `check_loadout` judges them.
7. A reconnect's `Damage` claim is refused exactly like its `Score`, `Health`,
   `Faction` and `Outcome` claims; the only state source stays the full
   authoritative snapshot.

Design values (`RateLimits::DESIGNED` = 96 intents per 60 server ticks; the
±64-tick input window) are newly authored engine design at a 60 Hz
simulation. No original network rate, tick window or abuse threshold has been
measured, and none is asserted. F58-D (capability `network_real`, which this
machine does not have) is what would give them measured evidence.

## Not known / not done here (gates later stages and fidelity claims)

1. **The wire has no abuse arm in `DisconnectReason`.**
   `cs_net::message::DisconnectReason` is still `Voluntary | Timeout |
   SessionEnded`. The bounded reason exists at the identity layer
   (`SessionViolation`), at the intent layer (`IntentRefusal::label()` with a
   `Display`), and in the server notices (`ServerNotice::CutOff { threat }`),
   but a `ServerPayload::Disconnect` packet cannot *tell the client* it was
   cut off for abuse. `message.rs` is not an F58-B owner path (nor F58-C's),
   so nothing was silently mapped onto "session ended" and the gap is filed as
   a follow-up task instead.
2. **The rate caps and tick windows need a host that supplies the server tick,
   and nothing does yet.** `SessionReceiver::receive_validated` and
   `validate_intent` take a `MatchStage`; the pinned transport pump
   (`cs_net::transport`, `cs_net::lifecycle`) calls `SessionGate::admit`
   directly and knows nothing of the stage, so *on the wire path* only the
   F54-A caps (packet size/count) and F54-C's bounded work queue are live.
   F58-C ("wire the implemented path into its actual producer and consumer")
   is the stage that must make the host pump pass its phase and tick; until it
   does, this module is the implemented and tested rule, not a measured
   runtime behavior. This is the same stage split F58-A used.
3. **The intent-layer loadout check does not re-run the F44 blueprint/budget
   judge.** `cs_net` cannot depend on content crates, and the
   `lobby::LoadoutValidator` instance is wired in `cs_app` (F55-B), not here.
   The intent layer therefore judges shape and the host's bans; the budget
   judge stays where it already runs (the lobby's readiness path). Recorded so
   no one claims an in-session loadout ask is fully F44-validated.
4. **`ClientIntent` is a server-side vocabulary, not a wire format.**
   `ClientPayload` still only carries `Input` and `Leave`, so no decoded
   packet can produce a `Claim*` ask today; the validator is the defence in
   depth that answers "and if one arrived anyway". `ThreatCase::
   ClientAuthoredTruth` stays `Structural` at the wire layer exactly as F58-A
   declared; `IntentRefusal::ClientAuthoredTruth`'s own disposition is
   `Disconnect` because at *this* layer the sender is not an honest client on
   a lossy link. The two statements are about different checks and neither
   changed the other.
5. **F58-A item 3 is confirmed, not changed.** `cs_app::network::recovery::
   fire_intent` still uses the peer number as the F27 `FireIntentId::producer`
   and the admitting packet's sequence as `sequence`. F58-B adds no second
   producer identity; an accepted intent is charged per peer and ordered by
   packet sequence, which is exactly what the intent id records.
6. **`cs_sim::damage::ActorId` and `cs_types::net::ActorId` remain two
   shapes** (F29-A follow-up, unchanged here); the cs_app test converts
   between them the same way `fire_intent` does.
7. **The intent tick window judges the packet's *newest* frame tick.** An
   `InputBatch` is bounded at the wire layer (`MAX_INPUT_FRAMES_PER_PACKET` 8,
   `MAX_INPUT_BATCH_SPAN_TICKS` 64, strictly increasing ticks), so with a ±64
   window an older frame inside an accepted batch may sit up to 128 ticks
   behind the server tick at the extreme edge. That staleness stays bounded
   and it cannot author a shot on its own: `cs_sim::weapons::FireResolver`
   refuses any intent whose tick is not exactly the tick it is resolving, so
   only a frame at the resolver's own tick spawns anything. Recorded so no
   one claims every frame of every accepted batch is individually inside the
   window.
