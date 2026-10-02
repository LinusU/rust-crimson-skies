//! The cockpit, chase, look and spyglass rigs (F21-B).
//!
//! Spec: `specs/F21-cameras-cockpit-views-and-spyglass.md`, stage
//! `### F21-B`. Shared contract: `docs/contracts/UI-NETWORK.md`.
//!
//! [`CameraRig`] turns the F21-A records into the pose a renderer uses. It
//! is a **consumer**: it reads the authoritative aircraft pose, the viewport
//! aspect, the frame's wall time and the session's published spyglass view,
//! and it writes nothing back. There is no flight state here, no selection
//! state, and no store handle — the sheet's deliverable is "CameraRig
//! consumes authoritative aircraft pose but never writes flight state", and
//! F21 non-negotiable behavior 3's "it must not change target authority or
//! fire direction" is enforced by that shape rather than by a check.
//!
//! # The four rigs
//!
//! | rig | eye | orientation | magnification |
//! | --- | --- | --- | --- |
//! | [`ViewRig::Cockpit`] | the declared cockpit binding, in the body frame | the aircraft's attitude with the pilot's declared head turn | 1 |
//! | [`ViewRig::Chase`] | the declared body offset | the aircraft's own axes | 1 |
//! | [`ViewRig::Look`] | unchanged — a look turns the view, it does not move the eye | the current rig's orientation with the clamped look offset | as before |
//! | [`ViewRig::Spyglass`] | the spyglass mode's own placement | aimed at the session's **selected** target | the mode's declared magnification |
//!
//! Both offsets are declared in the **aircraft's body frame** and read in that
//! frame's own convention: `forward_m` is a distance along the body's forward
//! axis, so a chase view's negative value puts the camera astern and looking
//! the way the aircraft flies. [`oriented_pose`] is where that convention is
//! turned into a world position, once, for every rig.
//!
//! The spyglass is the interesting one, because it is the only rig with an
//! input that can change underneath it mid-frame. F21 non-negotiable behavior
//! 3 requires it to show the selected target, handle invalid targets, obey its
//! own near/far planes and change neither target authority nor fire direction;
//! AC02 requires that destroying or switching the target mid-frame leaves no
//! stale entity access. Four rules get that, and each one is a separate
//! refusal rather than a best effort:
//!
//! 1. The rig **never** holds a handle to a target. It stores an [`ActorId`]
//!    and the position it published *in the frame it published it*; the next
//!    frame re-reads the session's [`SpyglassReadout`] and uses only what that
//!    read says. There is no entity, no pointer and no cached selection for a
//!    destroyed actor to survive in.
//! 2. A readout older than one already consumed **refuses**
//!    ([`RigError::StaleSpyglassReadout`]). A render frame between two fixed
//!    ticks legitimately re-reads the same tick, but a *smaller* tick is a
//!    consumer being fed a view from before a target died, and using it would
//!    put the last magnification back up.
//! 3. Losing or changing the selection drops the framed actor **in the same
//!    frame** ([`SpyglassAim::actor`] is `None` and
//!    [`SpyglassAim::dropped`] names who went). The mode stays the spyglass
//!    mode — so its own field of view and its own near/far planes are still
//!    the ones in force (behavior 3) — but nothing is magnified.
//! 4. A target the camera cannot be aimed at refuses
//!    ([`RigError::UnaimableTarget`]) *and* drops the framed actor, so a
//!    caller that keeps the last published frame still cannot read a live
//!    magnified actor out of the rig.
//!
//! # Smoothing, resettling and origin shifts
//!
//! The pose goes through [`PoseSmoother`], whose law is frame-rate
//! independent (non-negotiable behavior 4). The rig decides *when* to reseat,
//! from typed facts rather than from a distance heuristic:
//!
//! * a **different subject** — an [`ActorId`] is generation-qualified, so an
//!   aircraft swap arrives as a different actor and the camera and the framed
//!   target both start over;
//! * a [`OriginChange::Teleport`] — a world-space jump with no continuous
//!   path;
//! * the first frame of a rig.
//!
//! A [`OriginChange::Rebase`] does **not** reseat, and needs no conversion:
//! the rig's state is canonical f64 world space, which a rebase leaves alone
//! (F16 non-negotiable behavior 5). A rebase that reset the camera would
//! teleport it by the origin offset.
//!
//! # What is not claimed
//!
//! Every placement, head orientation, look limit and smoothing rate here is
//! project design. The original cockpit bindings, the original view list, the
//! original spyglass behavior and the original camera's lag are unmeasured;
//! F21-D compares them against original captures with the `gpu` and `retail`
//! capabilities. What this stage establishes is that the rigs behave
//! consistently with whatever the records declare, and refuse rather than
//! guess when the records say nothing.

