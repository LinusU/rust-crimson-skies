# F22-C: Focus, UI, replay and control ownership

Date: 2026-09-30. Task: F22-C "Wire focus, UI, replay and control ownership"
(`specs/F22-input-bindings-devices-and-control-ownership.md`, section
`### F22-C`). Shared contract: `docs/contracts/UI-NETWORK.md`. Capabilities
used: ordinary build/test only (no `CS_GAME_DIR` read, no evidence report
required, no `gpu`/`audio`/`human_play`/`network_real` needed).

## Files and the one observable failure (listed before editing)

- `crates/cs_app/src/input/session.rs` (new): the loop that owns the producer
  and the consumer — `InputSession`, `FrameInput`/`FrameOutcome`, `SessionMode`,
  `FocusOutcome`, `PauseReason`/`PauseDecision`/`PauseOutcome`,
  `ControlHandover`/`HandoverReason`, `InputFault`/`SuppressReason`,
  `UiRequest`, `FlightContent`, `SessionError`, `ReplayCursor`,
  `ReplayWindow`, `ReplayReport`, `ReplayError` and `CommandReplay`.
- `crates/cs_app/src/input/mod.rs`: the `session` module declaration and the
  re-exports.
- `crates/cs_app/src/input/devices.rs`: `DeviceAdapters::suppress` and
  `SuppressedHolds` (the release a focus loss, a pause and a handover use when
  nothing was removed), and `DeviceAdapters::neutralize_unreported` (the shared
  "which axes need a neutral sample" rule, so `finish_frame` and a caller with
  no frame to close cannot disagree).
- `crates/cs_types/src/input.rs`: `CommandStream`, `StreamError`,
  `InputFrame::take_ui_actions` and `InputFrame::is_inert`.
- `crates/cs_sim/src/control.rs`: `ControlBuffer::drain_pending` and
  `ControlBuffer::neutralize_axes` — the teardown half of the consumer.
- `crates/cs_app/tests/accept_f22_c_focus_ui_replay_ownership.rs` (new): the
  cross-crate acceptance scenarios.
- `crates/cs_app/src/lib.rs` (wiring only): module documentation.

**One observable failure:** the render frame's wall time is cut into render
frames differently on every machine. Replay the *same* quantized command stream
at 30, 60 and 144 FPS and the consumer must execute the same commands in the
same order, reach the same throttle and end in the same world state; anything
that applies a throttle step once per render frame, delivers an edge per frame
instead of per tick, or samples a held axis at frame resolution, disagrees
between the three.
`accept_f22_c_replay_the_same_quantized_command_stream_at_different_display_fps`
fails under every one of those.

## What the stage makes structural

1. **One session owns both ends of the path.** `InputSession` holds the
   `InputCollector` *and* the `ControlBuffer`, `ControlGate` and `ThrottleSteps`.
   A render frame is one `pump_frame` call, so the producer and the consumer
   cannot be wired differently by two callers, and the ticks come from a real
   `cs_sim::time::SimClock` rather than from a hand-counted loop.
2. **One owner of the context.** `InputSession::set_context` writes the
   collector's bindings *and* the control gate in the same call, and the
   collector and the gate read the context from the same place. A menu, a text
   field, a pause screen and the simulation therefore cannot disagree about who
   owns the devices — the disagreement that would let a text field fire the
   guns (non-negotiable behavior 5). A regression that writes only one of the
   two is caught by
   `accept_f22_c_ui_actions_are_requests_and_never_reach_the_control_buffer`,
   `accept_f22_c_focus_loss_pauses_single_player_and_only_neutralizes_multiplayer`
   and the context-agreement loop in
   `accept_f22_c_a_text_field_and_a_pause_screen_close_the_whole_input_path`.
3. **Focus loss pauses where it may and neutralizes where it may not**
   (non-negotiable behavior 4). `set_focus(false)` runs one handover — device
   holds released, undelivered buffer edges discarded, held axes set to exactly
   neutral — switches the context to `Cinematic`, and then asks `pause_local`,
   which is the one place the mode decides: `SessionMode::SinglePlayer` pauses,
   `SessionMode::Multiplayer` returns
   `PauseDecision::NoLocalAuthority` and the world's ticks keep committing. The
   input path has no network channel at all, so "do not pause the server" is
   structural rather than a promise. Focus gain restores the context the session
   had before the loss and **does not resume**: only `resume` does, so a window
   returning from the background never restarts a mission by itself.
