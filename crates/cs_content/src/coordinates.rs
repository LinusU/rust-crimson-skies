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
//! this module claims none. A test asserts that no declared source claims
//! `Origin::Installation`, and [`CoordinateSource::calibration`] hands every
//! one of them an empty [`UnitCalibration`] whose
//! [`claim_status`](UnitCalibration::claim_status) is
//! [`ClaimStatus::Unknown`].
//!
//! # F16-D: the calibration rule, checkable instead of remembered
//!
//! Non-negotiable behavior 1 — "Measure original scale, handedness, axis
//! order and angle units using at least three independent landmarks/behaviors.
//! A Blender transform is insufficient proof." — is a rule about *evidence*,
//! so it is represented as evidence. [`UnitCalibration`] keeps one
//! [`Landmark`] list per [`CalibratedQuantity`], refuses a repeated
//! description or a reused observation, requires at least
//! [`UnitCalibration::MIN_LANDMARKS`] independent landmarks **and one
//! observed [`LandmarkKind::Behavior`]** per quantity, and reports both the
//! remaining [`gaps`](UnitCalibration::gaps) and the strongest
//! [`claim_status`](UnitCalibration::claim_status) the recorded evidence
//! supports. Nothing in this module can manufacture a measurement: only an
//! [`EvidenceRecord`] that itself
//! [`verifies_original`](EvidenceRecord::verifies_original) can raise the
//! claim, and no such record exists in this tree.
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