use std::fmt;
use std::time::Duration;

use cs_content::cameras::{
    AspectRatio, CameraModeKind, CockpitBindingSource, LookLimits, Magnification,
};
use cs_sim::damage::ActorId;
use cs_types::Tick;
use cs_types::content::Origin;
use cs_types::space::{Quaternion, Radians, SpaceError, UnitVec3, WorldPosition};

use crate::origin::OriginChange;
use crate::targeting::{ClearedTarget, SpyglassReadout};

use super::modes::{LoweredCameraMode, LoweredCameraModes, LoweredPlacement};
use super::orientation;
use super::pose::{CameraPose, Framing, FramingError};
use super::projection::LoweredProjection;
use super::smoothing::{PoseSmoother, SmoothingError, SmoothingState};

/// Which rig a session is looking through.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ViewRig {
    /// The cockpit viewpoint, from the aircraft's declared binding.
    Cockpit,
    /// The external chase view, from the declared body offset.
    Chase,
    /// The rig that is already up, with a free-look offset applied.
    Look,
    /// The magnified view of the session's selected target.
    Spyglass,
}

impl ViewRig {
    /// Every rig, in a stable order.
    pub const ALL: &'static [ViewRig] = &[Self::Cockpit, Self::Chase, Self::Look, Self::Spyglass];

    /// The stable label used in ids and reports.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Cockpit => "cockpit",
            Self::Chase => "chase",
            Self::Look => "look",
            Self::Spyglass => "spyglass",
        }
    }

    /// The mode kind this rig runs under, when it names one.
    ///
    /// [`Self::Look`] names none: it is the rig that is already up, turned,
    /// so its mode is whatever the previous selection left in place.
    #[must_use]
    pub const fn mode_kind(self) -> Option<CameraModeKind> {
        match self {
            Self::Cockpit => Some(CameraModeKind::Cockpit),
            Self::Chase => Some(CameraModeKind::External),
            Self::Look => None,
            Self::Spyglass => Some(CameraModeKind::Spyglass),
        }
    }
}

impl fmt::Display for ViewRig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// Why a free-look offset was refused.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LookError {
    /// An offset component was NaN or infinite.
    NonFinite {
        /// Which component it was, `"yaw"` or `"pitch"`.
        field: &'static str,
    },
}

impl fmt::Display for LookError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NonFinite { field } => write!(f, "the look offset {field} must be finite"),
        }
    }
}

impl std::error::Error for LookError {}

/// A free-look offset: how far the pilot is looking away from the view's own
/// orientation.
///
/// Enhanced support, deliberately kept apart from the original default
/// mappings (the sheet's deliverable): the *limits* come from the mode's
/// declared [`LookLimits`], and an offset beyond them is clamped rather than
/// obeyed, because a look that turns the camera past vertical would put its up
/// axis on the view direction and leave no right axis at all.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LookOffset {
    yaw: Radians,
    pitch: Radians,
}

impl LookOffset {
    /// No look at all.
    pub const IDENTITY: Self = Self {
        yaw: Radians(0.0),
        pitch: Radians(0.0),
    };

    /// Assembles an offset from a yaw and a pitch in radians.
    ///
    /// # Errors
    ///
    /// [`LookError::NonFinite`] for a NaN or infinite component. The range is
    /// not checked here: [`clamped`](Self::clamped) applies the declared
    /// limits, and an input outside them is clamped rather than refused.
    pub fn new(yaw: Radians, pitch: Radians) -> Result<Self, LookError> {
        if !yaw.0.is_finite() {
            return Err(LookError::NonFinite { field: "yaw" });
        }
        if !pitch.0.is_finite() {
            return Err(LookError::NonFinite { field: "pitch" });
        }
        Ok(Self { yaw, pitch })
    }

    /// The yaw component.
    #[must_use]
    pub const fn yaw(self) -> Radians {
        self.yaw
    }

    /// The pitch component.
    #[must_use]
    pub const fn pitch(self) -> Radians {
        self.pitch
    }