4. **A UI action is a request, never a command.** Every `Action::Ui` leaves the
   frame at the producer/consumer boundary (`InputFrame::take_ui_actions`) and
   becomes a `UiRequest { action, tick, context }`. It never enters the control
   buffer, and the input session never performs it: the pause is the screen
   path's transaction (`InputSession::pause`), not the input path's. The
   acceptance test's `WorldTrace` counts any UI action that reaches the
   consumer, so the invariant is checked at the consumer and not only at the
   producer.
5. **A handover is a release, and it is one code path.** Focus loss, pause,
   pause-resume, a control transfer, a teardown and a restart all call the same
   private `handover`, so no path can release only the holds or only the queued
   edges. A handover reports what it released (`ControlHandover`), and a press
   that is dropped says so. `ControlBuffer::drain_pending` and
   `neutralize_axes` are the consumer's half; a second handover reports nothing,
   so a teardown that runs twice cannot deadlock or claim it released something.
6. **The gate is the single owner, and a loss of it is visible.** A live device
   report is only read while the session is actually the reader
   (`focused && gate.accepts_local_input()`); the device *set* is maintained
   either way, so a stick unplugged in the background is still reported lost.
   Content that arrives by another route while the seat does not own the actor —
   a replay, or a caller that resolved a source itself — is refused and reported
   as `InputFault::NotAuthoritative` instead of being dropped. A transfer can
   never steal an actor another authority holds, and that refusal is decided
   before anything is released, so it is cost-free.
7. **A paused session runs no input boundary** (`cs_sim::time::PausePolicy::
   Freeze`). Ticks asked for while paused are refused, counted in
   `ticks_refused` and reported as `InputFault::TicksWhilePaused` — never
   executed into a frozen world. UI actions keep flowing, because the pause
   screen is driven by them, and a press made before the pause is discarded at
   the pause rather than resurrected by the resume. `CommandReplay` freezes its
   own clock with the session, so a paused replay does not consume the recorded
   window it could not execute: the records are still ahead when it resumes.
8. **Replay is the recorded stream, and the record is made at the input
   boundary** (AC03). `start_recording` appends one `InputFrame` per boundary,
   holding the edges that boundary delivered and the quantized axis samples the
   aircraft was driven with; the ticks need not be contiguous, so a pause
   appears as an absence. `CommandStream` only moves forward, refuses a repeated
   tick, and has a stable FNV-1a `fingerprint` for an evidence report.
   `CommandReplay` owns a real `SimClock`, converts each render frame's wall
   time into whole fixed ticks, and feeds the stream back through the **same**
   `pump_frame` at any display rate.
9. **A replay window is a live-shaped frame, and its axes are the freshest
   sample.** The window `[tick, tick + ticks)` is stamped at its own first tick
   and every recorded tick in it is merged, so the display rate moves a
   command's *latency* (up to one frame's worth of ticks early) and never the
   command itself. A record the session has already passed is counted as
   `orphaned`, never delivered late, and a window with no record is an empty
   frame.
10. **Every fault is reported and no frame is lost with it.** A refused device
    report is an `InputFault::Device` and the frame keeps the input every other
    device delivered; a suppressed or non-authoritative frame is reported with
    its content; a refused stream record, a refused axis and a refused buffer
    are refused by name. The frame's faults appear in `FrameOutcome` *and* in
    the `take_faults` queue, so a caller that only drains the queue cannot miss
    one. Content reported as suppressed is **not** applied: a frame that arrives
    by another route into a closed path (an unfocused window, a pause, an actor
    this seat does not own) is reported and hands the simulation nothing, so
    "suppressed" can never mean "applied and ignored". The one thing a closed
    path still applies is an **inert** frame — empty, or restating an axis as
    exactly neutral — which is what stops a stale deflection without inventing a
    second neutralization rule.
