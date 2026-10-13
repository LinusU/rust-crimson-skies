# F54-C follow-up: the Bevy-scheduled session drive against a cs_sim ledger

Date: 2026-10-13. Task: Rally #592 "Drive a cs_net session from the Bevy schedule
and a cs_sim ledger" (follow-up from F54-C).
Capabilities used: ordinary build/test only. `network_local` loopback; no
original data read and nothing here asserts original networked behavior. All of
this is newly authored engine design.

## What this stage closes

F54-C's own "Not wired" record said a Bevy plugin or system that owns and pumps a
`cs_net` session was left for the next stage. This is that stage.

`crates/cs_app/src/network/session.rs` (new) binds the lifecycle to the
schedule:

- `NetHost` (a `Resource`) owns a `ServerSession`, the authoritative
  `NetStateLedger` it publishes from, the shared `WorldOrigin` frame, the
  per-peer `ControlBuffer`s the wire feeds and the remote `WeaponOrder` queue.
  Construction refuses a ledger and a session of different epochs
  (`NetHostError::ForeignSession`) so one generation's state can never publish
  under another's header.
- `NetClient` (a `Resource`) owns a `ClientSession` plus the origin and
  interpolation bounds a mirror is built from.
- `NetMirror` is a `Resource` newtype around `cs_app::network::physics::
  NetSession`. `physics.rs` stays deliberately free of Bevy (the typed boundary
  carries no schedule), and Bevy 0.19's `Resource` requires `Component`, so the
  newtype — not a `Resource` impl on `NetSession` — is what the world holds.
- `drive_net_session` is the exclusive system the
  `NetSessionDrivePlugin` installs in `FixedPostUpdate` after
  `PhysicsSystems::StepSimulation` (the same ordering the weapon schedule
  uses). Once per committed fixed tick it:

  1. Pumps each installed session and forwards every notice into
     `NetSessionReport` (bounded at 256 per side, overflow counted).
  2. Drains `ServerSession::drain_work` and consumes each `PeerInput`: frames
     fold into the peer's `ControlBuffer` (the F22-A authoritative control
     path); fire requests mint `FireIntent`s through
     `recovery::fire_intent` — the conversion `cs_net` cannot do because it
     must not depend on `cs_sim` — and queue as `WeaponOrder::Fire`s for the
     weapon schedule's own `ForeignTick`/`DuplicateIntent` judges. The
     ledger's input book is observed and the wire ack sent per packet.
  3. While `Live`, quantizes the ledger with `publish_snapshot` and puts it on
     the wire through `ServerSession::publish_snapshot`; every other phase is
     `NetPublish::Skipped` with its name, not a silent wire.
  4. Builds the client `NetMirror` the first run a grant exists, serving
     exactly the granted epoch; a mirror for another generation is torn down
     and replaced rather than fed a foreign epoch's traffic. Reliable event
     bodies apply through `apply_event`; the newest `snapshot()` is ingested
     at most once each (the mailbox is latest-wins; refeeding would replay
     one frame as if it arrived twice).
  5. Releases a departed peer's control state on `PeerLeft`/`CutOff`/`HungUp`:
     held axes neutralized, queued edges dropped, and the gate's actor
     ownership released — the gate is a separate record that does not know
     the peer left, so a released pilot that still owns its actor surrenders
     it rather than leaving a ghost no other peer may bind.
  6. Records every session resource that went away as a `NetTeardown` by
     side and generation, including what the mirror's own `teardown`
     released.

Everything the run lacked is named in the report (`NetDriveMissing::{Clock,
Timeline}`, `NetPublish::Skipped`, per-packet `ConsumedInput` records) rather
than guessed a step length, a committed tick or a successful outcome.

## Scope boundaries kept

- `SessionReceiver` and transport-gate reconciliation stay with the F58-C
  follow-up, Rally #1221: this drive consumes already-admitted `PeerInput`; it
  never re-runs admission.
- Deeper `NetSession` scheduling — sampling interpolated poses, local
  prediction reconciliation, tracer expiry — is F57-C.01 (Rally #657). This
  drive feeds the mirror; it does not render from it.

## Discovery: input sequence numbering crosses layers off by one

Wire input sequences are **zero-based**: `ClientTransport::send` stamps
`message.header.sequence` from `next_sequence`, initialized to 0
(`crates/cs_net/src/transport.rs`), so a client's first input packet is
sequence 0.

`NetStateLedger::observe_input` starts `acknowledged` at 0 and refuses
`sequence <= acknowledged` as `NetStateError::StaleInput`
(`crates/cs_sim/src/net_state.rs:1486`). The wire's legitimate first packet is
therefore always booked as a replay: `StaleInput { presented: 0,
acknowledged: 0 }`, deterministically, on every session.

Nothing downstream breaks: the packet is still consumed (frames applied, fires
minted), `acknowledge_input` still sends the wire ack, and
`acknowledged_input()` staying at 0 makes the published `acked_through`
correctly retire sequence 0. But the book's refusal record is a false replay
report on every session's first input, and which side owns the numbering is a
`cs_sim`/`cs_net` decision — `cs_sim` is outside this task's owner paths, so it
is filed as Rally #1385 rather than fixed here. The acceptance test asserts the
observed boundary and will need updating with whatever the ruling changes.

## Observable failure without the implementation

Before this binding, nothing in `cs_app` constructed or pumped a session:
admitted input could never reach a `ControlBuffer`, fire requests never became
`FireIntent`s, the ledger was never put on the wire, and a client never built
a mirror. Verified by disabling the plugin's `add_systems`: `NetSessionReport`
stays at `ticks: 0`, the handshake never grants and all three loopback
acceptance tests fail.

## Files

- `crates/cs_app/src/network/session.rs` — the binding described above.
- `crates/cs_app/src/network/mod.rs` — `pub mod session;` plus module docs.
- `crates/cs_app/tests/accept_f54_c_drive.rs` — four acceptance tests over a
  real loopback pair (one `PhysicsSession` world carrying both `NetHost` and
  `NetClient`, the listen-server shape): the full round trip including mirror
  replacement and both teardowns; a departed peer's control/ownership release;
  an unbound peer named rather than owned; and the missing-authority report.
- `docs/findings/2026-10-13-f54-c-session-drive-binding.md` — this note.

## Checks

- `cargo fmt --all -- --check` — clean.
- `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` — clean.
- `cargo test --workspace --locked` — all pass.
- `cargo test --workspace --locked -- accept_f54_c_drive_ --include-ignored` —
  4 tests discovered, all pass.
