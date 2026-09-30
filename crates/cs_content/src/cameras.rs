//! The declared camera modes and projection policy (F21-A).
//!
//! Spec: `specs/F21-cameras-cockpit-views-and-spyglass.md`, stage
//! `### F21-A`. Shared contract: `docs/contracts/UI-NETWORK.md`.
//!
//! This module is the **content half** of the camera contract: the
//! normalized records a mission/config importer produces, engine-free and
//! provenance-carrying. The runtime half (the lowered projection policy and
//! the framing math a renderer consumes) lives in `cs_app::camera`; this
//! crate cannot depend on Bevy, so the two are separated exactly like
//! `cs_content::environment` ↔ `cs_app::environment`.
//!
//! # Records
//!
//! * [`CameraModeKind`] — the *kinds* of view F21 names: an original
//!   cockpit, an external view, a spyglass, and an authored camera
//!   sequence. Which original modes exist and what they are called is
//!   unmeasured (F21-D), so the kind is a designed engine vocabulary, not a
//!   claim about the original UI.
//! * [`ProjectionPolicy`] — how a mode's field of view becomes a
//!   viewport-relative frustum: the authored field of view, the axis it is
//!   declared on (vertical vs horizontal), the reference aspect it was
//!   authored at, the framing rule for other aspects, and its own near/far
//!   clipping planes. Every field is a [`Resolved`], so an unmeasured value
//!   stays an explicit unknown and refuses to lower instead of becoming a
//!   silent default.
//! * [`AspectFraming`] — the aspect policy. F21 non-negotiable behavior 2
//!   forbids stretching art, so only the two aspect-correct rules exist as
//!   *usable* values; a declaration that asks for a stretch is refused at
//!   the lowering boundary rather than honoured.
//! * [`DeclaredCameraMode`] and [`DeclaredCameraModes`] — one mode, and one
//!   subject's set with a declared default. A set names at most one mode of
//!   each kind and its default must be present.
//!
//! # Designed vocabulary, not original data
//!
//! The original's view list, field of view, projection axis, near/far
//! planes, spyglass magnification and target-tracking behavior are all
//! unmeasured. Every value in [`declared_synthetic_camera_modes`] is newly
//! authored project design carrying [`Origin::SyntheticFixture`] and
//! designed provenance, recorded in
//! `docs/findings/2026-09-30-f21-a-camera-modes-and-projection-policy.md`.
//! Nothing here claims original behavior; F21-B implements the rigs from
//! these records and F21-D compares them against original captures.

use std::fmt;

use cs_types::content::{ContentId, ContentKind, Known, Origin, Provenance, Resolved};
use cs_types::evidence::ClaimId;
use cs_types::space::{Meters, Radians};

/// The kind of view a camera mode is.
///
/// Designed engine vocabulary: the original PC game's view list and names
/// are unmeasured. A kind is never inferred from *High Road to Revenge* or a
/// generic camera library (the `UI-NETWORK` "Modes and compatibility"
/// rule).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum CameraModeKind {
    /// The view from a verified cockpit model/config binding.
    Cockpit,
    /// An external view of the player's aircraft.
    External,
    /// A magnified view of the selected target.
    Spyglass,
    /// A camera driven by an authored camera track (a scripted sequence).
    AuthoredSequence,
}

impl CameraModeKind {
    /// Every kind, in a stable order.
    pub const ALL: &'static [CameraModeKind] = &[
        Self::Cockpit,
        Self::External,
        Self::Spyglass,
        Self::AuthoredSequence,
    ];

    /// The stable label used in ids and reports.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Cockpit => "cockpit",
            Self::External => "external",
            Self::Spyglass => "spyglass",
            Self::AuthoredSequence => "authored_sequence",
        }
    }
}

