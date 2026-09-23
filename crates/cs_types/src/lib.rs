//! Ids, units, typed inputs/outputs and immutable cross-boundary records.
//!
//! This crate is dependency-free by contract: no Bevy, Avian or renderer type
//! may appear here (`docs/01-ARCHITECTURE.md`). Everything below is newly
//! authored project type design; nothing in this file is derived from
//! original game data.

use std::fmt;

/// Zero-based simulation tick. Integer ticks are the only time value that may
/// cross crate boundaries.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Tick(pub u64);

/// Where a scene's content comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SceneProvenance {
    /// A newly authored, asset-free development scene. It is explicitly marked
    /// `SYNTHETIC` and is never selected as a replacement for missing retail
    /// content (F00 non-negotiable behavior 2).
    Synthetic,
}

impl SceneProvenance {
    /// Human-readable marker carried by the scene.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Synthetic => "SYNTHETIC",
        }
    }
}

/// Resource inserted into every synthetic scene world so runtime code and
/// tests can read the provenance marker instead of trusting a comment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SceneMarker(pub SceneProvenance);

/// How a body's motion is produced.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BodyKind {
    /// Pose and velocity are integrated by the physics engine (gravity and
    /// future flight forces act on it).
    Dynamic,
    /// Pose never integrates; the body is part of the world but immovable.
    Static,
}

/// Typed input for one body of the synthetic development scene.
///
/// All values use canonical SI units (meters, meters per second); the half
/// extents are half of each box dimension per axis. The values are newly
/// authored fixture data, not measurements from original content.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SyntheticBodySpec {
    pub kind: BodyKind,
    pub position_m: [f32; 3],
    pub linear_velocity_m_s: [f32; 3],
    pub half_extents_m: [f32; 3],
}

/// Structured validation failure for a [`SyntheticBodySpec`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SpecError {
    /// A named field contained NaN or infinity.
    NonFinite { field: &'static str },
    /// A box half extent was not strictly positive.
    NonPositiveHalfExtent { axis: &'static str },
}

impl fmt::Display for SpecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NonFinite { field } => write!(f, "{field} must be finite"),
            Self::NonPositiveHalfExtent { axis } => {
                write!(f, "half_extents_m[{axis}] must be greater than zero")
            }
        }
    }
}

impl std::error::Error for SpecError {}

const POSITION_FIELDS: [&str; 3] = ["position_m[0]", "position_m[1]", "position_m[2]"];
const VELOCITY_FIELDS: [&str; 3] = [
    "linear_velocity_m_s[0]",
    "linear_velocity_m_s[1]",
    "linear_velocity_m_s[2]",
];
const EXTENT_FIELDS: [&str; 3] = [
    "half_extents_m[0]",
    "half_extents_m[1]",
    "half_extents_m[2]",
];
const AXES: [&str; 3] = ["x", "y", "z"];

impl SyntheticBodySpec {
    /// The minimal synthetic fixture: a box dropped from 10 m at rest.
    ///
    /// Newly authored development content (`SYNTHETIC`); it is never used as a
    /// stand-in for missing retail content.
    pub const fn falling_box(kind: BodyKind) -> Self {
        Self {
            kind,
            position_m: [0.0, 10.0, 0.0],
            linear_velocity_m_s: [0.0, 0.0, 0.0],
            half_extents_m: [0.5, 0.5, 0.5],
        }
    }

    /// Validates the typed input before any world is built.
    ///
    /// Every failure names the offending field so the caller can report which
    /// input was rejected instead of silently dropping a body.
    pub fn validate(&self) -> Result<(), SpecError> {
        for (field, value) in POSITION_FIELDS.into_iter().zip(self.position_m) {
            if !value.is_finite() {
                return Err(SpecError::NonFinite { field });
            }
        }
        for (field, value) in VELOCITY_FIELDS.into_iter().zip(self.linear_velocity_m_s) {
            if !value.is_finite() {
                return Err(SpecError::NonFinite { field });
            }
        }
        for (index, (field, extent)) in EXTENT_FIELDS
            .into_iter()
            .zip(self.half_extents_m)
            .enumerate()
        {
            if !extent.is_finite() {
                return Err(SpecError::NonFinite { field });
            }
            if extent <= 0.0 {
                return Err(SpecError::NonPositiveHalfExtent { axis: AXES[index] });
            }
        }
        Ok(())
    }
}

/// Typed output: one sample read back from a simulated body.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BodySample {
    pub tick: Tick,
    pub position_m: [f32; 3],
    pub linear_velocity_m_s: [f32; 3],
}
