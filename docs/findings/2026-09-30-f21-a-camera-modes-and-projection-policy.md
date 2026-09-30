# F21-A: camera modes and projection policy

Date: 2026-09-30. Task: F21-A "Define camera modes and projection policy"
(`specs/F21-cameras-cockpit-views-and-spyglass.md`, section `### F21-A`).
Shared contract: `docs/contracts/UI-NETWORK.md`. Capabilities used: ordinary
build/test only — no `CS_GAME_DIR` read, no evidence report required, no GPU.

## Files and the one observable failure (the slice plan)

* `crates/cs_content/src/cameras.rs` (new): the Bevy-free declared contract —
  `CameraModeKind`, `FovAxis`, `AspectFraming`, the validated `AspectRatio`
  and `Magnification`, the `ProjectionPolicy` (every field a `Resolved`), the
  validated `DeclaredCameraMode` and `DeclaredCameraModes`, and the synthetic
  `declared_synthetic_camera_modes` fixture.
* `crates/cs_app/src/camera/pose.rs` (new): `CameraPose` (a read-only copy of
  the authoritative pose), `CameraBasis`, `Framing`, `FramingError`.
* `crates/cs_app/src/camera/projection.rs` (new): `lower_projection`,
  `LoweredProjection`, the vertical↔horizontal FOV conversion and the
  `framing_of` math.
* `crates/cs_app/src/camera/modes.rs` (new): `lower_camera_mode(s)`,
  `LoweredCameraMode(s)`, `CameraLowerError`.
* `crates/cs_app/src/camera/mod.rs` (new): module docs and re-exports.
* `crates/cs_content/src/lib.rs`, `crates/cs_app/src/lib.rs` (wiring only):
  module declarations and doc paragraphs. No logic lives there.
* `crates/cs_app/tests/camera/{main,common,records,projection,framing,modes}.rs`
  (new): the thirteen `accept_f21_a_*` acceptance tests.
* This file.

**One observable failure:** if the authored field of view is declared on the
wrong axis, the same invariant world target no longer keeps its vertical
viewport coordinate across 4:3, 16:9 and ultrawide — the image is framed by a
different angle than the one authored, so the vertical extent drifts with the
viewport (a stretch in everything but name). That is exactly AC01's
"compare framing at three aspect ratios using an invariant world-space
target" failing in the only way it can fail: the horizontal FOV grows but the
vertical one is not the authored one. The acceptance test
`accept_f21_a_lower_projection_converts_horizontal_to_vertical_and_back`
measures the conversion and
`accept_f21_a_framing_at_three_aspect_ratios_keeps_vertical_extent_and_reveals_more_world`
measures the invariant; the mutation under "Test sensitivity" below makes them
fail on purpose.

## What the stage defines

| record | answers | type |
| --- | --- | --- |
| `CameraModeKind` | what kinds of view exist | designed four-value vocabulary (`Cockpit`, `External`, `Spyglass`, `AuthoredSequence`) |
| `FovAxis` | which axis an authored FOV spans | `Vertical` \| `Horizontal` |
| `AspectFraming` | how framing survives an aspect change | `PreserveVertical` \| `PreserveHorizontal` (both aspect-correct); `Stretch` is declarable and refused |
| `AspectRatio` | the viewport shape the FOV was authored at | validated `f64`, finite and `> 0` |
| `Magnification` | the spyglass zoom | validated `f64`, finite and `> 0`; `1.0` is none |
| `ProjectionPolicy` | how one mode's FOV becomes a frustum | six `Resolved` fields: `fov`, `fov_axis`, `reference_aspect`, `framing`, `near_m`, `far_m` |
| `DeclaredCameraMode` | one mode | kind + projection + `Resolved` magnification + `Resolved` `tracks_target` |
| `DeclaredCameraModes` | one subject's modes | a `ContentKind::CameraTrack` subject, one mode per kind, a declared default that must be present |

Three decisions are worth recording:

1. **FOV is only meaningful with its axis and reference aspect.** The record
   keeps all three, so an importer that read a horizontal value cannot be
   silently reinterpreted as vertical (F21 non-negotiable behavior 2:
   "define vertical vs horizontal conversion"). `lower_projection` normalizes
   every declaration to the *vertical* FOV at the declared reference aspect
   before any framing happens.
2. **Aspect policy is a declared choice, not a renderer default.** Only the
   two aspect-correct rules are usable. `Stretch` exists so that a source
   declaring a non-uniform fill is *representable and refused*
   (`ProjectionLowerError::StretchFraming`) instead of being quietly treated
   as one of the correct rules. F21 behavior 2 forbids stretching art.