impl fmt::Display for CameraModeKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// The axis an authored field of view is declared on.
///
/// F21 non-negotiable behavior 2 requires "vertical vs horizontal
/// conversion" to be defined rather than assumed, so the axis is part of
/// the record: an importer that read a horizontal FOV without saying so
/// would otherwise be silently reinterpreted as vertical.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FovAxis {
    /// The authored value spans the viewport's height.
    Vertical,
    /// The authored value spans the viewport's width.
    Horizontal,
}

impl FovAxis {
    /// Both axes, in a stable order.
    pub const ALL: &'static [FovAxis] = &[Self::Vertical, Self::Horizontal];

    /// The stable label used in reports.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Vertical => "vertical",
            Self::Horizontal => "horizontal",
        }
    }
}

impl fmt::Display for FovAxis {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// How a projection keeps its framing when the viewport aspect changes.
///
/// Only the two aspect-correct rules are usable; [`Self::Stretch`] exists so
/// that a source declaring a non-uniform fill is *representable and
/// refused*, instead of being silently treated as one of the correct rules.
/// Stretching art is forbidden by F21 non-negotiable behavior 2.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AspectFraming {
    /// Keep the authored *vertical* field of view at every aspect; the
    /// horizontal field of view grows with a widening viewport ("Hor+"). A
    /// 4:3 player sees the same vertical extent as an ultrawide player and
    /// simply sees more world to the sides. This is the default designed
    /// rule.
    PreserveVertical,
    /// Keep the authored *horizontal* field of view at every aspect; the
    /// vertical field of view shrinks as the viewport widens, because a
    /// fixed horizontal extent spans a shorter image the wider it is
    /// ("Vert−"/"Hor−"). The sides stay framed and less world is revealed
    /// above and below.
    PreserveHorizontal,
    /// Keep both fields of view and stretch the image to the viewport
    /// (a non-uniform pixel aspect). Declarable, but refused at lowering:
    /// F21 non-negotiable behavior 2 forbids stretching art.
    Stretch,
}

impl AspectFraming {
    /// Every framing rule, in a stable order.
    pub const ALL: &'static [AspectFraming] = &[
        Self::PreserveVertical,
        Self::PreserveHorizontal,
        Self::Stretch,
    ];

    /// The stable label used in reports.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::PreserveVertical => "preserve-vertical",
            Self::PreserveHorizontal => "preserve-horizontal",
            Self::Stretch => "stretch",
        }
    }

    /// Whether this rule preserves pixel aspect (does not stretch art).
    #[must_use]
    pub const fn is_aspect_correct(self) -> bool {
        !matches!(self, Self::Stretch)
    }
}

impl fmt::Display for AspectFraming {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// Why a viewport [`AspectRatio`] was rejected.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AspectRatioError {
    /// The value was NaN or infinite.
    NonFinite,
    /// The value was zero or negative, so width/height had no usable ratio.
    NotPositive {
        /// The rejected value.
        value: f64,
    },
}

impl fmt::Display for AspectRatioError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NonFinite => write!(f, "the aspect ratio must be finite"),
            Self::NotPositive { value } => {
                write!(f, "the aspect ratio must be greater than zero, got {value}")
            }
        }
    }
}

impl std::error::Error for AspectRatioError {}

/// A validated viewport aspect ratio (width / height), finite and positive.
///
/// Aspect is a typed value so a zero, negative, NaN or infinite ratio can
/// never reach the projection math as a division that silently produces an
/// unbounded frustum.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AspectRatio(f64);

impl AspectRatio {
    /// The common 4:3 (1.333…) aspect.
    pub const FOUR_THREE: Self = Self(4.0 / 3.0);
    /// The common 16:9 (1.777…) widescreen aspect.
    pub const SIXTEEN_NINE: Self = Self(16.0 / 9.0);
    /// A 64:27 (2.370…) ultrawide aspect, wider than 21:9.
    pub const ULTRAWIDE_64_27: Self = Self(64.0 / 27.0);

