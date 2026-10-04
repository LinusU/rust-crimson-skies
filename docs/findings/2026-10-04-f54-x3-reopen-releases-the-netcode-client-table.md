# F54-X3: `HostTransport::reopen` released the session's bookkeeping but not the pinned client table

Date: 2026-10-04. Task: #606 F54-X3 "Reset the netcode client table on
`HostTransport::reopen` (retry after reopen can never be told)", opened from the
F54-X2 loopback investigation. Capabilities used: ordinary build/test,
`network_local` loopback. No original data, no `network_real`, no human play.
This is engine transport work on the F54-B pinned stack; nothing here claims
anything about the original game's networking.

## The split state

`HostTransport` holds two things that each keep their own per-client table:

- `self.clients` — this crate's bookkeeping (awaiting-hello, refused, peered),
  plus `RenetServer`'s reliable connections underneath it;
- `self.transport` — the pinned `NetcodeServerTransport`, whose private
  `NetcodeServer` owns `clients`, the slot table the `client_id` challenge
  checks against (`renet2_netcode-0.16.1/src/server.rs`,
  `renetcode2-0.16.1/src/server.rs`).

The old `reopen` cleared only the first: `self.clients.clear()` plus
`self.server.disconnect_all()`. `RenetServer::disconnect_all` **marks** the
reliable connections disconnected; it frees nothing at the netcode layer. The
netcode slot dies only in the sweep at the bottom of
`NetcodeServerTransport::update`, which disconnects every client whose reliable
connection reports `!is_connected` — and that sweep runs *after* the update's
packet-processing loop, not before it (`renet2_netcode-0.16.1/src/server.rs` —
inbound `process_packet` loop first, the `!server.is_connected` sweep last).

So a connection request from a client reconnecting with the same `client_id`,
arriving in the first `update` after `reopen`, is processed while the spent
epoch's slot still stands. In `renetcode2` both halves of that collision are
silent:

- a `ConnectionRequest` whose `client_id` already occupies a slot is denied by
  `connection_denied`, which on `NativeSocket` is a no-op — the client hears
  nothing and only retransmits on its own 250 ms pacing;
- a `ConnectionResponse` that names a slotted `client_id` is ignored outright
  (`find_client_slot_by_id(...).is_some()` → `Ok(ServerResult::None)`,
  `renetcode2-0.16.1/src/server.rs`), and its pending-client entry is already
  consumed — retries of that response are never matched again, so the client
  sits in `SendingConnectionResponse` until its own token timeout.

"Can never be told" is exact for the response case and effective for the
request case: the client cannot distinguish a silent deny from a lost packet,
and the window it waits in is the token's `timeout_seconds`, not the retry's.

## Why a naive reconnect self-heals, and where it does not

A scratch probe (same `client_id`, fresh socket, `reopen` between epochs, host
pumped every round) observed the reconnect granted in ~11 rounds: the client's
first request is silently denied, the same `update`'s sweep frees the stale
slot, and the client's 250 ms-paced retransmission then walks a clean table.
That is a self-heal, not a guarantee — it depends on the host pumping again
before the request lands, and it does nothing for the response collision
above, where the pending entry is already gone. The fix does not rely on the
race: the slots are dead before `reopen` returns.

## What changed

`crates/cs_net/src/transport.rs`:

- `HostTransport::reopen` now calls the pinned transport's own
  `disconnect_all(&mut self.server)`. `NetcodeServerTransport::disconnect_all`
  (documented in the pinned crate as the close/exit path: "sends the
  disconnect packet instantly") calls `NetcodeServer::disconnect` per held
  client, which frees the slot synchronously, emits that client's
  `Packet::Disconnect`, and removes its reliable connection — inside the
  `reopen` call, not on a later update. The method's returned count is the
  connection-layer count it actually hung up (`self.transport.connected_clients()`
  taken before the release), which is also what includes connections already
  condemned at the reliable layer but still holding slots.
- `HostTransport::connected_clients` now counts the pinned layer's table
  (`self.transport.connected_clients()`) instead of this crate's
  `self.clients` map. Its doc already promised "connections this transport
  still holds" — the netcode slot a condemned-but-unswept client still
  occupies — and the crate-side map could under-report it.

`crates/cs_net/src/lifecycle.rs`: `ServerSession::reopen`'s doc updated to
name what the count now covers.

Pinned versions untouched: `renet2 = "=0.16.1"`,
`renet2_netcode = "=0.16.1"`, `renetcode2 = "=0.16.1"` in
`crates/cs_net/Cargo.toml`.

## The test

`accept_f54_c_a_retry_tells_a_returning_client_its_verdict`
(`crates/cs_net/tests/accept_f54_c_lifecycle.rs`) drives real loopback:

1. joins two members (client ids `0xC1`, `0xC2`), `close`s the epoch and
   retries with `reopen`;
2. asserts `reopen` returned **2** — both spent-era slots released, including
   the ones `close` had already condemned at the reliable layer;
3. asserts `connected_clients() == 0` immediately after `reopen` — no
   spent-era slot survives at the connection layer;
4. reconnects client id `0xC1` on a fresh socket and asserts it is granted the
   **new** epoch and is a member;
5. connects client id `0xC2` with a `rules_sha256` the admission gate refuses
   and asserts the named `HandshakeReject::RulesMismatch` arrives — the client
   is told, not left waiting;
6. injects a packet stamped with the first epoch and asserts it is refused
   with `stale_session` and applies to nothing.

## Sensitivity

Each half of the change was removed in turn; the test fails on both:

| Removed | Failure |
| --- | --- |
| `self.transport.disconnect_all(&mut self.server)` reverted to `self.server.disconnect_all()`, count reverted to `self.clients.len()` | `the retry hung up both connections the spent epoch still held` — `left: 0, right: 2` |
| Only the release removed (`self.server.disconnect_all()`, new count kept) | `no spent-era slot survives at the connection layer` — `left: 2, right: 0` |

## Limits, honestly

- The reset covers connections the spent epoch holds. The pinned layer has no
  per-client `disconnect` on the transport (`disconnect_all` only), so a
  non-retry path (`disconnect_client`, `disconnect_peer`) still relies on the
  next `update`'s sweep to free one slot — a same-id reconnect inside that
  window sees the same transient deny and the same self-heal. That window is
  narrow and unchanged in scope by this task.
- `NetcodeServer::disconnect_all` sends `Packet::Disconnect` once; like every
  packet in the pinned layer it is not retransmitted. The hang-up tests
  already treat a lost one as a test failure rather than a retry.
- A same-id client racing a *live* (not spent-epoch) connection is
  first-come-first-served in the pinned layer — the loser's pending entry is
  consumed and its response ignored. That is inherent pinned behavior, not
  the retry's bug, and is not claimed fixed here.
