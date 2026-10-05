# F40-B: decoded playback and authored camera timelines

Task #165. Code: `cs_app::cinematics::playback` and `cs_app::cinematics::timeline`.
Tests are the `accept_f40_b_*` unit tests in those modules (the `tests/` owner
path holds no file: tests need crate-private fixtures, as in F40-A).

All behavior is **designed**, synthetic and not original-verified (at most
*checked*). No original media or camera track was read.

## One observable failure

Pause a playing video, keep feeding audio callbacks, resume: video must hold
while paused and stay within the drift tolerance after resuming. A clock that
keeps counting while paused makes
`accept_f40_b_pause_and_resume_keep_video_within_the_drift_tolerance` fail.

## Designed semantics

- Audio is the master clock (`MediaClock`, counted in played samples). Pause
  stops the clock, not the decoder; video asks the clock.
- `VideoPlayback::frame_due` shows the newest due frame, dropping older due
  frames to catch up; `drift` reports signed drift against a caller-supplied
  tolerance.
- Decoding is behind `FrameSource`; a `DecodeError` is fed to the F40-A player
  as `MediaFailure::DecodeFailed`, never a completion.
- `CameraTimeline::sample` is a pure function of the cinematic tick, linear
  between keyframes, clamped outside.

## Unknowns (not guessed)

- The original video container/codec and an approved decoder dependency or
  private transcoding cache: no real `FrameSource` exists yet. Needs F40-D
  (`retail`) to identify the format first.
- The approved drift tolerance: tests use one frame period (40 ms at 25 fps) as
  a design value only.
- The original camera track format, easing and units; keyframe interpolation
  is a designed linear choice. Orientation is not modelled yet.
- Wiring to the audio device and the renderer is F40-C.