11. **The manual source path is a real producer path.**
    `InputSession::observe_source` resolves one physical source through the
    session's gate and queues an accepted edge in the **consumer's** buffer,
    stamped at the session's current tick, so the next input boundary delivers
    it exactly once and the recorded stream contains it. It is deliberately not
    appended to the collector's frame: `pump_frame` opens every live frame with
    `begin_frame`, which discards whatever was collected before it, so an edge
    parked there would be reported to the caller and then silently dropped by
    the next frame.

## Designed vocabulary, not original data

Every mode, focus policy, pause reason, context transition, handover rule,
stream record, fingerprint and replay rule in this stage is newly authored
project design. The following are **unknown** and are not guessed here:

- what the original 2000 PC game does when its window loses focus: pause, show a
  menu, keep simulating, or nothing at all;
- whether the original pauses on focus loss in a *networked* session, and
  whether it has a networked input-ownership concept at all;
- whether the original shows a pause menu at all, and which screen it is;
- whether the original's flight bindings are reachable from a menu, and
  whether text entry in the original suppresses flight commands the way this
  stage's context gate does;
- whether the original recorded, replayed or verified input at all, and whether
  it had any notion of a per-tick command stream;
- what the original does with a device that goes quiet mid-mission, and what it
  does with a trigger that is still held when the window comes back;
- whether the original's mouse flight, throttle steps or trigger handling
  survive a pause or a focus change;
- where and in what format the original stored its key bindings and axis
  calibration (F22-B deferred the writer; it is still unwritten — see the tasks
  filed with this stage);
- how the original's analog axes are sampled over time — whether a frame's
  value is held for every substep it covers, which is the assumption F16-C and
  this stage's replay both rest on.

Affected content: the whole F22 input surface. Resolving tasks already in the
queue: **F22-D** (all declared device families and original command coverage).
The designed `SessionMode` split, the `Cinematic`-on-focus-loss context, the
`DESIGNED_STEP` of F22-B and `ActionMap::designed_default` are project defaults,
not measurements.

## Known limits of this stage (carried forward, not hidden)

- **The Bevy platform adapter is not written.** This stage wired the *policy*
  and the loop; nothing yet turns Bevy's `ButtonInput`, `Gamepad`, the raw mouse
  motion, `Window`'s focus events and `Time` into `DeviceEvent`s and one
  `pump_frame` call. `crates/cs_app/src/input` is deliberately ECS-free, so that
  seam is a separate, small piece of work and is filed as a task. Until it
  exists, "the real platform sources" are simulated by a caller.
- **Bindings and calibration still do not persist.** F22-B handed the atomic
  writer to this stage and it is **not** delivered here: the on-disk format and
  location would be a guess about the original's configuration layout, and the
  in-session store already satisfies everything this stage tests. The
  `CalibrationStore::unstable_devices` refusal a writer must honour is recorded
  and filed.
- **There is no in-flight way to pause from the flight context.** The designed
  map binds `Escape` to `UiAction::Pause`, and a `BindingTarget::Ui` belongs to
  `UiNavigation` by construction, so the pause path is only reachable from a
  menu. Giving flight context a pause binding needs a schema change (a
  flight-context action that requests a domain transaction) and is filed as a
  task rather than smuggled in as a special case.
- **A coarse display rate quantizes an *analog* signal in time.** AC03's
  invariant is over the sampled stream: with a held axis, 30/60/144 FPS agree
  exactly on the commands, the throttle, the world trace and the final state. A
  *changing* analog value is sampled at the display rate, so a 30 FPS run
  integrates a coarser signal than a 144 FPS run of the same wall time. That is
  a property of sampling, not a defect, and it is why the cross-rate world
  comparison uses a held axis. What the tests do pin for a coarse rate is the
  exact merge rule (`accept_f22_c_a_replay_window_merges_to_the_freshest_sample`)
  and the reference-rate round trip (the fingerprint).
