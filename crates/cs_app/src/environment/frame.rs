//! The sky frame: a world-oriented sky centred on the camera (F19-A).
//!
//! Spec: `specs/F19-sky-atmosphere-weather-and-visibility.md`, stage
//! `### F19-A`, non-negotiable behavior 3 and acceptance case AC01.
//!
//! F19 behavior 3 has two halves, and [`SkyFrame`] is where both become one
//! record a renderer consumes:
//!
//! * **centred on camera translation** — the dome's world position *is* the
//!   camera's world position ([`SkyFrame::dome_position`]). It is computed
//!   in **world** space, from [`SpatialAnchor::world`] or by converting the
//!   camera's local coordinate through
//!   [`WorldOrigin::world_of`](crate::origin::WorldOrigin::world_of), so
//!   rebasing the world — which moves the origin and rewrites every local
//!   coordinate — changes neither the dome's world position nor the frame's
//!   epoch-dependent view of it.
//! * **honouring world orientation** — the sky orientation and the sun
//!   direction are *world* values copied verbatim from the authored
//!   [`EnvironmentDefinition`](cs_content::environment::EnvironmentDefinition).
//!   No camera rotation and no origin translation is applied to them, so a
//!   rebase can neither rotate the sky nor move the sun.
//!
//! That is what AC01 measures: rebase the world under a fixed horizon and
//! the sky and sun direction stay stable. [`SkyFrame::is_centered_on`] makes
//! the other half observable too — a frame captured before a rebase is still
//! centred after it (a rebase does not pop the sky), while a frame captured
//! before the *camera moved* is detectably stale (a moving camera does).
//!
//! Nothing here renders: `cs_app::environment` owns the record, and F19-B's
//! renderer converts it into a dome and a light.

use std::fmt;

use cs_content::environment::{EnvironmentDefinition, SkyOrientation};
use cs_types::content::Resolved;
use cs_types::space::{LocalPosition, UnitVec3, WorldPosition};

use crate::origin::{OriginEpoch, OriginError, SpatialAnchor, WorldOrigin};

/// How far from the camera a dome may sit and still count as centred, in
/// meters.
///
/// A rebase converts a world coordinate through a new origin and back, which
/// costs one floating-point rounding of a value that can be kilometres
/// away — far below a millimetre of error. The tolerance absorbs that
/// rounding and nothing else: a dome left in the *local* frame is off by
/// the origin offset, which is hundreds of metres in the acceptance
/// scenario.
pub const SKY_CENTERING_TOLERANCE_M: f64 = 1e-6;

/// Why a [`SkyFrame`] could not be built from local coordinates.
#[derive(Clone, Debug, PartialEq)]
pub enum SkyFrameError {
    /// The camera's local coordinate could not be converted through the
    /// origin.
    Origin(OriginError),
}

impl fmt::Display for SkyFrameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Origin(error) => {
                write!(f, "the camera's local position is not convertible: {error}")
            }
        }
    }
}

impl std::error::Error for SkyFrameError {}

impl From<OriginError> for SkyFrameError {
    fn from(error: OriginError) -> Self {
        Self::Origin(error)
    }
}

/// One environment's sky as the renderer should draw it: where the dome
/// sits, which way it is oriented and where the sun points.
///
/// The frame is a *value*, not a system: capturing it costs nothing and it
/// can be held across a rebase. See the module documentation for why that
/// matters.
#[derive(Clone, Debug, PartialEq)]
pub struct SkyFrame {
    epoch: OriginEpoch,
    dome_position: WorldPosition,
    sky_orientation: Resolved<SkyOrientation>,
    sun_direction: Resolved<UnitVec3>,
}