    /// Validates an aspect ratio.
    ///
    /// # Errors
    ///
    /// [`AspectRatioError::NonFinite`] for NaN or infinity, and
    /// [`AspectRatioError::NotPositive`] for zero or a negative ratio.
    pub fn new(value: f64) -> Result<Self, AspectRatioError> {
        if !value.is_finite() {
            return Err(AspectRatioError::NonFinite);
        }
        if value <= 0.0 {
            return Err(AspectRatioError::NotPositive { value });
        }
        Ok(Self(value))
    }

    /// The width / height value.
    #[must_use]
    pub const fn value(self) -> f64 {
        self.0
    }
}

impl fmt::Display for AspectRatio {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:1", self.0)
    }
}

/// Why a [`Magnification`] was rejected.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MagnificationError {
    /// The value was NaN or infinite.
    NonFinite,
    /// The value was zero or negative, so it could not magnify.
    NotPositive {
        /// The rejected value.
        value: f64,
    },
}

impl fmt::Display for MagnificationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NonFinite => write!(f, "the magnification must be finite"),
            Self::NotPositive { value } => {
                write!(
                    f,
                    "the magnification must be greater than zero, got {value}"
                )
            }
        }
    }
}

impl std::error::Error for MagnificationError {}

/// A validated magnification factor: finite and strictly positive.
///
/// `1.0` is no magnification. Any positive factor is representable; whether
/// a factor is plausible for a view is the mode rules' concern (a spyglass
/// magnifies, the other kinds declare none). Zero, negative, NaN and
/// infinite factors are refused so a magnification can never be a divide by
/// zero or an unbounded zoom.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Magnification(f64);

impl Magnification {
    /// No magnification.
    pub const ONE: Self = Self(1.0);

    /// Validates a magnification factor.
    ///
    /// # Errors
    ///
    /// [`MagnificationError::NonFinite`] for NaN or infinity, and
    /// [`MagnificationError::NotPositive`] for zero or a negative factor.
    pub fn new(value: f64) -> Result<Self, MagnificationError> {
        if !value.is_finite() {
            return Err(MagnificationError::NonFinite);
        }
        if value <= 0.0 {
            return Err(MagnificationError::NotPositive { value });
        }
        Ok(Self(value))
    }

    /// The magnification factor.
    #[must_use]
    pub const fn value(self) -> f64 {
        self.0
    }
}

impl fmt::Display for Magnification {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}x", self.0)
    }
}

/// The declared projection policy of one camera mode.
///
/// Every field is a [`Resolved`]: an importer that could not evidence a
/// value leaves it an explicit unknown, and the lowering boundary in
/// `cs_app::camera` refuses rather than guessing a field of view, an axis,
/// a reference aspect, a framing rule or a clipping plane.
#[derive(Clone, Debug, PartialEq)]
pub struct ProjectionPolicy {
    /// The authored field of view, in radians within `(0, π)` when known.
    pub fov: Resolved<Radians>,
    /// The axis [`ProjectionPolicy::fov`] is declared on.
    pub fov_axis: Resolved<FovAxis>,
    /// The aspect ratio the authored field of view was authored at.
    pub reference_aspect: Resolved<AspectRatio>,
    /// How other aspects keep the scene framed.
    pub framing: Resolved<AspectFraming>,
    /// The near clipping distance, in canonical meters, when known.
    pub near_m: Resolved<Meters>,
    /// The far clipping distance, in canonical meters, when known.
    pub far_m: Resolved<Meters>,
}

impl ProjectionPolicy {
    /// The projection policy every synthetic camera mode shares: a vertical
    /// field of view authored at 4:3, with horizontal-plus framing.
    ///
    /// `vertical_fov` is in radians; it is not validated here (a caller that
    /// passes an out-of-range value gets a record the mode constructor
    /// refuses), which keeps the one validation path in
    /// [`DeclaredCameraMode::try_new`].
    fn designed(vertical_fov: Radians, near_m: f64, far_m: f64) -> Self {
        Self {
            fov: known(vertical_fov),
            fov_axis: known(FovAxis::Vertical),
            reference_aspect: known(AspectRatio::FOUR_THREE),
            framing: known(AspectFraming::PreserveVertical),
            near_m: known(Meters(near_m)),
            far_m: known(Meters(far_m)),
        }
    }
}

