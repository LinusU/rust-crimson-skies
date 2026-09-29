//! Source coordinate conventions and their adapters into canonical space
//! (F16-A).
//!
//! Spec: `specs/F16-coordinates-units-origin-management-and-clocks.md`,
//! stage `### F16-A` — "Verified per-format adapters map positions,
//! directions, normals, rotations, winding, distances and angles into that
//! convention exactly once." Shared contract:
//! `docs/contracts/FLIGHT-PHYSICS.md`.
//!
//! A [`SourceConvention`] is a *declaration*: which source axis feeds each
//! canonical axis (with its sign), the length scale, the angle unit, the
//! rotation sense and the front-face rule. [`SourceAdapter`] derives every
//! conversion from that one declaration, so a format is never described by
//! two independent hand-written conversion paths. Adapters are built with
//! [`SourceAdapter::new`] from a validated [`CoordinateSource`], which
//! carries the `Origin` and `Provenance` of the declaration itself.
//!
//! # What is measured and what is designed
//!
//! The three declared sources ([`CoordinateSource::declared`]) are **designed
//! declarations**, not measurements: an identity self-map plus two synthetic
//! fixtures that exercise every branch of the machinery. No original
//! handedness, axis order, scale or angle unit has been measured here, and
//! this module claims none — measuring them with three independent landmarks
//! (`F16` non-negotiable behavior 1) is F16-D. A test asserts that no
//! declared source claims `Origin::Installation`.
//!
//! # Derivations
//!
//! The axis map `M` is a signed permutation, hence orthogonal
//! (`M⁻¹ = Mᵀ`), so directions and normals transform with `M` itself and no
//! inverse-transpose is needed; the single positive `meters_per_unit` scale
//! never stretches an axis.
//!
//! A rotation's axis is an axial vector: mapping a rotation through `M`
//! keeps its angle and sends its axis to `det(M) · M · axis`, while the
//! scalar part is untouched. A source that declares the left-hand rule
//! (`RotationSense::LeftHandRule`) denotes `+θ` as a left-hand rotation, so
//! its vector part is negated once (turning that `+θ` into the right-hand
//! `−θ` in the same components) before the axis map runs. Every factor is
//! `±1` and its own inverse, which is what makes the round trip stable while
//! the absolute mapping stays pinned by an independent forward test.
//!
//! Winding has two separate answers that must agree:
//!
//! * [`SourceAdapter::winding_to_canonical`] flips the *label* exactly when
//!   the source declares clockwise front faces.
//! * [`SourceAdapter::reverses_vertex_order`] says whether a consumer must
//!   reverse vertex order when copying triangles. It is
//!   `(front CW) XOR (det(M) = −1) XOR (winding reference maps to −Z)`,
//!   because a triangle's cross product transforms as `det(M) · M · c` and
//!   the declared reference axis maps to `det(M) · sign · ẑ`.
//!
//! Together they guarantee that a front face is front (canonical
//! counter-clockwise) after conversion, whichever combination the source
//! declares.

use std::f64::consts::PI;

use cs_types::content::{Origin, Provenance};
use cs_types::evidence::ClaimId;
use cs_types::space::{Meters, Quaternion, Radians, SpaceError, UnitVec3, Winding, WorldPosition};

/// Declared round-trip tolerance for positions, in meters.
///
/// A position round trip is a signed permutation plus one multiply and one
/// divide by the same positive scale, so the error is a few ulps of the
/// f64 value; `1e-9` m is far above that for any coordinate this project
/// stores and far below any coordinate that would matter in play.
pub const POSITION_ROUND_TRIP_TOLERANCE_M: f64 = 1e-9;

/// Declared round-trip tolerance for distances, in meters. Same arithmetic
/// as a position's radial component, one scale less.
pub const DISTANCE_ROUND_TRIP_TOLERANCE_M: f64 = 1e-9;