use cs_types::asset_id::SourceSpan;
use cs_types::content::{Origin, Provenance};
use cs_types::evidence::{
    ClaimId, ClaimStatus, EvidenceRecord, EvidenceSource, ObservationLocator, ObservationMethod,
};
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
    landmarks: Vec<Landmark>,
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
            landmarks: Vec::new(),
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

    /// Records a landmark against this source's [`UnitCalibration`].
    ///
    /// The independence check runs through the calibration itself — the same
    /// rule a free-standing record applies — so a landmark that reuses another
    /// one's observation is refused here exactly as it would be there.
    ///
    /// # Errors
    ///
    /// [`CalibrationError::RepeatedDescription`] or
    /// [`CalibrationError::RepeatedObservation`].
    pub fn record_landmark(&mut self, landmark: Landmark) -> Result<(), CalibrationError> {
        let mut check = self.calibration();
        check.record(landmark.clone())?;
        self.landmarks.push(landmark);
        Ok(())
    }

    /// This source's calibration record under `F16` non-negotiable behavior 1.
    ///
    /// Every declared source starts with an **empty** calibration: no original
    /// handedness, axis order, scale or angle unit has been measured yet, and
    /// a caller that asks gets [`ClaimStatus::Unknown`] until someone records
    /// landmarks together with the evidence for them. A source built from a
    /// measurement — [`Self::retail_gamez`] is the first — replays the
    /// landmarks it recorded, and its gaps report is the *per-quantity* answer:
    /// an incompletely calibrated source can have the quantity it measured
    /// satisfied while the rest stay open, which is what
    /// `Unknown` looks like partway through. Returning the record rather than
    /// a "is calibrated" boolean is the point: the gaps are reportable, so an
    /// uncalibrated source cannot be mistaken for a calibrated one.
    #[must_use]
    pub fn calibration(&self) -> UnitCalibration {
        let mut calibration = UnitCalibration::empty(self.label.clone());
        for landmark in &self.landmarks {
            calibration
                .record(landmark.clone())
                .expect("a source's landmarks were validated when recorded");
        }
        calibration
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

    /// The coordinate convention of the original's GameZ containers, with its
    /// length scale measured (task #677).
    ///
    /// **One quantity measured, the rest declared.** The landmark census —
    /// recorded on this source's [`calibration`](Self::calibration) and
    /// written up in `docs/findings/2026-10-05-m01-lc-world-unit-roles.md` —
    /// pins [`CalibratedQuantity::Scale`] to **one stored unit per metre**:
    /// every animation container stores the Earth's gravitational acceleration
    /// in its payload header, and the human-scale and airframe meshes measure
    /// their known real sizes only under that unit. The axis map, rotation
    /// sense, angle unit and front-face rule are the **same identity
    /// declaration** the world import already applies: they are *not*
    /// measured, and the calibration's gap list says so, so this source is
    /// honestly `Unknown` at the whole-convention level while the scale is
    /// pinned.
    ///
    /// `source` is the installation span of a GameZ container the measurement
    /// ran over — the convention's provenance points at original bytes, which
    /// is what [`Origin::Installation`] is for. It is deliberately **not** a
    /// member of [`Self::declared`]: the declared registry is the designed
    /// self-map and its fixtures, and a measured source is built where its
    /// span exists.
    pub fn retail_gamez(source: SourceSpan) -> Self {
        let mut built = Self::new(
            "retail.gamez",
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
            )
            .expect("the measured GameZ convention is a valid declaration"),
            Origin::Installation {
                source: source.clone(),
            },
            Provenance::new(
                ClaimId::new(GAMEZ_VERTEX_UNIT_IS_THE_METRE)
                    .expect("the measured-unit claim id is valid"),
                ClaimStatus::ObservedTool,
                Some(source),
            )
            .expect("an observed_tool declaration with a span is valid"),
        )
        .expect("the measured GameZ source is a valid declaration");
        for landmark in gamez_scale_landmarks() {
            built
                .record_landmark(landmark)
                .expect("the measured scale landmarks are independent");
        }
        built
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

/// The stored length unit of the original's GameZ containers is the metre.
///
/// **Measured over the owner's retail installation (task #677), at
/// `observed_tool`.** The landmark census behind the declaration is
/// [`CoordinateSource::retail_gamez`]'s own
/// [`calibration`](CoordinateSource::calibration), and the write-up with the
/// per-container numbers is
/// `docs/findings/2026-10-05-m01-lc-world-unit-roles.md`. The claim is about
/// the stored unit only: the container family stores no axis map, handedness
/// or angle-unit declaration anyone has measured, and no original run
/// happened, so the convention's other quantities stay gaps.
pub const GAMEZ_VERTEX_UNIT_IS_THE_METRE: &str = "f18-world.gamez-vertex-unit-is-the-metre";

/// The scale landmarks the GameZ measurement rests on (task #677).
///
/// Every one is an independent observation over the owner's retail
/// installation — a different container, member family or record population —
/// and each says what it would take for the reading to be wrong in its
/// `limitations`. None of them alone is the proof: the metre is what the
/// *intersection* pins.
fn gamez_scale_landmarks() -> Vec<Landmark> {
    // Every observation was produced by running the production readers over
    // the owner's installation — a tool probe, which is the strongest claim a
    // measurement without an original run can carry (`observed_tool`). The
    // locator names the container family each census ran over.
    let evidence = |container: &str, limitation: &str| EvidenceRecord {
        source: EvidenceSource::ToolRun {
            tool: "cs_formats/cs_content GameZ, ZBD-anim and ZRD readers \
                   (task #677 census, re-run by the accept_m01_lc_world_unit_roles \
                   tests)"
                .to_owned(),
            version: env!("CARGO_PKG_VERSION").to_owned(),
        },
        fingerprint: None,
        locator: Some(ObservationLocator {
            container: container.to_owned(),
            span: None,
        }),
        method: ObservationMethod::ToolProbe,
        limitations: vec![limitation.to_owned()],
    };
    [
        Landmark::new(
            CalibratedQuantity::Scale,
            LandmarkKind::Artifact,
            "every one of the installation's 61 animation containers stores its \
             payload GRAVITY word as the f32 -9.8 (bits 0xC11CCCCD): the Earth's \
             gravitational acceleration, which reads as an SI value only when \
             the stored length unit is the metre",
            evidence(
                "zbd/*/mis_anim.zbd and cam_anim.zbd payload headers",
                "a stored constant is the format's own declared value, not an \
                 observed original run",
            ),
        ),
        Landmark::new(
            CalibratedQuantity::Scale,
            LandmarkKind::Behavior,
            "the pilot-figure subtrees (`cpilot`, `pickup_cpilot`) measure \
             about 0.7 x 1.9 x 0.5 stored units — a standing human's height \
             measured against a known size, which is a real pilot at the metre \
             and a 0.58 m figure at the foot",
            evidence(
                "zbd/planes.zbd aircraft scene roots",
                "the measured distance presumes the depicted object has its \
                 known real-world size",
            ),
        ),
        Landmark::new(
            CalibratedQuantity::Scale,
            LandmarkKind::Behavior,
            "the eleven airframe subtree roots span 8.8-27.2 stored units — \
             fighter-class aircraft measured against known airframe \
             dimensions; under the foot the largest 'fighter' would span 8.3 m",
            evidence(
                "zbd/planes.zbd airframe subtrees",
                "the same presumption, applied to a different record population",
            ),
        ),
        Landmark::new(
            CalibratedQuantity::Scale,
            LandmarkKind::Artifact,
            "the aircraft nodes' stored LOD switch ranges run 50-3000 units — \
             view-distance bands of tens of metres to about three kilometres, \
             which only read as distances in metres",
            evidence(
                "zbd/planes.zbd lod records",
                "an authored range is a hint about working distances, not a \
                 ruler: it corroborates, it does not pin",
            ),
        ),
        Landmark::new(
            CalibratedQuantity::Scale,
            LandmarkKind::Behavior,
            "the world containers' stored bounds run -16384..256 units, a \
             12-16 km archipelago theatre at the metre and a 3.7-5 km map — \
             with 300-m dogfight cells — at the foot",
            evidence(
                "zbd/*/gamez.zbd world records and partition grids",
                "map size is the loosest constraint: either unit produces a \
                 plausible theatre, so this landmark corroborates rather than \
                 decides",
            ),
        ),
    ]
    .into_iter()
    .collect::<Result<Vec<_>, _>>()
    .expect("the declared landmarks are described")
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

// ---------------------------------------------------------------------------
// F16-D: the three-landmark rule, made checkable instead of remembered
// ---------------------------------------------------------------------------

/// One property F16 non-negotiable behavior 1 requires to be **measured**
/// about an original source, rather than assumed.
///
/// "Measure original scale, handedness, axis order and angle units using at
/// least three independent landmarks/behaviors" names four quantities and one
/// evidence rule. [`UnitCalibration`] holds the four apart, so a source can be
/// measured on three of them and still be honestly unknown on the fourth.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CalibratedQuantity {
    /// `meters_per_unit`: what one source unit is worth in meters.
    Scale,
    /// Whether the source basis is right- or left-handed.
    Handedness,
    /// Which source component feeds which canonical axis, with which sign.
    AxisOrder,
    /// Whether source angles are radians or degrees.
    AngleUnit,
}

impl CalibratedQuantity {
    /// Every quantity the rule names, so a calibration can report all four.
    pub const ALL: [Self; 4] = [
        Self::Scale,
        Self::Handedness,
        Self::AxisOrder,
        Self::AngleUnit,
    ];

    /// Stable label for diagnostics and evidence records.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Scale => "scale",
            Self::Handedness => "handedness",
            Self::AxisOrder => "axis-order",
            Self::AngleUnit => "angle-unit",
        }
    }
}

