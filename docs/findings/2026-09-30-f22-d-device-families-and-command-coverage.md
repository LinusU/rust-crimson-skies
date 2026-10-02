# F22-D: All declared device families and original command coverage

Date: 2026-09-30. Task: F22-D "Test all declared device families and original
command coverage"
(`specs/F22-input-bindings-devices-and-control-ownership.md`, section
`### F22-D`). Shared contract: `docs/contracts/UI-NETWORK.md`. Capabilities
used: **ordinary build/test only** — no `$CS_GAME_DIR` read, no evidence
report required, no `gpu`/`audio`/`human_play`/`network_real` needed. The
`retail` capability exists on this machine; this stage deliberately did not
use it, because its own "Required capabilities" line says ordinary build/test
and reading the installation would make the stage evidence-bound
(`docs/contracts/CLI-EVIDENCE.md`).

## Files and the one observable failure (listed before editing)

- `crates/cs_types/src/input.rs`: `ActionMap::designed_default` (three
  bindings added so a declared command is reachable) and the coverage test
  `accept_f22_d_the_designed_map_reaches_every_declared_command_and_family`.
- `crates/cs_app/src/input/session.rs`: `InputSession::set_context` (a context
  that changes discards the buffer's undelivered edges and reports them),
  the `InputFault::Suppressed` doc, and the unit test
  `accept_f22_d_switching_context_discards_the_press_that_was_queued_for_it`.
- `crates/cs_app/src/input/mod.rs`: module documentation only (wiring-level
  doc: F22-D is named as a stage, the context-switch discard is stated).
- `crates/cs_sim/src/control.rs`: `ControlBuffer::drain_pending`'s doc (a
  context change is now one of its callers) and the consumer coverage test
  `accept_f22_d_the_consumer_buffers_every_declared_command`.
- `crates/cs_app/tests/accept_f22_d_device_families_and_command_coverage.rs`
  (new): the four cross-crate acceptance scenarios.
- `docs/findings/2026-09-30-f22-d-device-families-and-command-coverage.md`
  (this file).

**One observable failure:** a render frame that the clock commits no tick to
leaves its press queued in `ControlBuffer`, and the next input boundary —
wherever it happens to run — delivers it. Open a text field (or a menu)
between the press and that boundary and `InputSession::pump_frame` handed the
simulation `fire_primary` while the field owned the devices: AC04's own
scenario, "open text entry and confirm flight commands are not emitted",
failing with `outcome.delivered == [fire_primary]` and
`controls().pending_edges() == 1`. This is the ordinary case, not an exotic
one: at a display rate above the tick rate about half the frames commit no
tick at all. Probe row 1 below shows the test failing under exactly that
mutation.

The map half of the stage had its own observable failure: `Eject` and
`TargetPrev` were declared in `FlightCommand::ALL` but bound to no source, so
no device could ever produce them; probe row 2 shows both coverage tests
failing with them removed.

## What the stage makes testable