/// Declared round-trip tolerance for directions and normals (unitless).
/// The axis map multiplies by ±1 only, so this bound only has to absorb the
/// unit-length acceptance check.
pub const DIRECTION_ROUND_TRIP_TOLERANCE: f64 = 1e-12;

/// Declared round-trip tolerance for rotations, in quaternion component
/// units (unitless). The conversion permutes, negates and keeps the scalar
/// part; no trigonometry is involved.
pub const ROTATION_ROUND_TRIP_TOLERANCE: f64 = 1e-12;

/// Declared round-trip tolerance for angles, in radians. Degrees → radians
/// → degrees is one multiply by `π/180` and one by `180/π` in f64.
pub const ANGLE_ROUND_TRIP_TOLERANCE_RAD: f64 = 1e-12;

/// A canonical axis: the three source axes are named in source space and
/// mapped onto these.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Axis {
    /// Source component 0.
    X,
    /// Source component 1.
    Y,
    /// Source component 2.
    Z,
}

impl Axis {
    /// Index of this axis in a source's component array.
    #[must_use]
    pub const fn index(self) -> usize {
        match self {
            Self::X => 0,
            Self::Y => 1,
            Self::Z => 2,
        }
    }

    /// One-letter label for diagnostics.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::X => "x",
            Self::Y => "y",
            Self::Z => "z",
        }
    }
}

/// Sign with which a source component enters a canonical axis.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Sign {
    /// The component is used unchanged.
    Positive,
    /// The component is negated.
    Negative,
}

impl Sign {
    /// `+1.0` or `-1.0`.
    #[must_use]
    pub const fn factor(self) -> f64 {
        match self {
            Self::Positive => 1.0,
            Self::Negative => -1.0,
        }
    }

    /// Whether this sign negates the component.
    #[must_use]
    pub const fn is_negative(self) -> bool {
        matches!(self, Self::Negative)
    }
}

/// One row of the source → canonical axis map: which source axis feeds a
/// canonical axis, and with which sign.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SourceAxis {
    /// The source component feeding this canonical axis.
    pub axis: Axis,
    /// Whether that component is negated.
    pub sign: Sign,
}

impl SourceAxis {
    /// The source axis, used unchanged.
    #[must_use]
    pub const fn positive(axis: Axis) -> Self {
        Self {
            axis,
            sign: Sign::Positive,
        }
    }

    /// The source axis, negated.
    #[must_use]
    pub const fn negative(axis: Axis) -> Self {
        Self {
            axis,
            sign: Sign::Negative,
        }
    }
}

/// The unit a source's scalar angles are stored in.
///
/// Which unit a file uses is a declared property of its convention, never an
/// assumption: an adapter converts through it exactly once, and a declaration
/// that guesses the wrong unit is a wrong declaration, not a harmless
/// default.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AngleUnit {
    /// Source angles are radians.
    Radians,
    /// Source angles are degrees.
    Degrees,
}

impl AngleUnit {
    /// Converts a source angle into canonical radians.
    #[must_use]
    pub fn to_radians(self, value: f64) -> f64 {
        match self {
            Self::Radians => value,
            Self::Degrees => value * (PI / 180.0),
        }
    }

    /// Converts canonical radians into the source's angle unit.
    #[must_use]
    pub fn from_radians(self, value: f64) -> f64 {
        match self {
            Self::Radians => value,
            Self::Degrees => value * (180.0 / PI),
        }
    }
}

/// Which rule a source uses for the sense of a positive rotation.
///
/// This is a declared property of the source, not an assumption: a source
/// that rotates `+θ` by the left-hand rule denotes the opposite rotation
/// from one that uses the right-hand rule, and the adapter converts between
/// them explicitly instead of hoping both mean the same thing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RotationSense {
    /// `+θ` about an axis follows the right-hand rule in source coordinates.
    RightHandRule,
    /// `+θ` about an axis follows the left-hand rule in source coordinates.
    LeftHandRule,
}