3. **A camera is a read-only consumer of the pose.** `CameraPose` is a value
   copy; `CameraBasis` is derived; `framing_of` is a pure function of the
   two. Nothing in `cs_app::camera` holds or writes flight state (F21
   deliverable: "CameraRig consumes authoritative aircraft pose but never
   writes flight state").

## Vertical/horizontal conversion

`lower_projection` normalizes to the vertical field of view at the declared
reference aspect `a`:

```text
vertical_from_horizontal(h, a) = 2·atan(tan(h/2) / a)
horizontal_from_vertical(v, a) = 2·atan(tan(v/2) · a)
```

The two are inverses. At the reference aspect the round trip is exact to the
test tolerance: declaring `h` on the horizontal axis, lowering, and reading
`horizontal_fov_at(reference_aspect)` returns `h`. `LoweredProjection::
vertical_fov_at` and `horizontal_fov_at` implement the two framing rules:

* `PreserveVertical`: the vertical FOV is constant in aspect; the horizontal
  FOV grows as the viewport widens.
* `PreserveHorizontal`: the horizontal FOV is constant in aspect; the
  vertical FOV shrinks as the viewport widens.

Measured scenario (all dyadic where it matters): camera at the world origin
with the identity basis, an invariant target at `[30, 10, -100]`, and a
cockpit vertical FOV of 60° authored at 4:3. The target's vertical viewport
coordinate is `0.1 / tan(30°) = 0.17320508075688773` at 4:3, 16:9 and 64:27
(bit-identical, asserted to `1e-15`); the horizontal FOV grows
`4:3 < 16:9 < 64:27`; and a target 45° off-axis is cropped at 4:3 but inside
at 16:9. Under `PreserveHorizontal` the roles swap: the horizontal coordinate
is constant and the vertical FOV shrinks as the aspect grows.

## Refusal at the lowering boundary

A declared record is allowed to carry unknowns; the *lowering* boundary is
what refuses. `lower_projection` returns `ProjectionLowerError::UnknownField`
for any `Resolved::Unknown` field, so no renderer runs under a guessed FOV,
axis, reference aspect, framing rule or clipping plane. `lower_camera_mode`
extends this to each mode's magnification and `tracks_target`. This mirrors
`cs_app::targeting::lower_rules` and `cs_app::environment`.

`DeclaredCameraMode::try_new` refuses corrupt *known* values once:
`FovOutOfRange` outside `(0, π)`, `NonPositiveNear`, `ClippingNotOrdered`,
and `UnexpectedMagnification` when a non-spyglass mode declares a factor
other than 1. A set refuses a non-`CameraTrack` subject, an empty mode list,
a duplicated kind and a default that is not declared.

## Test sensitivity

Two mutations, applied and reverted, both against production code:

1. `LoweredProjection::horizontal_fov_at` made to ignore the aspect for
   `PreserveVertical` (returning the reference-aspect value) →
   `accept_f21_a_framing_at_three_aspect_ratios_keeps_vertical_extent_and_reveals_more_world`
   and `accept_f21_a_lower_projection_keeps_the_declared_vertical_policy`
   **fail**: the horizontal FOV no longer grows, so the 45° target is not
   revealed by 16:9 (2 failed).
2. The `StretchFraming` refusal removed from `lower_projection` (so a
   declared `Stretch` lowers) →
   `accept_f21_a_lower_projection_refuses_unknowns_stretch_and_corruption`
   **fails** at "a declared stretch framing must be refused".

Both were reverted; the tree is back to the implementation under review.

## Commands run (exit codes)

```text
cargo fmt --all -- --check                                       → 0
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings → 0
cargo test --workspace --locked                                  → 0
cargo test --workspace --locked -- accept_f21_a_ --include-ignored → 0 (13 tests, all passed)
```

The task selection discovers exactly the 13 `accept_f21_a_*` tests in
`crates/cs_app/tests/camera/`. None is `#[ignore]`d: nothing here needs
`CS_GAME_DIR`.

## Designed vs measured: the unknowns this stage met

Everything in this stage is **designed** engine contract. The following are
`unknown`, recorded rather than guessed; none of them blocks F21-A, and each
is named with the stage that resolves it:

| unknown | evidence | resolves in |
| --- | --- | --- |
| The original PC view list, its names and its default view | no original bytes read for this task (`CS_GAME_DIR` was not opened); `CameraModeKind` is a designed vocabulary | F21-D (`gpu` + `retail`) and any F21 import stage |
| The original field of view per view, the axis it was authored on and the reference aspect | `ProjectionPolicy` carries all three as `Resolved`; the fixture is newly authored | F21-D against private captures |
| The original near/far clipping planes | the fixture authors 0.1 m / 10 000 m (cockpit, external) and 1 m / 20 000 m (spyglass) as project design | F21-D |
| The original spyglass magnification and target-tracking behavior | `Magnification` and `tracks_target` are typed homes; the fixture's 4x is authored | F21-D, then F21-B |
| Whether the original stretches or letterboxes on non-4:3 aspects | `AspectFraming` models both correct rules; the original's choice is unmeasured | F21-D |
| Where camera mode records belong in the canonical catalog (`ContentKind` has no camera-*mode* namespace; only `CameraTrack` exists) | F21-A used the `CameraTrack` namespace for the subject and filed the gap rather than inventing a kind | #431 `F21-A-CATALOG-KIND`; owner decision on the canonical `IDENTITY-CONTENT` catalog contract |

## What is not claimed

A code/test pass awards at most **checked**. No original camera data was read,
no original view behavior was reproduced, no rig consumes these records yet,
and nothing here is `verified_original`. F21-B implements the
cockpit/chase/look/spyglass rigs from these records; F21-D is the stage that
may compare them against original captures. Synthetic fixtures alone cannot
certify original-data behavior.