    /// The offset the declared limits allow, never the requested one.
    ///
    /// `limits` was range-checked when it was declared and again when it was
    /// lowered, so the clamp here cannot panic and cannot produce a NaN.
    #[must_use]
    pub fn clamped(self, limits: LookLimits) -> Self {
        let yaw = Radians(self.yaw.0.clamp(-limits.yaw().0, limits.yaw().0));
        let pitch = Radians(self.pitch.0.clamp(-limits.pitch().0, limits.pitch().0));
        Self { yaw, pitch }
    }
}

/// Why the spyglass could not be aimed at its target.
#[derive(Clone, Debug, PartialEq)]
pub enum RigAimError {
    /// The target's position is the camera's own position, so there is no
    /// direction between them and no orientation to build.
    CoincidentWithCamera,
    /// The derived direction or orientation was rejected by the space
    /// boundary.
    Space(SpaceError),
}

impl fmt::Display for RigAimError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CoincidentWithCamera => write!(
                f,
                "the selected target is at the camera's own position, so it cannot be aimed at"
            ),
            Self::Space(error) => write!(f, "the aim was rejected: {error}"),
        }
    }
}

impl std::error::Error for RigAimError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::CoincidentWithCamera => None,
            Self::Space(error) => Some(error),
        }
    }
}

/// Why a rig could not produce a frame.
#[derive(Clone, Debug, PartialEq)]
pub enum RigError {
    /// A mode kind has no rig in this stage: an authored camera sequence is
    /// F21-C's, and this stage refuses it by name instead of silently
    /// behaving like the chase view.
    NoRigForMode {
        /// The mode kind that has no rig.
        kind: CameraModeKind,
    },
    /// The rig's mode is not declared in the owner's mode set.
    ModeNotDeclared {
        /// The rig that was asked for.
        rig: ViewRig,
        /// The mode kind it needs.
        kind: CameraModeKind,
    },
    /// A free-look offset was asked for on a rig that has none. The spyglass
    /// aims at the selection, so an offset applied on top of it would point
    /// the view away from the thing it exists to magnify.
    LookNotAvailable {
        /// The mode kind that cannot free-look.
        kind: CameraModeKind,
    },
    /// A free-look offset was refused.
    Look(LookError),
    /// The smoothing step was refused.
    Smoothing(SmoothingError),
    /// The spyglass's selected target has no orientation to aim at.
    UnaimableTarget {
        /// The actor the rig refused to frame.
        actor: ActorId,
        /// Why it could not be framed.
        reason: RigAimError,
    },
    /// The published spyglass view is older than one this rig already
    /// consumed.
    ///
    /// A render frame between two fixed ticks legitimately re-reads the same
    /// tick; a *smaller* one is a view from before a target was destroyed,
    /// and following it would put the last magnification back up (AC02).
    StaleSpyglassReadout {
        /// The tick the view was published at.
        read_at: Tick,
        /// The newest tick this rig has already consumed a view for.
        consumed_through: Tick,
    },
    /// The space boundary rejected a derived position or rotation.
    Space(SpaceError),
}

impl fmt::Display for RigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoRigForMode { kind } => write!(
                f,
                "the {kind} mode has no rig in this stage; an authored camera sequence is F21-C's"
            ),
            Self::ModeNotDeclared { rig, kind } => write!(
                f,
                "the {rig} rig needs a declared {kind} mode, and this owner's mode set has none"
            ),
            Self::LookNotAvailable { kind } => write!(
                f,
                "the {kind} mode does not free-look: it aims at the selected target instead"
            ),
            Self::Look(error) => write!(f, "{error}"),
            Self::Smoothing(error) => write!(f, "{error}"),
            Self::UnaimableTarget { actor, reason } => {
                write!(f, "the spyglass cannot aim at {}: {reason}", actor.serial)
            }
            Self::StaleSpyglassReadout {
                read_at,
                consumed_through,
            } => write!(
                f,
                "the published spyglass view is at tick {} but this rig already consumed tick {}",
                read_at.0, consumed_through.0
            ),
            Self::Space(error) => write!(f, "the camera pose was rejected: {error}"),
        }
    }
}

impl std::error::Error for RigError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::NoRigForMode { .. }
            | Self::ModeNotDeclared { .. }
            | Self::LookNotAvailable { .. } => None,
            Self::Look(error) => Some(error),
            Self::Smoothing(error) => Some(error),
            Self::UnaimableTarget { reason, .. } => Some(reason),
            Self::StaleSpyglassReadout { .. } => None,
            Self::Space(error) => Some(error),
        }
    }
}

impl From<LookError> for RigError {
    fn from(error: LookError) -> Self {
        Self::Look(error)
    }
}