/// Why a source declaration or label was rejected.
#[derive(Clone, Debug, PartialEq)]
pub enum SourceError {
    /// The source label was empty.
    EmptyLabel,
    /// A source axis fed two canonical axes (or none), so the map is not a
    /// signed permutation and has no inverse.
    RepeatedSourceAxis {
        /// The axis that appeared more than once.
        axis: Axis,
        /// The canonical axis whose row collides with an earlier row.
        canonical: &'static str,
    },
    /// `meters_per_unit` was NaN or infinite.
    NonFiniteMetersPerUnit,
    /// `meters_per_unit` was zero or negative, so lengths could not be
    /// mapped back.
    NonPositiveMetersPerUnit {
        /// The rejected value.
        value: f64,
    },
    /// The declared winding reference axis is not the source axis that
    /// feeds canonical Z, so a planar source triangle's winding has no
    /// canonical counterpart.
    WindingReferenceAxisMismatch {
        /// The axis the declaration names.
        declared: Axis,
        /// The axis that actually feeds canonical Z.
        feeds_canonical_z: Axis,
    },
}

impl std::fmt::Display for SourceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyLabel => write!(f, "coordinate source label must not be empty"),
            Self::RepeatedSourceAxis { axis, canonical } => write!(
                f,
                "source axis {} feeds canonical {canonical} more than once",
                axis.label()
            ),
            Self::NonFiniteMetersPerUnit => write!(f, "meters_per_unit must be finite"),
            Self::NonPositiveMetersPerUnit { value } => {
                write!(f, "meters_per_unit must be greater than zero, got {value}")
            }
            Self::WindingReferenceAxisMismatch {
                declared,
                feeds_canonical_z,
            } => write!(
                f,
                "winding reference axis {} does not feed canonical z ({} does)",
                declared.label(),
                feeds_canonical_z.label()
            ),
        }
    }
}

impl std::error::Error for SourceError {}

/// A validated declaration of one source's coordinate convention.
#[derive(Clone, Debug, PartialEq)]
pub struct SourceConvention {
    axes: [SourceAxis; 3],
    winding_reference: Axis,
    meters_per_unit: f64,
    angle_unit: AngleUnit,
    rotation_sense: RotationSense,
    front_face: Winding,
}

impl SourceConvention {
    /// Validates a declaration.
    ///
    /// `axes[i]` is the source axis feeding canonical X (`i = 0`), Y and Z.
    /// `winding_reference` is the axis the viewer sits on: front faces wind
    /// counter-clockwise when seen from `+winding_reference` looking toward
    /// the origin.
    ///
    /// # Errors
    ///
    /// [`SourceError::RepeatedSourceAxis`] when the map is not a
    /// permutation, [`SourceError::NonFiniteMetersPerUnit`] /
    /// [`SourceError::NonPositiveMetersPerUnit`] for the length scale, and
    /// [`SourceError::WindingReferenceAxisMismatch`] when the winding
    /// reference is not the source axis feeding canonical Z.
    pub fn new(
        axes: [SourceAxis; 3],
        winding_reference: Axis,
        meters_per_unit: f64,
        angle_unit: AngleUnit,
        rotation_sense: RotationSense,
        front_face: Winding,
    ) -> Result<Self, SourceError> {
        const CANONICAL: [&str; 3] = ["x", "y", "z"];
        let mut seen = [false; 3];
        for (index, row) in axes.iter().enumerate() {
            let slot = row.axis.index();
            if seen[slot] {
                return Err(SourceError::RepeatedSourceAxis {
                    axis: row.axis,
                    canonical: CANONICAL[index],
                });
            }
            seen[slot] = true;
        }
        if !meters_per_unit.is_finite() {
            return Err(SourceError::NonFiniteMetersPerUnit);
        }
        if meters_per_unit <= 0.0 {
            return Err(SourceError::NonPositiveMetersPerUnit {
                value: meters_per_unit,
            });
        }
        if winding_reference != axes[2].axis {
            return Err(SourceError::WindingReferenceAxisMismatch {
                declared: winding_reference,
                feeds_canonical_z: axes[2].axis,
            });
        }
        Ok(Self {
            axes,
            winding_reference,
            meters_per_unit,
            angle_unit,
            rotation_sense,
            front_face,
        })
    }

