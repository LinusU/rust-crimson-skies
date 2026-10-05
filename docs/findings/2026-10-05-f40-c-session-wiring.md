# F40-C: skip, pause, state transitions and failure recovery

Task #166. Code: `cs_app::cinematics::session` (`CinematicSession`,
`SemanticSink`, `MissionHandback`). Tests are the `accept_f40_c_*` unit tests in
that module (they need crate-private-style fixtures, as in F40-A/B).

All behavior is **designed**, synthetic and not original-verified (at most
*checked*). No original media, skip rule or pause rule was read.

## One observable failure

Swap aircraft mid-scene, then skip: control must return to the aircraft flown
*now*. Binding the hand-back to the aircraft at scene start (or dropping
`player_changed_aircraft`) makes
`accept_f40_c_aircraft_change_returns_control_to_the_current_actor` and the skip
test fail.

## Wiring

- Producers: audio-device sample counts (`step`), the skip press (`skip`), the
  decoder (`DecodeError` becomes `MediaFailure::DecodeFailed`).
- Consumer: a `SemanticSink` receives `take_control`, each objective event and
  `return_control`. Each action id reaches it once, also across `retry`.
- Pause stops the media clock; `step` is a no-op while paused.
  `simulation_paused` follows the scene's declared `PausePolicy` until the
  scene ends.
- Failure: `ApplyRemainingSemantics` applies the rest and returns control;
  `Block` keeps control held until `retry` (replays from the start, fresh media,
  no duplicate deliveries) or `abandon` (returns control, no further semantics).
  Neither is ever a completion. `cancel` returns control without semantics.
- Teardown: the video playback is dropped when the scene reaches any terminal
  state.

## Unknowns (not guessed)

- Whether the original resumes or restarts a scene after a media failure, and
  whether it lets the player skip every scene: `retry`-from-start and the
  per-scene `skippable` flag are designed choices.
- The mapping of audio samples to cinematic ticks uses a caller-supplied tick
  rate; the original rate is unmeasured.
- The real consumer (mission/sim control handoff and the audio device) is not
  yet connected: `MissionHandback` is the in-crate consumer. Resolved by F40-D
  (`retail`, `gpu`, `audio`) and the mission integration tasks.