/// What kind of observation a landmark is.
///
/// The split is the sheet's own: "A Blender transform is insufficient proof."
/// A static artifact tells you what a file *says*; only a behaviour tells you
/// what the thing *does*, and a complete calibration needs at least one of
/// those per quantity.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LandmarkKind {
    /// A static inspection: a header field, a value table, a transform stored
    /// in a modelling tool.
    Artifact,
    /// An observed behaviour: a measured distance, a turn, a loading pose, a
    /// travel measured against something already known.
    Behavior,
}

impl LandmarkKind {
    /// Stable label for diagnostics and evidence records.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Artifact => "artifact",
            Self::Behavior => "behavior",
        }
    }
}

/// Why a calibration or a landmark was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CalibrationError {
    /// A calibration must name the source it is about.
    EmptySource,
    /// A landmark must say what it observed. An undescribed landmark is not
    /// evidence, and repeating "looked at the model" three times would
    /// otherwise satisfy the three-landmark rule.
    EmptyDescription,
    /// The same description cannot be recorded twice for one quantity: two
    /// copies of one observation are not two landmarks.
    RepeatedDescription {
        /// The quantity the repeat was for.
        quantity: CalibratedQuantity,
        /// The description that appears twice.
        description: String,
    },
    /// One observation cannot be two independent landmarks of one quantity,
    /// however it is described.
    RepeatedObservation {
        /// The quantity the repeat was for.
        quantity: CalibratedQuantity,
        /// The description recorded first.
        first: String,
        /// The description that reuses its evidence.
        second: String,
    },
}