impl From<SmoothingError> for RigError {
    fn from(error: SmoothingError) -> Self {
        Self::Smoothing(error)
    }
}

impl From<SpaceError> for RigError {
    fn from(error: SpaceError) -> Self {
        Self::Space(error)
    }
}

/// Everything one frame's rig resolution reads.
///
/// A frame supplies all of it and holds nothing: the rig keeps no aircraft
/// handle, no selected entity and no world to look things up in.
#[derive(Clone, Copy, Debug)]
pub struct RigInputs<'a> {
    /// The tick this frame renders.
    pub at: Tick,
    /// The aircraft the camera follows. Generation-qualified, so an aircraft
    /// swap is a different value and reseats the rig.
    pub subject: ActorId,
    /// The authoritative aircraft pose. Read-only: the rig never writes it.
    pub aircraft: CameraPose,
    /// The viewport aspect this frame draws at.
    pub aspect: AspectRatio,
    /// The wall time this render frame covers.
    pub elapsed: Duration,
    /// The free-look input, when the session's input path produced one.
    pub look: Option<LookOffset>,
    /// The session's published spyglass view for this phase, when there is
    /// one. The rig never looks the target up itself, which is what makes it
    /// impossible for it to frame an actor targeting has dropped.
    pub spyglass: Option<&'a SpyglassReadout>,
    /// Whether the world origin was rebased or the aircraft teleported since
    /// the last frame (F16 non-negotiable behavior 5).
    pub origin_change: OriginChange,
}

/// What the spyglass rig is doing this frame.
///
/// `actor` is the answer to "is anything magnified right now": it is `None`
/// whenever nothing is, so a consumer cannot mistake "the mode is still the
/// spyglass" for "the last target is still framed".
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpyglassAim {
    /// The actor being magnified, or `None` when nothing is.
    pub actor: Option<ActorId>,
    /// The world position the camera is aimed at, when one is framed.
    pub aim: Option<WorldPosition>,
    /// The selection the published view cleared, when it cleared one.
    pub cleared: Option<ClearedTarget>,
    /// The actor the previous frame framed and this frame does not: the one
    /// that was destroyed, despawned or switched away from.
    pub dropped: Option<ActorId>,
    /// Whether the framed actor *changed* this frame, as opposed to appearing
    /// from nothing or being cleared.
    pub switched: bool,
}

impl SpyglassAim {
    /// Whether an actor is being magnified.
    #[must_use]
    pub const fn has_target(self) -> bool {
        self.actor.is_some()
    }
}

/// One frame's camera: everything a renderer needs and nothing it must guess.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RigFrame {
    /// The frame's tick.
    pub at: Tick,
    /// The rig that produced the frame.
    pub rig: ViewRig,
    /// The mode the rig ran under.
    pub mode: CameraModeKind,
    /// The camera's world pose after smoothing.
    pub pose: CameraPose,
    /// The projection to build the frustum from, at [`Self::aspect`].
    pub projection: LoweredProjection,
    /// The viewport aspect this frame was framed at.
    pub aspect: AspectRatio,
    /// The magnification the mode declares.
    pub magnification: Magnification,
    /// The clamped free-look offset that was applied, when one was asked for.
    pub look: Option<LookOffset>,
    /// What the spyglass rig is doing, when it is the spyglass rig.
    pub spyglass: Option<SpyglassAim>,
    /// How the pose came to be this frame.
    pub smoothing: SmoothingState,
}

impl RigFrame {
    /// The viewport coordinate of a world-space point in this frame.
    ///
    /// This is the composition F21-A set up: the frame's own pose basis and
    /// its own projection at its own aspect, so a consumer — the HUD's target
    /// box, a capture tool, a test — cannot frame a point through a different
    /// frustum than the one on screen.
    ///
    /// # Errors
    ///
    /// [`FramingError::BehindCamera`] when the point is at or behind the eye,
    /// and [`FramingError::Basis`] when the pose has no usable basis.
    pub fn framing_of(&self, target: WorldPosition) -> Result<Framing, FramingError> {
        self.projection
            .framing_of(self.aspect, self.pose.basis()?, target)
    }
}

/// The spyglass state one frame's resolution produced internally.
struct SpyglassFrame {
    /// The orientation the aim produced, when it produced one.
    rotation: Option<Quaternion>,
    /// What to report.
    aim: SpyglassAim,
}

