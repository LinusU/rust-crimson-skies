//! Canonical world space, physical units and typed spatial values (F16-A).
//!
//! Spec: `specs/F16-coordinates-units-origin-management-and-clocks.md`,
//! stage `### F16-A`. Shared contract: `docs/contracts/FLIGHT-PHYSICS.md`,
//! section "Coordinate convention" (body forward -Z, right +X, up +Y).
//!
//! The canonical convention is **right-handed, +Y up, aircraft forward -Z,
//! SI units and radians, with counter-clockwise front faces**. Per the spec's
//! "Deliverable and interfaces" that is a *project convention, not a claim
//! about the original game*: which handedness, axis order, scale and angle
//! units an original file uses has not been measured (F16-D), so nothing in
//! this module is derived from original data.
//!
//! `cs_types` must stay free of any Bevy, Avian or renderer dependency
//! (`docs/01-ARCHITECTURE.md`), so these are plain fields with validating
//! constructors instead of a math crate's vectors. The *source* side of the
//! contract — declaring a file's convention and converting through it exactly
//! once — lives in `cs_content::coordinates`.

use std::fmt;

/// Length in meters (SI).
///
/// Like [`Tick`](crate::Tick) this is a thin newtype over the number itself.
/// Its input is checked finite at the boundary that produces it: the
/// coordinate adapters in `cs_content::coordinates` reject non-finite
/// distances before a [`Meters`] value exists.
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
pub struct Meters(pub f64);

/// Angle in radians (SI plane angle).
///
/// Input is checked finite at the boundary that produces it; see [`Meters`].
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
pub struct Radians(pub f64);

/// Order in which a triangle's vertices wind, in canonical coordinates.
///
/// Canonical front faces are [`CounterClockwise`](Self::CounterClockwise)
/// when the triangle is projected along the canonical viewing axis (+Z). A
/// source declares its own rule in `cs_content::coordinates`; what survives
/// the conversion is that front faces stay front faces.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Winding {
    /// Canonical front faces wind counter-clockwise.
    CounterClockwise,
    /// Back faces in the canonical convention.
    Clockwise,
}

impl Winding {
    /// The canonical front-face rule.
    pub const CANONICAL_FRONT: Self = Self::CounterClockwise;

    /// The opposite winding.
    #[must_use]
    pub const fn flipped(self) -> Self {
        match self {
            Self::CounterClockwise => Self::Clockwise,
            Self::Clockwise => Self::CounterClockwise,
        }
    }

    /// Short label for diagnostics and fixtures.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::CounterClockwise => "ccw",
            Self::Clockwise => "cw",
        }
    }
}

/// Why a typed spatial value was rejected at its boundary.
#[derive(Clone, Debug, PartialEq)]
pub enum SpaceError {
    /// A named field contained NaN or infinity.
    NonFinite {
        /// The offending field, as the caller spelled it.
        field: &'static str,
    },
    /// A value that must be unit length was not; `length` is what was
    /// measured, so the diagnostic never silently renormalizes input.
    NotUnit {
        /// Measured length of the rejected vector or quaternion.
        length: f64,
    },
}

impl fmt::Display for SpaceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NonFinite { field } => write!(f, "{field} must be finite"),
            Self::NotUnit { length } => write!(
                f,
                "expected a unit-length vector, measured length is {length}"
            ),
        }
    }
}

impl std::error::Error for SpaceError {}

/// How far from exactly `1.0` a unit vector's length may fall and still be
/// accepted by [`UnitVec3::try_new`].
///
/// This is an acceptance bound for inputs that were already unit length
/// (parsers and the coordinate adapters preserve length exactly up to float
/// rounding); it is deliberately far too tight to hide a visibly wrong
/// vector, and a rejected value reports its measured length instead of being
/// renormalized.
pub const UNIT_LENGTH_TOLERANCE: f64 = 1e-6;

/// How far from exactly `1.0` a quaternion's length may fall and still be
/// accepted by [`Quaternion::try_new`].
///
/// Same intent as [`UNIT_LENGTH_TOLERANCE`]: a denormalized rotation is a
/// data error to report, not something to fix silently.
pub const QUATERNION_LENGTH_TOLERANCE: f64 = 1e-6;

