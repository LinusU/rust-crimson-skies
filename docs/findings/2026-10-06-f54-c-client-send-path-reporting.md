# The client send path is a report, not a discard: why a failed send is reportable, and what is now reported

Date: 2026-10-06. Author: bunny-alpha-1. Task: Rally #708.

Capabilities used: ordinary build/test plus `network_local` loopback (the
F54-C acceptance fixture). No original data was read, no `network_real`, no
human play; nothing here says anything about the original game's networking.

## The question the task asked

`ClientTransport::update` ended with
`let _ = self.transport.send_packets(&mut self.client);` and
`ClientTransport::disconnect` discarded the same result
(`crates/cs_net/src/transport.rs`). Is a failed client send **reportable**?

**Decision: yes.** Spec `specs/F54-modern-multiplayer-transport-and-authority-protocol.md`
`### F54-C` requires the stage to "include teardown/retry and **error
propagation**", and its owner paths are exactly this crate. The stage that
consumes it already claims the rule in prose: `crates/cs_net/src/lifecycle.rs`
opens with "Nothing is swallowed. Every refusal is a named value ... and
[`TransportFault`](ServerNotice::TransportFault) for a connection-layer error."
The host side of the same transport reports the same class of failure twice
over — `HostEvent::TransportFault` for the netcode update and
`HostEvent::ReplyFailed` for a verdict that could not be encoded — while the
client side reported its send path not at all. There is no spec text that makes
a client's send failure invisible, and `docs/contracts/UI-NETWORK.md` puts
"Lifecycle, rules ... are reliable/idempotent" on the wire path this call is
responsible for. Reportability therefore follows from the stage's own
definition; the remaining question was only *where* it surfaces.

## Why the gap is real (evidence, not inference)

Pinned stack, read from the frozen source
(`~/.cargo/registry/.../renet2_netcode-0.16.1/src/client.rs:95`):

```rust
pub fn send_packets(&mut self, connection: &mut RenetClient) -> Result<(), NetcodeTransportError> {
    if let Some(reason) = self.netcode_client.disconnect_reason() { return Err(...); }
    let packets = connection.get_packets_to_send();   // pops: renet's queues are emptied here
    for packet in packets {
        let (addr, payload) = self.netcode_client.generate_payload_packet(&packet)?;
        self.socket.send(addr, payload)?;
    }
    Ok(())
}
```

The packets are taken out of renet's queues *before* anything can fail. When
the `?` fires, those payloads have left the queue and never reached the socket,
and no later call can produce them again: for the unreliable/sequenced class
that is a silent loss, and for every class it is a payload no caller — test or
app — can tell apart from a payload the application never queued.

The failure is genuinely reachable, and the round it happens in has no other
voice. `renet2_netcode`'s transport update ends with
`if let Some((packet, addr)) = self.netcode_client.update(duration)`, and
`NetcodeClient::update` **logs** its own internal error and returns `None`
(`renetcode2-0.16.1/src/client.rs:291-299`) after having set the client state to
`Disconnected(...)` (`:343-346`). So on the round where the connect window is
exhausted, `NetcodeClientTransport::update` returns `Ok(())`, renet has not been
told anything yet, and `send_packets` is the *only* operation that observes the
dead connection — exactly once, at the end of that round.

Measured, not assumed: with the host silent and one round of
`window + 1` seconds handed to `update`, the round's event list was `[]` before
this change (the mutation run of
`accept_f54_c_a_client_send_that_never_left_is_reported_not_swallowed` prints
"the refused send is reported and nothing else happened this round: []"), and
the connection layer's own `TransportFault`/`Disconnected` pair follows on the
*next* round. The swallowed result was not a duplicate report; it was the first
report.

### The host has no equivalent gap

`NetcodeServerTransport::send_packets` (`renet2_netcode-0.16.1/src/server.rs:207`)
returns `()`, so there is no result to swallow at
`HostTransport::update`'s call site. Its per-packet failure path
(`send_packet_to_client`, same file) either disconnects the client whose address
is gone — which surfaces as `ServerEvent::ClientDisconnected` and then
`HostEvent::PeerDeparted` — or logs and lets renet's reliable layer retransmit
until the connection times out into a reported event. The client's `let _ =` was
therefore not mirroring a deliberate symmetry; it was the one place the pinned
stack *returns* a send verdict to its caller.

## What changed

All of it inside the F54 owner paths; no behaviour other than reporting changed.

* **`ClientEvent::SendFault { reason }`** (`crates/cs_net/src/transport.rs`) —
  emitted at the end of `ClientTransport::update` where the `let _ =` was.
* **One deliberate exception, so the report stays information:** a
  `NetcodeError::Disconnected(_)` that the connection already carries a reason
  for is *not* re-emitted. That fact is already `ClientEvent::TransportFault`
  (from `transport.update`) and `ClientEvent::Disconnected` (on the transition,
  exactly once), and the pinned layer reports it from every later round; without
  the exception one death would be restated forever, and every existing test
  that pumps a dead connection would see a new notice every round. Everything
  else — including a disconnect *this round* discovered, and any IO or payload
  refusal — is reported.