    /// The source → canonical axis map, canonical X first.
    #[must_use]
    pub const fn axes(&self) -> [SourceAxis; 3] {
        self.axes
    }

    /// The source axis the viewer sits on for winding purposes.
    #[must_use]
    pub const fn winding_reference(&self) -> Axis {
        self.winding_reference
    }

    /// Meters per source unit; finite and strictly positive.
    #[must_use]
    pub const fn meters_per_unit(&self) -> f64 {
        self.meters_per_unit
    }

    /// The unit source angles are stored in.
    #[must_use]
    pub const fn angle_unit(&self) -> AngleUnit {
        self.angle_unit
    }

    /// The rule a positive rotation follows in source coordinates.
    #[must_use]
    pub const fn rotation_sense(&self) -> RotationSense {
        self.rotation_sense
    }

    /// Which winding designates front faces in source coordinates, measured
    /// against [`winding_reference`](Self::winding_reference).
    #[must_use]
    pub const fn front_face(&self) -> Winding {
        self.front_face
    }

    /// Whether the axis map preserves orientation (`det(M) = +1`).
    ///
    /// An improper map mirrors the source: triangle winding flips and a
    /// rotation's axis is transformed as an axial vector.
    #[must_use]
    pub fn is_orientation_preserving(&self) -> bool {
        // A signed permutation's determinant is the permutation's parity
        // times the product of its signs.
        let mut inversions = 0usize;
        let indices = [
            self.axes[0].axis.index(),
            self.axes[1].axis.index(),
            self.axes[2].axis.index(),
        ];
        for first in 0..3 {
            for second in (first + 1)..3 {
                if indices[first] > indices[second] {
                    inversions += 1;
                }
            }
        }
        let parity_even = inversions.is_multiple_of(2);
        // The sign *product* is +1 exactly when an even number of rows is
        // negated: mirroring two axes leaves the map proper, while mirroring
        // one or three mirrors it.
        let negated_rows = self
            .axes
            .iter()
            .filter(|row| row.sign.is_negative())
            .count();
        let sign_product_positive = negated_rows.is_multiple_of(2);
        parity_even == sign_product_positive
    }

    /// Whether a consumer must reverse triangle vertex order when copying
    /// geometry through this convention, so that front faces stay canonical
    /// counter-clockwise.
    ///
    /// See the module documentation for the derivation.
    #[must_use]
    pub fn reverses_vertex_order(&self) -> bool {
        let front_is_cw = self.front_face == Winding::Clockwise;
        let improper = !self.is_orientation_preserving();
        let depth_negated = self.axes[2].sign.is_negative();
        front_is_cw != (improper != depth_negated)
    }
}

/// A source's coordinate convention together with where the declaration
/// itself came from.
///
/// `origin` and `provenance` come from the F14-A records
/// (`cs_types::content`), so a declaration can never be mistaken for a
/// measurement: `Origin::Installation` is reserved for a convention backed
/// by a source span, which is F16-D's work.
#[derive(Clone, Debug, PartialEq)]
pub struct CoordinateSource {
    label: String,
    convention: SourceConvention,
    origin: Origin,
    provenance: Provenance,
}

impl CoordinateSource {
    /// Validates a source declaration together with its identity.
    ///
    /// # Errors
    ///
    /// [`SourceError::EmptyLabel`], or anything
    /// [`SourceConvention::new`] reports.
    pub fn new(
        label: impl Into<String>,
        convention: SourceConvention,
        origin: Origin,
        provenance: Provenance,
    ) -> Result<Self, SourceError> {
        let label = label.into();
        if label.is_empty() {
            return Err(SourceError::EmptyLabel);
        }
        Ok(Self {
            label,
            convention,
            origin,
            provenance,
        })
    }