/// One session's camera: the mode set, the current rig and the smoothing
/// state between frames.
///
/// The rig owns no ECS resource and no world. A session installs one, calls
/// [`resolve`](Self::resolve) once per render frame with the frame's
/// [`RigInputs`], and hands the [`RigFrame`] to whatever draws — F21-C is the
/// stage that wires that into the session's schedule.
#[derive(Clone, Debug, PartialEq)]
pub struct CameraRig {
    modes: LoweredCameraModes,
    mode: CameraModeKind,
    looking: bool,
    subject: Option<ActorId>,
    smoother: PoseSmoother,
    framed: Option<ActorId>,
    readout_tick: Option<Tick>,
}

impl CameraRig {
    /// The default smoothing response, in reciprocal seconds.
    ///
    /// Project design: it closes 63% of the remaining distance every 83 ms,
    /// which is a quick follow without a visible snap. The original camera's
    /// own lag is unmeasured (F21-D).
    pub const DEFAULT_RESPONSE_PER_S: f64 = 12.0;

    /// Builds a rig over `modes`, in the set's declared default view, with
    /// [`Self::DEFAULT_RESPONSE_PER_S`].
    ///
    /// # Errors
    ///
    /// [`RigError::NoRigForMode`] when the declared default is an authored
    /// camera sequence: that rig belongs to F21-C, and starting there would
    /// mean inventing a pose.
    pub fn new(modes: LoweredCameraModes) -> Result<Self, RigError> {
        Self::with_response(modes, Self::DEFAULT_RESPONSE_PER_S)
    }

    /// Builds a rig with an explicit smoothing response.
    ///
    /// # Errors
    ///
    /// [`RigError::NoRigForMode`] for an authored-sequence default, and
    /// [`RigError::Smoothing`] for a response that is not finite and positive.
    pub fn with_response(modes: LoweredCameraModes, response_per_s: f64) -> Result<Self, RigError> {
        let mode = modes.default_mode();
        if mode == CameraModeKind::AuthoredSequence {
            return Err(RigError::NoRigForMode { kind: mode });
        }
        Ok(Self {
            modes,
            mode,
            looking: false,
            subject: None,
            smoother: PoseSmoother::new(response_per_s)?,
            framed: None,
            readout_tick: None,
        })
    }

    /// The mode set this rig runs.
    #[must_use]
    pub const fn modes(&self) -> &LoweredCameraModes {
        &self.modes
    }

    /// Where the mode set came from: `Origin::SyntheticFixture` is
    /// development content, never a verified original binding.
    #[must_use]
    pub const fn origin(&self) -> &Origin {
        self.modes.origin()
    }

    /// The mode the current view runs under.
    #[must_use]
    pub const fn mode(&self) -> CameraModeKind {
        self.mode
    }

    /// Whether a free-look offset is being applied to the current mode.
    #[must_use]
    pub const fn is_looking(&self) -> bool {
        self.looking
    }

    /// Which rig is up.
    #[must_use]
    pub const fn rig(&self) -> ViewRig {
        if self.looking {
            ViewRig::Look
        } else {
            match self.mode {
                CameraModeKind::Cockpit => ViewRig::Cockpit,
                CameraModeKind::External => ViewRig::Chase,
                CameraModeKind::Spyglass => ViewRig::Spyglass,
                // Unreachable: `new` refuses an authored-sequence default,
                // `set_rig` has no rig that names one, and F21-C resolves an
                // authored sequence in `camera::session`, which keeps the
                // player's rigs here. Kept total so a future mode kind cannot
                // fall through to a wrong answer.
                CameraModeKind::AuthoredSequence => ViewRig::Chase,
            }
        }
    }

    /// The aircraft the rig is currently bound to.
    #[must_use]
    pub const fn subject(&self) -> Option<ActorId> {
        self.subject
    }

    /// The actor the spyglass is magnifying right now.
    ///
    /// `None` after a clear, after a stale readout, after an unaimable
    /// target and before anything has been framed: this is the single
    /// question AC02 asks, and the rig answers it from its own state rather
    /// than from whatever frame the caller last received.
    #[must_use]
    pub const fn framed_target(&self) -> Option<ActorId> {
        self.framed
    }

    /// The binding the current rig's eye came from, when it is a cockpit.
    ///
    /// The answer to F21 non-negotiable behavior 1's provenance question. It
    /// is a property of the *mode*, so it is asked of the rig rather than
    /// carried per frame: paired with [`origin`](Self::origin) a consumer can
    /// tell a binding read out of a verified model node from one that only
    /// exists in a synthetic fixture.
    #[must_use]
    pub fn cockpit_binding(&self) -> Option<&CockpitBindingSource> {
        self.modes
            .get(self.mode)?
            .placement()
            .cockpit()
            .map(super::modes::LoweredCockpitViewpoint::source)
    }