impl SkyFrame {
    /// Captures the frame at the camera's **world** position.
    ///
    /// This is the path a running session uses: the camera's anchor already
    /// knows its world position and the origin epoch it was converted in,
    /// so the frame cannot accidentally read a local coordinate as a world
    /// one.
    #[must_use]
    pub fn capture(environment: &EnvironmentDefinition, camera: &SpatialAnchor) -> Self {
        Self {
            epoch: camera.epoch(),
            dome_position: camera.world(),
            sky_orientation: environment.sky_orientation().clone(),
            sun_direction: environment.sun_direction().clone(),
        }
    }

    /// Captures the frame from the camera's **local** position and the
    /// origin it lives in.
    ///
    /// The local coordinate is converted through
    /// [`WorldOrigin::world_of`](crate::origin::WorldOrigin::world_of), so
    /// the dome is placed in world space on both sides of a rebase. This is
    /// the constructor that fails observably if the conversion is skipped:
    /// the dome would be parked at the camera's local coordinate and jump by
    /// the origin offset the moment the world rebased.
    ///
    /// # Errors
    ///
    /// [`SkyFrameError::Origin`] when the local coordinate does not convert
    /// (a non-finite sum at the origin).
    pub fn from_local(
        environment: &EnvironmentDefinition,
        origin: &WorldOrigin,
        camera_local: LocalPosition,
    ) -> Result<Self, SkyFrameError> {
        Ok(Self {
            epoch: origin.epoch(),
            dome_position: origin.world_of(camera_local)?,
            sky_orientation: environment.sky_orientation().clone(),
            sun_direction: environment.sun_direction().clone(),
        })
    }

    /// The origin epoch this frame was captured in.
    ///
    /// The epoch is *evidence about where the camera was when the frame was
    /// taken*, not an input to the dome's world pose: a frame whose epoch is
    /// older than the current origin but whose camera has not moved is still
    /// valid, which is exactly the AC01 situation.
    #[must_use]
    pub const fn epoch(&self) -> OriginEpoch {
        self.epoch
    }

    /// The dome's position in world meters: the camera's world position at
    /// capture time.
    #[must_use]
    pub const fn dome_position(&self) -> WorldPosition {
        self.dome_position
    }

    /// The sky's world orientation, or the explicit unknown its evidence
    /// left. Carried verbatim: neither the camera nor the origin may
    /// rotate it.
    #[must_use]
    pub fn sky_orientation(&self) -> &Resolved<SkyOrientation> {
        &self.sky_orientation
    }

    /// The sun's world direction, or the explicit unknown its evidence
    /// left. Carried verbatim for the same reason.
    #[must_use]
    pub fn sun_direction(&self) -> &Resolved<UnitVec3> {
        &self.sun_direction
    }

    /// The known sun direction, or `None` when the record left it unknown —
    /// an unknown sun is reported, never replaced by a default.
    #[must_use]
    pub fn known_sun_direction(&self) -> Option<UnitVec3> {
        match &self.sun_direction {
            Resolved::Known(known) => Some(known.value),
            Resolved::Unknown { .. } => None,
        }
    }

    /// The horizon normal of the frame's orientation, or `None` when the
    /// orientation is unknown.
    #[must_use]
    pub fn horizon_normal(&self) -> Option<UnitVec3> {
        match &self.sky_orientation {
            Resolved::Known(known) => Some(known.value.horizon_normal()),
            Resolved::Unknown { .. } => None,
        }
    }

    /// Whether the dome still sits on `camera_world` within `tolerance_m`.
    ///
    /// A rebase leaves this `true` (both points are world positions and
    /// neither moved), while a camera that flew away since the capture
    /// leaves it `false` — the caller must capture a fresh frame. F19-B uses
    /// it to detect a stale frame instead of popping the sky.
    #[must_use]
    pub fn is_centered_on(&self, camera_world: WorldPosition, tolerance_m: f64) -> bool {
        let dome = self.dome_position.to_array();
        let camera = camera_world.to_array();
        let distance = (0..3)
            .map(|axis| (dome[axis] - camera[axis]).powi(2))
            .sum::<f64>()
            .sqrt();
        distance <= tolerance_m
    }
}