    /// The source's stable label, e.g. `fixture.z-up-right-handed-degrees`.
    #[must_use]
    pub fn label(&self) -> &str {
        &self.label
    }

    /// The declared convention.
    #[must_use]
    pub const fn convention(&self) -> &SourceConvention {
        &self.convention
    }

    /// Where the declaration came from.
    #[must_use]
    pub fn origin(&self) -> &Origin {
        &self.origin
    }

    /// The claim this declaration backs.
    #[must_use]
    pub const fn provenance(&self) -> &Provenance {
        &self.provenance
    }

    /// Every source declared at F16-A.
    ///
    /// These are designed declarations: an identity self-map plus two
    /// synthetic fixtures that together exercise proper and improper axis
    /// maps, both rotation senses, both angle units, a scale other than one
    /// and both front-face labels. None is attributed to an original file.
    pub fn declared() -> Vec<Self> {
        vec![
            Self::canonical(),
            Self::z_up_right_handed_degrees_fixture(),
            Self::left_handed_z_up_centimeters_fixture(),
        ]
    }

    /// The canonical convention itself, as a source: mapping through it is
    /// the identity.
    fn canonical() -> Self {
        Self::build(
            "canonical",
            SourceConvention::new(
                [
                    SourceAxis::positive(Axis::X),
                    SourceAxis::positive(Axis::Y),
                    SourceAxis::positive(Axis::Z),
                ],
                Axis::Z,
                1.0,
                AngleUnit::Radians,
                RotationSense::RightHandRule,
                Winding::CounterClockwise,
            ),
            Origin::Designed,
            "f16a.source.canonical",
        )
    }

    /// Synthetic fixture: right-handed, Z up, Y forward, degrees.
    ///
    /// Newly authored fixture content (`Origin::SyntheticFixture`); it is
    /// never a stand-in for a missing original convention.
    fn z_up_right_handed_degrees_fixture() -> Self {
        Self::build(
            "fixture.z-up-right-handed-degrees",
            SourceConvention::new(
                [
                    SourceAxis::positive(Axis::X),
                    SourceAxis::positive(Axis::Z),
                    SourceAxis::negative(Axis::Y),
                ],
                Axis::Y,
                1.0,
                AngleUnit::Degrees,
                RotationSense::RightHandRule,
                Winding::CounterClockwise,
            ),
            Origin::SyntheticFixture,
            "f16a.source.z-up-fixture",
        )
    }

    /// Synthetic fixture: left-handed, X forward, Y right, Z up, stored in
    /// centimeters and degrees, clockwise front faces and left-hand-rule
    /// rotations.
    ///
    /// Newly authored fixture content (`Origin::SyntheticFixture`); it
    /// exercises the improper (mirroring) axis map and the declared
    /// left-hand rotation sense.
    fn left_handed_z_up_centimeters_fixture() -> Self {
        Self::build(
            "fixture.left-handed-z-up-centimeters-degrees",
            SourceConvention::new(
                [
                    SourceAxis::positive(Axis::Y),
                    SourceAxis::positive(Axis::Z),
                    SourceAxis::negative(Axis::X),
                ],
                Axis::X,
                0.01,
                AngleUnit::Degrees,
                RotationSense::LeftHandRule,
                Winding::Clockwise,
            ),
            Origin::SyntheticFixture,
            "f16a.source.left-handed-z-up-fixture",
        )
    }

    /// Builds a declared source; the declared fixtures are constants whose
    /// validity the `accept_f16_a_` declaration tests re-check on every run.
    fn build(
        label: &str,
        convention: Result<SourceConvention, SourceError>,
        origin: Origin,
        claim: &str,
    ) -> Self {
        let convention = convention.expect("declared source convention is valid");
        let provenance =
            Provenance::designed(ClaimId::new(claim).expect("declared claim id is valid"));
        Self::new(label, convention, origin, provenance)
            .expect("declared coordinate source is valid")
    }
}