- **A session does not invent a re-arm requirement.** After a focus gain or a
  resume, control returns from the platform's next report, because a device
  report is the device's whole state; a trigger that is still held reads as
  pressed. What the stage guarantees is the other half: nothing fires while the
  path is closed, and nothing queued before the closure is delivered after it.
- **The `NotAuthoritative` fault repeats every frame** while a caller keeps
  feeding a session that does not own the actor. That is deliberate — it is a
  wiring error and it is visible — but a caller that hands the actor to another
  authority is expected to stop feeding that session.
- **A neutral a finished frame writes is written once.**
  `DeviceAdapters::neutralize_unreported` re-bases what the last frame drove to
  the axes a frame stated *before* the neutrals are added, so a released axis is
  stated exactly neutral once and not restated on every later frame. Restating
  it would be noise in the producer's frame and a phantom axis in every
  suppression report. The device-level record (`driven_axes`) is separate: only
  a fresh report from that device corrects it, so a suppression between the two
  still names the axis the device last drove.
- **A paused replay runs behind the wall clock.** `CommandReplay::frame` does
  not advance its clock while the session is paused, because the world's time is
  frozen while it is paused and a window read now could not be executed. The
  replay resumes from the same stream position after the resume, which is what
  keeps `is_complete()` honest.
- **A refused window still counts as read.** `ReplayReport::delivered` and
  `ReplayWindow::matched` count the stream being consumed, not the commands
  being executed: a session that refuses a frame names every command it refused
  in that frame's `FrameOutcome`, which is where a caller reads it.
- **`CommandStream::fingerprint` is FNV-1a**, a change-detection fingerprint,
  not a cryptographic digest. An evidence report that needs a cryptographic
  commitment must hash the stream itself.
- **A paused session's held axes still track the live device state** in the
  buffer (no boundary runs, so nothing is executed) and are re-stated neutral by
  the resume's handover. The buffer therefore never carries a paused deflection
  into the resumed world.

Follow-up tasks filed with this stage, each scoped so an agent can pick it up
with a fresh context:

- **#404 `F22-E`** — persist bindings and calibration atomically, refusing
  index-only device identities (the writer F22-B deferred and non-negotiable
  behavior 5 requires).
- **#405 `F22-F`** — wire the Bevy platform sources (`ButtonInput`, `Gamepad`,
  mouse motion, window focus, `Time`) to `InputSession::pump_frame`, the missing
  producer for `DeviceEvent`.
- **#406 `F22-G`** — let a flight context request a domain transaction, so the
  pause path is reachable from flight without a special case in the session.


## Tests

Task-test prefix `accept_f22_c_`. The selection
`cargo test --workspace --locked -- accept_f22_c_ --include-ignored` discovers
and runs **19** tests, all passing:

- `cs_types::input` (1): the stream records ascending quantized ticks, refuses a
  repeated or stale tick without half-applying it, looks a record up by tick,
  fingerprints stably and changes when a tick or a command changes, two
  readings inside one quantization step are one command, and the frame split and
  `is_inert` rule.
- `cs_sim::control` (1): `drain_pending` discards in order and never delivers
  later, `neutralize_axes` states and names the axes it changed and reports
  nothing the second time, and the buffer still works afterwards.
- `cs_app::input::devices` (2): `suppress` releases the holds and the driven
  axes without a `DeviceLoss`, leaves the device set, the calibration and the
  report counter alone, neutralizes nothing twice, and lets a fresh report
  re-establish control; and a neutral sample is written once and not restated.
- `cs_app::input::session` (10): focus loss pausing single-player and only
  neutralizing multiplayer, including the ticks-while-paused refusal and the
  device removed while unfocused; UI actions as requests with the input session
  performing nothing; a control handover releasing holds, queued edges and axes,
  a server-owned actor executing nothing, a replay refused at it, and a
  cost-free refused transfer; the manual source path queuing its edge for the
  next boundary; teardown stopping the loop and a restart arming a new session; a
  refused device report reported and keeping other input; a pause refusing ticks
  while keeping the pause screen alive; a replay window merging to the freshest
  sample and counting orphans; and a recorded stream replaying at 30/60/144 FPS.
