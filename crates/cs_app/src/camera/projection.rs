//! The lowered projection policy and the framing math (F21-A).
//!
//! Spec: `specs/F21-cameras-cockpit-views-and-spyglass.md`, stage
//! `### F21-A`, non-negotiable behavior 2. Shared contract:
//! `docs/contracts/UI-NETWORK.md`.
//!
//! The declared [`ProjectionPolicy`] becomes a [`LoweredProjection`] here —
//! the application boundary, the same shape as `cs_app::environment`'s
//! records or `cs_app::targeting::lower_rules`. Every [`Resolved::Unknown`]
//! **refuses** rather than guessing a field of view, an axis, a reference
//! aspect, a framing rule or a clipping plane, and a declared
//! [`AspectFraming::Stretch`] is refused because F21 non-negotiable behavior
//! 2 forbids stretching art.
//!
//! # Vertical vs horizontal
//!
//! An authored FOV is only meaningful together with the axis it was authored
//! on and the aspect it was authored at. [`lower_projection`] normalizes
//! every declaration to the **vertical** field of view at the declared
//! reference aspect:
//!
//! ```text
//! vertical_from_horizontal(h, a) = 2·atan(tan(h/2) / a)
//! horizontal_from_vertical(v, a) = 2·atan(tan(v/2) · a)
//! ```
//!
//! The two are inverses, which the acceptance tests check by round-tripping
//! a horizontal declaration back through [`LoweredProjection::horizontal_fov_at`].
//!
//! # Aspect-correct framing
//!
//! * [`AspectFraming::PreserveVertical`]: the **vertical** FOV is the
//!   authored one at every aspect, and the horizontal FOV grows with the
//!   aspect. The *same* world target therefore keeps its vertical viewport
//!   coordinate at 4:3, 16:9 and ultrawide while more world is revealed to
//!   the sides.
//! * [`AspectFraming::PreserveHorizontal`]: the **horizontal** FOV is the
//!   authored one at every aspect, and the vertical FOV shrinks as the
//!   viewport widens. The sides stay framed and less world is revealed
//!   above and below.
//!
//! Neither rule changes the pixel aspect, so art is never stretched; the
//! acceptance test `accept_f21_a_framing_at_three_aspect_ratios_...` measures
//! exactly that.

use std::cmp::Ordering;
use std::f64::consts::PI;
use std::fmt;

use cs_content::cameras::{AspectFraming, AspectRatio, FovAxis, ProjectionPolicy};
use cs_types::content::Resolved;
use cs_types::evidence::ClaimId;
use cs_types::space::{Meters, Radians, WorldPosition};

use super::pose::{CameraBasis, Framing, FramingError};

/// Why a declared projection could not be lowered.
#[derive(Clone, Debug, PartialEq)]
pub enum ProjectionLowerError {
    /// A policy field is `Resolved::Unknown`: no renderer may run under a
    /// guessed field of view, axis, reference aspect or framing rule.
    UnknownField {
        /// Which policy field is unknown.
        field: &'static str,
        /// The claim the unknown is recorded under.
        claim_id: ClaimId,
        /// Why the value is unknown.
        reason: String,
    },
    /// A known numeric field was NaN or infinite.
    NonFiniteField {
        /// Which policy field it was.
        field: &'static str,
    },
    /// The known field of view fell outside `(0, π)`.
    FovOutOfRange {
        /// The rejected angle in radians.
        radians: f64,
    },
    /// The known near clipping plane was zero or negative.
    NonPositiveNear {
        /// The rejected distance in meters.
        meters: f64,
    },
    /// The known near plane was not in front of the known far plane.
    ClippingNotOrdered {
        /// The rejected near distance.
        near_m: f64,
        /// The rejected far distance.
        far_m: f64,
    },
    /// The declared framing rule stretches the image, which F21
    /// non-negotiable behavior 2 forbids.
    StretchFraming,
}