/// A position in canonical world space, in meters, relative to whichever
/// world-origin epoch the surrounding context declares (`cs_app::origin`).
///
/// f64 is what makes a far-away origin usable: physics and rendering work in
/// the f32 [`LocalPosition`] of one origin frame, while long-lived world
/// identity (projectiles, triggers, AI paths, camera history) stays in these
/// f64 coordinates through every rebase (`F16` non-negotiable behavior 2).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WorldPosition {
    x: f64,
    y: f64,
    z: f64,
}

impl WorldPosition {
    /// Validates the three world coordinates.
    ///
    /// # Errors
    ///
    /// [`SpaceError::NonFinite`] naming the first non-finite component.
    pub fn try_new(value: [f64; 3]) -> Result<Self, SpaceError> {
        const FIELDS: [&str; 3] = ["world.x", "world.y", "world.z"];
        check_finite(value, FIELDS)?;
        Ok(Self {
            x: value[0],
            y: value[1],
            z: value[2],
        })
    }

    /// Canonical +X component, in meters.
    #[must_use]
    pub const fn x(self) -> f64 {
        self.x
    }

    /// Canonical +Y component, in meters.
    #[must_use]
    pub const fn y(self) -> f64 {
        self.y
    }

    /// Canonical +Z component, in meters.
    #[must_use]
    pub const fn z(self) -> f64 {
        self.z
    }

    /// The three components as an array.
    #[must_use]
    pub const fn to_array(self) -> [f64; 3] {
        [self.x, self.y, self.z]
    }
}

/// A position in one origin frame's local space, in meters, stored as f32
/// for the physics and render side.
///
/// The local frame is always relative to a `WorldOrigin` (`cs_app::origin`);
/// a [`LocalPosition`] carries no epoch of its own, so it must never outlive
/// the origin it was produced from without being converted back through
/// [`WorldPosition`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LocalPosition {
    x: f32,
    y: f32,
    z: f32,
}

impl LocalPosition {
    /// The local frame's origin corner.
    pub const ZERO: Self = Self {
        x: 0.0,
        y: 0.0,
        z: 0.0,
    };

    /// Validates the three local coordinates.
    ///
    /// # Errors
    ///
    /// [`SpaceError::NonFinite`] naming the first non-finite component
    /// (including a component that overflowed f32 when it was cast).
    pub fn try_new(value: [f32; 3]) -> Result<Self, SpaceError> {
        const FIELDS: [&str; 3] = ["local.x", "local.y", "local.z"];
        check_finite(
            [
                f64::from(value[0]),
                f64::from(value[1]),
                f64::from(value[2]),
            ],
            FIELDS,
        )?;
        Ok(Self {
            x: value[0],
            y: value[1],
            z: value[2],
        })
    }

    /// Canonical +X component, in meters.
    #[must_use]
    pub const fn x(self) -> f32 {
        self.x
    }

    /// Canonical +Y component, in meters.
    #[must_use]
    pub const fn y(self) -> f32 {
        self.y
    }

    /// Canonical +Z component, in meters.
    #[must_use]
    pub const fn z(self) -> f32 {
        self.z
    }

    /// The three components as an array.
    #[must_use]
    pub const fn to_array(self) -> [f32; 3] {
        [self.x, self.y, self.z]
    }
}

/// A unit-length vector in canonical space: a direction or a surface normal.
///
/// Directions and normals are the same type because the coordinate adapters
/// declare a single positive length scale, so there is no per-axis stretch
/// that would need an inverse-transpose (`F16` deliverable: adapters map
/// "directions, normals" into the canonical convention).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UnitVec3 {
    x: f64,
    y: f64,
    z: f64,
}

impl UnitVec3 {
    /// The canonical forward axis of an aircraft: -Z (FLIGHT-PHYSICS,
    /// "Coordinate convention").
    pub const FORWARD: Self = Self {
        x: 0.0,
        y: 0.0,
        z: -1.0,
    };

    /// The canonical up axis: +Y.
    pub const UP: Self = Self {
        x: 0.0,
        y: 1.0,
        z: 0.0,
    };

    /// Validates a direction or normal.
    ///
    /// The input is neither normalized nor clamped: a non-unit vector is
    /// reported with its measured length so the caller can attribute it to
    /// its source.
    ///
    /// # Errors
    ///
    /// [`SpaceError::NonFinite`] for a NaN/infinite component, otherwise
    /// [`SpaceError::NotUnit`] when the length differs from 1 by more than
    /// [`UNIT_LENGTH_TOLERANCE`].
    pub fn try_new(value: [f64; 3]) -> Result<Self, SpaceError> {
        const FIELDS: [&str; 3] = ["vector.x", "vector.y", "vector.z"];
        check_finite(value, FIELDS)?;
        let length = (value[0] * value[0] + value[1] * value[1] + value[2] * value[2]).sqrt();
        if (length - 1.0).abs() > UNIT_LENGTH_TOLERANCE {
            return Err(SpaceError::NotUnit { length });
        }
        Ok(Self {
            x: value[0],
            y: value[1],
            z: value[2],
        })
    }

