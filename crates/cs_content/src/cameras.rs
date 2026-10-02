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
//!   owner's set with a declared default. A set names at most one mode of
//!   each kind and its default must be present.
//! * [`BodyOffset`], [`CockpitViewpoint`], [`CockpitBindingSource`],
//!   [`DeclaredPlacement`] and [`LookLimits`] — the F21-B stage's records:
//!   **where** a mode's camera sits relative to the aircraft it follows. A
//!   cockpit mode may only declare a [`CockpitViewpoint`] — a named
//!   model/config binding — so F21 non-negotiable behavior 1 ("cockpit
//!   viewpoint comes from verified model/config bindings", and a HUD-only
//!   synthetic camera is not a replacement for every original cockpit) is
//!   enforced by the record rather than by a renderer's good intentions.
//! * [`owns_camera_modes`] — the canonical rule for which catalog kind a mode
//!   set may be attached to, decided in task #431 (`F21-A-CATALOG-KIND`).
//!
//! # The mode set's namespace: a subordinate record, not a catalog kind
//!
//! A mode set has **no namespace of its own**. It is a *subordinate* record
//! addressed inside the catalog element that owns the camera — the aircraft a
//! session flies, or the launchable content a session starts from — and
//! [`owns_camera_modes`] is the total rule for those owners. It is deliberately
//! not a catalog [`ContentId`] of its own and deliberately not stored under
//! [`ContentKind::CameraTrack`], for two recorded reasons:
//!
//! 1. `IDENTITY-CONTENT`'s "Required catalog collections" reserves no
//!    camera-mode collection, and `ContentKind` is documented as the union of
//!    that list and spec F14's deliverable list. A new kind would therefore
//!    claim a namespace the canonical contract does not reserve.
//! 2. `camera_track` already means "an authored in-engine camera sequence":
//!    `cs_content::cinematics` names a cinematic's `InEngine` presentation by
//!    a [`ContentKind::CameraTrack`] id and accepts a camera track as a
//!    cinematic subject. Storing a player's view set in the same namespace
//!    would make one id address two unrelated records, and — because
//!    `cs_content::mods::overrides` classifies `camera_track` as
//!    [`OverrideEffect::Cosmetic`](crate::mods::OverrideEffect) —
//!    would let a cosmetic-only mod change the player's camera modes without
//!    marking a session, save, replay or handshake for gameplay reasons.
//!
//! The decision, the evidence behind it and what it deliberately leaves open
//! are recorded in
//! `docs/findings/2026-10-02-f21-a-catalog-kind-camera-mode-owner-namespace.md`.
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

/// A placement expressed in the **aircraft's own body frame**: metres along
/// the body's right, up and forward axes (F16 canonical axes: `+X` right,
/// `+Y` up, `-Z` forward).
///
/// Each component is a *distance along the axis it names*, not that axis's
/// canonical component: a positive [`forward_m`](Self::forward_m) puts the
/// camera ahead of the body origin along the body's forward direction, and a
/// negative one puts it behind. The sign is stated here because a chase view
/// that lands *in front of* the aircraft, looking the way the aircraft flies,
/// shows the pilot nothing at all.
///
/// A body-frame offset is deliberately *not* a world offset: it is the same
/// three numbers for every aircraft pose, so a rig applies it to the
/// authoritative pose it reads instead of caching a world position that a
/// later pose would contradict.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BodyOffset {
    right_m: Meters,
    up_m: Meters,
    forward_m: Meters,
}

impl BodyOffset {
    /// The aircraft's own origin: the camera sits exactly on the body
    /// origin, with no offset along any axis.
    pub const ZERO: Self = Self {
        right_m: Meters(0.0),
        up_m: Meters(0.0),
        forward_m: Meters(0.0),
    };