impl fmt::Display for ProjectionLowerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownField {
                field,
                claim_id,
                reason,
            } => write!(
                f,
                "projection field {field} is unknown ({}: {reason}) and cannot be lowered",
                claim_id.as_str()
            ),
            Self::NonFiniteField { field } => write!(f, "projection field {field} must be finite"),
            Self::FovOutOfRange { radians } => {
                write!(f, "the field of view {radians} rad is outside (0, π)")
            }
            Self::NonPositiveNear { meters } => {
                write!(f, "the near plane must be positive, got {meters} m")
            }
            Self::ClippingNotOrdered { near_m, far_m } => write!(
                f,
                "the near plane ({near_m} m) must be closer than the far plane ({far_m} m)"
            ),
            Self::StretchFraming => write!(
                f,
                "a stretch framing rule is forbidden: it would distort authored art"
            ),
        }
    }
}

impl std::error::Error for ProjectionLowerError {}

fn lower_field<T>(field: &'static str, value: &Resolved<T>) -> Result<T, ProjectionLowerError>
where
    T: Clone,
{
    match value {
        Resolved::Known(known) => Ok(known.value.clone()),
        Resolved::Unknown { claim_id, reason } => Err(ProjectionLowerError::UnknownField {
            field,
            claim_id: claim_id.clone(),
            reason: reason.clone(),
        }),
    }
}

/// The vertical field of view equivalent to `horizontal` at `aspect`.
fn vertical_from_horizontal(horizontal: Radians, aspect: AspectRatio) -> Radians {
    Radians(2.0 * ((horizontal.0 * 0.5).tan() / aspect.value()).atan())
}

/// The horizontal field of view equivalent to `vertical` at `aspect`.
fn horizontal_from_vertical(vertical: Radians, aspect: AspectRatio) -> Radians {
    Radians(2.0 * ((vertical.0 * 0.5).tan() * aspect.value()).atan())
}

/// A declared projection policy as a renderer consumes it: one canonical
/// vertical field of view at a reference aspect, plus the framing rule and
/// clipping planes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LoweredProjection {
    vertical_fov: Radians,
    reference_aspect: AspectRatio,
    framing: AspectFraming,
    near_m: Meters,
    far_m: Meters,
}

impl LoweredProjection {
    /// The vertical field of view at [`Self::reference_aspect`], in radians.
    #[must_use]
    pub const fn vertical_fov(self) -> Radians {
        self.vertical_fov
    }

    /// The aspect the authored field of view was declared at.
    #[must_use]
    pub const fn reference_aspect(self) -> AspectRatio {
        self.reference_aspect
    }

    /// The declared framing rule (never [`AspectFraming::Stretch`]).
    #[must_use]
    pub const fn framing_policy(self) -> AspectFraming {
        self.framing
    }

    /// The near clipping distance.
    #[must_use]
    pub const fn near_m(self) -> Meters {
        self.near_m
    }

    /// The far clipping distance.
    #[must_use]
    pub const fn far_m(self) -> Meters {
        self.far_m
    }

    /// The vertical field of view at `aspect`.
    ///
    /// Constant in `aspect` for [`AspectFraming::PreserveVertical`] (the
    /// authored vertical extent is preserved); shrunk as the viewport widens
    /// for [`AspectFraming::PreserveHorizontal`].
    #[must_use]
    pub fn vertical_fov_at(self, aspect: AspectRatio) -> Radians {
        match self.framing {
            AspectFraming::PreserveVertical => self.vertical_fov,
            AspectFraming::PreserveHorizontal => vertical_from_horizontal(
                horizontal_from_vertical(self.vertical_fov, self.reference_aspect),
                aspect,
            ),
            // Lowering refuses this; the arm exists only so the match is
            // total. Returning the authored value invents no framing.
            AspectFraming::Stretch => self.vertical_fov,
        }
    }

    /// The horizontal field of view at `aspect`.
    ///
    /// Constant in `aspect` for [`AspectFraming::PreserveHorizontal`]; grown
    /// as the viewport widens for [`AspectFraming::PreserveVertical`].
    #[must_use]
    pub fn horizontal_fov_at(self, aspect: AspectRatio) -> Radians {
        match self.framing {
            AspectFraming::PreserveVertical => horizontal_from_vertical(self.vertical_fov, aspect),
            AspectFraming::PreserveHorizontal => {
                horizontal_from_vertical(self.vertical_fov, self.reference_aspect)
            }
            AspectFraming::Stretch => {
                horizontal_from_vertical(self.vertical_fov, self.reference_aspect)
            }
        }
    }