    /// Canonical +X component.
    #[must_use]
    pub const fn x(self) -> f64 {
        self.x
    }

    /// Canonical +Y component.
    #[must_use]
    pub const fn y(self) -> f64 {
        self.y
    }

    /// Canonical +Z component.
    #[must_use]
    pub const fn z(self) -> f64 {
        self.z
    }

    /// The three components as an array.
    #[must_use]
    pub const fn to_array(self) -> [f64; 3] {
        [self.x, self.y, self.z]
    }
}

/// A unit quaternion rotation in canonical space, stored `(x, y, z, w)`.
///
/// The component *order* is a typed-API convention; how a file orders its
/// four floats on disk is the parser's business (`cs_formats`), and how a
/// source's rotation sense maps onto it is `cs_content::coordinates`.
/// Positive rotations follow the right-hand rule in canonical coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Quaternion {
    x: f64,
    y: f64,
    z: f64,
    w: f64,
}

impl Quaternion {
    /// The identity rotation.
    pub const IDENTITY: Self = Self {
        x: 0.0,
        y: 0.0,
        z: 0.0,
        w: 1.0,
    };

    /// Validates a `(x, y, z, w)` rotation.
    ///
    /// # Errors
    ///
    /// [`SpaceError::NonFinite`] for a NaN/infinite component, otherwise
    /// [`SpaceError::NotUnit`] when the length differs from 1 by more than
    /// [`QUATERNION_LENGTH_TOLERANCE`].
    pub fn try_new(value: [f64; 4]) -> Result<Self, SpaceError> {
        const FIELDS: [&str; 4] = ["rotation.x", "rotation.y", "rotation.z", "rotation.w"];
        check_finite(value, FIELDS)?;
        let length =
            (value[0] * value[0] + value[1] * value[1] + value[2] * value[2] + value[3] * value[3])
                .sqrt();
        if (length - 1.0).abs() > QUATERNION_LENGTH_TOLERANCE {
            return Err(SpaceError::NotUnit { length });
        }
        Ok(Self {
            x: value[0],
            y: value[1],
            z: value[2],
            w: value[3],
        })
    }

    /// The rotation of `angle` about `axis`, right-hand rule in canonical
    /// coordinates.
    ///
    /// # Errors
    ///
    /// Whatever [`Quaternion::try_new`] reports for a non-finite angle; a
    /// valid axis and angle always produce a unit quaternion.
    pub fn from_axis_angle(axis: UnitVec3, angle: Radians) -> Result<Self, SpaceError> {
        let half = angle.0 * 0.5;
        let (sin, cos) = half.sin_cos();
        let [ax, ay, az] = axis.to_array();
        Self::try_new([ax * sin, ay * sin, az * sin, cos])
    }

    /// Rotates the direction or normal `value` by this rotation.
    ///
    /// # Errors
    ///
    /// [`SpaceError::NotUnit`] or [`SpaceError::NonFinite`] if the result is
    /// not a usable unit vector, which can only happen if this rotation was
    /// built from non-finite components outside [`Quaternion::try_new`].
    pub fn rotate(self, value: UnitVec3) -> Result<UnitVec3, SpaceError> {
        // v' = v + 2w(q_v × v) + 2q_v × (q_v × v)
        let [vx, vy, vz] = value.to_array();
        let cross = [
            self.y * vz - self.z * vy,
            self.z * vx - self.x * vz,
            self.x * vy - self.y * vx,
        ];
        let twice_w_cross = [
            2.0 * self.w * cross[0],
            2.0 * self.w * cross[1],
            2.0 * self.w * cross[2],
        ];
        let q_cross = [
            self.y * cross[2] - self.z * cross[1],
            self.z * cross[0] - self.x * cross[2],
            self.x * cross[1] - self.y * cross[0],
        ];
        UnitVec3::try_new([
            vx + twice_w_cross[0] + 2.0 * q_cross[0],
            vy + twice_w_cross[1] + 2.0 * q_cross[1],
            vz + twice_w_cross[2] + 2.0 * q_cross[2],
        ])
    }