impl std::fmt::Display for CalibrationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptySource => write!(f, "a calibration must name its source"),
            Self::EmptyDescription => {
                write!(f, "a landmark must describe what it observed")
            }
            Self::RepeatedDescription {
                quantity,
                description,
            } => write!(
                f,
                "landmark {description:?} is already recorded for {}",
                quantity.label()
            ),
            Self::RepeatedObservation {
                quantity,
                first,
                second,
            } => write!(
                f,
                "landmark {second:?} reuses the observation behind {first:?}, so it is not \
                 an independent {} landmark",
                quantity.label()
            ),
        }
    }
}

impl std::error::Error for CalibrationError {}

/// One landmark: an observation that pins one [`CalibratedQuantity`], with
/// the evidence that backs it.
///
/// A landmark is attributed twice over — by its [`kind`](Self::kind) and by
/// its [`evidence`](Self::evidence) — and both are part of what the
/// three-landmark rule counts.
#[derive(Clone, Debug, PartialEq)]
pub struct Landmark {
    quantity: CalibratedQuantity,
    kind: LandmarkKind,
    description: String,
    evidence: EvidenceRecord,
}

impl Landmark {
    /// Builds a landmark.
    ///
    /// # Errors
    ///
    /// [`CalibrationError::EmptyDescription`] for an empty description.
    pub fn new(
        quantity: CalibratedQuantity,
        kind: LandmarkKind,
        description: impl Into<String>,
        evidence: EvidenceRecord,
    ) -> Result<Self, CalibrationError> {
        let description = description.into();
        if description.trim().is_empty() {
            return Err(CalibrationError::EmptyDescription);
        }
        Ok(Self {
            quantity,
            kind,
            description,
            evidence,
        })
    }

    /// Which property this landmark pins.
    #[must_use]
    pub const fn quantity(&self) -> CalibratedQuantity {
        self.quantity
    }

    /// Whether this is a static artifact or an observed behavior.
    #[must_use]
    pub const fn kind(&self) -> LandmarkKind {
        self.kind
    }

    /// What was observed, in the recorder's words.
    #[must_use]
    pub fn description(&self) -> &str {
        &self.description
    }

    /// The evidence backing this landmark.
    #[must_use]
    pub const fn evidence(&self) -> &EvidenceRecord {
        &self.evidence
    }
}

/// What a calibration still lacks for one quantity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CalibrationGap {
    /// The quantity with a gap.
    pub quantity: CalibratedQuantity,
    /// How many landmarks are recorded for it.
    pub landmarks_recorded: usize,
    /// How many the rule requires.
    pub landmarks_required: usize,
    /// How many of the recorded ones are observed behaviors.
    pub behavior_landmarks: usize,
}

impl std::fmt::Display for CalibrationGap {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}: {}/{} landmarks, {}/1 behaviors",
            self.quantity.label(),
            self.landmarks_recorded,
            self.landmarks_required,
            self.behavior_landmarks
        )
    }
}

/// One source's calibration record under F16 non-negotiable behavior 1.
///
/// This is F16-D's "calibrate original units" made checkable. It does not
/// measure anything by itself: it records landmarks, refuses the ones that
/// are not independent, and reports — per quantity and as a whole — whether
/// the three-landmark rule is met and what claim that can support.
///
/// Two different questions are kept apart on purpose:
///
/// * [`is_complete`](Self::is_complete) is a **shape** question: are there
///   three independent landmarks, at least one of them a behavior, for every
///   quantity? A calibration built from synthetic fixtures can be complete.
/// * [`claim_status`](Self::claim_status) is a **strength** question, and only
///   the [`EvidenceRecord`]s decide it. A complete calibration whose evidence
///   is newly authored content claims [`ClaimStatus::Unknown`], not
///   `verified_original`.
///
/// So an `accept_f16_d_`-green build of this module can never be read as "the
/// original convention is known": the declared sources start empty (see
/// [`CoordinateSource::calibration`]) and stay
/// [`ClaimStatus::Unknown`] until someone records original observations.
#[derive(Clone, Debug, PartialEq)]
pub struct UnitCalibration {
    source: String,
    landmarks: Vec<Landmark>,
}

