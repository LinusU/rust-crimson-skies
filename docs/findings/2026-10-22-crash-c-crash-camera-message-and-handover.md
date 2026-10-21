# #1855 PARITY4-CRASH-C: the detached crash camera, `Fatal Crash!` and the key handover

Date: 2026-10-22. Implementer: `claude-1`. Rule source:
`2026-10-22-crash-a-fatal-contact-and-crash-camera.md` (`OWNER-STATIC`, nothing here is
`verified_original`); trigger: `2026-10-22-crash-b-fatal-terrain-contact.md`.

## What is in the build

* `playtest::fatal_contact::CrashSequence` is inserted on the fatal tick by `judge_terrain_contacts`. It carries the
  crash record, the camera point, the placeholder target and a `released` flag.
* **Camera.** `external_view::drive_external_camera` holds the camera at `crash_camera_point`: 45 above the impact
  position (`crash_y`) and 30 along the aircraft's horizontal heading (`crash_horiz`), looking at the wreck every frame.
  It runs after the cockpit camera driver, the pilot-eye views stand down (`step_crash_sequence` forces the chase view)
  and the view and look keys are inert (`original_keys`; the original's guard is `V+0x91d`).
* **HUD.** The text layer (`fit_safe_area`), the gauge cluster (`fit_hud_viewport`) and the target markers
  (`update_contacts`) are held back like the launch cinematic holds them.
* **`Fatal Crash!`** is the installation's `MSG_CRASH` (string id 162) through `MissionMessages`, drawn centred in the
  objective marker's blue by `MissionCrashMessagePlugin`. With no catalog the line shows the key `MSG_CRASH`, as the
  objective line does.
* **Handover.** The first key pressed after the crash sets `released` (`step_crash_sequence`, read right after the input
  system so a key a binding consumes still counts). `MissionHost::settle` returns `None` while a crash sequence is
  unreleased; once released it settles the usual failure (`source=player_destroyed`, exit 1) with the debrief read at
  the **crash** tick (`MissionDebrief::run_ticks == crash.tick`), because the original stops its clock in the removal
  path. A restart or a teardown removes the sequence.

## Limits (unresolved)

* **Message look is designed.** Position (30 % from the top), size (22 px), font and the blue are not measured; only
  the owner's "small, centred, blue, above the plane" is known. The unit of the `5.0f` argument of `0x4588e0` is
  unknown, so the message stays up for the whole sequence instead of a duration.
* **`Press Any Key` (id 163) is not drawn:** its consumer is not located (crash-A finding §3). Any keyboard key
  releases the run; mouse and pad buttons do not. Whether the original accepts them is unknown.
* **Placeholder target `(1, 20, 0)`** is stored on the sequence (`CRASH_PLACEHOLDER_TARGET`) from the owner's
  observation; how the original draws it is untraced, so nothing draws it. The target indicator is hidden with the HUD.
* **Camera clamp.** The original raises the anchor against the terrain with `0x42c580` (roles of its arguments not
  decoded); that clamp is not modelled, so on a slope the point can lie inside the ground. `crash_min_elev` (the value
  the engine reads; the file's `crash_elev` is not read) is unmeasured.
* **Numbers are constants,** `crash_horiz` 30 and `crash_y` 45 as recorded in the crash-A finding. `camparam.zrd` itself
  is still read by nothing.
* **Debrief screen.** The Mission Failed screen is #1655. Until it exists the handover is the settled
  `MissionTerminal` (failure, with the debrief fields) and the `AppExit` it already sends.
* **`__eh_frame` gate (task #336).** On this machine `cs` was already 16,800,428 B (limit 16,777,216 B) before this
  change; the change adds about 16 KB (16,816,716 B), of which 8.6 KB is the message plugin. The gate is
  macOS-only, so Linux CI is unaffected.
