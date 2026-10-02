//! Frame-rate independent camera smoothing (F21-B).
//!
//! Spec: `specs/F21-cameras-cockpit-views-and-spyglass.md`, stage
//! `### F21-B`, non-negotiable behavior 4: "camera smoothing is frame-rate
//! independent, reset on teleport/plane swap and preserved correctly through
//! origin shifts". Shared contract: `docs/contracts/UI-NETWORK.md`.
//!
//! # The law, and why it is frame-rate independent
//!
//! The camera closes a fixed *fraction of the remaining distance per second*,
//! not a fixed fraction per frame:
//!
//! ```text
//! α(dt) = 1 − e^(−k·dt)
//! pose ← pose + α(dt)·(desired − pose)
//! ```
//!
//! The point is that `1 − α(dt) = e^(−k·dt)`, so after `n` frames the weight
//! still on the starting pose is the product of `n` such factors:
//! `e^(−k·Σdt)`. Over the same wall time the result is the **same number** at
//! 30, 60 or 144 FPS, to floating-point rounding. The alternatives both fail
//! it: a constant `α` per frame makes a fast machine lag more frames than a
//! slow one, and a velocity in metres per second multiplied by `dt` is exact
//! only for a straight line, not for the exponential approach a camera needs.
//!
//! The rotation is interpolated the same way, on the shortest arc: the two
//! quaternions are negated into the same hemisphere and then
//! normalized-linear-interpolated. Sign-corrected nlerp, not slerp, is a
//! deliberate choice — with the `α` above, both reach the target on the same
//! schedule, and nlerp needs no arc-angle series, so it stays exact where
//! slerp's `sin(θ)/θ` limit would be a branch of its own.
//!
//! # Canonical world space
//!
//! The smoothed pose is a [`CameraPose`] in canonical f64 world space, the
//! same space the aircraft's authoritative pose is published in. That is what
//! makes "preserved correctly through origin shifts" true by construction: an
//! [`OriginShift`](crate::origin::OriginShift) moves the *local* f32 frame and
//! leaves world identity alone, so a rebase has nothing to convert here and
//! cannot make the camera jump. A teleport, which *is* a world-space move, is
//! the case that needs [`PoseSmoother::snap`] — the rig decides, and it
//! decides from the [`OriginChange`](crate::origin::OriginChange) value rather
//! than from a distance heuristic.
//!
//! The response rate is project design; the original camera's own smoothing
//! is unmeasured (F21-D).

use std::fmt;
use std::time::Duration;

use cs_types::space::{Quaternion, SpaceError, WorldPosition};

use super::pose::CameraPose;

/// Why a smoothing operation was refused.
#[derive(Clone, Debug, PartialEq)]
pub enum SmoothingError {
    /// The response rate was NaN, infinite or not positive, so no smoothing
    /// law is defined for it.
    InvalidResponse {
        /// The rejected rate, in reciprocal seconds.
        per_second: f64,
    },
    /// The space boundary rejected a derived value.
    Space(SpaceError),
}

impl fmt::Display for SmoothingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidResponse { per_second } => write!(
                f,
                "the camera response rate must be finite and positive, got {per_second}/s"
            ),
            Self::Space(error) => write!(f, "the camera pose was rejected: {error}"),
        }
    }
}

impl std::error::Error for SmoothingError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::InvalidResponse { .. } => None,
            Self::Space(error) => Some(error),
        }
    }
}

impl From<SpaceError> for SmoothingError {
    fn from(error: SpaceError) -> Self {
        Self::Space(error)
    }
}

/// How a camera's published pose came to be what it is this frame.
///
/// A consumer that draws every frame does not need this, but a consumer that
/// captures, records or hashes poses does: it must be able to say whether the
/// camera was still moving or had come to rest, without comparing two
/// positions itself and guessing the tolerance.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SmoothingState {
    /// The camera jumped to the desired pose: the first frame, a teleport, or
    /// a plane swap. The pose is the desired one exactly.
    Reseated,
    /// The pose is exactly the pose the rig asked for.
    Settled,
    /// The pose is not the one the rig asked for, so the camera is still on
    /// its way — including a frame the clock gave no wall time, which leaves
    /// the camera where it was rather than where it was asked to be.
    Tracking,
}

impl SmoothingState {
    /// Whether the camera is exactly where the rig asked it to be.
    #[must_use]
    pub const fn is_settled(self) -> bool {
        matches!(self, Self::Settled | Self::Reseated)
    }
}

/// The exponential smoother a rig runs its camera pose through.
///
/// The smoother owns no flight state: it is handed the *desired* pose a rig
/// derived from the authoritative aircraft pose and returns where the camera
/// actually is. It never writes back.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PoseSmoother {
    response_per_s: f64,
    pose: Option<CameraPose>,
    desired: Option<CameraPose>,
    state: SmoothingState,
}

impl PoseSmoother {
    /// A smoother with no pose yet, closing a fraction of the remaining
    /// distance `response_per_s` times per second.
    ///
    /// # Errors
    ///
    /// [`SmoothingError::InvalidResponse`] when the rate is not finite and
    /// strictly positive: zero would never move the camera and a negative one
    /// would run it away from the desired pose.
    pub fn new(response_per_s: f64) -> Result<Self, SmoothingError> {
        if !response_per_s.is_finite() || response_per_s <= 0.0 {
            return Err(SmoothingError::InvalidResponse {
                per_second: response_per_s,
            });
        }
        Ok(Self {
            response_per_s,
            pose: None,
            desired: None,
            state: SmoothingState::Reseated,
        })
    }