* **`ClientTransport::disconnect` now returns `Result<(), TransportError>`**
  instead of discarding it, and `ClientSession::leave` turns its verdict into
  its already-documented `ClientFault::Transport`. The hang-up itself is
  unconditional: the phase still becomes `Closed(LeftVoluntarily)`, because a
  session must not stay open because its farewell's report was an error.
* **`ClientSession::handle`** maps `ClientEvent::SendFault` to
  `ClientNotice::Dropped { reason: ClientFault::Transport(...) }` and changes no
  phase: teardown still comes from the connection layer's own verdict on the
  following round, so reporting a refused send closes nothing early and hides
  nothing.
* **Tests**, all three with the stage prefix, each driving production code and
  each failing when the part of the change it covers is removed (the runs are
  recorded under *Mutation runs* below):
  * `accept_f54_c_a_client_send_that_never_left_is_reported_not_swallowed` —
    transport level: the silent round reports exactly `[SendFault]`, the next
    round reports `[TransportFault, Disconnected]`, and the hang-up returns an
    error.
  * `accept_f54_c_a_refused_send_reaches_the_session_owner_as_a_notice` — the
    same round through `ClientSession::pump`: one named notice, phase still
    open, and the layer's verdict still closes it one round later.
  * `accept_f54_c_leaving_after_the_layer_gave_up_reports_the_hang_up` — the
    farewell half: `leave()` in that same round returns
    `Err(ClientFault::Transport)` carrying the layer's verdict instead of the
    `Ok` it used to return, the phase still becomes
    `Closed(LeftVoluntarily)`, and a later `leave()` stays refused.
  * `RawPeer::status` learned a `SendFault` label and the F54-B disconnect test
    learned to handle the hang-up's `Result`; neither assertion changed.

## A related discovery, deliberately not fixed here

`ClientTransport::disconnect` marks the renet connection disconnected *before*
it flushes, and renet's `get_packets_to_send()` returns nothing for a
disconnected client (renet2-0.16.1/src/remote_connection.rs:363, :590-594).
Confirmed against the pinned stack with a throwaway test (a queued message
yields one packet while connected, none after `disconnect()`; the probe was run
and deleted): **`ClientSession::leave`'s reliable `ClientPayload::Leave` is
never written to the socket by that call.** The host still sees `PeerLeft` in
`accept_f54_c_the_client_leaves_reliably_and_the_host_departs_it`, but only
because a later pump makes the netcode layer emit its own disconnect packet —
an app that stops pumping after a successful `leave()` tells the host nothing
until the host's connect window expires.

That is an ordering/teardown bug and a behaviour change, not a reporting gap, so
it is filed as **#713 (F54-C9)** instead of being folded into this task: this
task changes no behaviour without a spec-backed reason (AGENTS rules 1 and 4).

## Honest limits

* The conclusion "reportable" is a reading of the F54-B/C stage text and the
  `UI-NETWORK` contract against the pinned layer's source. It is not evidence
  about any original behaviour, and nothing here is `network_real`.
* Nothing here proves the swallowed send caused any observed failure. #695's
  hostile-peer flake is a *receiving*-path loss (its own finding says so); this
  task only closes the contract gap that made "cannot send" and "not trying"
  indistinguishable while diagnosing it.
* The deduplication rule is a judgement: a `Disconnected` verdict the connection
  already carries is treated as already reported. A future reader who wants the
  raw per-round error should remove that filter knowingly — it exists to keep
  one death from being reported every round, not to hide a class of failure.

## Mutation runs

Two separate mutations, each applied to the working tree, run with
`cargo test -p cs_net --test accept_f54_c_lifecycle`, then reverted with
`git checkout --` (both files were clean afterwards, and the branch was
re-checked before committing):

1. **The push is removed again** — `if let Some(reason) = refused { events.push(
   ClientEvent::SendFault { .. }) }` replaced by `let _ = refused;`:
   `31 passed; 3 failed`. All three tests of this change failed, the first with
   "the refused send is reported and nothing else happened this round: []" —
   the exact empty event list the swallowed `let _ =` produced.
2. **Only the farewell's verdict is discarded again** — `ClientSession::leave`
   returning `Ok(())` instead of its `farewell` result:
   `33 passed; 1 failed`. Only
   `accept_f54_c_leaving_after_the_layer_gave_up_reports_the_hang_up` failed,
   with "the hang-up carries the layer's verdict instead of discarding it:
   Ok(())" — so the third test covers the `leave` half on its own, and the two
   earlier tests do not.

## Sources

* Task Rally #708 (question, acceptance criteria) and its description's pointer
  to `docs/findings/2026-10-06-f54-c-hostile-peer-flake-loopback-path.md`,
  "Honest limits".
* `specs/F54-modern-multiplayer-transport-and-authority-protocol.md`,
  `### F54-B` / `### F54-C`.
* `docs/contracts/UI-NETWORK.md`, "Reliability and prediction".
* `renet2 =0.16.1`, `renet2_netcode =0.16.1`, `renetcode2 =0.16.1` sources as
  read from the frozen Cargo registry (line references above).
* `crates/cs_net/tests/accept_f54_c_lifecycle.rs` — the three tests added here
  and the mutation runs that proved they fail without the change.
