# F21-C: script cameras, deterministic capture flags and the camera session

Date: 2026-10-03. Task: F21-C "Integrate script cameras and deterministic
capture flags" (`specs/F21-cameras-cockpit-views-and-spyglass.md`, section
`### F21-C`). Shared contract: `docs/contracts/UI-NETWORK.md`. Capabilities used:
ordinary build/test only — no `CS_GAME_DIR` read, no evidence report required, no
GPU.

## Files and the one observable failure (the slice plan)

* `crates/cs_app/src/camera/script.rs` (new): `ScriptCameraRequest`,
  `ScriptedShot`, `ScriptSubject`, `ScriptEndReason`, `ScriptCameraError`.
* `crates/cs_app/src/camera/capture.rs` (new): `CaptureRequest`,
  `CaptureError`, `CaptureTarget`, `CaptureOverride`, `CaptureReport`,
  `PinnedProjection`, `MagnificationPin`, `Narrowing`, `pin`,
  `ProjectionPinError`.
* `crates/cs_app/src/camera/session.rs` (new): `CameraSession`,
  `SessionFrameInputs`, `BodyPose`, `SessionFrame`, `SessionView`,
  `CameraAuthority`, `CameraEvent`, `SessionError`.
* `crates/cs_app/src/camera/rig.rs`: `CameraRig::can_select` (a probe so a
  producer can validate a request before the frame that applies it) and
  `oriented_pose`/`selection` made crate-visible or shared.
* `crates/cs_app/src/camera/mod.rs`, `crates/cs_app/src/lib.rs`: module
  declarations, re-exports and doc paragraphs only. No logic.
* `crates/cs_app/tests/camera/{script,capture,session}.rs` (new), the F21-C
  helpers in `common.rs`, and the module list in `tests/camera/main.rs`: the 28
  `accept_f21_c_*` tests.
* This file.

**One observable failure:** if a session let a scripted camera hold its
binding across an aircraft swap, a capture taken after the swap would draw
through a body that no longer exists — the camera magnifies a wreck and the
capture's report names an actor that is gone. The swap is not a subtle
degradation: `ActorId` is generation-qualified, so a new body is a *different*
binding, and a session that keeps the old one is pointing at a despawned body.
That is AC03's "swap aircraft during a scripted capture and verify camera binds
to the new player body" failing in the only way it can fail. The minimum
scenario is `accept_f21_c_swapping_aircraft_during_a_scripted_capture_rebinds_to_the_new_body`,
with the destroyed-subject, span-end, teardown and retry cases beside it; the
mutations under "Test sensitivity" make them fail on purpose.

## What the stage adds

F21-A declared the records and F21-B implemented four rigs, but nothing *ran*
them: `CameraRig::resolve` takes a `RigInputs` and returns a `RigFrame`, and
there was no owner of either. `CameraSession` is that owner, and it is the whole
of stage C:

| producer | call | what the session guarantees |
| --- | --- | --- |
| a mission script or cinematic | `request_script` | a bounded, half-open span of ticks; at most one script; a mode set with no authored-sequence mode refuses rather than behaving like the chase view; a refusal mutates nothing so the corrected request retries |
| a capture tool or the future `--screenshot` | `apply_capture` | the request names its mission, tick, pose, aspect, view and settings or it is refused; it applies to exactly its tick and is retired there; a view-pinning capture and a running script cannot both claim a frame |
| the input/UI path | `select_rig` | the F21-B rigs, unchanged, with the same refusals |
| the renderer or capture tool | `frame` → `SessionFrame` | one authority per frame, named; the camera to draw in `view`; everything that changed in `events` |

Three rules carry the stage:

1. **One authority per frame, named.** A frame is the player's rig *or* a
   scripted camera, never both, and `CameraAuthority` says which. A scripted
   frame carries `rig: None` and a view with no subject when the shot pins a
   pose, so a consumer cannot read a scripted shot as the player's view.
2. **Refusals are non-mutating and retryable; ends are terminal and reported.** A
   refused request changes nothing. A script that *ends* — its span ran out, its
   body is gone, the producer released it, the session was torn down — reports
   `CameraEvent::ScriptEnded` with the reason on the frame the camera comes
   back, so nothing disappears silently.
3. **A capture is one frame and then it is gone.** A pending capture whose tick
   has passed is retired with `CameraEvent::CaptureMissed` rather than held
   against every later mission.