impl UnitCalibration {
    /// How many independent landmarks each quantity needs.
    pub const MIN_LANDMARKS: usize = 3;

    /// An empty calibration for `source`.
    ///
    /// # Errors
    ///
    /// [`CalibrationError::EmptySource`] for an empty source label.
    pub fn new(source: impl Into<String>) -> Result<Self, CalibrationError> {
        let source = source.into();
        if source.trim().is_empty() {
            return Err(CalibrationError::EmptySource);
        }
        Ok(Self::empty(source))
    }

    /// The validated constructor's inner half; a validated
    /// [`CoordinateSource`] label is never empty.
    fn empty(source: String) -> Self {
        Self {
            source,
            landmarks: Vec::new(),
        }
    }

    /// The source this calibration is about.
    #[must_use]
    pub fn source(&self) -> &str {
        &self.source
    }

    /// Every recorded landmark, in the order they were recorded.
    #[must_use]
    pub fn landmarks(&self) -> &[Landmark] {
        &self.landmarks
    }

    /// Records one landmark.
    ///
    /// Independence is enforced here rather than trusted: a description
    /// already recorded for the same quantity, and any landmark reusing the
    /// same evidence for the same quantity, are both refused. That is what
    /// stops one inspection from being counted three times.
    ///
    /// # Errors
    ///
    /// [`CalibrationError::RepeatedDescription`] or
    /// [`CalibrationError::RepeatedObservation`].
    pub fn record(&mut self, landmark: Landmark) -> Result<(), CalibrationError> {
        let quantity = landmark.quantity();
        for existing in &self.landmarks {
            if existing.quantity() != quantity {
                continue;
            }
            if existing.description() == landmark.description() {
                return Err(CalibrationError::RepeatedDescription {
                    quantity,
                    description: landmark.description().to_string(),
                });
            }
            if same_observation(existing.evidence(), landmark.evidence()) {
                return Err(CalibrationError::RepeatedObservation {
                    quantity,
                    first: existing.description().to_string(),
                    second: landmark.description().to_string(),
                });
            }
        }
        self.landmarks.push(landmark);
        Ok(())
    }

    /// How many landmarks are recorded for one quantity.
    #[must_use]
    pub fn landmark_count(&self, quantity: CalibratedQuantity) -> usize {
        self.landmarks
            .iter()
            .filter(|landmark| landmark.quantity() == quantity)
            .count()
    }

    /// How many of a quantity's landmarks are observed behaviors.
    #[must_use]
    pub fn behavior_landmark_count(&self, quantity: CalibratedQuantity) -> usize {
        self.landmarks
            .iter()
            .filter(|landmark| {
                landmark.quantity() == quantity && landmark.kind() == LandmarkKind::Behavior
            })
            .count()
    }

    /// Every quantity that still falls short of the rule, in the rule's order.
    #[must_use]
    pub fn gaps(&self) -> Vec<CalibrationGap> {
        CalibratedQuantity::ALL
            .into_iter()
            .filter_map(|quantity| {
                let landmarks_recorded = self.landmark_count(quantity);
                let behavior_landmarks = self.behavior_landmark_count(quantity);
                if landmarks_recorded >= Self::MIN_LANDMARKS && behavior_landmarks >= 1 {
                    return None;
                }
                Some(CalibrationGap {
                    quantity,
                    landmarks_recorded,
                    landmarks_required: Self::MIN_LANDMARKS,
                    behavior_landmarks,
                })
            })
            .collect()
    }