/// Why a [`DeclaredCameraMode`] was rejected.
#[derive(Clone, Debug, PartialEq)]
pub enum CameraModeError {
    /// The known field of view was NaN or infinite.
    NonFiniteFov,
    /// The known field of view fell outside `(0, π)`.
    FovOutOfRange {
        /// The rejected angle in radians.
        radians: f64,
    },
    /// The known near clipping distance was NaN or infinite.
    NonFiniteNear,
    /// The known far clipping distance was NaN or infinite.
    NonFiniteFar,
    /// The known near clipping distance was zero or negative.
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
    /// A non-spyglass mode declared a known magnification other than 1.
    ///
    /// Magnification is the spyglass's own property; a cockpit or external
    /// view that magnifies is a declaration error, not a variant.
    UnexpectedMagnification {
        /// The mode kind that declared it.
        kind: CameraModeKind,
        /// The rejected factor.
        value: f64,
    },
}

impl fmt::Display for CameraModeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NonFiniteFov => write!(f, "the field of view must be finite"),
            Self::FovOutOfRange { radians } => {
                write!(f, "the field of view {radians} rad is outside (0, π)")
            }
            Self::NonFiniteNear => write!(f, "the near clipping plane must be finite"),
            Self::NonFiniteFar => write!(f, "the far clipping plane must be finite"),
            Self::NonPositiveNear { meters } => {
                write!(
                    f,
                    "the near clipping plane must be positive, got {meters} m"
                )
            }
            Self::ClippingNotOrdered { near_m, far_m } => write!(
                f,
                "the near clipping plane ({near_m} m) must be closer than the far plane ({far_m} m)"
            ),
            Self::UnexpectedMagnification { kind, value } => {
                write!(f, "the {kind} mode must not magnify, but declared {value}x")
            }
        }
    }
}

impl std::error::Error for CameraModeError {}

/// One declared camera mode: its kind, its projection and its behavior
/// flags.
///
/// The mode is a *sub-record* of a [`DeclaredCameraModes`] set, so it carries
/// no subject of its own — its `kind` is its identity within the set. The
/// [`Resolved`] fields carry their own provenance.
#[derive(Clone, Debug, PartialEq)]
pub struct DeclaredCameraMode {
    kind: CameraModeKind,
    projection: ProjectionPolicy,
    magnification: Resolved<Magnification>,
    tracks_target: Resolved<bool>,
}

impl DeclaredCameraMode {
    /// Assembles and validates a declared mode.
    ///
    /// Only *known* values are range-checked; an unknown is a valid declared
    /// state that the lowering boundary refuses. A non-spyglass mode must
    /// declare [`Magnification::ONE`] when it declares a magnification at
    /// all.
    ///
    /// # Errors
    ///
    /// [`CameraModeError`] for a corrupt known field of view or clipping
    /// range, or a non-spyglass magnification.
    pub fn try_new(
        kind: CameraModeKind,
        projection: ProjectionPolicy,
        magnification: Resolved<Magnification>,
        tracks_target: Resolved<bool>,
    ) -> Result<Self, CameraModeError> {
        validate_projection(&projection)?;
        if let Resolved::Known(known) = &magnification
            && kind != CameraModeKind::Spyglass
            && known.value != Magnification::ONE
        {
            return Err(CameraModeError::UnexpectedMagnification {
                kind,
                value: known.value.value(),
            });
        }
        Ok(Self {
            kind,
            projection,
            magnification,
            tracks_target,
        })
    }

    /// Which kind of view this mode is.
    #[must_use]
    pub const fn kind(&self) -> CameraModeKind {
        self.kind
    }

    /// The declared projection policy.
    #[must_use]
    pub const fn projection(&self) -> &ProjectionPolicy {
        &self.projection
    }

