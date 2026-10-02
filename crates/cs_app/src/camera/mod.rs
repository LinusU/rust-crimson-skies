//! The camera application boundary: projection policy, framing and the
//! lowered mode records (F21-A), and the cockpit/chase/look/spyglass rigs
//! (F21-B).
//!
//! Spec: `specs/F21-cameras-cockpit-views-and-spyglass.md`, stages
//! `### F21-A` and `### F21-B`. Shared contract:
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
//!
//! F21-C wires the rigs into the session's schedule and script cameras; F21-D
//! compares original view behavior and needs `gpu` + `retail`, which neither
//! stage claims.
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

pub mod modes;
pub mod orientation;
pub mod pose;
pub mod projection;
pub mod rig;
pub mod smoothing;

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
pub use smoothing::{PoseSmoother, SmoothingError, SmoothingState};
