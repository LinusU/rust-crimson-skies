# F21-B: the cockpit, chase, look and spyglass rigs

Date: 2026-10-03. Task: F21-B "Implement cockpit/chase/look/spyglass rigs"
(`specs/F21-cameras-cockpit-views-and-spyglass.md`, section `### F21-B`).
Shared contract: `docs/contracts/UI-NETWORK.md`. Capabilities used: ordinary
build/test only — no `CS_GAME_DIR` read, no evidence report required, no GPU.

## Files and the one observable failure (the slice plan)

* `crates/cs_content/src/cameras.rs`: **where** a mode's camera sits.
  `BodyOffset`, `CockpitBindingSource`, `CockpitViewpoint`, `LookLimits`,
  `ViewpointError`, `DeclaredPlacement`, and two new `CameraModeError`
  variants; `DeclaredCameraMode` gains `placement` and `look_limits`. The
  synthetic fixture declares a bound cockpit eye, a chase offset and the
  spyglass's body-origin eye.
* `crates/cs_app/src/camera/orientation.rs` (new): `compose`,
  `rotate_vector`, `yaw_pitch`, `look_rotation`, `direction_to` — the
  canonical rotation math `cs_types::space` does not provide.
* `crates/cs_app/src/camera/smoothing.rs` (new): `PoseSmoother`,
  `SmoothingState`, `SmoothingError` — the frame-rate independent follow.
* `crates/cs_app/src/camera/rig.rs` (new): `CameraRig`, `ViewRig`,
  `LookOffset`, `RigInputs`, `RigFrame`, `SpyglassAim`, `RigError`,
  `RigAimError`, `LookError`.
* `crates/cs_app/src/camera/modes.rs`: `LoweredPlacement`,
  `LoweredCockpitViewpoint`, `LoweredCameraMode::{placement, look_limits}`,
  `LoweredCameraModes::origin`.
* `crates/cs_app/src/camera/mod.rs`, `crates/cs_app/src/lib.rs`,
  `crates/cs_content/src/lib.rs`: module declarations, re-exports and doc
  paragraphs only. No logic.
* `crates/cs_app/tests/camera/{rig,spyglass,smoothing}.rs` (new) and the
  shared helpers in `common.rs`: the twenty-six `accept_f21_b_*` tests. The
  three F21-A test files changed only where `DeclaredCameraMode::try_new`
  gained two arguments.
* This file.

**One observable failure:** if the spyglass caches its target instead of
re-reading the session's published selection every frame, a target destroyed
between two render frames keeps its magnification for one frame longer than
it exists — the camera magnifies a wreck and a capture taken on that frame
records a target that was already gone. That is AC02's "destroy or switch the
spyglass target mid-frame without stale entity access" failing in the only way
it can fail. The minimum scenario is
`accept_f21_b_destroying_the_spyglass_target_drops_the_magnification_in_the_same_frame`,
with the switch, the stale-replay and the unaimable-target cases beside it; the
mutations under "Test sensitivity" make them fail on purpose.

## What the stage adds to the records

F21-A defined *which* views exist and how a field of view becomes a frustum.
F21-B adds the two things a rig needs to place a camera, and makes both part
of the declared mode:

| record | answers | type |
| --- | --- | --- |
| `BodyOffset` | where, in the aircraft's own body frame | metres along the body's right/up/forward axes; only non-finite input is refused |
| `CockpitBindingSource` | which binding the eye was read from | `ModelNode { node }` \| `ConfigKey { key }`, name non-empty and ≤ 64 bytes |
| `CockpitViewpoint` | the verified cockpit viewpoint | binding + `BodyOffset` + `Resolved` yaw and pitch |
| `DeclaredPlacement` | which of the two a mode declares | `Cockpit(CockpitViewpoint)` \| `BodyOffset(BodyOffset)` |
| `LookLimits` | how far a free-look offset may turn the view | yaw in `(-π, π)`, pitch in `(-π/2, π/2]` |

