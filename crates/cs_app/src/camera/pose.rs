//! Camera pose, basis and the normalized framing of a world-space point
//! (F21-A).
//!
//! Spec: `specs/F21-cameras-cockpit-views-and-spyglass.md`, stage
//! `### F21-A`, non-negotiable behavior 2. Shared contract:
//! `docs/contracts/UI-NETWORK.md`.
//!
//! [`CameraPose`] is an **input** record: a position and a rotation copied
//! from the authoritative aircraft/frame, never owned here.
//! [`CameraPose::basis`] derives the right/up/forward [`CameraBasis`], and
//! [`crate::camera::LoweredProjection::framing_of`] turns a world-space
//! target into normalized viewport coordinates. Nothing in this module
//! writes flight state, holds it mutably or advances it: a camera is a
//! consumer of the authoritative pose (F21 deliverable), so the whole
//! boundary is `&`-borrowed and value-returning.
//!
//! Framing is expressed in the renderer's normalized coordinates: `0.0` is
//! the centre, `±1.0` is an edge, and `is_inside` says whether the point is
//! within the viewport. That is what the aspect tests compare: under
//! aspect-correct framing the *same* world target keeps a constant vertical
//! coordinate across aspects instead of moving or stretching.

use std::fmt;

use cs_types::space::{Quaternion, SpaceError, UnitVec3, WorldPosition};

/// Why framing a world-space point was rejected.
#[derive(Clone, Debug, PartialEq)]
pub enum FramingError {
    /// The point is at or behind the camera's near side: it has no
    /// perspectively projected position.
    BehindCamera,
    /// A derived camera basis was not usable (a non-finite or non-unit axis
    /// after rotation).
    Basis(SpaceError),
}

impl fmt::Display for FramingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BehindCamera => write!(f, "the point is behind the camera and cannot be framed"),
            Self::Basis(error) => write!(f, "the camera basis is unusable: {error}"),
        }
    }
}

impl std::error::Error for FramingError {}

impl From<SpaceError> for FramingError {
    fn from(error: SpaceError) -> Self {
        Self::Basis(error)
    }
}

/// A camera's pose: where it is and how it is rotated, in canonical world
/// space.
///
/// This is a *copy* of authoritative state, never a handle to it. The
/// rotation is a canonical right-handed quaternion whose forward axis is
/// `-Z`, so `basis` is well defined for any pose (F16: forward -Z, up +Y).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CameraPose {
    position: WorldPosition,
    rotation: Quaternion,
}

impl CameraPose {
    /// Builds a pose from an authoritative position and rotation.
    ///
    /// Both inputs are already validated by their own constructors, so this
    /// cannot fail.
    #[must_use]
    pub const fn new(position: WorldPosition, rotation: Quaternion) -> Self {
        Self { position, rotation }
    }

    /// The camera's world position, in meters.
    #[must_use]
    pub const fn position(self) -> WorldPosition {
        self.position
    }

    /// The camera's rotation.
    #[must_use]
    pub const fn rotation(self) -> Quaternion {
        self.rotation
    }

    /// The camera's orthonormal basis, rotated from the canonical axes.
    ///
    /// `forward` is `rotation · (-Z)`, `up` is `rotation · (+Y)` and
    /// `right` is `rotation · (+X)`.
    ///
    /// # Errors
    ///
    /// [`SpaceError`] only if a rotated axis is somehow not a unit vector,
    /// which a validated [`Quaternion`] cannot produce.
    pub fn basis(self) -> Result<CameraBasis, SpaceError> {
        let x_axis = UnitVec3::try_new([1.0, 0.0, 0.0])?;
        Ok(CameraBasis {
            position: self.position,
            right: self.rotation.rotate(x_axis)?,
            up: self.rotation.rotate(UnitVec3::UP)?,
            forward: self.rotation.rotate(UnitVec3::FORWARD)?,
        })
    }
}

/// A camera position together with its orthonormal right/up/forward axes.
///
/// Derived once from a [`CameraPose`] and then reused for every target in a
/// frame; it is a value, so a renderer cannot mutate the pose through it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CameraBasis {
    position: WorldPosition,
    right: UnitVec3,
    up: UnitVec3,
    forward: UnitVec3,
}

impl CameraBasis {
    /// The camera's world position.
    #[must_use]
    pub const fn position(self) -> WorldPosition {
        self.position
    }

    /// The camera's right axis (`+X` in camera space).
    #[must_use]
    pub const fn right(self) -> UnitVec3 {
        self.right
    }

    /// The camera's up axis (`+Y` in camera space).
    #[must_use]
    pub const fn up(self) -> UnitVec3 {
        self.up
    }

    /// The camera's forward axis (`-Z` in camera space).
    #[must_use]
    pub const fn forward(self) -> UnitVec3 {
        self.forward
    }
}

/// A world-space point's position in the viewport, in normalized
/// coordinates.
///
/// `x` and `y` are `0.0` at the centre and `±1.0` at the left/right and
/// bottom/top edges; the sign follows the camera's right and up axes. The
/// values are not clamped, so a point outside the viewport reports *how far*
/// outside it is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Framing {
    x: f64,
    y: f64,
}

impl Framing {
    /// Builds a framing from its normalized coordinates.
    #[must_use]
    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }

    /// The normalized horizontal coordinate.
    #[must_use]
    pub const fn x(self) -> f64 {
        self.x
    }

    /// The normalized vertical coordinate.
    #[must_use]
    pub const fn y(self) -> f64 {
        self.y
    }

    /// Whether the point falls within the viewport (`|x| ≤ 1` and
    /// `|y| ≤ 1`).
    #[must_use]
    pub fn is_inside(self) -> bool {
        self.x.abs() <= 1.0 && self.y.abs() <= 1.0
    }
}

impl fmt::Display for Framing {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "({:.4}, {:.4})", self.x, self.y)
    }
}
