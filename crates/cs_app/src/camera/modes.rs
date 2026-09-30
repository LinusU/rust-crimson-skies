//! Lowering declared camera modes into runtime mode records (F21-A).
//!
//! Spec: `specs/F21-cameras-cockpit-views-and-spyglass.md`, stage
//! `### F21-A`. Shared contract: `docs/contracts/UI-NETWORK.md`.
//!
//! [`DeclaredCameraModes`] is the content half; [`LoweredCameraModes`] is
//! what a session's camera path consumes. The boundary is deliberately
//! total on the refusing side: a mode whose projection or behavior flag is
//! `Resolved::Unknown` refuses by name, so a renderer never runs under a
//! guessed magnification or target-tracking flag — the same discipline
//! `cs_app::targeting::lower_rules` and `cs_app::environment` follow.
//!
//! F21-B implements the actual cockpit/external/look/spyglass rigs on top
//! of these records; this stage only defines and lowers them.

use std::fmt;

use cs_content::cameras::{CameraModeKind, DeclaredCameraMode, DeclaredCameraModes, Magnification};
use cs_types::content::Resolved;
use cs_types::evidence::ClaimId;

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

/// One declared mode as the runtime consumes it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LoweredCameraMode {
    kind: CameraModeKind,
    projection: LoweredProjection,
    magnification: Magnification,
    tracks_target: bool,
}

impl LoweredCameraMode {
    /// Which kind of view this mode is.
    #[must_use]
    pub const fn kind(self) -> CameraModeKind {
        self.kind
    }

    /// The lowered projection policy.
    #[must_use]
    pub const fn projection(self) -> LoweredProjection {
        self.projection
    }

    /// The magnification factor (`1.0` for every non-spyglass mode).
    #[must_use]
    pub const fn magnification(self) -> Magnification {
        self.magnification
    }

    /// Whether the mode tracks the currently selected target.
    #[must_use]
    pub const fn tracks_target(self) -> bool {
        self.tracks_target
    }
}

/// One subject's lowered camera mode set: a default plus every mode.
#[derive(Clone, Debug, PartialEq)]
pub struct LoweredCameraModes {
    default_mode: CameraModeKind,
    modes: Vec<LoweredCameraMode>,
}

impl LoweredCameraModes {
    /// The mode a session starts in.
    #[must_use]
    pub const fn default_mode(&self) -> CameraModeKind {
        self.default_mode
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
/// magnification or target-tracking flag.
pub fn lower_camera_mode(mode: &DeclaredCameraMode) -> Result<LoweredCameraMode, CameraLowerError> {
    let projection = lower_projection(mode.projection())?;
    let magnification = resolve_mode_field(mode.kind(), "magnification", mode.magnification())?;
    let tracks_target = resolve_mode_field(mode.kind(), "tracks_target", mode.tracks_target())?;
    Ok(LoweredCameraMode {
        kind: mode.kind(),
        projection,
        magnification,
        tracks_target,
    })
}

/// Lowers a declared camera mode set.
///
/// The default mode is copied from the declared set, which validated that it
/// is one of the set's own modes.
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
        modes: lowered,
    })
}
