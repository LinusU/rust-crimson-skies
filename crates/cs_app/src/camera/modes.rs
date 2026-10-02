//! Lowering declared camera modes into runtime mode records (F21-A, F21-B).
//!
//! Spec: `specs/F21-cameras-cockpit-views-and-spyglass.md`, stages
//! `### F21-A` and `### F21-B`. Shared contract:
//! `docs/contracts/UI-NETWORK.md`.
//!
//! [`DeclaredCameraModes`] is the content half; [`LoweredCameraModes`] is
//! what a session's camera path consumes. The boundary is deliberately
//! total on the refusing side: a mode whose projection, placement,
//! magnification or target-tracking flag is an explicit unknown refuses by
//! name, so a renderer never runs under a guessed magnification, a guessed
//! cockpit eye or a guessed target-tracking flag — the same discipline
//! `cs_app::targeting::lower_rules` and `cs_app::environment` follow.
//!
//! F21-A defined and lowered the modes and their projection policy. F21-B adds
//! the two fields the rigs consume: [`LoweredPlacement`] — **where** the
//! camera sits relative to the aircraft, and the free-look
//! [`LookLimits`] — plus the [`LoweredCockpitViewpoint`] that keeps the
//! binding a cockpit eye was read from, so F21 non-negotiable behavior 1 stays
//! answerable after lowering.

use std::fmt;

use cs_content::cameras::{
    BodyOffset, CameraModeKind, CockpitBindingSource, DeclaredCameraMode, DeclaredCameraModes,
    DeclaredPlacement, LookLimits, Magnification,
};
use cs_types::content::{Origin, Resolved};
use cs_types::evidence::ClaimId;
use cs_types::space::Radians;

use super::projection::{LoweredProjection, ProjectionLowerError, lower_projection};

/// Why declared camera modes could not be lowered.
#[derive(Clone, Debug, PartialEq)]
pub enum CameraLowerError {
    /// A mode's projection policy could not be lowered.
    Projection(ProjectionLowerError),
    /// A mode behavior field is `Resolved::Unknown`: no session may run a
    /// mode under a guessed magnification or target-tracking flag.
    UnknownField {
        /// Which mode declared it.
        mode: CameraModeKind,
        /// Which field is unknown.
        field: &'static str,
        /// The claim the unknown is recorded under.
        claim_id: ClaimId,
        /// Why the value is unknown.
        reason: String,
    },
}

impl fmt::Display for CameraLowerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Projection(error) => write!(f, "{error}"),
            Self::UnknownField {
                mode,
                field,
                claim_id,
                reason,
            } => write!(
                f,
                "the {mode} mode's {field} is unknown ({}: {reason}) and cannot be lowered",
                claim_id.as_str()
            ),
        }
    }
}

impl std::error::Error for CameraLowerError {}

impl From<ProjectionLowerError> for CameraLowerError {
    fn from(error: ProjectionLowerError) -> Self {
        Self::Projection(error)
    }
}

/// A verified cockpit viewpoint as a rig consumes it: the binding it was read
/// from, the body-frame offset it sits at, and the pilot's head orientation.
///
/// The binding is kept, not dropped, so a frame can report where its eye came
/// from. F21 non-negotiable behavior 1 is a question about provenance
/// ("verified model/config bindings"), and a rig that threw the binding away
/// at lowering time would leave nothing to answer it with.
#[derive(Clone, Debug, PartialEq)]
pub struct LoweredCockpitViewpoint {
    source: CockpitBindingSource,
    offset: BodyOffset,
    yaw: Radians,
    pitch: Radians,
}

impl LoweredCockpitViewpoint {
    /// The model node or configuration key the eye was bound from.
    #[must_use]
    pub const fn source(&self) -> &CockpitBindingSource {
        &self.source
    }

    /// Where the eye sits in the aircraft's body frame.
    #[must_use]
    pub const fn offset(&self) -> BodyOffset {
        self.offset
    }

    /// The pilot's head yaw, known: an unknown refused at lowering.
    #[must_use]
    pub const fn yaw(&self) -> Radians {
        self.yaw
    }

    /// The pilot's head pitch, known: an unknown refused at lowering.
    #[must_use]
    pub const fn pitch(&self) -> Radians {
        self.pitch
    }
}

/// Where a lowered mode's camera sits relative to the aircraft it follows.
#[derive(Clone, Debug, PartialEq)]
pub enum LoweredPlacement {
    /// A verified cockpit viewpoint.
    Cockpit(LoweredCockpitViewpoint),
    /// A body-frame offset from the aircraft's origin.
    BodyOffset(BodyOffset),
}

impl LoweredPlacement {
    /// The stable label used in reports.
    #[must_use]
    pub const fn label(&self) -> &'static str {
        match self {
            Self::Cockpit(_) => "cockpit_viewpoint",
            Self::BodyOffset(_) => "body_offset",
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

    /// The head orientation this placement declares, when it declares one.
    ///
    /// A body offset has none: the camera looks along the aircraft's own axes.
    #[must_use]
    pub const fn head_orientation(&self) -> Option<(Radians, Radians)> {
        match self {
            Self::Cockpit(viewpoint) => Some((viewpoint.yaw(), viewpoint.pitch())),
            Self::BodyOffset(_) => None,
        }
    }

    /// The cockpit viewpoint, when this placement is one.
    #[must_use]
    pub const fn cockpit(&self) -> Option<&LoweredCockpitViewpoint> {
        match self {
            Self::Cockpit(viewpoint) => Some(viewpoint),
            Self::BodyOffset(_) => None,
        }
    }
}

/// One declared mode as the runtime consumes it.
#[derive(Clone, Debug, PartialEq)]
pub struct LoweredCameraMode {
    kind: CameraModeKind,
    projection: LoweredProjection,
    magnification: Magnification,
    tracks_target: bool,
    placement: LoweredPlacement,
    look_limits: LookLimits,
}

impl LoweredCameraMode {
    /// Which kind of view this mode is.
    #[must_use]
    pub const fn kind(&self) -> CameraModeKind {
        self.kind
    }