    /// Assembles a body-frame offset.
    ///
    /// # Errors
    ///
    /// [`ViewpointError::NonFiniteOffset`] when a component is NaN or
    /// infinite. Negative components are ordinary — "behind the aircraft" is
    /// a negative forward distance — so only non-finite input is refused.
    pub fn new(right_m: Meters, up_m: Meters, forward_m: Meters) -> Result<Self, ViewpointError> {
        const AXES: [&str; 3] = ["right_m", "up_m", "forward_m"];
        for (axis, value) in AXES.into_iter().zip([right_m, up_m, forward_m]) {
            if !value.0.is_finite() {
                return Err(ViewpointError::NonFiniteOffset { axis });
            }
        }
        Ok(Self {
            right_m,
            up_m,
            forward_m,
        })
    }

    /// The offset along the body's right axis (`+X`).
    #[must_use]
    pub const fn right_m(self) -> Meters {
        self.right_m
    }

    /// The offset along the body's up axis (`+Y`).
    #[must_use]
    pub const fn up_m(self) -> Meters {
        self.up_m
    }

    /// The distance along the body's forward axis (`-Z`): positive is ahead
    /// of the body origin, negative is behind it.
    #[must_use]
    pub const fn forward_m(self) -> Meters {
        self.forward_m
    }
}

/// The longest cockpit binding name this contract accepts, in bytes.
///
/// Bounded because the name is a stable key an importer reads out of a model
/// node or a configuration file: it is echoed into diagnostics and evidence
/// records, so it cannot be an unbounded string from an untrusted source.
pub const MAX_COCKPIT_BINDING_NAME: usize = 64;

/// Where a cockpit viewpoint was bound from.
///
/// F21 non-negotiable behavior 1 requires the cockpit viewpoint to come from
/// a *verified model/config binding*. Making the source part of the record is
/// what lets a consumer ask that question later instead of assuming it: a
/// binding names the model node or the configuration key it was read from,
/// and the mode set's [`Origin`](cs_types::content::Origin) says whether those
/// bytes were the owner's installation or a synthetic fixture.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum CockpitBindingSource {
    /// Read from a node of the aircraft's cockpit model.
    ModelNode {
        /// The node the viewpoint was read from.
        node: String,
    },
    /// Read from a configuration key.
    ConfigKey {
        /// The configuration key the viewpoint was read from.
        key: String,
    },
}

impl CockpitBindingSource {
    /// The bound name — a model node or a configuration key.
    #[must_use]
    pub fn name(&self) -> &str {
        match self {
            Self::ModelNode { node } => node,
            Self::ConfigKey { key } => key,
        }
    }

    /// The stable label used in reports.
    #[must_use]
    pub const fn label(&self) -> &'static str {
        match self {
            Self::ModelNode { .. } => "model_node",
            Self::ConfigKey { .. } => "config_key",
        }
    }
}

impl fmt::Display for CockpitBindingSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.label(), self.name())
    }
}

/// A verified cockpit viewpoint: the binding it was read from, where it sits
/// in the aircraft's body frame, and how the pilot's head is oriented there.
///
/// The yaw and pitch are [`Resolved`] like every other unmeasured value in
/// this module: an importer that read a viewpoint position but not its
/// orientation leaves the orientation an explicit unknown and the lowering
/// boundary refuses rather than assuming the pilot looks along the nose.
#[derive(Clone, Debug, PartialEq)]
pub struct CockpitViewpoint {
    source: CockpitBindingSource,
    offset: BodyOffset,
    yaw: Resolved<Radians>,
    pitch: Resolved<Radians>,
}