## The subject is a role, not an instance

`ScriptSubject::Player` names *whoever the player currently flies* and is
re-resolved from the producer's frame every time, so an aircraft swap is a
`SubjectRebound` and a reseat. `ScriptSubject::Actor` names one body, and a body
the producer's frame no longer publishes ends the script with
`ScriptEndReason::SubjectGone` on that same frame.

The one case that had to be decided rather than assumed: **the player role with
no published pose is not a death.** The producer states the player body every
frame and publishes a list of body poses; a body it still names but has not
written a pose for is a producer that has not finished the frame, so the frame
refuses with `SessionError::PlayerPoseMissing` and the next frame retries.
Ending the script there would let one incomplete producer frame cut a cinematic
short — and the same rule holds whether or not a script is driving, so a script
framing the player role behaves like the player's own camera.

## The projection bridge (F21-A review note)

The F21-A review asked that this stage bridge `cs_app::camera::LoweredProjection`
(`f64`) to `cs_app::render::capture::Projection` (`f32`) explicitly instead of
growing a second, competing projection owner. `camera::capture::pin` does that
and nothing else:

* the only inputs are the frame's own lowered policy, the capture's aspect and
  the mode's declared magnification, so a capture cannot pin a frustum no mode
  declared;
* the declared framing rule is applied **first** in `f64`, and the
  magnification is then folded in by dividing the half-angle *tangent* — which
  composes exactly for any factor instead of approximating the angle;
* the `f64 → f32` cast is unavoidable, so both ends are kept
  (`Narrowing::exact`, `Narrowing::pinned`, and `PinnedProjection::max_drift`)
  and a value with no `f32` near it refuses by name
  (`ProjectionPinError::Unrepresentable`) rather than pinning an infinity;
* a capture that pins a view switches the rig for exactly that frame and
  switches it back on the way out, **including on the error paths**, and the
  report names the view it returned to.

## Design decisions that are not measurements

Recorded here because they are choices, not findings:

| decision | what was chosen | why it is a design |
| --- | --- | --- |
| how a magnification reaches a frustum | the capture folds the factor into the **vertical** field of view (`tan(v/2)/f`) and reports it (`CaptureOverride::Magnification`, `MagnificationPin`) | the original's spyglass may have narrowed its field of view, scaled the projection or drawn a separate pass; nothing measured says which, so the engine states which *it* used instead of presenting one as the original's |
| a scripted camera's framing | the declared authored-sequence placement in the body frame, through the *same* `rig::oriented_pose` the rigs use, and the body's own axes | two copies of the `forward_m` sign convention would be two answers to one question, and the chase sign is exactly what F21-B's review caught |
| what a scripted camera may do | hold one exact pose, or frame one subject, over one span of ticks | F40-A/F40-B own authored camera *timelines* (decoding, keyframes, playback clock) in `cs_app::cinematics/`; a seam that could exist before them is the point |
| smoothing under a capture | a pinned pose is held exactly (`Smoothing::Reseated`) and the report says `Smoothing { bypassed: true }`; the rig's own smoother is untouched | a reproducible frame must not depend on the wall time between frames; the live camera carries on from where it was when the capture retires |
| the response rate of a scripted camera | the player's rig's own rate, through the same `PoseSmoother` law | one designed number, not two |
| the request identity of an authored camera | a `camera_track`-kind `ContentId` | `cs_content::cinematics` addresses an in-engine camera that way, and `UI-NETWORK` requires stable typed ids: one id, one record |

## Test sensitivity

Nine mutations, applied and reverted, all against production code, run as
`cargo test -p cs_app --test camera -- accept_f21_c_` (28 tests):

1. `CameraSession::scripted_view` stops emitting `SubjectRebound` → **1 fails**:
   `accept_f21_c_swapping_aircraft_during_a_scripted_capture_rebinds_to_the_new_body`.
2. the same function stops reseating on a rebound → **1 fails**: the same test
   (the camera would drag across the world instead of jumping to the new body).
3. the `SubjectGone` branch no longer clears the script → **1 fails**:
   `accept_f21_c_a_script_naming_the_destroyed_body_hands_the_camera_back_in_the_same_frame`.
4. `camera::capture::pin` derives the declared field of view from somewhere
   other than the frame's own policy (`+ 1e-4`) → **1 fails**:
   `accept_f21_c_the_pinned_capture_projection_is_derived_from_the_lowered_policy`.