    /// The four components as an array, in `(x, y, z, w)` order.
    #[must_use]
    pub const fn components(self) -> [f64; 4] {
        [self.x, self.y, self.z, self.w]
    }
}

/// Rejects the first non-finite value, naming the field the caller uses for
/// it. Every boundary in this crate reports input instead of repairing it.
fn check_finite<const N: usize>(
    values: [f64; N],
    fields: [&'static str; N],
) -> Result<(), SpaceError> {
    for (value, field) in values.into_iter().zip(fields) {
        if !value.is_finite() {
            return Err(SpaceError::NonFinite { field });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Boundary rejection (F16 non-negotiable behavior: reject non-finite
    /// input where it enters) for every typed value, plus the measured
    /// length a non-unit vector reports instead of being renormalized.
    #[test]
    fn accept_f16_a_space_values_reject_nonfinite_and_nonunit_inputs() {
        assert_eq!(
            WorldPosition::try_new([1.0, f64::NAN, 3.0]),
            Err(SpaceError::NonFinite { field: "world.y" })
        );
        assert_eq!(
            LocalPosition::try_new([f32::INFINITY, 0.0, 0.0]),
            Err(SpaceError::NonFinite { field: "local.x" })
        );
        assert_eq!(
            UnitVec3::try_new([0.0, 0.0, f64::NEG_INFINITY]),
            Err(SpaceError::NonFinite { field: "vector.z" })
        );
        assert_eq!(
            UnitVec3::try_new([0.0, 2.0, 0.0]),
            Err(SpaceError::NotUnit { length: 2.0 })
        );
        assert_eq!(
            Quaternion::try_new([0.0, 0.0, 0.0, f64::NAN]),
            Err(SpaceError::NonFinite {
                field: "rotation.w"
            })
        );
        assert_eq!(
            Quaternion::try_new([1.0, 1.0, 1.0, 1.0]),
            Err(SpaceError::NotUnit { length: 2.0 })
        );
        assert!(UnitVec3::try_new([0.0, 1.0, 0.0]).is_ok());
        assert!(Quaternion::IDENTITY.rotate(UnitVec3::UP).is_ok());
    }

    /// The canonical convention itself: right-handed, +Y up, forward -Z, so
    /// a +90° rotation about +Y takes +X to -Z, and +90° about +X takes +Y
    /// to +Z. `Winding::CANONICAL_FRONT` is CCW and `flipped` is an
    /// involution.
    #[test]
    fn accept_f16_a_canonical_rotation_is_right_handed_y_up_forward_negative_z() {
        let yaw = Quaternion::from_axis_angle(UnitVec3::UP, Radians(std::f64::consts::FRAC_PI_2))
            .expect("quarter turn is unit length");
        let turned = yaw
            .rotate(UnitVec3::try_new([1.0, 0.0, 0.0]).expect("unit"))
            .expect("rotated direction stays unit");
        let expected = [0.0, 0.0, -1.0];
        for (index, (actual, wanted)) in turned.to_array().into_iter().zip(expected).enumerate() {
            assert!(
                (actual - wanted).abs() < 1e-12,
                "right-handed +90° about +Y must map +X to -Z, component {index}: {actual} != {wanted}"
            );
        }

        let roll = Quaternion::from_axis_angle(
            UnitVec3::try_new([1.0, 0.0, 0.0]).expect("unit"),
            Radians(std::f64::consts::FRAC_PI_2),
        )
        .expect("quarter turn is unit length");
        let turned = roll
            .rotate(UnitVec3::try_new([0.0, 1.0, 0.0]).expect("unit"))
            .expect("rotated direction stays unit");
        assert!(
            turned.y().abs() < 1e-12 && (turned.z() - 1.0).abs() < 1e-12,
            "right-handed +90° about +X must map +Y to +Z, got {:?}",
            turned.to_array()
        );

        assert_eq!(Winding::CANONICAL_FRONT, Winding::CounterClockwise);
        assert_eq!(Winding::CounterClockwise.flipped(), Winding::Clockwise);
        assert_eq!(Winding::Clockwise.flipped().flipped(), Winding::Clockwise);
        assert_eq!(Winding::CounterClockwise.label(), "ccw");
        assert_eq!(Winding::Clockwise.label(), "cw");
    }
}