- `crates/cs_app/tests/accept_f22_c_focus_ui_replay_ownership.rs` (6): AC03 end
  to end (live 144 FPS recording replayed at 30/60/144, identical world trace,
  throttle, axis state and tick count, fingerprint round trip at the reference
  rate, bounded lateness, and a frame split that really differs); focus in both
  modes with a frozen clock and a device removed while unfocused; text entry
  and a pause screen closing the whole path with the context-agreement loop; a
  handover, teardown and restart across a refused transfer; the replay's own
  refusals for a torn-down session, a drifted clock and a multiplayer clock
  policy; and a recorded stream fed into a closed path (an unfocused window, and
  a pause) reported, never executed, and re-read after the resume.

No test needs the original installation, so none is `#[ignore]`d.

## Mutation probe (test sensitivity)

Each mutation below was applied to the branch, the `accept_f22_c_` selection
was run, and the mutation was reverted. Every one that changes behavior is
caught.

| Mutation | Caught by |
| --- | --- |
| a handover no longer discards the buffer's undelivered edges | `accept_f22_c_a_pause_refuses_ticks_but_keeps_the_pause_screen_alive`, `..._teardown_stops_the_loop_...`, `..._control_handover_releases_holds_...`, `accept_f22_c_ownership_handover_and_teardown_...` (4) |
| `set_context` writes the collector's context but not the gate's | `accept_f22_c_ui_actions_are_requests_...`, `..._focus_loss_pauses_single_player_...`, `accept_f22_c_focus_loss_pauses_single_player_...`, `accept_f22_c_a_text_field_and_a_pause_screen_...` (4) |
| the UI/flight split is not applied, so a UI action enters the control buffer | `accept_f22_c_ui_actions_are_requests_...`, `..._a_pause_refuses_ticks_...`, `accept_f22_c_a_text_field_and_a_pause_screen_...` (3) |
| a replay window takes the *first* axis sample of the window instead of the freshest | `accept_f22_c_a_replay_window_merges_to_the_freshest_sample` |
| `pause_local` ignores the mode, so a networked session may pause locally | `..._focus_loss_pauses_single_player_...`, `accept_f22_c_focus_loss_pauses_single_player_...` (2) |
| a focus gain resumes the pause | `..._focus_loss_pauses_single_player_...`, `accept_f22_c_focus_loss_pauses_single_player_...` (2) |
| a teardown keeps the buffer's queued edges | `..._a_pause_refuses_ticks_...`, `..._teardown_stops_the_loop_...`, `..._control_handover_releases_holds_...`, `accept_f22_c_ownership_handover_and_teardown_...` (4) |
| the command stream is recorded per render frame instead of per tick | `..._teardown_stops_the_loop_...`, `accept_f22_c_ownership_handover_and_teardown_...`, `accept_f22_c_replay_the_same_quantized_command_stream_...` (3) |
| the replay steps one tick per frame instead of the clock's committed ticks | `accept_f22_c_replay_the_same_quantized_command_stream_...` |
| `ControlBuffer::neutralize_axes` neutralizes nothing | `..._teardown_stops_the_loop_...`, `..._focus_loss_pauses_single_player_...`, `..._control_handover_releases_holds_...`, and `accept_f22_c_draining_and_neutralizing_end_the_local_hold` (4) |

### The review's own probes (2026-09-30)

Each was applied to the branch, the `accept_f22_c_` selection was run, and the
mutation was reverted. Each one is a real defect the review found and fixed,
not a hypothetical: the first three were live holes in the submitted code and
the fourth was a change to F22-B's behavior the refactor did not need.