    /// The lowered projection policy.
    #[must_use]
    pub const fn projection(&self) -> LoweredProjection {
        self.projection
    }

    /// The magnification factor (`1.0` for every non-spyglass mode).
    #[must_use]
    pub const fn magnification(&self) -> Magnification {
        self.magnification
    }

    /// Whether the mode tracks the currently selected target.
    #[must_use]
    pub const fn tracks_target(&self) -> bool {
        self.tracks_target
    }

    /// Where this mode's camera sits relative to the aircraft it follows.
    #[must_use]
    pub const fn placement(&self) -> &LoweredPlacement {
        &self.placement
    }

    /// The free-look limits of this mode, known: an unknown refused at
    /// lowering.
    #[must_use]
    pub const fn look_limits(&self) -> LookLimits {
        self.look_limits
    }
}

/// One subject's lowered camera mode set: a default plus every mode.
#[derive(Clone, Debug, PartialEq)]
pub struct LoweredCameraModes {
    default_mode: CameraModeKind,
    origin: Origin,
    modes: Vec<LoweredCameraMode>,
}

impl LoweredCameraModes {
    /// The mode a session starts in.
    #[must_use]
    pub const fn default_mode(&self) -> CameraModeKind {
        self.default_mode
    }

    /// Where the declared set came from.
    ///
    /// Copied through the lowering boundary so a rig can still answer F21
    /// non-negotiable behavior 1's provenance question: an eye bound from
    /// `Origin::SyntheticFixture` is development content and can never be
    /// reported as a verified original binding.
    #[must_use]
    pub const fn origin(&self) -> &Origin {
        &self.origin
    }

    /// Every lowered mode, in authored order.
    #[must_use]
    pub fn modes(&self) -> &[LoweredCameraMode] {
        &self.modes
    }

    /// The lowered mode of a given kind, when present.
    #[must_use]
    pub fn get(&self, kind: CameraModeKind) -> Option<&LoweredCameraMode> {
        self.modes.iter().find(|mode| mode.kind() == kind)
    }

    /// How many modes the set holds.
    #[must_use]
    pub fn len(&self) -> usize {
        self.modes.len()
    }

    /// Whether the set holds no modes. A lowered set never is: the declared
    /// set it comes from must declare at least one mode.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.modes.is_empty()
    }
}

fn resolve_mode_field<T>(
    mode: CameraModeKind,
    field: &'static str,
    value: &Resolved<T>,
) -> Result<T, CameraLowerError>
where
    T: Clone,
{
    match value {
        Resolved::Known(known) => Ok(known.value.clone()),
        Resolved::Unknown { claim_id, reason } => Err(CameraLowerError::UnknownField {
            mode,
            field,
            claim_id: claim_id.clone(),
            reason: reason.clone(),
        }),
    }
}

/// Lowers one declared camera mode.
///
/// # Errors
///
/// [`CameraLowerError::Projection`] for an unknown or corrupt projection
/// field, and [`CameraLowerError::UnknownField`] for an unknown
/// magnification, target-tracking flag, cockpit head orientation or free-look
/// limit.
pub fn lower_camera_mode(mode: &DeclaredCameraMode) -> Result<LoweredCameraMode, CameraLowerError> {
    let projection = lower_projection(mode.projection())?;
    let magnification = resolve_mode_field(mode.kind(), "magnification", mode.magnification())?;
    let tracks_target = resolve_mode_field(mode.kind(), "tracks_target", mode.tracks_target())?;
    let placement = lower_placement(mode.kind(), mode.placement())?;
    let look_limits = resolve_mode_field(mode.kind(), "look_limits", mode.look_limits())?;
    Ok(LoweredCameraMode {
        kind: mode.kind(),
        projection,
        magnification,
        tracks_target,
        placement,
        look_limits,
    })
}

/// Lowers a declared placement, refusing an unknown head orientation.
///
/// A body offset has nothing to resolve — it was range-checked when it was
/// declared — so only a cockpit viewpoint can refuse here, and it refuses on
/// the two angles that decide where the pilot is looking from inside the
/// cockpit.
fn lower_placement(
    kind: CameraModeKind,
    placement: &DeclaredPlacement,
) -> Result<LoweredPlacement, CameraLowerError> {
    let DeclaredPlacement::Cockpit(viewpoint) = placement else {
        return Ok(LoweredPlacement::BodyOffset(placement.offset()));
    };
    let yaw = resolve_mode_field(kind, "cockpit_viewpoint.yaw", viewpoint.yaw())?;
    let pitch = resolve_mode_field(kind, "cockpit_viewpoint.pitch", viewpoint.pitch())?;
    Ok(LoweredPlacement::Cockpit(LoweredCockpitViewpoint {
        source: viewpoint.source().clone(),
        offset: viewpoint.offset(),
        yaw,
        pitch,
    }))
}

/// Lowers a declared camera mode set.
///
/// The default mode is copied from the declared set, which validated that it
/// is one of the set's own modes, and so is its [`Origin`].
///
/// # Errors
///
/// [`CameraLowerError`] for the first mode that refuses to lower.
pub fn lower_camera_modes(
    modes: &DeclaredCameraModes,
) -> Result<LoweredCameraModes, CameraLowerError> {
    let lowered = modes
        .modes()
        .iter()
        .map(lower_camera_mode)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(LoweredCameraModes {
        default_mode: modes.default_mode(),
        origin: modes.origin().clone(),
        modes: lowered,
    })
}
