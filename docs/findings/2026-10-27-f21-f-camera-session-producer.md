# F21-F: the render-frame loop that drives `CameraSession` (task #537)

## Decision

`playtest::follow_camera` is the render-frame loop that owns the camera
session. It already was the only system that resolved a camera each rendered
frame (it drove a bare `CameraRig`), it runs in `Update` after the physics
writes the authoritative `Position`/`Rotation`, and the mission window and the
mission smoke both fly through the playtest plugin (`PlaytestCamera`).
`PlaytestCamera` now holds a `CameraSession` instead of a `CameraRig`.

The animation (`animation/capture.rs`) and render (`render/sync.rs`) seams were
checked and rejected as owners: neither runs a per-render-frame loop over the
player's body; they consume what a producer publishes.

## What each frame does

* Publishes the player body's pose as the only `BodyPose`, the viewport aspect
  and the frame's wall time in a `SessionFrameInputs`; `at` is the physics tick
  ledger, no longer a constant `Tick(0)`.
* Moves the camera entity to `SessionFrame::view.pose`, so the session frame is
  the one place the entity's transform comes from. `PlaytestCamera::frame`
  stays the rig's own frame (`SessionFrame::rig`) for existing readers.
* Keeps `session_frame`, `events` (every `CameraEvent`, in order) and
  `last_error`. A refused frame is recorded in `last_error` and retried on the
  next frame rather than being dropped silently.
* A reset (new aircraft, new pilot) calls `CameraSession::reset`, keeps what it
  reports in `events`, and reselects the chase rig.

## Not done / not claimed

* Look input and the spyglass readout are still `None`: the mission window's
  look, spyglass and cockpit/external views (`mission_session::look_view`,
  `spyglass`, `cockpit_camera`, `external_view`) drive their own Bevy cameras
  and do not go through the session. Moving them is camera policy work and
  is not part of this task; nothing yet requests a script camera or capture
  through the live session (only `run::run_capture` does).
* The actor id is still the playtest's local one (session 1, serial =
  resets + 1); the mission host's `ActorId` for the player is not yet
  published to the camera.
* No original camera behavior is verified.