    /// The smoother, for a consumer that wants to report the camera's state.
    #[must_use]
    pub const fn smoother(&self) -> PoseSmoother {
        self.smoother
    }

    /// Whether `rig` could be selected right now.
    ///
    /// The same two rules [`set_rig`](Self::set_rig) enforces, asked without
    /// mutating: a producer that has to validate a request *before* the moment
    /// it applies it (F21-C's capture flags) can refuse at the boundary instead
    /// of discovering the refusal at the frame it was going to draw.
    #[must_use]
    pub fn can_select(&self, rig: ViewRig) -> bool {
        self.selection(rig).is_ok()
    }

    /// The mode kind `rig` selects, or `None` for the free-look rig.
    ///
    /// # Errors
    ///
    /// [`RigError::ModeNotDeclared`] when the owner's set declares no mode for
    /// `rig`, and [`RigError::LookNotAvailable`] when a look is asked for while
    /// the spyglass is up.
    fn selection(&self, rig: ViewRig) -> Result<Option<CameraModeKind>, RigError> {
        if let Some(kind) = rig.mode_kind() {
            if self.modes.get(kind).is_none() {
                return Err(RigError::ModeNotDeclared { rig, kind });
            }
            return Ok(Some(kind));
        }
        if self.mode == CameraModeKind::Spyglass {
            return Err(RigError::LookNotAvailable { kind: self.mode });
        }
        Ok(None)
    }

    /// Restores a view this rig was switched away from, mode and free look
    /// included.
    ///
    /// [`set_rig`](Self::set_rig) restores a *rig*, and
    /// [`ViewRig::Look`] names no mode: selecting it puts the free look back up
    /// over whatever mode the last selection left in place. A caller that
    /// switched a one-frame override in and out with `set_rig` alone would come
    /// back with the free look up **and the override's mode underneath it** — a
    /// player left looking around in a view they never chose. A one-frame
    /// override therefore records the mode and the free-look flag as they were
    /// ([`mode`](Self::mode), [`is_looking`](Self::is_looking)) and puts them
    /// back whole, which is what F21-C's capture flags do.
    ///
    /// # Errors
    ///
    /// [`RigError::ModeNotDeclared`] when `mode` is not in the owner's declared
    /// set. Nothing is mutated on error.
    pub(crate) fn restore_view(
        &mut self,
        mode: CameraModeKind,
        looking: bool,
    ) -> Result<(), RigError> {
        if self.modes.get(mode).is_none() {
            return Err(RigError::ModeNotDeclared {
                rig: self.rig(),
                kind: mode,
            });
        }
        self.mode = mode;
        // The one rule `set_rig` keeps for the free look: it is refused while the
        // spyglass is up, so restoring one over a spyglass would leave the rig in
        // a state it refuses to resolve.
        self.looking = looking && mode != CameraModeKind::Spyglass;
        Ok(())
    }

    /// Selects a rig.
    ///
    /// # Errors
    ///
    /// [`RigError::ModeNotDeclared`] when the owner's set declares no mode for
    /// the requested rig, and [`RigError::LookNotAvailable`] when a look is
    /// asked for while the spyglass is up. Nothing is mutated on error.
    pub fn set_rig(&mut self, rig: ViewRig) -> Result<(), RigError> {
        match self.selection(rig)? {
            Some(kind) => {
                self.mode = kind;
                self.looking = false;
            }
            None => self.looking = true,
        }
        Ok(())
    }

    /// Forgets the session's camera state, so a rebuild cannot inherit it.
    ///
    /// The end-of-session path: the smoothed pose, the bound aircraft and the
    /// framed target all go, and the next frame reseats. A new session's rig
    /// must not open with the previous pilot's camera position or target.
    pub fn reset(&mut self) {
        self.smoother.clear();
        self.subject = None;
        self.framed = None;
        self.readout_tick = None;
        self.looking = false;
    }

