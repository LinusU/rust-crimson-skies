# F29-C.6: the mission host's eject edge calls `confirm_bailout`

Date: 2026-10-14. Task: F29-C.6 "Call confirm_bailout from the mission host's
eject edge" (Rally #674), the producer half of F29-C
(`specs/F29-damage-zones-armor-destruction-and-bailout.md`, section `### F29-C`,
acceptance case **AC04**), wired under
`specs/F39-objectives-triggers-timers-spawn-groups-and-dialogue-cues.md`,
stage `### F39-C`. Shared contracts: `docs/contracts/SCRIPT-MISSION.md`
("Objective event ordering") and `docs/contracts/FLIGHT-PHYSICS.md`
("Commands belong to one simulation tick and actor generation").

Task test prefix: **`accept_f29_c_6_`** (3 tests in
`crates/cs_app/tests/accept_f29_c_6_eject_edge_bailout.rs`).

Capabilities used: ordinary build/test only — this task measures nothing in
the owner's installation, so no `private/evidence/` report is produced.

## Files and the one observable failure (listed before editing)

- `crates/cs_app/src/input/session.rs` (**producer**): the
  `EjectConfirmation` record, `InputSession`'s `eject_confirmations` queue,
  the `pump_frame` boundary-loop fill, the
  `eject_confirmations`/`take_eject_confirmations` accessors, and the
  `restart` clear.
- `crates/cs_app/src/input/mod.rs`: the `EjectConfirmation` re-export
  (wiring only).