Three decisions are worth recording:

1. **The placement belongs to the mode, and the kind decides which one.**
   `DeclaredCameraMode::try_new` refuses a `Cockpit` mode that declares a bare
   body offset (`CockpitViewpointRequired`) and refuses any other kind that
   claims a cockpit viewpoint (`UnexpectedCockpitViewpoint`). F21
   non-negotiable behavior 1 ("cockpit viewpoint comes from verified
   model/config bindings", and a HUD-only synthetic camera is not a
   replacement for every original cockpit) is therefore enforced by the record,
   not by a renderer's good intentions: an aircraft with no binding declares
   **no cockpit mode at all**, which is the only honest way to say "this
   aircraft has no cockpit view".
2. **The binding survives lowering.** `LoweredCockpitViewpoint` keeps the
   source, and `LoweredCameraModes` keeps the set's `cs_types::content::Origin`,
   so a consumer can still answer the provenance question after lowering: a
   `CockpitBindingSource` on a set whose origin is `SyntheticFixture` is
   development content and can never be reported as a verified original
   binding. `CameraRig::cockpit_binding()` and `CameraRig::origin()` are the
   accessors.
3. **`LookLimits` is a `Resolved`.** Free look is enhanced support — the
   sheet's deliverable keeps "modern free-look/controller support" separate
   from the original default mappings — but it is still a value a source may
   not have, so an unknown look limit refuses at lowering
   (`CameraLowerError::UnknownField { field: "look_limits" }`) rather than
   defaulting to "no limit".

## The four rigs

| rig | eye | orientation | magnification |
| --- | --- | --- | --- |
| `Cockpit` | the bound viewpoint's body offset, rotated by the aircraft's attitude | the aircraft's attitude composed with the pilot's declared head turn | 1 |
| `Chase` | the declared body offset | the aircraft's own axes | 1 |
| `Look` | unchanged — a look turns the view, it does not move the eye | the current rig's orientation with the offset clamped to the mode's `LookLimits` | as before |
| `Spyglass` | the spyglass mode's own placement | aimed at the session's selected target | the mode's declared magnification |

`RigFrame` is a `Copy` value carrying the tick, the rig, the mode, the smoothed
pose, the lowered projection, the aspect, the magnification, the *clamped* look
offset, the spyglass aim and the smoothing state — everything a renderer needs
and nothing it has to reconstruct. `RigFrame::framing_of` composes the frame's
own pose basis with its own projection at its own aspect, so a consumer cannot
place a reticle through a different frustum than the one on screen.

## AC02: no stale entity access