    /// Produces this frame's camera.
    ///
    /// # Errors
    ///
    /// [`RigError`] for every refusal above. Two of them mutate the rig before
    /// reporting, deliberately: [`RigError::StaleSpyglassReadout`] and
    /// [`RigError::UnaimableTarget`] both drop [`framed_target`](Self::framed_target)
    /// first, so a caller that keeps the last published frame after an error
    /// still cannot read a live magnified actor out of the rig. Every other
    /// error leaves the rig exactly as it was, and the next frame retries with
    /// the same code.
    pub fn resolve(&mut self, inputs: &RigInputs<'_>) -> Result<RigFrame, RigError> {
        // The mode is copied out of the mode set rather than borrowed from
        // it: the rest of the frame mutates the rig, and one small record copy
        // per render frame is cheaper than a self-referential borrow. Nothing
        // here is on a path that runs per *fixed* tick, and the copy is of the
        // already-lowered record, not of any declared content.
        let mode = self.require_mode()?.clone();
        if self.looking && mode.kind() == CameraModeKind::Spyglass {
            return Err(RigError::LookNotAvailable { kind: mode.kind() });
        }

        // A different aircraft is a different generation, so the camera and
        // the target it was framing both start over. This is the plane-swap
        // half of non-negotiable behavior 4, and it happens before the
        // spyglass is read so this frame's aim is the new aircraft's.
        let swapped = self.subject.is_some_and(|bound| bound != inputs.subject);
        if swapped {
            self.framed = None;
            self.readout_tick = None;
        }

        let (eye, base_rotation) = oriented_pose(mode.placement(), inputs.aircraft)?;
        let spyglass = if mode.kind() == CameraModeKind::Spyglass {
            Some(self.aim_spyglass(&mode, inputs, eye)?)
        } else {
            None
        };

        // The look offset is clamped once and reported as clamped, so a
        // consumer can never act on a turn the rig did not apply.
        let look = if self.looking {
            Some(match inputs.look {
                Some(look) => look.clamped(mode.look_limits()),
                None => LookOffset::IDENTITY,
            })
        } else {
            None
        };
        let rotation = match spyglass.as_ref().and_then(|aim| aim.rotation) {
            Some(aimed) => aimed,
            None => match look {
                Some(look) => orientation::compose(
                    orientation::yaw_pitch(look.yaw(), look.pitch())?,
                    base_rotation,
                )?,
                None => base_rotation,
            },
        };

        let desired = CameraPose::new(eye, rotation);
        let reseated = swapped
            || self.smoother.pose().is_none()
            || inputs.origin_change == OriginChange::Teleport;
        let pose = if reseated {
            self.smoother.snap(desired);
            desired
        } else {
            self.smoother.advance(desired, inputs.elapsed)?
        };
        self.subject = Some(inputs.subject);

        Ok(RigFrame {
            at: inputs.at,
            rig: self.rig(),
            mode: mode.kind(),
            pose,
            projection: mode.projection(),
            aspect: inputs.aspect,
            magnification: mode.magnification(),
            look,
            spyglass: spyglass.map(|aim| aim.aim),
            smoothing: self.smoother.state(),
        })
    }

    /// The mode the current rig runs under.
    fn require_mode(&self) -> Result<&LoweredCameraMode, RigError> {
        self.modes.get(self.mode).ok_or(RigError::ModeNotDeclared {
            rig: self.rig(),
            kind: self.mode,
        })
    }

