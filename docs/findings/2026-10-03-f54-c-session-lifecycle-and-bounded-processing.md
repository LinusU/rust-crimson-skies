# F54-C: the wired session lifecycle, bounded queues, teardown, retry and error propagation

Date: 2026-10-03. Task: F54-C "Wire server/client lifecycle and bounded message
processing" (`specs/F54-modern-multiplayer-transport-and-authority-protocol.md`).
Capabilities used: ordinary build/test only. `CS_GAME_DIR` was present on this
machine but this stage reads no original data: `network_local` loopback, no
`network_real`, no retail input. Everything here is newly authored engine design
on the F54-B transport; nothing claims anything about the original game's
networking, and no original multiplayer content has been loaded.

## What this stage closes

F54-B recorded six deliberate gaps. This stage closed four of them and named the
rest.

| F54-B finding | F54-C |
| --- | --- |
| "Client-side dedup of server packets is not gated yet" | `ClientSession::accept` deduplicates reliable events by `EventId` and keeps only the newest snapshot tick. |
| "No resend/pacing policy yet … ack tracking" | `ClientSession` tracks the acknowledgment window, retires covered packets and retransmits the oldest unacknowledged packet verbatim. |
| "Rejected clients are not force-disconnected" | `ServerSession::pump` hangs up on a refused or unanswerable connection one round later, and reports the hang-up. |
| "A `ModSetMismatch` reply can exceed `MAX_PACKET_BYTES` … the client sees no reply" | The encode failure is no longer swallowed: `HostEvent::ReplyFailed`, then a hang-up. |
| "`SessionReceiver` (cs_app) vs `HostTransport`'s gate" | Left to F58-C, as F54-B said. `ServerSession` owns *one* gate (the transport's) so the host has a single production admission path; `cs_app::network::recovery::SessionReceiver` remains the boundary for non-transport feeds. |
| "truncating the diff to fit the cap is follow-up work" | Still follow-up; filed as a task. The refusal is now visible, which is what makes the bounded loss acceptable until it is fixed. |

## Files and the observable failure

- `crates/cs_net/src/lifecycle.rs` (new). `ServerSession` wraps `HostTransport` and
  owns the host phase machine (`ServerPhase::{Gathering, Live, Finished,
  Closed}`), the bounded work queue, the reliable event publisher, the snapshot
  publisher, the input acknowledgment, explicit teardown (`close`) and retry
  (`reopen`). `ClientSession` wraps `ClientTransport` and owns
  `ClientPhase::{Connecting, Joining, Joined, Live, Finished, Closed}`, the local
  input producer, the dedup seen-set, the newest-snapshot slot, the
  acknowledgment window and the retransmit window.
- `crates/cs_net/src/transport.rs`. Two changes the lifecycle cannot exist
  without, both additions rather than behaviour changes:
  `HostEvent::PeerPacket` now carries `Option<AdmittedInput>` (the bounded
  tick-stamped frames an admitted packet carried) next to the fire requests —
  without it the decoded frames were dropped on the floor and **no host had a
  production path from a wire packet to the simulation**; and
  `HostEvent::ReplyFailed` plus `HostTransport::{disconnect_client, reopen}`.
- `crates/cs_net/src/bounds.rs`. Three new caps: `MAX_WORK_PER_PUMP` (64),
  `MAX_SEEN_EVENTS` (256), `MAX_UNACKED_PACKETS` (8).
- `crates/cs_net/tests/accept_f54_c_lifecycle.rs` (new) and
  `crates/cs_net/tests/support/f54_c_fuzz.rs` (new): 26 tests at implementation
  time, 29 after the review pass below.
- `crates/cs_net/tests/accept_f54_b_pinned_transport.rs`: four `PeerPacket`
  destructuring patterns gained the new `input` field, and each gained an
  assertion — `Some(AdmittedInput)` on the admitted delivery, `None` on the
  replayed/out-of-order one. No assertion was weakened or removed.
- `crates/cs_net/src/lib.rs`: `pub mod lifecycle;` and a doc paragraph.
- Observable failure without the implementation: there was no session at all.
  `HostTransport` reported events and `ClientTransport` sent bytes, but nothing
  owned the launch/finish/teardown transitions, nothing converted locally
  sampled ticks into wire input, nothing drained admitted input into a consumer,
  nothing deduplicated a replayed reliable event on the client, nothing
  retransmitted a lost input packet, and an unencodable handshake answer was
  silently discarded with `let _ =`.

## The acceptance scenario (spec F54 AC03)

`accept_f54_c_fuzzed_packets_are_bounded_and_never_produce_a_non_finite_value`
and `accept_f54_c_the_client_consumer_survives_the_whole_corpus`.

The corpus is **deterministic**, not a `cargo-fuzz` entry point: CI has no
sanitizer, no time budget and no original data, and a reviewer has to see the
identical corpus, so `corpus(seed)` is a pure function of a fixed seed over a
spelled-out xorshift64\* generator (`tests/support/f54_c_fuzz.rs`). Each run
covers the hand-placed hostile shapes plus ~200 seed-derived mutations, across
four fixed seeds — over 500 cases per selection.

Every buffer goes to production code only: `cs_net::codec::decode_client_packet`
/ `decode_server_packet`, then `ClientSession::accept` (which is exactly what
`ClientSession::pump` runs per arrived packet), then
`cs_net::snapshot::Snapshot::decode` and the `ActorRecord` dequantizers. The three
properties asserted:

1. **Nothing panics.** The decoder and the consumer are total.
2. **Every refusal is named.** Each error's `Display` must name the field or the
   cap, so an unexplained failure is distinguishable from a missing one.
3. **No decoded value is non-finite.**

**On "NaNs".** The wire has no float field at all, so a NaN cannot *enter* as a
value — it can only be a byte pattern. That is the invariant under test, and it
is a real one: the corpus writes all five non-finite `f32` patterns (quiet NaN,
negative NaN, ±infinity, signalling NaN) at **every offset** of otherwise-valid
hello, input and server-message buffers, plus at 32 random offsets of mutated
packets, and asserts that anything that decodes yields finite axis samples,
finite snapshot positions/velocities/flight channels and a known origin epoch.
The complementary production door is `ClientSession::submit_sample`, which
refuses a non-finite *local* sample with `ClientFault::Axis(AxisValueError::
NonFinite)` before it can become a packet — that is the only way a NaN could ever
reach a wire field, and `accept_f54_c_a_local_sample_that_is_not_finite_never_
becomes_a_packet` drives all five patterns into a live client.

**Oversized counts** are placed from the real grammar so each reaches the field it
targets rather than failing on the first byte: `input.frames`, `frame.axes`,
`frame.edges` and `compat.mods` all at `u16::MAX`, `snapshot.payload` at
`u32::MAX`, the packet itself at `MAX_PACKET_BYTES + 1`, and a live end-to-end
flood of 65+ oversized and malformed buffers at a real host.

**Invalid ids** are placed the same way: zero session epoch, zero peer id in a
grant, zero protocol version, zero epoch inside an `EventId`, an unknown packet
tag, a server message in the client decoder and a client message in the server
decoder.

A fourth, end-to-end variant
(`accept_f54_c_a_hostile_peer_is_cut_off_without_disturbing_the_others`) pushes
the corpus's small shapes over a real socket from a second, handshake-complete
peer and asserts the abusive connection is cut off by the *declared* threat
dispositions while the honest peer's session keeps working. This is also the only
place the `MalformedMessage` → absorb versus `OversizedMessage`/`UnauthenticatedPeer`
→ disconnect distinction is exercised, and it is why `HostEvent::PacketDropped`
is mapped through `ThreatCase::disposition` rather than a private rule.

## What is bounded, and the deliberate limits

| Bound | Value | Why |
| --- | --- | --- |
| `MAX_WORK_PER_PUMP` | 64 | A peer's send rate is not a caller's; the surplus is refused and the peer cut off (`ResourceExhaustion`). |
| `MAX_SEEN_EVENTS` | 256 | Evicts the **oldest** `EventId`, so a long session cannot grow the dedup set and a re-applied old event is visible rather than silently suppressed forever. |
| `MAX_UNACKED_PACKETS` | 8 | Retransmit memory is `8 * MAX_PACKET_BYTES`. Eviction drops the oldest *unacknowledged* packet; the sequence is never recycled, so the host's replay window still refuses it if it ever arrives. |
| `MAX_INPUT_FRAMES_PER_PACKET` | 8 (pre-existing) | The client's pending input stops at the packet's own bound: a larger batch is unencodable anyway. |
| `INPUT_RETRY_INTERVAL` | 250 ms | A *window*, not a rate limiter. The `ImpossibleRate` pacing and per-tick request caps are F58-B's declared work and this module does not invent numbers for them. |

Also deliberate, and recorded rather than silently chosen:

- **A queue overflow disconnects immediately.** That is blunt: one client's
  burst cuts it off rather than rate-limiting it first. F58-B's per-peer rate cap
  should make the cut-off the last resort; until then this is the only bound
  that cannot be gamed. Filed as a task.
- **`ServerSession::reopen` keeps the socket.** A retry does not depend on the
  same port still being free, and the connection layer's per-connection state
  dies with the disconnected clients. The alternative (rebinding) failed on
  `EADDRINUSE` in testing, which is why the socket is kept.
- **`ClientSession::accept` re-runs `expect_session` and `validate`.** The codec
  already validates every decoded packet, but `accept` is a public entry point and
  an oversized snapshot payload must not reach a consumer whoever built the
  message.
- **Snapshot payloads are decoded on arrival.** The client stores the validated
  `Snapshot`, not the raw bytes, so what the F57-B interpolation buffer receives
  is a schema-checked record set. A payload that fails the schema is refused with
  `SnapshotError`, whose own docs require the field to be named.
- **`ServerNotice::Dropped` fires for absorbed gate refusals too.** A replay that
  produced no work must still be visible, or "absorbed" and "silently lost" look
  identical from outside. `DropReason::Refused(SessionViolation)` carries it.
- **A client's `ClientEvent::TransportFault` is treated as fatal** and closes the
  session. `NetcodeClientTransport::update` failing means the connection layer
  itself is broken; keeping a half-open session would only hide it. Whether a
  transient socket error should instead be retried is F58-C's reconnect policy,
  not a number to guess at here.
- **No netcode token issuer.** `Unsecure` remains the connection-layer mode, as
  F54-B recorded; the application handshake still gates compatibility.

## Test sensitivity

Each probe below was applied to `lifecycle.rs`, run, and reverted; at
implementation time the restored file passed 26/26. Removing the module from
`lib.rs` makes the test target fail to compile, so the tests cannot pass without
it.

| Implementation removed | Tests that failed |
| --- | --- |
| the `MAX_SEEN_EVENTS` eviction loop | 1 (`…_the_event_memory_is_bounded_and_evicts_the_oldest`) |
| the non-finite sample check in `submit_sample` | 1 (`…_a_local_sample_that_is_not_finite_never_becomes_a_packet`) |
| the `MAX_WORK_PER_PUMP` cap | 1 (`…_the_work_queue_refuses_the_surplus_and_cuts_the_abusive_peer`) |
| the `MAX_UNACKED_PACKETS` cap | 2 |
| the epoch check in `accept` | 3 |
| the refused-client teardown | 1 (`…_a_refused_client_is_told_why_and_then_hung_up_on`) |

## Review pass (bunny-alpha-1, 2026-10-03)

The implementer and the reviewer of this stage are the same agent name
(`bunny-alpha-1`); the review context was fresh (a new session that saw only the
task, the spec and the diff). Under `AGENTS.md` that is **not** independent
review, and nothing here is original-reference evidence either way.

What the review found and fixed on the branch:

1. **A flaky acceptance test.** `…_the_retransmit_window_is_bounded_and_resends_
   the_exact_bytes` waited exactly two exchange rounds for the host's input
   acknowledgment. Measured over repeated runs, that acknowledgment arrives in
   2 rounds usually and 5 occasionally, so the test failed intermittently (it
   failed on review within three runs). The wait is now `Link::pump_until`,
   which pumps until the *condition* holds within `MAX_ROUNDS` and fails
   otherwise. Every assertion is unchanged — only the waiting changed. The other
   three fixed-round waits (`pump(4)` before the admitted work, before the ack,
   `pump(16)` before the teardown, the finish loop) became the same bounded
   wait, because the same flake would have found them.
2. **`ClientPhase::Closed` was not terminal in `accept`.** The client's consumer
   applied whatever arrived, so a reliable `Launched` the host had already sent
   before its `Disconnect` moved a closed client back to `Live` — and
   `ClientPhase::open()` then let it produce input again for a session it had
   left. `accept` now refuses every packet with `ClientFault::Closed`, naming the
   closure; `…_a_closed_client_session_is_terminal` covers a late launch, a late
   finish, a late snapshot and a late acknowledgment.
3. **Publishing into a spent epoch was a silent no-op.** `announce`,
   `announce_spawn`, `announce_removal` and `publish_snapshot` broadcast to a
   session whose members had all been hung up and reported `Ok`. They now
   refuse with `ServerFault::WrongPhase`, which is what a caller needs to tell a
   published fact from a dropped one
   (`…_a_spent_epoch_refuses_to_publish`).
4. **Two variants that could never be observed.** `ServerNotice::Phase` and
   `ServerFault::WorkQueueFull` were never constructed anywhere: a host caller
   *is* the one that calls `launch`/`finish`/`close`/`reopen` and each returns
   its own result, and the queue-overflow refusal is already a named
   `ServerNotice::Dropped { QueueOverflow }` plus `CutOff { ResourceExhaustion }`.
   Both were removed rather than left as a lie in the public API. The client's
   `ClientNotice::Phase` stays, because there the phase moves from a packet.
5. **`HostTransport::disconnect_client` documented `false` for an already-gone
   connection and always returned `true`.** It now returns whether the transport
   still knew that client.
6. **`HostTransport::reopen` left live connections behind** while its doc said
   their state died with them: it cleared the tables but never told the
   connection layer. A reset now hangs up every connection it held
   (`…_a_retry_hangs_up_the_connections_that_hold_no_peer`). Honest limit: the
   end state is the same one round later, because the refused connection's next
   packet is refused as `NoPeer` and cut off, so this specific line is *not*
   separately discriminating in the test table.
7. **The lifecycle re-decided the declared threat table.** `pump` had a private
   `match` over `DropReason` that happened to agree with
   `ThreatCase::disposition`. The classification now lives once, in
   `DropReason::threat`, and `pump` asks the declared table — which is what this
   document claimed it did.
8. **`…_the_lifecycle_never_advertises_another_protocol_revision` exercised
   nothing but a fixture tautology** and a `Display` string. It is now
   `…_a_wrong_protocol_revision_is_refused_through_the_lifecycle`: a real
   handshake with a client offering revision 2, refused with the named reason on
   both sides, with no member left behind.

Probes run during the review (each applied, run and reverted): the terminal-phase
guard in `accept`, `ServerTransport::reopen`'s hang-up, `ServerSession::close`'s
member hang-up, and the publish guard all fail at least one test when removed.
The restored branch passes 29/29.

Also deliberate, and recorded rather than silently chosen (review addition):

- **A closed client session keeps no late traffic at all.** Refusing every
  inbound packet after teardown also means a late acknowledgment no longer
  retires the retransmit window. That is correct — the session is over — but it
  means `ClientSession::snapshot()`/`acked_through()` are the *last* accepted
  values, not a live view, once closed.
- **The client keeps only the newest snapshot.** `ClientSession::latest` is a
  single `(Tick, Snapshot)` slot, which is enough to prove newest-wins and to
  hand F57-B a schema-checked record set, but it is **not** enough to
  interpolate: a real buffer needs a bounded run of frames with a delay window
  and per-generation actor separation. F57-B owns that buffer and will have to
  replace this slot; F54-C claims no interpolation capability.

## Not claimed here

- No `network_real` evidence. Loopback on one machine is `network_local`; the
  two-machine connect/launch/finish/disconnect run and the latency/loss
  characterization of `INPUT_RETRY_INTERVAL` are F54-D, which needs the
  `network_real` capability this machine does not have.
- No original multiplayer content parity (F56) and no original mode/scenario
  table. Nothing here was compared against the original game.
- A code/test pass awards at most **checked**, per the sheet.