    /// Frames a world-space point through the camera basis at `aspect`.
    ///
    /// The point is projected with the frustum's tangents, so the returned
    /// coordinates are independent of distance: two points on one ray frame
    /// identically.
    ///
    /// # Errors
    ///
    /// [`FramingError::BehindCamera`] when the point has no component along
    /// the camera's forward axis.
    pub fn framing_of(
        self,
        aspect: AspectRatio,
        basis: CameraBasis,
        target: WorldPosition,
    ) -> Result<Framing, FramingError> {
        let [tx, ty, tz] = target.to_array();
        let [px, py, pz] = basis.position().to_array();
        let delta = [tx - px, ty - py, tz - pz];

        let forward = basis.forward().to_array();
        let depth = dot(delta, forward);
        if depth.partial_cmp(&0.0) != Some(Ordering::Greater) {
            return Err(FramingError::BehindCamera);
        }

        let right = basis.right().to_array();
        let up = basis.up().to_array();
        let x_component = dot(delta, right);
        let y_component = dot(delta, up);

        let vertical_half = self.vertical_fov_at(aspect).0 * 0.5;
        let horizontal_half = self.horizontal_fov_at(aspect).0 * 0.5;
        Ok(Framing::new(
            (x_component / depth) / horizontal_half.tan(),
            (y_component / depth) / vertical_half.tan(),
        ))
    }
}

fn dot(first: [f64; 3], second: [f64; 3]) -> f64 {
    first[0] * second[0] + first[1] * second[1] + first[2] * second[2]
}

/// Lowers a declared projection policy into the record a renderer consumes.
///
/// The authored field of view is normalized to the vertical axis at the
/// declared reference aspect, the framing rule is checked to be
/// aspect-correct, and every unknown refuses.
///
/// # Errors
///
/// [`ProjectionLowerError`] for any `Resolved::Unknown` field, a corrupt
/// known value or a declared [`AspectFraming::Stretch`].
pub fn lower_projection(
    policy: &ProjectionPolicy,
) -> Result<LoweredProjection, ProjectionLowerError> {
    let fov = lower_field("fov", &policy.fov)?;
    let axis = lower_field("fov_axis", &policy.fov_axis)?;
    let reference_aspect = lower_field("reference_aspect", &policy.reference_aspect)?;
    let framing = lower_field("framing", &policy.framing)?;
    let near_m = lower_field("near_m", &policy.near_m)?;
    let far_m = lower_field("far_m", &policy.far_m)?;

    if !framing.is_aspect_correct() {
        return Err(ProjectionLowerError::StretchFraming);
    }

    if !fov.0.is_finite() {
        return Err(ProjectionLowerError::NonFiniteField { field: "fov" });
    }
    if !(fov.0 > 0.0 && fov.0 < PI) {
        return Err(ProjectionLowerError::FovOutOfRange { radians: fov.0 });
    }
    if !near_m.0.is_finite() {
        return Err(ProjectionLowerError::NonFiniteField { field: "near_m" });
    }
    if near_m.0 <= 0.0 {
        return Err(ProjectionLowerError::NonPositiveNear { meters: near_m.0 });
    }
    if !far_m.0.is_finite() {
        return Err(ProjectionLowerError::NonFiniteField { field: "far_m" });
    }
    if near_m.0.partial_cmp(&far_m.0) != Some(Ordering::Less) {
        return Err(ProjectionLowerError::ClippingNotOrdered {
            near_m: near_m.0,
            far_m: far_m.0,
        });
    }

    let vertical_fov = match axis {
        FovAxis::Vertical => fov,
        FovAxis::Horizontal => vertical_from_horizontal(fov, reference_aspect),
    };

    Ok(LoweredProjection {
        vertical_fov,
        reference_aspect,
        framing,
        near_m,
        far_m,
    })
}