impl CockpitViewpoint {
    /// Assembles a cockpit viewpoint.
    ///
    /// # Errors
    ///
    /// [`ViewpointError`] when the binding name is empty or longer than
    /// [`MAX_COCKPIT_BINDING_NAME`]. Only *known* orientation values are
    /// range-checked; an unknown is a valid declared state that the lowering
    /// boundary refuses.
    pub fn try_new(
        source: CockpitBindingSource,
        offset: BodyOffset,
        yaw: Resolved<Radians>,
        pitch: Resolved<Radians>,
    ) -> Result<Self, ViewpointError> {
        match source.name().len() {
            0 => return Err(ViewpointError::EmptyBindingName),
            len if len > MAX_COCKPIT_BINDING_NAME => {
                return Err(ViewpointError::BindingNameTooLong { len });
            }
            _ => {}
        }
        if let Resolved::Known(known) = &yaw {
            require_finite_angle("yaw", known.value)?;
        }
        if let Resolved::Known(known) = &pitch {
            require_finite_angle("pitch", known.value)?;
        }
        Ok(Self {
            source,
            offset,
            yaw,
            pitch,
        })
    }

    /// The model node or configuration key this viewpoint was bound from.
    #[must_use]
    pub const fn source(&self) -> &CockpitBindingSource {
        &self.source
    }

    /// Where the eye sits in the aircraft's body frame.
    #[must_use]
    pub const fn offset(&self) -> BodyOffset {
        self.offset
    }

    /// The pilot's head yaw, or its explicit unknown.
    #[must_use]
    pub const fn yaw(&self) -> &Resolved<Radians> {
        &self.yaw
    }

    /// The pilot's head pitch, or its explicit unknown.
    #[must_use]
    pub const fn pitch(&self) -> &Resolved<Radians> {
        &self.pitch
    }
}

/// The declared placement of one mode's camera relative to its aircraft.
#[derive(Clone, Debug, PartialEq)]
pub enum DeclaredPlacement {
    /// A verified cockpit model/config binding (F21 non-negotiable behavior
    /// 1). Only a [`CameraModeKind::Cockpit`] mode may declare it.
    ///
    /// Boxed because a viewpoint carries two provenance-carrying angles and
    /// an enum is as large as its largest variant: without the box every
    /// external and spyglass placement would carry a cockpit record's worth
    /// of inline space. Build it with [`DeclaredPlacement::at_cockpit`].
    Cockpit(Box<CockpitViewpoint>),
    /// A body-frame offset from the aircraft's origin, with the camera
    /// oriented along the aircraft's own axes.
    BodyOffset(BodyOffset),
}

impl DeclaredPlacement {
    /// A placement at a verified cockpit viewpoint.
    #[must_use]
    pub fn at_cockpit(viewpoint: CockpitViewpoint) -> Self {
        Self::Cockpit(Box::new(viewpoint))
    }

    /// The stable label used in reports.
    #[must_use]
    pub const fn label(&self) -> &'static str {
        match self {
            Self::Cockpit(_) => "cockpit_viewpoint",
            Self::BodyOffset(_) => "body_offset",
        }
    }

    /// The cockpit binding, when this placement is one.
    #[must_use]
    pub const fn cockpit(&self) -> Option<&CockpitViewpoint> {
        match self {
            Self::Cockpit(viewpoint) => Some(viewpoint),
            Self::BodyOffset(_) => None,
        }
    }

    /// The body-frame offset, whichever placement this is.
    #[must_use]
    pub const fn offset(&self) -> BodyOffset {
        match self {
            Self::Cockpit(viewpoint) => viewpoint.offset(),
            Self::BodyOffset(offset) => *offset,
        }
    }
}

impl fmt::Display for DeclaredPlacement {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cockpit(viewpoint) => write!(f, "cockpit_viewpoint({})", viewpoint.source()),
            Self::BodyOffset(offset) => write!(
                f,
                "body_offset(right {} m, up {} m, forward {} m)",
                offset.right_m().0,
                offset.up_m().0,
                offset.forward_m().0
            ),
        }
    }
}

/// The limits of a free-look offset, in radians.
///
/// Free look is **enhanced** support: the sheet's deliverable keeps "modern
/// free-look/controller support" separate from the original default
/// mappings, so these limits are project design, not a claim about the
/// original game. They are declared per mode because they are a property of
/// the rig, and they are typed because an unclamped look would flip the
/// camera's up axis and turn a view into a disorientation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LookLimits {
    yaw: Radians,
    pitch: Radians,
}

