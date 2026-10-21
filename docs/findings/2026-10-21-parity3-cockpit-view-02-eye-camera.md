# PARITY3-COCKPIT-VIEW.02 (#1810): the cockpit eye camera

Date: 2026-10-21. Parent #1738. Code:
`crates/cs_app/src/mission_session/cockpit_camera.rs`, wired in
`mission_session/compose.rs` (`install_cockpit_camera`, the `MissionCockpitEye`
resource). Tests: `crates/cs_app/tests/accept_parity3_cockpit_view_02.rs`,
prefix `accept_parity3_cockpit_view_02_`. Predecessors:
`2026-10-11-t542-cockpit-camera-eye-placement.md`,
`2026-10-21-parity3-cockpit-view-01-original-view-cycle.md`.

Nothing here is `verified_original`; no original run happened.

## What the camera does

While `MissionViewMode` is `Cockpit` or `SecondEye` (original modes 7 and 6,
both of which reach the original's eye transform `0x42d980`), the camera is
placed each frame, in `PostUpdate` before transform propagation, at
`body.pose * stored cockpit_camera translation`, with the body's rotation (so
it looks along body forward with the body's roll). There is no smoothing. The
stored translation is read at composition from the player airframe's own
node array: the one `cockpit_camera` object node under the `player` root
(`player_pfighter`: `2.84e-14, 0.75, -0.2`, T542's table). More than one, or
none, refuses the composition by name.

The exterior airframe is hidden by hiding the player body (every part inherits
visibility; the parts only ever use `Inherited`/`Hidden`). The chase rig keeps
resolving underneath, as the launch camera's does, and is re-seated
(`PlaytestCamera::reseat`) on the frame the view leaves the eye views, so it
snaps to the chase pose instead of swinging. The launch cinematic keeps the
camera while it plays; a stage without a player airframe (synthetic) has no
eye and keeps the chase view in every mode.

## Designed (labelled, not measured)

* **Axis map and unit of the stored triple.** The same reading as the exterior
  meshes: one stored unit is one metre, stored axes unchanged, turned by the
  same `nose_mapping(STORED_AIRCRAFT_NOSE_AXIS)` yaw (the identity). This is
  consistent with the stored `-Z` nose (the eye's `z = -0.2` is slightly
  forward of the body origin, `y = 0.75` above it) but is not measured.
* **Field of view.** The camera keeps the chase view's designed projection
  (55° vertical). The engine image was **not** searched for the cockpit modes'
  field of view in this slice; it stays unknown.

## Unknown (rule 4)

* The cockpit modes' vertical field of view.
* The **gunsight offset** (where the reticle sits relative to the eye): not
  recovered; the task title names it but this slice does not claim it.
* What distinguishes mode 6 from mode 7: both use the same eye here.
* The original's eye orientation basis (yaw/pitch from the camera state) was
  not traced (T542); body orientation is the designed choice.
* Whether the original hides the exterior by hiding the body or by drawing
  only `cockpit1`: this slice hides the body; the interior is a later stage.