1. **Every declared device family drives the simulation.** For each of
   `DeviceClass::ALL` (`keyboard`, `mouse`, `gamepad`, `joystick`), every
   source the designed map binds for that family is exercised through its own
   `DeviceEvent` variant: an edge target must arrive as exactly one entry in
   `FrameOutcome::delivered`, an axis target must arrive in
   `ControlBuffer::axis` as *reading × binding scale* within the `i16`
   quantization tolerance (a digital source contributes `1.0` and the
   binding's `scale` carries its direction), and releasing the source must
   return the axis to exactly `0.0` without re-firing the edge. A family with
   no binding fails the first assertion, so a declared family can never
   quietly become unreachable.
2. **Every declared command reaches the consumer.** All 18
   `FlightCommand::ALL` entries have a source in the designed map and are
   observed where the simulation reads them: an edge in `delivered` exactly
   once with nothing left queued, an axis in the control buffer at the value
   the report asked for, and the four throttle commands as the position they
   actually moved `ThrottleSteps` to (`ThrottleIdle`/`ThrottleFull` are set
   up at the opposite end first, so "the direct setting wins" is visible).
   All seven `UiAction::ALL` entries arrive as one `UiRequest` stamped with
   `InputContext::UiNavigation` and its tick, with the control buffer and the
   flight-consumer trace untouched — and the same source emits nothing in
   text entry.
3. **The consumer covers the vocabulary too.** The `cs_sim::control` test
   walks `FlightCommand::ALL` and checks each command is buffered the way its
   kind demands: a continuous command is held across boundaries (still driven
   after `begin_tick`), an edge command is delivered at its tick exactly once
   and never returns on a later substep, and an edge never moves an axis.
4. **AC04 end to end, with the contrast that makes it meaningful.** One
   session with all four families connected: first a full report per family
   in flight context, which must deliver *every* bound edge command and drive
   all four continuous axes (so the assertions below cannot pass because
   nothing works); then a queued press and the field opening; then the field
   itself.

## The two repairs

**A press belongs to the context it was made in.** `InputSession::set_context`
now drains the buffer's undelivered edges when the context actually changes
and reports them as one `InputFault::Suppressed { reason: Context }` naming
the discarded edges. This is the same rule F22-C wrote for pause, focus loss
and teardown ("a press made before the pause is discarded at the pause rather
than resurrected by the resume"), applied to the one path that had no
handover. What it deliberately does *not* do:

- it does not `suppress` the device holds, so a closed screen never invents a
  re-arm requirement (F22-C's documented decision);
- it does not neutralize the axes — a session always runs a frame before its
  boundaries, and that frame is neutral in a non-flight context (the adapters
  write the neutral, F22-B's rule), so the buffer cannot carry a deflection
  into the screen's ticks;
- it does not pause, drop or re-stamp anything else: the world keeps ticking
  (`ticks_ran == 4` in the AC04 window) and the reports are *dropped*, not
  refused, so they are not faults.

**The default map reaches everything it declares.** Three bindings were added
to `ActionMap::designed_default`, each documented at its site: `Digit3` →
`Eject` (the number row, away from the flight cluster and the fire keys, so a
mis-press cannot cost the aircraft), `ArrowLeft` → `TargetPrev` and
`ArrowRight` → `TargetNext` (left is the previous target, right the next; the
same keys navigate when a screen owns the devices, which puts the
two-context case the action map documents into the map the game itself uses).
The map still validates itself at construction, and no existing test pins
those sources as unbound (`Key::ArrowUp` and `Key::Digit2`, which F22-A does
pin, are untouched).

## Designed vocabulary, not original data

Every binding, reading, device identity, tick and threshold in this stage is
newly authored project design. The following are **unknown** and are not
guessed here:

- **Which commands the original 2000 PC game exposes, how it labels them and
  which keys, mouse controls, gamepad or HOTAS controls it binds them to.**
  F22-D measured coverage of *this project's declared* vocabulary only. No
  original file was read for this stage (capability: ordinary build/test), so
  "original command coverage" in the stage's title is discharged as: coverage
  of the declared commands end to end, plus an explicit record that the
  original's own set is unmeasured. Resolving task: **#411 `F22-H`** (measure
  the original's control vocabulary and bindings from the installation, with
  source spans, installation fingerprint and a CLI-EVIDENCE report).
- **Whether the original suppresses anything at all while text is entered**,
  and what it does with a press made just before a screen opened. Still
  unknown for the same reason F22-C recorded it; F22-D changed this project's
  rule, not that question.
- **Runtime behavior of the original's controls** (which key fires in the
  running game, what its text entry does to the flight bindings) is not
  observable from files: it needs an owner-supplied original run
  (**#358 `REF-OWNER-FIRST-CAPTURE`**, blocked on the owner).
- **The mouse wheel cannot be reported.** `MouseAxis::Wheel` and
  `AxisChannel::Mouse(MouseAxis::Wheel)` are declared and calibratable, but
  `DeviceEvent::MouseFrame` carries only `motion_x`/`motion_y`, so no report
  can deliver a wheel reading to the calibration pass. The wheel is unbound in
  the designed map — F22-B pins that, and this stage did not weaken it — so
  no declared command is unreachable because of it, but a declared channel of
  a declared family has no producer. Resolving task: **#412 `F22-I`**.
  **Resolved by F22-I** (`#412`): `DeviceEvent::MouseFrame` now carries a
  `wheel` reading and `calibrated_readings` calibrates it like any other
  relative channel, so the statement above describes this stage's tree and no
  longer the repository's; see
  `docs/findings/2026-10-02-f22-i-mouse-wheel-device-reading.md`. No runtime
  path reports a wheel yet (no Bevy input producer exists), which is F22-F's
  work, and the wheel is still unbound by design.

## Observations recorded rather than hidden

- **A hold is keyed by `(device, action)`, not by source.** Two sources of one
  device bound to the same action collapse into one hold, so two such keys
  held in the same report produce one edge: `T` and `ArrowRight` are both
  `TargetNext`, which is why the AC04 trace below has 17 edges where 18
  individual key presses would be. For a level-triggered report this is the
  right shape (the action, not the key, is what the consumer executes) and it
  is visible in the recorded trace instead of being smoothed over. The
  coverage assertions are set-based for exactly this reason.
- **A still-held trigger does not fire twice across a text field at the
  session level.** The session drops device reports while text entry owns the
  devices, so the hold established before the field opened survives it and the
  press is not repeated when the field closes. F22-B's collector-level test
  pins the opposite for a collector that is fed reports directly (there the
  context gate releases the hold, so the still-held trigger reads as a new
  press). The session is the production path, both tests pass, and neither
  behavior violates non-negotiable behavior 5; the difference is recorded here
  so the next reader does not assume the two layers are interchangeable.
- **The AC04 coverage assertion is self-consistent with the map.** Its
  "delivers every bound edge command" comparison computes the expectation from
  the same map it drives, so probe row 2 does not fail it; the two dedicated
  coverage tests are what catch a lost binding. That is deliberate — AC04's
  job is the text-entry contrast — but it means AC04 alone is not a coverage
  test.

## Tests

Task-test prefix `accept_f22_d_`. The selection
`cargo test --workspace --locked -- accept_f22_d_ --include-ignored`
discovers and runs **7** tests, all passing (exit 0):

- `cs_types::input` (1): every declared flight command and UI action resolves
  from the designed map in its own context, is gated in text entry and
  cinematics, never resolves to the wrong kind of action in a menu, and every
  declared device family has a binding whose class is a declared class.
- `cs_sim::control` (1): the consumer buffers every declared command the way
  its kind demands (axis held across boundaries, edge delivered once, edge
  never moves an axis).
- `cs_app::input::session` (1): switching context discards the press queued
  for the old one, reports it once, delivers nothing at the boundary inside
  the screen, and does not resurrect it when flight returns — for both
  `UiNavigation` and `TextEntry`.
- `crates/cs_app/tests/accept_f22_d_device_families_and_command_coverage.rs`
  (4): the four families end to end with exact expected axis values; all 18
  flight commands with their consumer observation (including the throttle
  positions); all 7 UI actions as screen-only requests with the text-entry
  failure case; and AC04 (see the next section).

No test needs the original installation, so none is `#[ignore]`d.

## AC04 in detail, and the evidence it produces

`accept_f22_d_open_text_entry_and_confirm_flight_commands_are_not_emitted`:

1. *Control case.* One full report per family (every flight-bound source of
   that family) delivers the whole bound edge set — all 14 edge commands —
   and drives `Pitch`, `Roll`, `Yaw` and `Throttle` non-neutral, with the
   bound throttle settings moving `ThrottleSteps` to `FULL`. Every family
   contributes: the keyboard's digital axes and 15 keys, the mouse's button
   and relative motion, the gamepad's sticks and trigger, the stick's indexed
   axes and buttons.
2. *The queued press.* A frame with `ticks == 0` presses `Space`:
   `pending_edges() == 1`, nothing delivered yet.
3. *Text entry.* The field opens; the queued press is discarded and reported
   (`InputFault::Suppressed { reason: Context }`, naming `fire_primary`). Then
   the same four reports run for four ticks: `delivered` is empty,
   `ui_requests == 0`, `pending_edges == 0`, all four axes exactly `0.0`,
   the throttle unchanged, `ticks_ran == 4` (the world keeps ticking),
   `dropped_events == 4` (every report dropped, none refused), the frame
   inert (`suppressed` empty, `suppress_reason == None`) and fault-free.
4. *Loss while typing.* `DeviceEvent::Removed` for the stick is still applied
   and reported: one `DeviceLoss`, because the device set is maintained while
   the readings are not read.
5. *Restore and evidence.* Back in flight the leftover holds are released, the
   same reports deliver the same command set again, and the recorded stream
   is read back: every record inside the field's tick window carries **no**
   edge and only neutral axis samples, and the stream's whole edge list is
   exactly what the restored path delivered.

**Actual input fingerprint and consumer trace** (the sheet's evidence
requirement), from that recorded window — reproduce with
`cargo test --workspace --locked --test accept_f22_d_device_families_and_command_coverage accept_f22_d_open_text_entry -- --nocapture`:

- 7 recorded ticks, fingerprint **`0xec85a638d974b020`** (FNV-1a; a
  change-detection digest, not a cryptographic one — see F22-C's note).
- Consumer trace, in delivery order (17 edges, 14 distinct commands):
  `throttle_step_up, throttle_step_down, throttle_idle, throttle_full,
  fire_primary, fire_secondary, cycle_weapon, drop_ordnance, countermeasure,
  target_next, target_prev, toggle_gear, flap_step, eject,` then the restore
  frame's `fire_primary, fire_primary, fire_secondary` (the stick had been
  removed by then, so the keyboard, mouse and gamepad carry it).
- The first 14 entries are exactly `FlightCommand::ALL` minus the four
  continuous commands — every declared edge command, once, from a real device
  report.

## Mutation probe (test sensitivity)

Each mutation was applied to the branch, the `accept_f22_d_` selection was
run with `--no-fail-fast`, and the mutation was reverted
(`git status --short` clean after each). Every row that changes behavior is
caught.

| Mutation | Caught by |
| --- | --- |
| `InputSession::set_context` no longer discards the buffer's undelivered edges (the submitted repair removed) | `accept_f22_d_open_text_entry_and_confirm_flight_commands_are_not_emitted`, `accept_f22_d_switching_context_discards_the_press_that_was_queued_for_it` (2) |
| `ActionMap::designed_default` drops `Digit3 → Eject` | `accept_f22_d_every_declared_flight_command_reaches_the_consumer`, `accept_f22_d_the_designed_map_reaches_every_declared_command_and_family` (2) |
| `ControlBuffer::begin_tick` returns the edges but re-queues them (the re-fire bug) | `accept_f22_d_open_text_entry_...`, `accept_f22_d_every_declared_device_family_drives_the_simulation`, `accept_f22_d_every_declared_flight_command_reaches_the_consumer`, `accept_f22_d_the_consumer_buffers_every_declared_command` (4) |
| `ActionMap::designed_default` drops `Backspace → Ui(Cancel)` | `accept_f22_d_every_declared_ui_action_reaches_only_the_screen_path`, `accept_f22_d_the_designed_map_reaches_every_declared_command_and_family` (2) |
| `DeviceAdapters::calibrated_readings` ignores the mouse's motion (reports `0.0` for both axes) | `accept_f22_d_every_declared_device_family_drives_the_simulation` (1) |

Two rows are worth reading as limits rather than as good news: the family test
is the only one sensitive to the mouse's own axis path, because the
per-command test drives a command through the *first* source bound to it
(`Yaw`'s first source is `Key::A`, not the mouse) — which is exactly why the
per-family test exists. And the AC04 row does not appear under the map
mutations, for the reason recorded under "Observations".

## Checks

Run locally by the implementer before hand-over (all exit 0):

```
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --locked
cargo test --workspace --locked -- accept_f22_d_ --include-ignored
```

The workspace run reports 130 passing test binaries' results with no failure;
the task selection discovers and runs 7 tests, all passing. The three
mutation-probe-era edits were reverted before these runs and
`git status --short` was empty.

## Review

**Not yet reviewed.** This branch is submitted for review by a different agent
instance; per `AGENTS.md` and the owner's 2026-09-28 directive the reviewer
must record the actual implementer and reviewer identities and whether the
reviewer's context was fresh, and a review by the same agent that implemented
this work is not independent evidence. What the reviewer should look at: the
`set_context` discard (is draining on *any* context change the right scope,
and is a `Suppressed` fault the right report for it), the three new default
bindings (designed choices, documented at their sites), whether the coverage
tests assert production values rather than restating the implementation, and
the unknowns above — especially that nothing in this stage may be read as
measurement of the original game.