impl LookLimits {
    /// A mode that does not free-look: every offset is clamped to zero.
    pub const FIXED: Self = Self {
        yaw: Radians(0.0),
        pitch: Radians(0.0),
    };

    /// Assembles the limits from a half-extent per axis.
    ///
    /// # Errors
    ///
    /// [`ViewpointError`] when a half-extent is not finite, the yaw is not
    /// inside `(-π, π)` or the pitch is not inside `[-π/2, π/2]`. The pitch
    /// bound keeps the camera's up axis from becoming parallel to its view
    /// direction, where no right axis exists.
    pub fn new(yaw: Radians, pitch: Radians) -> Result<Self, ViewpointError> {
        require_finite_angle("yaw", yaw)?;
        require_finite_angle("pitch", pitch)?;
        if yaw.0.abs() >= std::f64::consts::PI {
            return Err(ViewpointError::LookYawOutOfRange { radians: yaw.0 });
        }
        if pitch.0.abs() > std::f64::consts::FRAC_PI_2 {
            return Err(ViewpointError::LookPitchOutOfRange { radians: pitch.0 });
        }
        Ok(Self { yaw, pitch })
    }

    /// The yaw half-extent.
    #[must_use]
    pub const fn yaw(self) -> Radians {
        self.yaw
    }

    /// The pitch half-extent.
    #[must_use]
    pub const fn pitch(self) -> Radians {
        self.pitch
    }
}

/// Rejects a NaN or infinite look angle, naming the axis.
fn require_finite_angle(field: &'static str, value: Radians) -> Result<(), ViewpointError> {
    if value.0.is_finite() {
        Ok(())
    } else {
        Err(ViewpointError::LookAngleNonFinite { field })
    }
}

/// Why a viewpoint, offset or look-limit record was rejected.
#[derive(Clone, Debug, PartialEq)]
pub enum ViewpointError {
    /// A body offset carried a NaN or infinite component.
    NonFiniteOffset {
        /// Which axis it was, `"right_m"`, `"up_m"` or `"forward_m"`.
        axis: &'static str,
    },
    /// A cockpit binding named no model node or configuration key.
    EmptyBindingName,
    /// A cockpit binding name exceeded [`MAX_COCKPIT_BINDING_NAME`].
    BindingNameTooLong {
        /// The rejected name length in bytes.
        len: usize,
    },
    /// A look angle was NaN or infinite.
    LookAngleNonFinite {
        /// Which angle it was, `"yaw"` or `"pitch"`.
        field: &'static str,
    },
    /// A yaw half-extent fell outside `(-π, π)`.
    LookYawOutOfRange {
        /// The rejected angle in radians.
        radians: f64,
    },
    /// A pitch half-extent fell outside `[-π/2, π/2]`.
    LookPitchOutOfRange {
        /// The rejected angle in radians.
        radians: f64,
    },
}

impl fmt::Display for ViewpointError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NonFiniteOffset { axis } => write!(f, "the body offset {axis} must be finite"),
            Self::EmptyBindingName => {
                write!(
                    f,
                    "a cockpit viewpoint must name the model node or configuration key it was read from"
                )
            }
            Self::BindingNameTooLong { len } => write!(
                f,
                "a cockpit binding name is at most {MAX_COCKPIT_BINDING_NAME} bytes, got {len}"
            ),
            Self::LookAngleNonFinite { field } => {
                write!(f, "the look limit {field} must be finite")
            }
            Self::LookYawOutOfRange { radians } => {
                write!(f, "the look limit yaw {radians} rad is outside (-π, π)")
            }
            Self::LookPitchOutOfRange { radians } => {
                write!(
                    f,
                    "the look limit pitch {radians} rad is outside [-π/2, π/2]"
                )
            }
        }
    }
}