    /// Whether every quantity has three independent landmarks and at least
    /// one behavior among them.
    ///
    /// This is a statement about the *shape* of the evidence, never about its
    /// originality: see [`claim_status`](Self::claim_status).
    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.gaps().is_empty()
    }

    /// The strongest claim this calibration can support.
    ///
    /// `VerifiedOriginal` requires a complete calibration **and** every
    /// landmark's own [`EvidenceRecord::verifies_original`]. A complete
    /// calibration whose landmarks are newly authored content claims
    /// [`ClaimStatus::Unknown`]; a document-only one claims
    /// [`ClaimStatus::Documented`]; a tool run over produced artifacts claims
    /// [`ClaimStatus::ObservedTool`].
    #[must_use]
    pub fn claim_status(&self) -> ClaimStatus {
        if !self.is_complete() {
            return ClaimStatus::Unknown;
        }
        if self
            .landmarks
            .iter()
            .all(|landmark| landmark.evidence().verifies_original())
        {
            return ClaimStatus::VerifiedOriginal;
        }
        if self
            .landmarks
            .iter()
            .any(|landmark| matches!(landmark.evidence().source, EvidenceSource::SyntheticFixture))
        {
            return ClaimStatus::Unknown;
        }
        if self
            .landmarks
            .iter()
            .all(|landmark| matches!(landmark.evidence().source, EvidenceSource::Document(_)))
        {
            return ClaimStatus::Documented;
        }
        if self
            .landmarks
            .iter()
            .any(|landmark| matches!(landmark.evidence().source, EvidenceSource::ToolRun { .. }))
        {
            return ClaimStatus::ObservedTool;
        }
        ClaimStatus::Unknown
    }

    /// The strongest claim the evidence for **one quantity** supports.
    ///
    /// [`claim_status`](Self::claim_status) is the whole-convention answer: a
    /// calibration that has not measured all four quantities claims nothing,
    /// which is the right answer for "what convention does this source use".
    /// A consumer that converts a *single* quantity — the world import's
    /// `meters_per_unit` is the scale's — asks here instead: `Unknown` while
    /// the quantity's own landmark rule is unmet (fewer than three
    /// independent landmarks, or no observed behavior among them), otherwise
    /// the class *that quantity's* evidence reaches under the same ladder
    /// [`claim_status`](Self::claim_status) applies.
    #[must_use]
    pub fn quantity_status(&self, quantity: CalibratedQuantity) -> ClaimStatus {
        let landmarks: Vec<&Landmark> = self
            .landmarks
            .iter()
            .filter(|landmark| landmark.quantity() == quantity)
            .collect();
        if landmarks.len() < Self::MIN_LANDMARKS
            || !landmarks
                .iter()
                .any(|landmark| landmark.kind() == LandmarkKind::Behavior)
        {
            return ClaimStatus::Unknown;
        }
        if landmarks
            .iter()
            .all(|landmark| landmark.evidence().verifies_original())
        {
            return ClaimStatus::VerifiedOriginal;
        }
        if landmarks
            .iter()
            .any(|landmark| matches!(landmark.evidence().source, EvidenceSource::SyntheticFixture))
        {
            return ClaimStatus::Unknown;
        }
        if landmarks
            .iter()
            .all(|landmark| matches!(landmark.evidence().source, EvidenceSource::Document(_)))
        {
            return ClaimStatus::Documented;
        }
        if landmarks
            .iter()
            .any(|landmark| matches!(landmark.evidence().source, EvidenceSource::ToolRun { .. }))
        {
            return ClaimStatus::ObservedTool;
        }
        ClaimStatus::Unknown
    }

    /// A one-line summary for a findings note or an evidence record.
    #[must_use]
    pub fn describe(&self) -> String {
        let gaps = self.gaps();
        let detail = if gaps.is_empty() {
            "complete".to_string()
        } else {
            gaps.iter()
                .map(|gap| gap.to_string())
                .collect::<Vec<_>>()
                .join("; ")
        };
        format!(
            "{}: {} ({} landmarks, claim {})",
            self.source,
            detail,
            self.landmarks.len(),
            self.claim_status().label()
        )
    }
}

/// Whether two evidence records describe the **same observation**.
///
/// `EvidenceRecord`'s own `PartialEq` also covers `limitations`, which is the
/// recorder's self-assessment of the observation rather than part of it. The
/// independence rule is about *what was observed, where and how*, so a second
/// landmark may not be bought by re-wording a limitations string: the first
/// version of this check compared whole records, and pasting one inspection
/// three times with a different caveat on each paste read as three
/// independent landmarks — exactly what non-negotiable behavior 1 forbids.
fn same_observation(first: &EvidenceRecord, second: &EvidenceRecord) -> bool {
    first.source == second.source
        && first.fingerprint == second.fingerprint
        && first.locator == second.locator
        && first.method == second.method
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