- `crates/cs_app/src/objectives.rs` (**session gate**):
  `ObjectiveSession::confirm_bailout`, the `pending_refusals` queue the
  refused registrations wait in, `step` lifting them into
  `SessionTick::refusals`, `retry` clearing them, and the doc updates the
  new path makes honest (`SessionRefusal::Transition`, `SessionTick::refusals`,
  the `PilotBailedOut` arm's "nothing reaches it today" note).
- `crates/cs_app/tests/accept_f29_c_6_eject_edge_bailout.rs` (**new**).
- `crates/cs_app/tests/accept_f29_c_04_bailout_session_consumer.rs`: the
  module doc's "this session exposes no mutable path to its runtime" claim,
  now false — the test itself is unchanged and still pins the unconfirmed
  refusal.
- This file.

No protected path, no `Cargo.toml`, no binary, no original data committed.

**One observable failure, before the change.** F29-C.4 built the declaration
gate — `ObjectiveRuntime::confirm_bailout` — and its finding named the two
halves that were still missing: nothing read `ControlBuffer`'s
`FlightCommand::Eject` edge, **and** `ObjectiveSession` exposed only
`runtime() -> &ObjectiveRuntime`, so a host holding the session could not
register a confirmation at all. In the wired app every `PilotBailout`
lifecycle fact was refused `BailoutRefusal::Unconfirmed`, which
`accept_f29_c_04_bailout_session_consumer.rs` pins as the then-honest state.

## The designed behavior

Two surfaces, and the once-per-press rule lives where the edge is delivered:

1. **The producer records at the input boundary, not per frame.**
   `InputSession::pump_frame`'s boundary loop already asks
   `ControlBuffer::begin_tick` for each tick's edges — the delivery that
   makes an edge once-per-press across physics substeps. Each delivered
   `Action::Flight(FlightCommand::Eject)` edge now also pushes one
   `EjectConfirmation { tick, confirmation }`, stamped with the boundary's
   tick and `BailoutConfirmation::PlayerEject { context }` under the context
   that owns the devices *there*. `set_context` discards pending edges when
   the context changes, so the stamped context is always the press's own —
   and `take_eject_confirmations` is the once-only handoff, so a mission
   host ticking twice inside one render frame still registers each press
   once. A per-frame caller reading `FrameOutcome::delivered` would get the
   same edge again on a re-read and meet `BailoutRefusal::AlreadyConfirmed`;
   the queue makes "once per press" a property of the producer, not of the
   consumer's discipline.
2. **The session's mutable path is `ObjectiveSession::confirm_bailout`.** It
   forwards to the runtime's gate — which re-checks `admits()` where the
   confirmation is recorded, so a struct-literal `PlayerEject { context }`
   produced under a context that accepts no flight command is refused — and
   a refused registration is queued as `SessionRefusal::Transition { reason }`
   for the next `step` to lift into `SessionTick::refusals`. Both surfaces
   tell the truth: the caller gets the `Err(BailoutRefusal)`, and a consumer
   reading only the tick report still learns the press was refused — the
   same "a refusal is a queryable fact" rule the stream-side refusals
   follow, and the shape the F29-C.4 finding asked for.

**Identity** is the caller's, explicitly: `confirm_bailout(actor, ...)` takes
the mission `cs_script::ir::ActorId` the objective runtime is keyed by, and
the world's `cs_types::net::ActorId` → mission `ActorId` bridge stays the
host's surface (`cs_sim::mission` says so in the same words) — the tests name
the actor rather than exercising a mapping production code does not have.

**What this slice deliberately does not wire.** The mission host's
`step`/`mission_host_tick` composition — which holds `ObjectiveSession` and
could drain `PlatformInput`'s session for these records — still has no
world→mission actor bridge for the player's eject edge, and inventing one is
exactly what this task forbids. The surfaces here are the seam that host
consumes; the bridge it feeds them with is a follow-up, not a hidden helper.

## Tests

Every test drives production code: a real `InputSession` over the designed
default map with a connected fixture keyboard, real `DeviceEvent` frames
through `pump_frame`, real `ControlBuffer` boundaries, and a launched
`ObjectiveSession` over the declared F39-C fixture stepped with real
`TickInput` lifecycle facts. No test-only bridge.

| test | what it pins |
| --- | --- |
| `accept_f29_c_6_a_delivered_eject_edge_confirms_the_bailout` (minimum) | one press across a two-boundary frame delivers one edge and one `EjectConfirmation{tick: Tick(0), PlayerEject{Flight}}`; the taken queue is drained; a second press is a second record at its own boundary tick; `confirm_bailout` registers the first, refuses the second as `AlreadyConfirmed`; the tick reporting `PilotBailout` emits `PilotBailedOut{actor, confirmation}` carrying the first press's confirmation, names the refused repeat on `SessionTick::refusals`, counts no kill, keeps the airframe in the live registry, and settles no outcome |
| `accept_f29_c_6_a_cinematic_stamped_eject_is_refused_by_name` | the live session under `InputContext::Cinematic` drops the press unread — no edge, no record; the same edge delivered by a bare real `ControlBuffer` is refused by `BailoutConfirmation::from_action` *and* by `confirm_bailout` as `InputContextRefused{context: Cinematic}`; the tick reporting the lifecycle fact names both `SessionRefusal::Transition{InputContextRefused}` and `Transition{Unconfirmed}` in order, emits no `PilotBailedOut`, latches no transition |
| `accept_f29_c_6_a_scripted_confirmation_needs_no_input_edge` | `BailoutConfirmation::scripted` registers through the same session path with no input edge at all — `confirm_bailout` is a session surface, not an input special case |

## Mutation probes

Each probe edited one production line, ran the `accept_f29_c_6_` selection,
recorded the failing tests, and restored the file from a saved copy.

| probe | edit | result |
| --- | --- | --- |
| the boundary producer is removed | the `pump_frame` eject-recording loop deleted | 1 of 3 failed: the minimum — `take_eject_confirmations` came back empty, so nothing was ever registered |
| the refused registration is dropped | `confirm_bailout`'s error-path queue push removed (the `Err` still returned) | 2 of 3 failed: the cinematic case and the minimum's `AlreadyConfirmed` report — the caller's `Err` alone is not a session fact |
| the record is per-frame, not per-boundary | the fill moved out of the boundary loop to iterate `delivered` once per frame | 1 of 3 failed: the minimum — the record carried `Tick(2)` (the frame's last boundary), not `Tick(0)` where the edge actually delivered |

## What is NOT claimed

No original-fidelity claim of any kind. The `Digit3` → `Eject` binding is
this project's **designed** default (F22-A); the original's eject binding is
F22-D's measurement, not this task's. What the original did when its eject
ran — and what a bailout did to a mission's result — is measured nowhere here
(`BailoutResultPolicy::Unmeasured` stands, per the F29-C.4 finding; F29-D owns
the result question). This stage can award at most **checked**.

## Follow-ups left open

- **The host-side composition.** A `MissionHost`-holding system that drains
  `take_eject_confirmations` and calls `confirm_bailout` with the player's
  mission `ActorId` still does not exist: it needs the world→mission actor
  bridge the task names as the host's surface, which no production code has.
  That wiring — plus the lifecycle producer that would feed
  `TickInput::lifecycles` — is the remaining gap between "the surfaces exist"
  and "the wired app confirms a bailout".

## Sources

- `specs/F29-damage-zones-armor-destruction-and-bailout.md` (`### F29-C`,
  AC04), `specs/F39-*.md` (`### F39-C`), `docs/contracts/SCRIPT-MISSION.md`,
  `docs/contracts/FLIGHT-PHYSICS.md`.
- `docs/findings/2026-10-05-f29-c-4-bailout-mission-transition.md` — the gate
  this task feeds, and the follow-up that named the two missing halves.
- `docs/findings/2026-10-02-f29-c-damage-consumers.md` — the lifecycle
  producers the `PilotBailout` fact arrives through.
- `docs/findings/2026-10-03-f39-c-mission-program-ui-and-dialogue-wiring.md`
  — the `ObjectiveSession` producer→runtime→consumer shape `confirm_bailout`
  joins.
- `crates/cs_app/src/input/session.rs` (the `UiRequest`/`take_faults`
  once-only-drain pattern), `crates/cs_app/src/input/platform.rs` (the Bevy
  producer pass), `crates/cs_sim/src/objectives/bailout.rs` (the gate),
  `crates/cs_sim/src/control.rs` (`ControlBuffer::begin_tick`),
  `crates/cs_types/src/input.rs` (`InputContext::accepts`,
  `ActionMap::designed_default`).