impl std::error::Error for ViewpointError {}

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
    /// A cockpit mode declared a bare body offset instead of a verified
    /// cockpit viewpoint.
    ///
    /// F21 non-negotiable behavior 1: the cockpit viewpoint comes from
    /// verified model/config bindings, and a HUD-only synthetic camera is not
    /// a replacement for every original cockpit. A mode that cannot name a
    /// binding therefore declares no cockpit at all rather than an invented
    /// eye.
    CockpitViewpointRequired {
        /// The mode kind that lacked one.
        kind: CameraModeKind,
    },
    /// A mode that is not a cockpit declared a cockpit viewpoint.
    ///
    /// Only a [`CameraModeKind::Cockpit`] mode *is* a cockpit view; the
    /// other kinds place themselves with a body offset, and claiming the
    /// binding would let an external or spyglass view be described as
    /// "the cockpit" while sitting somewhere else.
    UnexpectedCockpitViewpoint {
        /// The mode kind that declared it.
        kind: CameraModeKind,
    },
    /// A placement or look-limit record was rejected.
    Viewpoint(ViewpointError),
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
            Self::CockpitViewpointRequired { kind } => write!(
                f,
                "the {kind} mode must place itself with a verified cockpit viewpoint binding, \
                 not a bare body offset"
            ),
            Self::UnexpectedCockpitViewpoint { kind } => write!(
                f,
                "only a cockpit mode declares a cockpit viewpoint; the {kind} mode must use a body offset"
            ),
            Self::Viewpoint(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for CameraModeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Viewpoint(error) => Some(error),
            _ => None,
        }
    }
}

impl From<ViewpointError> for CameraModeError {
    fn from(error: ViewpointError) -> Self {
        Self::Viewpoint(error)
    }
}

/// One declared camera mode: its kind, its projection, its placement and its
/// behavior flags.
///
/// The mode is a *sub-record* of a [`DeclaredCameraModes`] set, so it carries
/// no subject of its own — its `kind` is its identity within the set. The
/// [`Resolved`] fields carry their own provenance.
///
/// `placement` is **where** this mode's camera sits relative to the aircraft
/// it follows (F21-B), and `look_limits` is how far a free-look offset may
/// turn the view. Both belong to the mode because both are properties of the
/// view rather than of the session: an aircraft with no verified cockpit
/// binding declares no cockpit mode at all (F21 non-negotiable behavior 1),
/// and every aircraft that does declare one shares the same eye and limits.
#[derive(Clone, Debug, PartialEq)]
pub struct DeclaredCameraMode {
    kind: CameraModeKind,
    projection: ProjectionPolicy,
    magnification: Resolved<Magnification>,
    tracks_target: Resolved<bool>,
    placement: DeclaredPlacement,
    look_limits: Resolved<LookLimits>,
}