| Mutation | Caught by |
| --- | --- |
| an unfocused session applies a frame it reported as suppressed (the submitted code) | `accept_f22_c_a_replayed_stream_is_reported_not_executed_while_the_path_is_closed` |
| `CommandReplay::frame` advances its clock while the session is paused (the submitted code), which consumed the window it could not execute and then failed with a `TickDivergence` on the next frame | the same test |
| `observe_source` reports an accepted action without queueing it (the submitted code parked it in the collector's frame, which the next `begin_frame` discarded) | `accept_f22_c_the_manual_source_path_queues_its_edge_for_the_next_boundary` |
| `finish_frame` remembers the neutral it just wrote as a driven axis, so every later frame restates it (the submitted refactor) | `accept_f22_c_a_neutral_sample_is_written_once_and_not_restated` |

### The review's own probes (2026-09-30)

Each was applied to the branch, the `accept_f22_c_` selection was run, and the
mutation was reverted. Each one is a real defect the review found and fixed,
not a hypothetical: the first three were live holes in the submitted code and
the fourth was a change to F22-B's behavior the refactor did not need.

One latent inconsistency was fixed without a discriminating test, because it is
unreachable with the current invariants and therefore cannot be provoked: a
refused `CommandStream` record and a refused axis were queued as faults but
never appeared in the `FrameOutcome`, while the module promises that a frame's
faults are in both places. `record_tick` now takes the frame's fault list.

One probe was a **semantic no-op** and is recorded rather than hidden: replacing
the focus path's `match self.mode` with an unconditional `pause_local` call
changed no behavior at all, because `pause_local` is itself the one place the
mode decides. The redundant `match` was removed rather than left in place, and
the probe that matters — removing the mode check from `pause_local` — is the row
above.

The `first-sample` mutation was initially **not** caught, and that is worth
recording the way F22-B recorded its calibration hole: with a *held* axis (the
script the cross-rate test uses, which is what makes the world comparison valid
across display rates) every sample in a window is identical, so first and last
cannot differ. The merge rule is now pinned directly by
`accept_f22_c_a_replay_window_merges_to_the_freshest_sample`, which drives
`ReplayCursor` with a hand-built stream whose samples differ, and the mutation
fails there.

## Checks

Run locally by the implementer before hand-over and again by the review after
its fixes (all exit 0):

```
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --locked
cargo test --workspace --locked -- accept_f22_c_ --include-ignored
```

`cargo doc -p cs_app --no-deps` reports seven warnings, and the review
re-checked the same seven with and without this stage's changes: they are
pre-existing and none of them is in code this stage added
(`Resolved::Unknown` in `animation`, the private `Self::calibrated_readings`
link in F22-B's `apply`, `LoadState::permits`, three unresolved links in
`world`/`sensor`, and `run.rs`'s redundant link target).

## Review

**Not independent.** The implementer of this stage and the reviewer are the
same agent instance and the same model (`bunny-2`). The review is a second pass
over the same knowledge, not a fresh-context check, and it is **not**
independent review and not original-reference evidence. F22-D remains the stage
that measures anything about the original game.

What the review (2026-09-30) did:

1. Read the whole branch against `specs/F22-...`'s `### F22-C` section, the
   `docs/contracts/UI-NETWORK.md` rules the stage claims, and `AGENTS.md`.
2. Found and fixed four defects, all recorded with a probe under "The review's
   own probes" above: a suppressed frame was still applied while the window was
   unfocused, a paused replay consumed the window it could not execute and then
   failed with a clock divergence, the manual source path reported an accepted
   action it then dropped, and the extracted neutralization helper made every
   later frame restate a neutral it had already written. Three new tests pin
   them (19 `accept_f22_c_` tests in total).
3. Removed `InputCollector::push_action`, which existed only for the manual
   path this review changed, and added `InputFault::Buffer` so a buffer refusal
   on that path is reported rather than turned into a `Some`.
4. Corrected the documentation that had drifted from the code: what
   `ReplayReport::delivered` and `is_complete` count, what
   `neutralize_unreported` shares with `suppress` (they do not share the
   method; `suppress` is the no-frame case and unions the device-level record
   too), and that a recorded neutral is written once.

Judged acceptable and left alone, with the reasons recorded above rather than
hidden: the AC03 invariant is over the sampled stream, so the cross-rate world
comparison uses a held axis and the merge rule has its own test; a focus gain or
a resume does not invent a re-arm requirement, because a device report is the
device's whole state; and `NotAuthoritative` repeats every frame while a caller
keeps feeding a session that does not own the actor, which is a wiring error
that is meant to be visible.

