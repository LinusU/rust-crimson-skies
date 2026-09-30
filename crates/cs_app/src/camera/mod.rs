//! The camera application boundary: projection policy, framing and the
//! lowered mode records (F21-A).
//!
//! Spec: `specs/F21-cameras-cockpit-views-and-spyglass.md`, stage
//! `### F21-A`. Shared contract: `docs/contracts/UI-NETWORK.md`.
//!
//! F21-A is deliberately **not** a runtime. This module is the one place the
//! declared records (`cs_content::cameras`) meet the renderer, and it stays
//! free of ECS, assets and the Bevy render world so it can be exercised
//! headless:
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
//!   mode whose projection, magnification or target-tracking flag is an
//!   explicit unknown.
//!
//! Stage F21-B implements the cockpit/external/look/spyglass rigs on top of
//! these records; F21-C wires script cameras and deterministic capture
//! flags; F21-D compares original view behavior and needs `gpu` + `retail`,
//! which this stage does not claim.
//!
//! What is **not** claimed here: no original mode list, field of view,
//! projection axis, near/far plane, magnification or target-tracking
//! behavior was read or reproduced. Every fixture value is newly authored
//! design. The unknowns this stage met are recorded in
//! `docs/findings/2026-09-30-f21-a-camera-modes-and-projection-policy.md`.
//!
//! [`ProjectionPolicy`]: cs_content::cameras::ProjectionPolicy
//! [`DeclaredCameraModes`]: cs_content::cameras::DeclaredCameraModes

pub mod modes;
pub mod pose;
pub mod projection;

pub use modes::{
    CameraLowerError, LoweredCameraMode, LoweredCameraModes, lower_camera_mode, lower_camera_modes,
};
pub use pose::{CameraBasis, CameraPose, Framing, FramingError};
pub use projection::{LoweredProjection, ProjectionLowerError, lower_projection};