    /// Reads the published spyglass view once and reports what it did.
    ///
    /// The whole of AC02 is in this function: it takes the selection from the
    /// frame's [`SpyglassReadout`] and nothing else, it refuses a view older
    /// than one it has consumed, it compares against the *actor id* it framed
    /// last frame rather than against a cached position, and it drops the
    /// framed actor on every path that cannot frame one.
    fn aim_spyglass(
        &mut self,
        mode: &LoweredCameraMode,
        inputs: &RigInputs<'_>,
        eye: WorldPosition,
    ) -> Result<SpyglassFrame, RigError> {
        let previous = self.framed;
        let cleared = inputs.spyglass.and_then(|readout| readout.cleared);

        if let Some(readout) = inputs.spyglass {
            if let Some(consumed) = self.readout_tick
                && readout.at < consumed
            {
                // The view predates something this rig already acted on, so
                // its target is one that may already be gone. Refuse, and
                // leave no magnified actor behind.
                self.framed = None;
                return Err(RigError::StaleSpyglassReadout {
                    read_at: readout.at,
                    consumed_through: consumed,
                });
            }
            self.readout_tick = Some(readout.at);
        }

        // A mode that declares it does not track a target gets no target: the
        // rig follows the declaration instead of the selection (behavior 3 —
        // the spyglass changes no target authority, and it takes no target
        // authority either).
        let selected = match inputs.spyglass.and_then(|readout| readout.target.as_ref()) {
            Some(target) if mode.tracks_target() => Some((target.actor, target.position)),
            _ => None,
        };

        let (rotation, actor, aim) = match selected {
            Some((actor, position)) => {
                let direction = match orientation::direction_to(eye, position) {
                    Ok(Some(direction)) => direction,
                    Ok(None) => {
                        self.framed = None;
                        return Err(RigError::UnaimableTarget {
                            actor,
                            reason: RigAimError::CoincidentWithCamera,
                        });
                    }
                    Err(error) => {
                        self.framed = None;
                        return Err(RigError::UnaimableTarget {
                            actor,
                            reason: RigAimError::Space(error),
                        });
                    }
                };
                self.framed = Some(actor);
                (
                    Some(aim_at(inputs.aircraft, direction)?),
                    Some(actor),
                    Some(position),
                )
            }
            None => {
                // Nothing selected, or nothing selected that this mode frames:
                // the magnification comes down in this same frame.
                self.framed = None;
                (None, None, None)
            }
        };

        Ok(SpyglassFrame {
            rotation,
            aim: SpyglassAim {
                actor,
                aim,
                cleared,
                dropped: previous.filter(|was| Some(*was) != actor),
                switched: previous.is_some() && actor.is_some() && previous != actor,
            },
        })
    }
}

/// Places a camera from a declared placement and the authoritative aircraft
/// pose: the eye in world space, and the orientation it starts from.
///
/// The offset is rotated by the aircraft's own rotation, so the same three
/// declared metres mean the same thing at every attitude — and the world
/// position is derived from the pose the frame read, never from a value a
/// previous frame cached.
///
/// `pub(crate)` because F21-C's scripted camera places its eye from the *same*
/// declared placement records, through this one function. Two copies of the
/// `forward_m` sign convention would be two answers to one question, and the
/// chase-view sign is exactly the kind of thing that has to be right once.
pub(crate) fn oriented_pose(
    placement: &LoweredPlacement,
    aircraft: CameraPose,
) -> Result<(WorldPosition, Quaternion), RigError> {
    let offset = placement.offset();
    // The declared `forward_m` is a distance along the body's *forward* axis,
    // which is canonical `-Z`, so it enters the canonical component vector
    // negated. Getting this wrong does not look like a sign error: the eye
    // lands 12 m in front of an aircraft that is flying away from it, and the
    // chase view shows the pilot empty sky.
    let rotated = orientation::rotate_vector(
        aircraft.rotation(),
        [offset.right_m().0, offset.up_m().0, -offset.forward_m().0],
    )?;
    let [px, py, pz] = aircraft.position().to_array();
    let eye = WorldPosition::try_new([px + rotated[0], py + rotated[1], pz + rotated[2]])?;

    let rotation = match placement.head_orientation() {
        Some((yaw, pitch)) => {
            orientation::compose(orientation::yaw_pitch(yaw, pitch)?, aircraft.rotation())?
        }
        None => aircraft.rotation(),
    };
    Ok((eye, rotation))
}

/// The rotation that looks along `direction` while keeping the aircraft's
/// attitude.
///
/// [`orientation::look_rotation`] needs an up hint that is not parallel to
/// the view direction, and the canonical `+Y` is the wrong one to hand it: a
/// spyglass tracking a target directly overhead would refuse every time. The
/// aircraft's own **up** axis is the right hint — it is the attitude the
/// pilot is in — and the aircraft's **right** axis is the fallback for the one
/// attitude where up is parallel to the view. They are orthogonal, so one of
/// the two always works and the aim never refuses for want of a roll.
///
/// The consequence is stated rather than hidden: a camera looking straight up
/// keeps the aircraft's right axis as its up, which is the roll a real
/// chase/spyglass view has, and it is deterministic rather than arbitrary.
///
/// # Errors
///
/// [`SpaceError`] when the aircraft's own basis or the derived rotation is
/// unusable.
fn aim_at(aircraft: CameraPose, direction: UnitVec3) -> Result<Quaternion, SpaceError> {
    let basis = aircraft.basis()?;
    match orientation::look_rotation(direction, basis.up()) {
        Ok(rotation) => Ok(rotation),
        Err(error @ SpaceError::NotUnit { .. }) => {
            orientation::look_rotation(direction, basis.right()).map_err(|_| error)
        }
        Err(error) => Err(error),
    }
}