The rig holds no entity, no pointer and no selection store: `resolve` takes a
`RigInputs` (tick, subject `ActorId`, aircraft pose, aspect, wall time, look,
`OriginChange`, and the session's published `SpyglassReadout`) and returns a
frame. There is nothing in `cs_app::camera` that can reach an ECS entity, which
is the structural half of "without stale entity access"; the behavioural half is
four rules, each its own refusal:

1. **Re-read every frame.** The only source of a target is the frame's
   `SpyglassReadout`, so a target cannot survive in a cache — the rig's own
   state holds the `ActorId` it framed last frame and nothing else, and it uses
   it to *compare*, never to position.
2. **A stale view refuses.** `RigError::StaleSpyglassReadout` when the
   published view is at a tick the rig already consumed. Ticks are not wall
   time: a render frame between two fixed ticks legitimately re-reads the same
   tick (asserted legal), while a *smaller* one is a view from before a kill
   and would put the last magnification back up.
3. **A clear drops the actor in the same frame.** `SpyglassAim::actor` is
   `None`, `dropped` names who went and `cleared` carries F30-C's record; the
   mode stays the spyglass mode, so its own field of view and its own near/far
   planes are still the ones in force (behavior 3) — the rig does not swap the
   mode to hide the miss.
4. **An unaimable target refuses *and* drops the actor**, so a caller that
   keeps the last published frame after an error still cannot read a live
   magnified actor out of the rig.

`tracks_target` is honoured in both directions: a mode that declares it does not
track a target gets none, even with a live selection.

**The aim's roll is stated, not invented.** `orientation::look_rotation` needs
an up hint that is not parallel to the view direction, and the canonical `+Y` is
the wrong hint for a spyglass: a target directly overhead makes it degenerate.
`rig::aim_at` hands it the *aircraft's* up axis — the attitude the pilot is in —
and falls back to the aircraft's right axis for the one attitude where up is
parallel to the view. They are orthogonal, so the aim never refuses for want of
a roll. A camera looking straight up therefore keeps the aircraft's right axis
as its up, which is deterministic and is what a real chase or spyglass view
does. This was found by a test failing on `SpaceError::NotUnit { length: 0.0 }`
with the fixture's raider spread along `+X`, `+Y` and `-Z`.

## Smoothing, resettling and origin shifts (non-negotiable behavior 4)

`PoseSmoother` closes a fixed *fraction of the remaining distance per second*:

```text
α(dt) = 1 − e^(−k·dt)
pose  ← pose + α(dt)·(desired − pose)
```

Because `1 − α(dt) = e^(−k·dt)`, the weight left on the starting pose after `n`
frames is `e^(−k·Σdt)` — the **same number** over the same wall time at 30, 60
or 144 FPS, up to floating-point rounding. The rotation takes the same path by
sign-corrected normalized linear interpolation (nlerp): `q` and `−q` are the same
rotation, so without the sign flip an interpolation turns the long way round,
and nlerp needs no `sin(θ)/θ` limit branch to stay exact.

A per-frame constant fraction fails this and a velocity in metres per second is
only exact for a straight line; the test asserts the residual
`exp(−k)·(start − desired)` to `1e-6` rather than "close enough", because the
second number is the one that would differ if the law were per-frame.

The rig decides *when* to reseat from typed facts, never from a distance
heuristic:

* a **different subject** — an `ActorId` is generation-qualified, so an aircraft
  swap arrives as a different actor; the camera, the framed target *and* the
  consumed-readout tick all restart (the last one matters: a new session
  generation restarts the tick counter, and a stale check carried across the swap
  would refuse the new observer's first view);
* a `cs_app::origin::OriginChange::Teleport` — a world-space jump;
* the first frame.

A `Rebase` does **not** reseat and needs no conversion: the rig's state is
canonical f64 world space, which a rebase leaves alone (F16 non-negotiable
behavior 5). `accept_f21_b_a_rebase_moves_the_local_frame_without_moving_the_camera`
drives the real `OriginShift` over a real `SpatialAnchor` and compares every
frame against a control rig that is never told a rebase happened.

## Test sensitivity

Five mutations, applied and reverted, all against production code, run as
`cargo test -p cs_app --test camera -- accept_f21_b_`:

1. `PoseSmoother::advance`'s `α(dt)` replaced with a constant `0.1` (the
   per-frame lerd this stage exists to avoid) → **3 fail**:
   `accept_f21_b_smoothing_is_frame_rate_independent_at_30_60_and_144_fps`,
   `accept_f21_b_free_look_turns_the_view_only_and_is_clamped_to_the_declared_limits`,
   `accept_f21_b_switching_the_spyglass_target_reaims_and_names_the_dropped_actor`.
2. `CameraRig::aim_spyglass` keeps the framed actor on a stale readout instead
   of dropping it → **1 fails**:
   `accept_f21_b_a_spyglass_view_older_than_one_already_consumed_is_refused`.
3. The same function keeps the framed actor on an unaimable target → **1
   fails**:
   `accept_f21_b_an_unaimable_spyglass_target_is_refused_and_not_framed`.
4. `lower_placement` lowers an unknown cockpit head yaw as `0.0` instead of
   refusing → **1 fails**:
   `accept_f21_b_unknown_look_limits_and_cockpit_orientation_refuse_to_lower`.
5. `DeclaredCameraMode::try_new` accepts a `Cockpit` mode with a bare body
   offset → **1 fails**:
   `accept_f21_b_a_cockpit_mode_needs_a_binding_and_no_other_mode_may_claim_one`.

All were reverted; the tree is back to the implementation under review.

## Commands run (exit codes)

```text
cargo fmt --all -- --check                                          → 0
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings → 0
cargo test --workspace --locked                                     → 0
cargo test --workspace --locked -- accept_f21_b_ --include-ignored    → 0 (26 tests, all passed)
```

The task selection discovers exactly the 26 `accept_f21_b_*` tests in
`crates/cs_app/tests/camera/` (`rig.rs` 7, `spyglass.rs` 10, `smoothing.rs` 9).
None is `#[ignore]`d: nothing here needs `CS_GAME_DIR`. The fifteen F21-A tests
in the same binary still pass unchanged in intent.

## Designed vs measured: the unknowns this stage met

Everything in this stage is **designed** engine contract. The following are
`unknown`, recorded rather than guessed; none of them blocks F21-B, and each is
named with the stage that resolves it:

| unknown | evidence | resolves in |
| --- | --- | --- |
| Whether any original cockpit viewpoint binding exists, and in which model node or configuration key | no original bytes were read for this task; `CockpitBindingSource` is a designed vocabulary and the fixture binds `synthetic.pilot_eye` | F21-D (`gpu` + `retail`), then an F21 import stage |
| The original eye offset and head orientation per aircraft | `CockpitViewpoint::offset` is a `BodyOffset` (authored 1.2 m up, 1.5 m forward); yaw and pitch are `Resolved` and refuse when unmeasured | F21-D against original captures |
| The original external/chase geometry | `BodyOffset` (authored 3 m up, 12 m back); the chase looks along the aircraft's own axes, which is design | F21-D |
| The original spyglass placement and whether it looks through the cockpit or the body origin | the fixture declares `BodyOffset::ZERO` and the rig aims from wherever the mode places it | F21-D |
| The original free-look range, and whether the original binds free look at all | `LookLimits` is a `Resolved` (authored ±120° yaw, ±60° pitch) and free look is enhanced support per the sheet's deliverable | F21-D, or never, if the original has none |
| The original camera's smoothing rate and whether it lags position, rotation or both | `CameraRig::DEFAULT_RESPONSE_PER_S` is 12/s and the lag is a project choice; the *law* is required, the rate is not | F21-D |
| Whether the original camera looks at the selected target with the aircraft's attitude as its up hint, or with a fixed world up | `rig::aim_at` uses the aircraft's up with the aircraft's right as the documented fallback | F21-D |
| Whether the original's cockpit/chase/spyglass modes have their own clipping planes as ours do | the fixture gives the spyglass 1 m/20 km against the others' 0.1 m/10 km, which is design | F21-D |
| Whether the original magnifies by narrowing the field of view, by scaling the projection, or by rendering a separate pass | `Magnification` is reported per frame and the lowered projection narrows the FOV; a renderer still has to choose how to draw it | F21-C (wiring) and F21-D |

A follow-up this stage found and did **not** fix, because it is outside the
owner paths: `cs_types::space::UnitVec3` names the canonical forward and up axes
as constants and leaves the right axis to each caller, so
`camera::orientation::canonical_right` restates `+X`. That is a naming gap in
`cs_types`, not a defect; it is filed as a follow-up task.

## What is not claimed

A code/test pass awards at most **checked**. No original camera data was read,
no original view behavior was reproduced, no original binding, offset, look
range or smoothing rate was measured, and nothing here is `verified_original`.
The rig is implemented but not yet scheduled: F21-C wires it into the session's
producer and consumer (script cameras, capture flags, the renderer that draws a
`RigFrame`), and F21-D is the stage that may compare any of this against
original captures. Synthetic fixtures alone cannot certify original-data
behavior.