/// Converts values from one declared source convention into canonical space,
/// and back, exactly once.
///
/// Every method derives from the source's [`SourceConvention`]; there is no
/// per-format conversion code. Inputs are validated at the boundary:
/// non-finite numbers are refused by name and non-unit normals or rotations
/// are refused with their measured length.
#[derive(Clone, Debug, PartialEq)]
pub struct SourceAdapter {
    source: CoordinateSource,
    orientation: f64,
    rotation_sense: f64,
    meters_per_unit: f64,
    unit_scale: f64,
}

const POSITION_FIELDS: [&str; 3] = ["position[0]", "position[1]", "position[2]"];
const DIRECTION_FIELDS: [&str; 3] = ["direction[0]", "direction[1]", "direction[2]"];
const NORMAL_FIELDS: [&str; 3] = ["normal[0]", "normal[1]", "normal[2]"];

impl SourceAdapter {
    /// Builds the adapter for a validated source.
    ///
    /// The source was validated when it was constructed, so this cannot
    /// fail: the orientation sign, rotation sense and inverse scale are
    /// derived, never configured a second time.
    #[must_use]
    pub fn new(source: CoordinateSource) -> Self {
        let convention = source.convention();
        let orientation = if convention.is_orientation_preserving() {
            1.0
        } else {
            -1.0
        };
        let rotation_sense = match convention.rotation_sense() {
            RotationSense::RightHandRule => 1.0,
            RotationSense::LeftHandRule => -1.0,
        };
        let meters_per_unit = convention.meters_per_unit();
        Self {
            unit_scale: 1.0 / meters_per_unit,
            meters_per_unit,
            orientation,
            rotation_sense,
            source,
        }
    }

    /// Every declared source, as adapters. See
    /// [`CoordinateSource::declared`].
    pub fn declared() -> Vec<Self> {
        CoordinateSource::declared()
            .into_iter()
            .map(Self::new)
            .collect()
    }

    /// The source this adapter converts.
    #[must_use]
    pub const fn source(&self) -> &CoordinateSource {
        &self.source
    }

    /// Whether triangle vertex order must be reversed when copying geometry
    /// through this source. See
    /// [`SourceConvention::reverses_vertex_order`].
    #[must_use]
    pub fn reverses_vertex_order(&self) -> bool {
        self.source.convention().reverses_vertex_order()
    }

    /// Maps a source vector through the axis map (no scale).
    fn map(&self, value: [f64; 3]) -> [f64; 3] {
        let axes = self.source.convention().axes();
        [
            axes[0].sign.factor() * value[axes[0].axis.index()],
            axes[1].sign.factor() * value[axes[1].axis.index()],
            axes[2].sign.factor() * value[axes[2].axis.index()],
        ]
    }

    /// Maps a canonical vector back through `Mᵀ`.
    fn unmap(&self, value: [f64; 3]) -> [f64; 3] {
        let axes = self.source.convention().axes();
        let mut out = [0.0; 3];
        for (index, row) in axes.iter().enumerate() {
            out[row.axis.index()] = row.sign.factor() * value[index];
        }
        out
    }

    /// Source position → canonical world position, in meters.
    ///
    /// # Errors
    ///
    /// [`SpaceError::NonFinite`] naming the offending source component.
    pub fn position_to_canonical(&self, value: [f64; 3]) -> Result<WorldPosition, SpaceError> {
        check_finite(value, POSITION_FIELDS)?;
        let mapped = self.map(value);
        WorldPosition::try_new([
            mapped[0] * self.meters_per_unit,
            mapped[1] * self.meters_per_unit,
            mapped[2] * self.meters_per_unit,
        ])
    }

