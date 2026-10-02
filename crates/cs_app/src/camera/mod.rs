//! The camera application boundary: projection policy, framing and the
//! lowered mode records (F21-A), the cockpit/chase/look/spyglass rigs
//! (F21-B), and the session that runs them under script cameras and
//! deterministic capture flags (F21-C).
//!
//! Spec: `specs/F21-cameras-cockpit-views-and-spyglass.md`, stages
//! `### F21-A`, `### F21-B` and `### F21-C`. Shared contract:
//! `docs/contracts/UI-NETWORK.md`.
//!
//! This module is the one place the declared records (`cs_content::cameras`)
//! meet the renderer, and it stays free of ECS, assets and the Bevy render
//! world so it can be exercised headless:
//!
//! * [`projection`] lowers a declared
//!   [`ProjectionPolicy`](cs_content::cameras::ProjectionPolicy) into a
//!   [`LoweredProjection`] and owns the framing math: the vertical field of
//!   view at any aspect, the horizontal field of view, and the normalized
//!   viewport coordinate of a world-space point. It refuses every unknown
//!   and refuses a stretch framing rule (F21 non-negotiable behavior 2 —
//!   art is never stretched).
//! * [`pose`] owns [`CameraPose`], the *input* copy of the authoritative
//!   aircraft pose, and [`CameraBasis`], its derived right/up/forward axes.
//!   Nothing here writes flight state: the pose is a value, the basis a
//!   derived value, and framing is a pure function of the two.
//! * [`modes`] lowers a declared [`DeclaredCameraModes`] set into the
//!   [`LoweredCameraModes`] a session's camera path consumes, refusing a
//!   mode whose projection, placement, magnification or target-tracking flag
//!   is an explicit unknown.
//! * [`orientation`] is the canonical rotation math a rig needs and
//!   `cs_types::space` does not provide: composition, rotation of a
//!   non-unit vector, the yaw/pitch of a head turn and the look-at basis.
//! * [`smoothing`] owns [`PoseSmoother`], the frame-rate independent
//!   exponential follow (F21 non-negotiable behavior 4) and how a frame says
//!   whether it moved.
//! * [`rig`] owns [`CameraRig`]: the four rigs, the [`LookOffset`] a free
//!   look applies, and the rules that keep a destroyed or switched spyglass
//!   target from leaving a stale magnified actor behind.
//! * [`script`] owns the scripted camera request a producer hands the session:
//!   which authored camera, for which bounded span, pinning a pose or framing a
//!   body — and the refusals it gets back before anything is applied. It carries
//!   no timeline; F40 owns authored camera timelines.
//! * [`capture`] owns the deterministic capture request (mission, tick, world
//!   pose, aspect, view and fixed comparison settings), the override report
//!   every applied capture must produce, and the [`PinnedProjection`] a capture
//!   pins — derived from the frame's own lowered policy, with the `f64 → f32`
//!   narrowing kept visible rather than hidden behind a cast.
//! * [`session`] is the integration: [`CameraSession`] owns the player's
//!   [`CameraRig`], at most one scripted camera and at most one pending capture,
//!   and produces one [`SessionFrame`] per render frame naming the authority
//!   that drew it. It is where AC03 — *swap aircraft during a scripted capture
//!   and verify the camera binds to the new player body* — is decided.
//!
//! F21-D compares original view behavior and needs `gpu` + `retail`, which no
//! stage here claims.
//!
//! What is **not** claimed here: no original mode list, field of view,
//! projection axis, near/far plane, magnification, target-tracking behavior,
//! cockpit binding, look limit or smoothing rate was read or reproduced. Every
//! fixture value is newly authored design. The unknowns this stage met are
//! recorded in
//! `docs/findings/2026-09-30-f21-a-camera-modes-and-projection-policy.md` and
//! `docs/findings/2026-10-03-f21-b-camera-rigs.md`.
//!
//! [`ProjectionPolicy`]: cs_content::cameras::ProjectionPolicy
//! [`DeclaredCameraModes`]: cs_content::cameras::DeclaredCameraModes

pub mod capture;
pub mod modes;
pub mod orientation;
pub mod pose;
pub mod projection;
pub mod rig;
pub mod script;
pub mod session;
pub mod smoothing;

pub use capture::{
    CaptureError, CaptureOverride, CaptureReport, CaptureRequest, CaptureTarget, MagnificationPin,
    Narrowing, PinnedProjection, ProjectionPinError, pin,
};
pub use modes::{
    CameraLowerError, LoweredCameraMode, LoweredCameraModes, LoweredCockpitViewpoint,
    LoweredPlacement, lower_camera_mode, lower_camera_modes,
};
pub use orientation::{compose, direction_to, look_rotation, rotate_vector, yaw_pitch};
pub use pose::{CameraBasis, CameraPose, Framing, FramingError};
pub use projection::{LoweredProjection, ProjectionLowerError, lower_projection};
pub use rig::{
    CameraRig, LookError, LookOffset, RigAimError, RigError, RigFrame, RigInputs, SpyglassAim,
    ViewRig,
};
pub use script::{
    ScriptCameraError, ScriptCameraRequest, ScriptEndReason, ScriptSubject, ScriptedShot,
};
pub use session::{
    BodyPose, CameraAuthority, CameraEvent, CameraSession, SessionError, SessionFrame,
    SessionFrameInputs, SessionView,
};
pub use smoothing::{PoseSmoother, SmoothingError, SmoothingState};