    /// The declared magnification, or its explicit unknown.
    #[must_use]
    pub const fn magnification(&self) -> &Resolved<Magnification> {
        &self.magnification
    }

    /// Whether the mode tracks the currently selected target.
    #[must_use]
    pub const fn tracks_target(&self) -> &Resolved<bool> {
        &self.tracks_target
    }
}

/// Why a [`DeclaredCameraModes`] set was rejected.
#[derive(Clone, Debug, PartialEq)]
pub enum CameraModesError {
    /// The subject id was not in the camera namespace.
    SubjectKindMismatch {
        /// The offending id.
        subject: ContentId,
    },
    /// The set declared no modes.
    Empty,
    /// Two modes shared one kind.
    DuplicateKind {
        /// The duplicated kind.
        kind: CameraModeKind,
    },
    /// The declared default was not one of the set's modes.
    MissingDefault {
        /// The declared default.
        default_mode: CameraModeKind,
    },
}

impl fmt::Display for CameraModesError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SubjectKindMismatch { subject } => {
                write!(f, "{subject} is not in the camera namespace")
            }
            Self::Empty => write!(f, "a camera mode set must declare at least one mode"),
            Self::DuplicateKind { kind } => {
                write!(f, "the {kind} mode is declared more than once")
            }
            Self::MissingDefault { default_mode } => {
                write!(
                    f,
                    "the default mode {default_mode} is not declared in the set"
                )
            }
        }
    }
}

impl std::error::Error for CameraModesError {}

/// One subject's declared camera mode set.
///
/// `subject` is a [`ContentKind::CameraTrack`] id — the canonical catalog's
/// camera namespace — and `default_mode` names the mode a session starts
/// in. The default must be present, at most one mode of each kind may be
/// declared, and every mode must validate.
#[derive(Clone, Debug, PartialEq)]
pub struct DeclaredCameraModes {
    subject: ContentId,
    origin: Origin,
    default_mode: CameraModeKind,
    modes: Vec<DeclaredCameraMode>,
    provenance: Provenance,
}

impl DeclaredCameraModes {
    /// Assembles and validates a declared camera mode set.
    ///
    /// # Errors
    ///
    /// [`CameraModesError`] for a non-camera subject id, an empty set, a
    /// duplicated kind, a default that is not a declared mode or an invalid
    /// mode.
    pub fn try_new(
        subject: ContentId,
        origin: Origin,
        default_mode: CameraModeKind,
        modes: Vec<DeclaredCameraMode>,
        provenance: Provenance,
    ) -> Result<Self, CameraModesError> {
        if subject.kind() != ContentKind::CameraTrack {
            return Err(CameraModesError::SubjectKindMismatch { subject });
        }
        if modes.is_empty() {
            return Err(CameraModesError::Empty);
        }
        let mut seen = std::collections::BTreeSet::new();
        for mode in &modes {
            if !seen.insert(mode.kind()) {
                return Err(CameraModesError::DuplicateKind { kind: mode.kind() });
            }
        }
        if modes.iter().all(|mode| mode.kind() != default_mode) {
            return Err(CameraModesError::MissingDefault { default_mode });
        }
        Ok(Self {
            subject,
            origin,
            default_mode,
            modes,
            provenance,
        })
    }

    /// The camera namespace id this set belongs to.
    #[must_use]
    pub fn subject(&self) -> &ContentId {
        &self.subject
    }

    /// Where the record came from.
    #[must_use]
    pub const fn origin(&self) -> &Origin {
        &self.origin
    }

    /// The mode a session starts in.
    #[must_use]
    pub const fn default_mode(&self) -> CameraModeKind {
        self.default_mode
    }

    /// The declared modes, in authored order.
    #[must_use]
    pub fn modes(&self) -> &[DeclaredCameraMode] {
        &self.modes
    }

