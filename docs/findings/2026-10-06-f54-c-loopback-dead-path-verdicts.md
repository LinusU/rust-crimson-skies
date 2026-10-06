# F54-C loopback tests: every live-path wait now classifies verdict vs dead path, once

Date: 2026-10-06. Author: swe2-max-1. Task: Rally #707.

Capabilities used: ordinary build/test, `network_local` loopback. No original
data, no `network_real`, no human play. Engine test design on the F54-B
pinned transport; nothing here claims anything about the original game's
networking.

## The problem

Rally #695 gave one test — the hostile-peer cut-off — the split between "the
session answered" and "the loopback carried nothing". The remaining ~30 tests
in `accept_f54_c_lifecycle.rs` still shared the old fixture: `Link::joined`,
`RawPeer::handshake` and the `pump_until*` waits all panicked at the session
when what had actually died was a socket the kernel's demultiplexer stopped
routing. Under the committed F54-X10 churn driver those flakes showed up in
every live-path test of the file, not just the hostile one — the same
failure, measured in #695's table as the "handshake-class" majority (104/600
runs at the time, versus 11 runs of the reported signature).

## What changed

All of it in `crates/cs_net/tests/accept_f54_c_lifecycle.rs`; no production
code changed, no assertion weakened, nothing skipped.

1. **The shared helpers express the distinction once.**
   `Err(DeadPath)` is now the only rebuildable outcome: a wait returns it when
   the connection layer's own state — `disconnect_reason`, `is_connected`, or
   a `TransportLost` closure — proves the path ended before the awaited thing
   could be observed. `live_fixture` runs an attempt up to
   `DEAD_PATH_ATTEMPTS` times, rebuilding only on `DeadPath`; a panic inside
   an attempt is a verdict and leaves the loop directly.