    /// The configured response rate, in reciprocal seconds.
    #[must_use]
    pub const fn response_per_s(self) -> f64 {
        self.response_per_s
    }

    /// Where the camera is, when it has been placed at least once.
    #[must_use]
    pub const fn pose(self) -> Option<CameraPose> {
        self.pose
    }

    /// The pose the rig last asked for, when it has asked.
    #[must_use]
    pub const fn desired(self) -> Option<CameraPose> {
        self.desired
    }

    /// How the current pose came to be.
    #[must_use]
    pub const fn state(self) -> SmoothingState {
        self.state
    }

    /// Whether the camera is exactly where the rig asked it to be.
    #[must_use]
    pub fn is_settled(self) -> bool {
        self.pose.is_some() && self.pose == self.desired
    }

    /// Jumps straight to `desired`, discarding the lag.
    ///
    /// This is the teleport, plane-swap and first-frame path (F21
    /// non-negotiable behavior 4): there is no continuous motion to
    /// interpolate from, so carrying the previous pose forward would drag the
    /// camera across the world over the following frames.
    pub fn snap(&mut self, desired: CameraPose) {
        self.pose = Some(desired);
        self.desired = Some(desired);
        self.state = SmoothingState::Reseated;
    }

    /// Forgets the pose entirely, so the next [`advance`](Self::advance)
    /// reseats instead of interpolating.
    ///
    /// The end-of-session path: a rig that is torn down and rebuilt for the
    /// next generation must not inherit the previous one's camera position.
    pub fn clear(&mut self) {
        self.pose = None;
        self.desired = None;
        self.state = SmoothingState::Reseated;
    }

    /// Moves toward `desired` over `elapsed` wall time and returns the pose
    /// the camera is now at.
    ///
    /// The first frame reseats (there is nothing to interpolate from). A
    /// zero-length frame records what it was asked for and leaves the camera
    /// where it is: no wall time passed, so no smoothing happened, and a
    /// frame rate of zero is not a camera that lags behind — but the camera is
    /// then *not* settled, because it is not where it was asked to be.
    ///
    /// # Errors
    ///
    /// [`SmoothingError::Space`] when an interpolated component is not a
    /// usable position or rotation. Nothing is mutated on error.
    pub fn advance(
        &mut self,
        desired: CameraPose,
        elapsed: Duration,
    ) -> Result<CameraPose, SmoothingError> {
        if self.pose.is_none() {
            self.snap(desired);
            return Ok(desired);
        }
        let seconds = elapsed.as_secs_f64();
        if seconds <= 0.0 {
            self.desired = Some(desired);
            let pose = self.pose.expect("the pose exists");
            self.state = if pose == desired {
                SmoothingState::Settled
            } else {
                SmoothingState::Tracking
            };
            return Ok(pose);
        }

        let alpha = 1.0 - (-self.response_per_s * seconds).exp();
        let current = self.pose.expect("the pose exists");
        let blended = CameraPose::new(
            lerp_position(current.position(), desired.position(), alpha)?,
            nlerp(current.rotation(), desired.rotation(), alpha)?,
        );
        self.pose = Some(blended);
        self.desired = Some(desired);
        self.state = if blended == desired {
            SmoothingState::Settled
        } else {
            SmoothingState::Tracking
        };
        Ok(blended)
    }
}

/// Interpolates a position by `alpha` of the way from `from` to `to`.
///
/// # Errors
///
/// [`SpaceError::NonFinite`] when a component of the result is NaN or
/// infinite.
fn lerp_position(
    from: WorldPosition,
    to: WorldPosition,
    alpha: f64,
) -> Result<WorldPosition, SpaceError> {
    let [fx, fy, fz] = from.to_array();
    let [tx, ty, tz] = to.to_array();
    WorldPosition::try_new([
        fx + (tx - fx) * alpha,
        fy + (ty - fy) * alpha,
        fz + (tz - fz) * alpha,
    ])
}

/// Interpolates a rotation by `alpha` of the way along the shortest arc.
///
/// The sign correction is what makes "shortest arc" true: `q` and `-q` are the
/// same rotation, so an interpolation between them without flipping would turn
/// the long way round. The result is renormalized because nlerp does not
/// preserve unit length by itself.
///
/// # Errors
///
/// [`SpaceError::NonFinite`] or [`SpaceError::NotUnit`] when the intermediate
/// is degenerate — only reachable if a rotation reached this function outside
/// `Quaternion::try_new`.
fn nlerp(from: Quaternion, to: Quaternion, alpha: f64) -> Result<Quaternion, SpaceError> {
    let first = from.components();
    let mut second = to.components();
    let dot =
        first[0] * second[0] + first[1] * second[1] + first[2] * second[2] + first[3] * second[3];
    if dot < 0.0 {
        for component in &mut second {
            *component = -*component;
        }
    }
    let mut blended = [0.0; 4];
    for (index, component) in blended.iter_mut().enumerate() {
        *component = first[index] + (second[index] - first[index]) * alpha;
    }
    let length = (blended[0] * blended[0]
        + blended[1] * blended[1]
        + blended[2] * blended[2]
        + blended[3] * blended[3])
        .sqrt();
    if !length.is_finite() || length == 0.0 {
        return Err(SpaceError::NotUnit { length });
    }
    for component in &mut blended {
        *component /= length;
    }
    Quaternion::try_new(blended)
}