5. a capture's pinned pose is smoothed like any other frame instead of being
   held → **2 fail**: the tick test and the AC03 scenario.
6. a capture is retired a frame late → **1 fails**:
   `accept_f21_c_a_capture_applies_to_exactly_its_tick_and_a_missed_one_is_retired`.
7. `apply_capture` accepts a view-pinning capture over a running script → **1
   fails**:
   `accept_f21_c_a_capture_that_pins_a_view_and_a_script_cannot_both_claim_a_frame`.
8. `reset` keeps the pending capture for the next session generation → **1
   fails**: `accept_f21_c_a_session_reset_ends_the_script_and_drops_the_pending_capture`.

(The first pass of mutation 6 did not fail; the frame after a taken capture now
asserts that *no event* was reported, which is what catches it, and it fails on
the re-run.)

All were reverted; the tree is back to the implementation under review.

## Commands run (exit codes)

```text
cargo fmt --all -- --check                                          → 0
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings → 0
cargo test --workspace --locked                                     → 0
cargo test --workspace --locked -- accept_f21_c_ --include-ignored    → 0 (28 tests, all passed)
```

The task selection discovers exactly the 28 `accept_f21_c_*` tests in
`crates/cs_app/tests/camera/` (`script.rs` 7, `capture.rs` 6, `session.rs` 15).
None is `#[ignore]`d: nothing here needs `CS_GAME_DIR`. The F21-A and F21-B
tests in the same binary (15 and 26) still pass.

## Designed vs measured: the unknowns this stage met

Everything in this stage is **designed** engine contract. The following are
`unknown`, recorded rather than guessed; none of them blocks F21-C, and each is
named with the stage that resolves it:

| unknown | evidence | resolves in |
| --- | --- | --- |
| Whether an original authored camera track exists, what ids it uses and what a scripted camera looks like in it | no original bytes were read; `ContentKind::CameraTrack` and `cs_content::cinematics`' `InEngine` presentation are the only design evidence | F40-A (inventory) and F40-B (playback), then an F21/F40 import stage |
| How the original applies magnification (narrow field of view, projection scale, separate pass) | nothing measured; the capture states that it narrows the vertical field of view | F21-D (`gpu` + `retail`) |
| Whether an original camera sequence ends on a tick, on a body event, or on a skip request | the seam's span and its four `ScriptEndReason` values are design | F40-C (skip/pause/teardown), F21-D |
| Whether an original camera swaps aircraft mid-sequence, and what the camera does | the player role rebinding is design; no original sequence was observed | F21-D; F40's AC03 covers the control handover |
| What the original's screenshot or capture mode accepts (world pose flags, tick flags, deterministic settings) | `cs_app::cli` still refuses every flag outside `--synthetic --headless`, so no original CLI was read or reproduced; `CaptureRequest` is the record a parser would build | a CLI stage (F00-C follow-up, filed below) and F21-D |
| Whether a scripted camera follows a subject with the body's own axes or aims at it | the seam uses the declared placement and the body's axes; aiming at a subject would be a second placement convention | F40-B, F21-D |
| Whether a capture should override the live camera or replace it | the stage overrides for one frame and reports every override; a "replace" policy is a producer decision this seam does not make | F17-C (`retail`, `gpu`) |

### Follow-ups filed, not fixed here

Two gaps were found and are outside this stage's owner paths, so they are tasks
rather than edits:

1. **The `--screenshot` command line does not exist.** `cs_app::cli` rejects
   every flag outside `--synthetic --headless`, so nothing parses the flags a
   capture needs. `camera::CaptureRequest` is the record such a parser would
   build, and the report is what the CLI must print.
2. **No producer drives a `CameraSession` yet.** A mission script, a cinematic
   player or a replay reader has to call `request_script`/`apply_capture`; the
   session is a value that accepts them and nothing in this stage calls it.

## What is not claimed

A code/test pass awards at most **checked**. No original camera data was read,
no original view, scripted-camera or capture behavior was reproduced, no
original binding, offset, look range, smoothing rate or magnification mechanism
was measured, and nothing here is `verified_original`. Synthetic fixtures alone
cannot certify original-data behavior. F21-D is the stage that may compare any
of this against original captures with the `gpu` and `retail` capabilities.