    /// The mode of a given kind, when declared.
    #[must_use]
    pub fn get(&self, kind: CameraModeKind) -> Option<&DeclaredCameraMode> {
        self.modes.iter().find(|mode| mode.kind() == kind)
    }

    /// Where the record itself came from.
    #[must_use]
    pub const fn provenance(&self) -> &Provenance {
        &self.provenance
    }
}

/// Validates the *known* values of one projection policy.
fn validate_projection(projection: &ProjectionPolicy) -> Result<(), CameraModeError> {
    if let Resolved::Known(known) = &projection.fov {
        if !known.value.0.is_finite() {
            return Err(CameraModeError::NonFiniteFov);
        }
        if !(known.value.0 > 0.0 && known.value.0 < std::f64::consts::PI) {
            return Err(CameraModeError::FovOutOfRange {
                radians: known.value.0,
            });
        }
    }

    let near = match &projection.near_m {
        Resolved::Known(known) => {
            if !known.value.0.is_finite() {
                return Err(CameraModeError::NonFiniteNear);
            }
            if known.value.0 <= 0.0 {
                return Err(CameraModeError::NonPositiveNear {
                    meters: known.value.0,
                });
            }
            Some(known.value.0)
        }
        Resolved::Unknown { .. } => None,
    };

    let far = match &projection.far_m {
        Resolved::Known(known) => {
            if !known.value.0.is_finite() {
                return Err(CameraModeError::NonFiniteFar);
            }
            Some(known.value.0)
        }
        Resolved::Unknown { .. } => None,
    };

    if let (Some(near_m), Some(far_m)) = (near, far)
        && near_m >= far_m
    {
        return Err(CameraModeError::ClippingNotOrdered { near_m, far_m });
    }
    Ok(())
}

// ----------------------------------------------------------- fixture ------

fn claim() -> ClaimId {
    ClaimId::new("f21a.synthetic-camera-modes").expect("valid claim id")
}

fn known<T>(value: T) -> Resolved<T> {
    Resolved::Known(Known::new(value, Provenance::designed(claim())))
}

fn degrees(value: f64) -> Radians {
    Radians(value * std::f64::consts::PI / 180.0)
}

/// The minimal synthetic camera mode fixture: a cockpit, an external view
/// and a magnifying spyglass, with the cockpit as the default.
///
/// Every value is newly authored project design ([`Origin::SyntheticFixture`]
/// with designed provenance); it can never be mistaken for the original
/// view list, and it cannot stand in for it. The spyglass carries its own
/// near/far planes and a 4x magnification to exercise the mode-specific
/// fields; the cockpit and external views declare [`Magnification::ONE`].
#[must_use]
pub fn declared_synthetic_camera_modes() -> DeclaredCameraModes {
    let cockpit = DeclaredCameraMode::try_new(
        CameraModeKind::Cockpit,
        ProjectionPolicy::designed(degrees(60.0), 0.1, 10_000.0),
        known(Magnification::ONE),
        known(false),
    )
    .expect("the synthetic cockpit mode is valid");
    let external = DeclaredCameraMode::try_new(
        CameraModeKind::External,
        ProjectionPolicy::designed(degrees(55.0), 0.1, 10_000.0),
        known(Magnification::ONE),
        known(false),
    )
    .expect("the synthetic external mode is valid");
    let spyglass = DeclaredCameraMode::try_new(
        CameraModeKind::Spyglass,
        ProjectionPolicy::designed(degrees(20.0), 1.0, 20_000.0),
        known(Magnification::new(4.0).expect("4.0 is a valid magnification")),
        known(true),
    )
    .expect("the synthetic spyglass mode is valid");

    DeclaredCameraModes::try_new(
        ContentId::from_source(ContentKind::CameraTrack, "synthetic.camera-modes")
            .expect("fixture subject id is valid"),
        Origin::SyntheticFixture,
        CameraModeKind::Cockpit,
        vec![cockpit, external, spyglass],
        Provenance::designed(claim()),
    )
    .expect("the declared synthetic camera mode fixture is valid")
}