    /// Canonical world position → source position, in source units.
    #[must_use]
    pub fn position_from_canonical(&self, value: WorldPosition) -> [f64; 3] {
        let [x, y, z] = self.unmap(value.to_array());
        [
            x * self.unit_scale,
            y * self.unit_scale,
            z * self.unit_scale,
        ]
    }

    /// Source direction → canonical direction. Length is unchanged: the axis
    /// map is a signed permutation and the scale is a single positive
    /// number, so directions are not scaled.
    ///
    /// # Errors
    ///
    /// [`SpaceError::NonFinite`] for a non-finite component, otherwise
    /// [`SpaceError::NotUnit`] if the source direction was not unit length.
    pub fn direction_to_canonical(&self, value: [f64; 3]) -> Result<UnitVec3, SpaceError> {
        check_finite(value, DIRECTION_FIELDS)?;
        UnitVec3::try_new(self.map(value))
    }

    /// Canonical direction → source direction.
    #[must_use]
    pub fn direction_from_canonical(&self, value: UnitVec3) -> [f64; 3] {
        self.unmap(value.to_array())
    }

    /// Source surface normal → canonical normal. The same linear map as a
    /// direction, under the single-scale rule above; the word "normal" is
    /// kept in the API because the deliverable names both.
    ///
    /// # Errors
    ///
    /// [`SpaceError::NonFinite`] for a non-finite component, otherwise
    /// [`SpaceError::NotUnit`] if the source normal was not unit length.
    pub fn normal_to_canonical(&self, value: [f64; 3]) -> Result<UnitVec3, SpaceError> {
        check_finite(value, NORMAL_FIELDS)?;
        UnitVec3::try_new(self.map(value))
    }

    /// Canonical normal → source normal.
    #[must_use]
    pub fn normal_from_canonical(&self, value: UnitVec3) -> [f64; 3] {
        self.unmap(value.to_array())
    }

    /// Source rotation → canonical rotation, honouring the declared rotation
    /// sense and the map's orientation (see the module documentation).
    ///
    /// # Errors
    ///
    /// [`SpaceError::NonFinite`] naming the offending component, otherwise
    /// [`SpaceError::NotUnit`] if the source rotation was not unit length.
    pub fn rotation_to_canonical(&self, value: [f64; 4]) -> Result<[f64; 4], SpaceError> {
        // One validation path: the canonical type's own constructor, which
        // reports the offending component or the measured length.
        let rotation = Quaternion::try_new(value)?;
        let [x, y, z, w] = rotation.components();
        let sensed = [
            self.rotation_sense * x,
            self.rotation_sense * y,
            self.rotation_sense * z,
        ];
        let mapped = self.map(sensed);
        Ok([
            self.orientation * mapped[0],
            self.orientation * mapped[1],
            self.orientation * mapped[2],
            w,
        ])
    }

    /// Canonical rotation → source rotation. The canonical side is typed, so
    /// it is validated by construction; the returned source components are
    /// the inverse of [`Self::rotation_to_canonical`].
    #[must_use]
    pub fn rotation_from_canonical(&self, value: Quaternion) -> [f64; 4] {
        let [x, y, z, w] = value.components();
        let unmapped = self.unmap([x, y, z]);
        let factor = self.orientation * self.rotation_sense;
        [
            factor * unmapped[0],
            factor * unmapped[1],
            factor * unmapped[2],
            w,
        ]
    }

    /// Source distance → canonical meters.
    ///
    /// # Errors
    ///
    /// [`SpaceError::NonFinite`] for the field `distance`.
    pub fn distance_to_canonical(&self, value: f64) -> Result<Meters, SpaceError> {
        check_finite([value], ["distance"])?;
        Ok(Meters(value * self.meters_per_unit))
    }

    /// Canonical meters → source distance.
    #[must_use]
    pub fn distance_from_canonical(&self, value: Meters) -> f64 {
        value.0 * self.unit_scale
    }