impl DeclaredCameraMode {
    /// Assembles and validates a declared mode.
    ///
    /// Only *known* values are range-checked; an unknown is a valid declared
    /// state that the lowering boundary refuses. A non-spyglass mode must
    /// declare [`Magnification::ONE`] when it declares a magnification at
    /// all, and the placement must match the kind: a cockpit mode needs a
    /// [`CockpitViewpoint`], every other kind a [`BodyOffset`].
    ///
    /// # Errors
    ///
    /// [`CameraModeError`] for a corrupt known field of view or clipping
    /// range, a non-spyglass magnification, a placement that does not match
    /// the mode's kind, or a rejected [`ViewpointError`].
    #[allow(clippy::too_many_arguments)]
    pub fn try_new(
        kind: CameraModeKind,
        projection: ProjectionPolicy,
        magnification: Resolved<Magnification>,
        tracks_target: Resolved<bool>,
        placement: DeclaredPlacement,
        look_limits: Resolved<LookLimits>,
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
        match (&placement, kind) {
            (DeclaredPlacement::Cockpit(_), CameraModeKind::Cockpit) => {}
            (DeclaredPlacement::Cockpit(_), kind) => {
                return Err(CameraModeError::UnexpectedCockpitViewpoint { kind });
            }
            (DeclaredPlacement::BodyOffset(_), CameraModeKind::Cockpit) => {
                return Err(CameraModeError::CockpitViewpointRequired { kind });
            }
            (DeclaredPlacement::BodyOffset(_), _) => {}
        }
        Ok(Self {
            kind,
            projection,
            magnification,
            tracks_target,
            placement,
            look_limits,
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

    /// Where this mode's camera sits relative to the aircraft it follows.
    #[must_use]
    pub const fn placement(&self) -> &DeclaredPlacement {
        &self.placement
    }

    /// The free-look limits of this mode, or its explicit unknown.
    #[must_use]
    pub const fn look_limits(&self) -> &Resolved<LookLimits> {
        &self.look_limits
    }
}

/// Whether a catalog kind may own a declared camera mode set.
///
/// The rule is total over [`ContentKind::ALL`] — there is no fall-through and
/// no "unknown owner" case — and it is deliberately two roles:
///
/// * [`ContentKind::Airframe`]: the aircraft a session flies. It owns the
///   views it offers, which is where F21 non-negotiable behavior 1 ("cockpit
///   viewpoint comes from verified model/config bindings") puts the cockpit
///   view: an aircraft with no verified cockpit binding declares no `cockpit`
///   mode at all.
/// * every kind [`ContentKind::is_launchable`] accepts — a campaign mission,
///   an instant-action scenario and a multiplayer scenario: the launchable
///   content a session starts from, which owns the view a session begins in.
///
/// The launchable half is expressed as [`ContentKind::is_launchable`] rather
/// than as a restated list, so the owner vocabulary cannot drift from the
/// launchable baseline the catalog already measures readiness over: a kind
/// that becomes launchable owns a view set without this function being
/// edited. Every other kind is refused, including
/// [`ContentKind::CameraTrack`]: a camera track is an authored sequence,
/// which a mode of kind [`CameraModeKind::AuthoredSequence`] will reference
/// (F21-C) — it is never the subject that owns a view set.
#[must_use]
pub const fn owns_camera_modes(kind: ContentKind) -> bool {
    matches!(kind, ContentKind::Airframe) || kind.is_launchable()
}

/// Why a [`DeclaredCameraModes`] set was rejected.
#[derive(Clone, Debug, PartialEq)]
pub enum CameraModesError {
    /// The subject id is not a kind that may own a mode set, so the set has
    /// no catalog element to be addressed inside. See
    /// [`owns_camera_modes`].
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
            Self::SubjectKindMismatch { subject } => write!(
                f,
                "a camera mode set belongs to the airframe a session flies or to the launchable content it starts from, so {} is not an owner",
                subject
            ),
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

/// One owner's declared camera mode set.
///
/// `subject` is the catalog [`ContentId`] of the element that *owns* the
/// camera — see [`owns_camera_modes`] for the kinds that may own one and for
/// why a mode set is subordinate rather than a namespace of its own. The
/// owner is how the set is addressed: nothing in this record gives the set
/// an identity of its own. `default_mode` names the mode a session starts
/// in, and it must be present; at most one mode of each kind may be
/// declared and every mode must validate.
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
    /// [`CameraModesError`] for a subject that is not a kind which may own a
    /// mode set, an empty set, a duplicated kind, a default that is not a
    /// declared mode or an invalid mode.
    pub fn try_new(
        subject: ContentId,
        origin: Origin,
        default_mode: CameraModeKind,
        modes: Vec<DeclaredCameraMode>,
        provenance: Provenance,
    ) -> Result<Self, CameraModesError> {
        if !owns_camera_modes(subject.kind()) {
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

    /// The owner id this set is addressed inside.
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

/// The synthetic airframe that owns the declared camera modes
/// (`airframe` kind). It is a design fixture key, not an original plane.
pub const SYNTHETIC_CAMERA_MODE_OWNER_KEY: &str = "synthetic.camera-plane";

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
///
/// The placements are designed the same way: the cockpit binds a **synthetic**
/// model node (`synthetic.pilot_eye`) one and a half metres forward and 1.2 m
/// above the body origin, the chase view sits 12 m behind and 3 m above it, and
/// the spyglass looks through the body origin. The cockpit node name is a
/// fixture key: it is not a node of any original model, and the fixture's
/// [`Origin::SyntheticFixture`] is exactly what keeps that from being read as
/// a verified binding.
///
/// The owner is a synthetic **airframe** (`airframe/synthetic.camera-plane`),
/// because a view set is what an aircraft offers a session: the cockpit mode
/// is only available to an aircraft with a verified cockpit binding (F21
/// non-negotiable behavior 1), and this fixture owns no original plane.
/// Nothing about the airframe is a claim that such a plane exists.
#[must_use]
pub fn declared_synthetic_camera_modes() -> DeclaredCameraModes {
    let cockpit = DeclaredCameraMode::try_new(
        CameraModeKind::Cockpit,
        ProjectionPolicy::designed(degrees(60.0), 0.1, 10_000.0),
        known(Magnification::ONE),
        known(false),
        DeclaredPlacement::at_cockpit(
            CockpitViewpoint::try_new(
                CockpitBindingSource::ModelNode {
                    node: "synthetic.pilot_eye".to_owned(),
                },
                BodyOffset::new(Meters(0.0), Meters(1.2), Meters(1.5))
                    .expect("the synthetic pilot eye offset is finite"),
                known(Radians(0.0)),
                known(Radians(0.0)),
            )
            .expect("the synthetic cockpit viewpoint is valid"),
        ),
        known(synthetic_look_limits()),
    )
    .expect("the synthetic cockpit mode is valid");
    let external = DeclaredCameraMode::try_new(
        CameraModeKind::External,
        ProjectionPolicy::designed(degrees(55.0), 0.1, 10_000.0),
        known(Magnification::ONE),
        known(false),
        DeclaredPlacement::BodyOffset(
            // Negative is behind: the chase view is 12 m astern and 3 m above
            // the body origin, which is the whole point of a chase view.
            BodyOffset::new(Meters(0.0), Meters(3.0), Meters(-12.0))
                .expect("the synthetic chase offset is finite"),
        ),
        known(synthetic_look_limits()),
    )
    .expect("the synthetic external mode is valid");
    let spyglass = DeclaredCameraMode::try_new(
        CameraModeKind::Spyglass,
        ProjectionPolicy::designed(degrees(20.0), 1.0, 20_000.0),
        known(Magnification::new(4.0).expect("4.0 is a valid magnification")),
        known(true),
        // The spyglass looks *through* the aircraft along the selected
        // target, so its own eye is the body origin; the frustum it uses is
        // the spyglass mode's, with the spyglass's own clipping planes.
        DeclaredPlacement::BodyOffset(BodyOffset::ZERO),
        known(synthetic_look_limits()),
    )
    .expect("the synthetic spyglass mode is valid");

    DeclaredCameraModes::try_new(
        ContentId::from_source(ContentKind::Airframe, SYNTHETIC_CAMERA_MODE_OWNER_KEY)
            .expect("fixture owner id is valid"),
        Origin::SyntheticFixture,
        CameraModeKind::Cockpit,
        vec![cockpit, external, spyglass],
        Provenance::designed(claim()),
    )
    .expect("the declared synthetic camera mode fixture is valid")
}

/// The synthetic airframe's free-look limits: 120° either way and 60° up or
/// down.
///
/// Project design, not a measured original range (see [`LookLimits`]): the
/// numbers only have to keep the camera's up axis away from its view
/// direction and give a pilot a wide glance.
fn synthetic_look_limits() -> LookLimits {
    LookLimits::new(
        Radians(120.0_f64.to_radians()),
        Radians(60.0_f64.to_radians()),
    )
    .expect("the synthetic look limits are in range")
}