2. **Every live-path test is an attempt.** `Link::try_joined` hands back the
   unfinished link as `Err(Box<Link>)` so the caller classifies it
   (`ungranted`); `RawPeer::try_handshake` is `Option`-returning; the
   `try_pump_until*` family replaces the panicking waits; `try_pump_peer_until`
   re-fires droppable-channel payloads each round (the #695 rule: the
   expectation rests on the session's answer, not on which datagram survived).
   Classifiers by role: `silent_client`/`ungranted_client` for a `ClientSession`,
   `ungranted_peer`/`peer_delivery`/`peer_path_dead` for a `RawPeer`.
3. **A host hang-up is a verdict, not a dead path.** `DisconnectedByServer`
   is the pinned layer reporting that the *host* acted — the session deciding.
   `peer_path_dead`/`peer_delivery`/`try_pump_until`'s tail exclude it: a
   teardown observation that ends `DisconnectedByServer` is asserted, never
   rebuilt. A hang-up that arrives *ahead* of a refusal it was meant to
   follow is still a dead path — the answer under test never arrived.
4. **A connection still nominally `Connected` is not proof of a live path.**
   The pinned `renetcode2` client times out only after `timeout_seconds`
   without a *received* packet, and every packet that does land —
   including one-way keepalives trickling through a churn-degraded path —
   resets that clock. A fixture whose sockets connected and then died
   mid-scenario therefore presents as "connection held for the whole
   window and nothing ever arrived", not as a disconnect. That signature
   is classified `DeadPath`: the host emits a notice for every packet it
   processes, so a whole window of silence can only mean nothing was ever
   delivered to it.
5. **A connection that died *before* the observation the attempt needs is the
   path's failure.** Examples: the refused client whose connection is gone
   before `reopen` has nothing to release (the retry's count is then the
   fixture's answer, not the session's); the spent-era connections in the
   retry tests must still stand when `reopen` runs or the count is
   unobservable; the quiet-window client's connection dropping inside the
   *two-minute* window is path death, while the same drop inside the
   *one-second* window is the window arithmetic itself — the assert knows the
   difference by `still_connected`.

## The race the conversion exposed

The first version of `try_pump_until` early-exited the wait the round
`ClientTransport::disconnect_reason` became `Some`. Three tests then failed
deterministically — no churn needed — on the teardown waits
(`try_pump_until_closed`, the retry's hang-up wait).

The pinned stack materializes a disconnect in two steps: the netcode layer's
`disconnect_reason` is set in the pump that decodes `Packet::Disconnect`,
while the renet-level `is_connected` flip and the `Disconnected` client event
land on the *next* `transport.update`, when `NetcodeClientTransport::update`
sees the reason at its top and calls `disconnect_due_to_transport`. A wait
whose condition includes `!is_connected` — a teardown must see the connection
out, not just the message — was being abandoned exactly one round before its
condition could come true, and `path_dead` then (correctly) reported
`DisconnectedByServer` as a verdict and panicked "never observed".

Fix: the early exit waits until both layers agree the end is real —
`disconnect_reason().is_some() && !is_connected()`. A `Connecting` client is
unaffected (its reason is `None` until it gives up).

## Sizing the rebuild bound — measured

Same machine, same driver as #695's table
(`F54X10_MODE=transport F54X10_PROCS=6 F54X10_PAIRS=250 F54X10_SPINNERS=4`,
looped `f54x10_fleet` for the whole measurement), running the whole
`accept_f54_c_` selection:

| bound | runs passed | failure shape |
| --- | --- | --- |
| `DEAD_PATH_ATTEMPTS` = 6, back-to-back rebinds | 45/50 | every failed run = one or more fixtures whose *all six* attempts came up `connecting`/`closed` — stillborn pairs inside one churn burst |
| `DEAD_PATH_ATTEMPTS` = 24, back-to-back rebinds | 49/50 | one run: **24** consecutive `connecting` deaths inside a ~200 ms sequence — the burst outlived the attempts, not the dice |
| 24 attempts, 100 ms spacing, conn-held-silent still a panic | 46/50 | 4 runs: one fixture each ending `is_connected() == true` after the whole window with nothing session-level ever arriving — the nominally-connected dead path described above |
| 24 attempts, 100 ms spacing, conn-held-silent as `DeadPath` | **50/50** | fleet churned 42 cycles spanning the whole run |

(Measurement caveat worth repeating: the first "50/50" reading of the
spaced variant was invalid — the backgrounded churn loop was reaped ~10 s
into the run, so most of it actually ran quiet. The 46/50 and the final
50/50 rows are with the fleet verified cycling across the entire window.)

The three failure classes the measurements imply:

* **Stillborn pair** (iid per attempt): the kernel demultiplexer never routes
  a fresh pair. F54-X10 measured 1–69% of transport pairs dying under this
  driver; our run implies ~0.5 per attempt at the worst. The count is the
  lever: `0.5^24 ≈ 6e-8` per fixture.
* **Churn burst** (correlated): a wall-clock window in which *no* fresh pair
  routes. Back-to-back stillborn attempts burn ~8 ms each, so 24 of them span
  under half a second — inside one burst. A 100 ms pause between rebuilds
  spreads a 24-attempt sequence over ~2.5 s+, past the bursts observed, at a
  cost paid only on paths that were already declared dead. The pause is
  between fixtures, never inside one, so it cannot change a session's answer.
* **Nominally-connected silence** (the newest class): the netcode handshake
  completes, then the path degrades to one-directional — stray host
  keepalives still land, resetting `last_packet_received_time`, so the
  client stays `is_connected() == true` for the whole 120 s window while no
  session packet ever arrives. Neither a delivered verdict nor a
  layer-reported death — exactly the "carried nothing" shape the task's
  rule says to rebuild, which is why `silent_client`, `ungranted_client`,
  `ungranted_peer`, `peer_delivery` and `try_pump_until`'s tail all read it
  as `DeadPath` now. A deterministic session bug on a healthy host fails
  identically on all 24 attempts and is still reported — as a failure.

Every failure line in the measured runs names itself a fixture failure —
"the loopback never carried this fixture" with each dead end's reason —
never a session verdict, which is the point of the split.

## Honest limits

* A burst that outlives a whole rebuild sequence still fails the run — the
  bound is a count, not a guarantee, and its report then claims this host's
  loopback could not be had, which is the honest statement.
* `DEAD_PATH_ATTEMPTS` = 24 buys worst-case ~2.5–10 s of rebuild spread per
  fixture under this driver; on a quiet host the first attempt succeeds and
  the bound costs nothing.
* The kernel mechanism remains inferred (no dtrace/kdebug on this host); what
  is measured is that fresh pairs either route or they don't, and that
  rebuilds spread over wall-clock time are what recover.
* Loopback on one machine is `network_local` per `AGENTS.md`; nothing here is
  `network_real` evidence, and no original data was read.

## Sources

* Task Rally #707 (acceptance criteria) and #695 (the first dead-path split).
* `docs/findings/2026-10-06-f54-c-hostile-peer-flake-loopback-path.md` — the
  same fault seen at one test; this file generalizes it.
* `docs/findings/2026-10-04-f54-x10-loopback-loss-diagnosis.md` — orphaned
  sockets, mid-life unreachability, the churn-driver recipe.
* `docs/findings/2026-10-04-f54-x1-retry-hang-up-test-determinism.md` — no
  wall clock in the pinned stack; pump-round budgets.
* `renetcode2-0.16.1` `NetcodeClient::process_packet`/`update` and
  `renet2_netcode-0.16.1` `NetcodeClientTransport::update` — the two-step
  disconnect materialization behind the `try_pump_until` early-exit race,
  and the `last_packet_received_time` reset that lets trickling keepalives
  hold a dead path's connection marked `Connected`.
* `crates/cs_net/tests/accept_f54_x10_loopback_loss.rs` — `f54x10_fleet`, the
  committed churn driver used for the measurement.