    /// Source angle → canonical radians.
    ///
    /// # Errors
    ///
    /// [`SpaceError::NonFinite`] for the field `angle`.
    pub fn angle_to_canonical(&self, value: f64) -> Result<Radians, SpaceError> {
        check_finite([value], ["angle"])?;
        Ok(Radians(
            self.source.convention().angle_unit().to_radians(value),
        ))
    }

    /// Canonical radians → source angle unit.
    #[must_use]
    pub fn angle_from_canonical(&self, value: Radians) -> f64 {
        self.source.convention().angle_unit().from_radians(value.0)
    }

    /// Source winding label → canonical winding label. Flips exactly when
    /// the source declares clockwise front faces; the mirroring of the axis
    /// map is carried by [`Self::reverses_vertex_order`] instead, so label
    /// and vertex order cannot double-flip.
    #[must_use]
    pub fn winding_to_canonical(&self, value: Winding) -> Winding {
        if self.source.convention().front_face() == Winding::Clockwise {
            value.flipped()
        } else {
            value
        }
    }

    /// Canonical winding label → source winding label (the same flip; the
    /// mapping is its own inverse).
    #[must_use]
    pub fn winding_from_canonical(&self, value: Winding) -> Winding {
        self.winding_to_canonical(value)
    }
}

/// Rejects the first non-finite input component, naming it the way the
/// caller spelled it, before any conversion runs.
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

    /// Every declaration rule has a failure case: a non-permutation axis
    /// map, a zero, negative and non-finite scale, a winding reference that
    /// does not feed canonical Z, and an empty label.
    #[test]
    fn accept_f16_a_invalid_source_declarations_are_refused() {
        let good = [
            SourceAxis::positive(Axis::X),
            SourceAxis::positive(Axis::Y),
            SourceAxis::positive(Axis::Z),
        ];

        let repeated = SourceConvention::new(
            [
                SourceAxis::positive(Axis::X),
                SourceAxis::positive(Axis::X),
                SourceAxis::positive(Axis::Z),
            ],
            Axis::Z,
            1.0,
            AngleUnit::Radians,
            RotationSense::RightHandRule,
            Winding::CounterClockwise,
        );
        assert_eq!(
            repeated,
            Err(SourceError::RepeatedSourceAxis {
                axis: Axis::X,
                canonical: "y"
            })
        );

        for (scale, expected) in [
            (0.0, SourceError::NonPositiveMetersPerUnit { value: 0.0 }),
            (
                -0.01,
                SourceError::NonPositiveMetersPerUnit { value: -0.01 },
            ),
            (f64::NAN, SourceError::NonFiniteMetersPerUnit),
            (f64::INFINITY, SourceError::NonFiniteMetersPerUnit),
        ] {
            let result = SourceConvention::new(
                good,
                Axis::Z,
                scale,
                AngleUnit::Radians,
                RotationSense::RightHandRule,
                Winding::CounterClockwise,
            );
            assert_eq!(result, Err(expected), "scale {scale} must be refused");
        }

        let wrong_reference = SourceConvention::new(
            good,
            Axis::X,
            1.0,
            AngleUnit::Radians,
            RotationSense::RightHandRule,
            Winding::CounterClockwise,
        );
        assert_eq!(
            wrong_reference,
            Err(SourceError::WindingReferenceAxisMismatch {
                declared: Axis::X,
                feeds_canonical_z: Axis::Z
            })
        );

        let convention = SourceConvention::new(
            good,
            Axis::Z,
            1.0,
            AngleUnit::Radians,
            RotationSense::RightHandRule,
            Winding::CounterClockwise,
        )
        .expect("valid declaration");
        assert_eq!(
            CoordinateSource::new(
                "",
                convention,
                Origin::Designed,
                Provenance::designed(ClaimId::new("f16a.test.empty").expect("claim id"))
            ),
            Err(SourceError::EmptyLabel)
        );
    }
}
