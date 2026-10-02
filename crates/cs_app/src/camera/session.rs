//! The camera session: the producer/consumer wiring for script cameras and
//! deterministic captures (F21-C).
//!
//! Spec: `specs/F21-cameras-cockpit-views-and-spyglass.md`, stage
//! `### F21-C`: "Wire the implemented path into its actual producer and
//! consumer; include teardown/retry and error propagation." Shared contract:
//! `docs/contracts/UI-NETWORK.md`.
//!
//! # What this stage wires
//!
//! F21-A declared the records, F21-B implemented the rigs, and nothing yet
//! *ran* them: [`CameraRig::resolve`] takes a [`RigInputs`] and returns a
//! [`RigFrame`], and there was no owner of either. [`CameraSession`] is that
//! owner. It holds the player's [`CameraRig`], accepts scripted camera requests
//! ([`super::script`]) and deterministic capture requests ([`super::capture`]),
//! and produces exactly one [`SessionFrame`] per render frame naming the
//! authority that drew it.
//!
//! ```text
//!   producer                         CameraSession                     consumer
//!   ────────                         ─────────────                     ────────
//!   script  ──request_script──▶  ┌───────────────────┐
//!   capture ──apply_capture───▶   │  rig + script     │  frame(at, bodies,
//!   view    ──select_rig─────▶    │  + capture        │  aspect, elapsed, …)
//!                                └───────────────────┘            │
//!                                                                    ▼
//!                                                    SessionFrame { authority, view,
//!                                                                     rig, capture, events }
//! ```
//!
//! # The three rules that make the wiring honest
//!
//! 1. **One authority per frame, named.** A frame is either the player's rig or
//!    a scripted camera, never both, and [`CameraAuthority`] says which. A
//!    consumer that draws a scripted shot cannot mistake it for the player's
//!    view, and one that draws the player's view is told when the script took
//!    and handed the camera back.
//! 2. **Refusals are non-mutating and retryable; ends are terminal and
//!    reported.** A request that is refused changes nothing, so the corrected
//!    request can be retried. A script that *ends* — its span ran out, its body
//!    is gone, the producer released it, the session was torn down — reports
//!    [`CameraEvent::ScriptEnded`] with the reason on the frame the camera
//!    returns, so nothing disappears silently. An event the session earned
//!    before a frame it then *refused* waits in
//!    [`pending_events`](CameraSession::pending_events) and is carried by the
//!    next frame that can be drawn: the session's own state has already moved,
//!    so it can never report that event a second time, and a frame that dropped
//!    it would drop it for good.
//! 3. **A capture is one frame and then it is gone.** A
//!    [`CaptureRequest`] applies to exactly the frame at its tick and is
//!    retired there. A pinned pose, a pinned rig and a pinned aspect cannot leak
//!    into the frames around it, which is what "reproducible" has to mean for a
//!    tool that drives a live session.
//!
//! # AC03: an aircraft swap during a scripted capture
//!
//! The minimum scenario is a property of this module and not of the rig. A
//! capture is pending at tick 900, a scripted camera is driving, and the player
//! swaps aircraft at tick 880. The frame at 880 reports
//! [`CameraEvent::SubjectRebound`] naming both bodies, the scripted camera
//! resolves its subject again (the player role is re-resolved every frame, so
//! it lands on the new body), and the frame at 900 draws through the new body
//! with a report that names the pose, aspect and frustum the capture pinned. A
//! script that named the *old* body with
//! [`ScriptSubject::Actor`](super::script::ScriptSubject::Actor) instead ends
//! with [`ScriptEndReason::SubjectGone`] in that same frame and the player's
//! rig takes the camera back. Neither half is a guess: the swap is a different
//! [`ActorId`], which is a different camera binding.
//!
//! # What is not claimed
//!
//! No original camera, script camera, capture flag or CLI exists in the
//! original's data as implemented here: the mission-script producer, the
//! cinematic timeline (F40) and the `--screenshot` command line (F00-C's
//! `cs_app::cli`) are all outside this stage's owner paths. What this stage
//! establishes is that the seam they need exists, is typed, refuses rather than
//! guesses, and that a scripted camera and a deterministic capture behave as
//! specified when driven through it.

use std::fmt;
use std::time::Duration;

use cs_content::cameras::{AspectRatio, CameraModeKind, Magnification};
use cs_sim::damage::ActorId;
use cs_types::Tick;
use cs_types::content::ContentId;
use cs_types::space::WorldPosition;

use crate::origin::OriginChange;
use crate::targeting::SpyglassReadout;

use super::capture::{
    CaptureError, CaptureReport, CaptureRequest, CaptureTarget, ProjectionPinError,
};
use super::pose::{CameraPose, Framing, FramingError};
use super::projection::LoweredProjection;
use super::rig::{CameraRig, LookOffset, RigError, RigFrame, RigInputs, ViewRig, oriented_pose};
use super::script::{ScriptCameraError, ScriptCameraRequest, ScriptEndReason, ScriptedShot};
use super::smoothing::{PoseSmoother, SmoothingError, SmoothingState};

/// One body's authoritative pose, as the producer publishes it for a frame.
///
/// A **copy**, like everything else the camera reads: the session keeps no
/// entity, no handle and no ECS resource, so a body that leaves the world
/// leaves the frame's input list and nothing else has to be invalidated.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BodyPose {
    /// The body.
    pub actor: ActorId,
    /// Its authoritative pose in canonical world space.
    pub pose: CameraPose,
}

impl BodyPose {
    /// A body at `pose`.
    #[must_use]
    pub const fn new(actor: ActorId, pose: CameraPose) -> Self {
        Self { actor, pose }
    }
}

/// Everything one frame's camera resolution reads from its producer.
#[derive(Clone, Copy, Debug)]
pub struct SessionFrameInputs<'a> {
    /// The tick this frame renders.
    pub at: Tick,
    /// The body the player currently flies.
    ///
    /// It is not a cached id: the producer states it every frame, which is what
    /// makes an aircraft swap arrive as a different binding (AC03).
    pub player: ActorId,
    /// Every body the camera may look at this frame, in producer order.
    ///
    /// A scripted subject that is not in this list is gone, and the session
    /// ends that script rather than following a body it can no longer see.
    pub bodies: &'a [BodyPose],
    /// The viewport aspect this frame draws at, unless a capture overrides it.
    pub aspect: AspectRatio,
    /// The wall time this render frame covers.
    pub elapsed: Duration,
    /// The free-look input, when the input path produced one.
    pub look: Option<LookOffset>,
    /// The session's published spyglass view, when there is one.
    pub spyglass: Option<&'a SpyglassReadout>,
    /// Whether the world origin was rebased or a body teleported since the last
    /// frame.
    pub origin_change: OriginChange,
}

impl SessionFrameInputs<'_> {
    /// The pose of `actor`, when this frame publishes one.
    #[must_use]
    pub fn pose_of(&self, actor: ActorId) -> Option<CameraPose> {
        self.bodies
            .iter()
            .find(|body| body.actor == actor)
            .map(|body| body.pose)
    }

    /// Whether this frame publishes a pose for `actor`.
    #[must_use]
    pub fn contains(&self, actor: ActorId) -> bool {
        self.bodies.iter().any(|body| body.actor == actor)
    }
}

/// Which authority drew a frame.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CameraAuthority {
    /// The player's own view, through the session's rig.
    Player {
        /// Which rig is up.
        rig: ViewRig,
    },
    /// A scripted camera.
    Script {
        /// The authored camera that asked.
        camera: ContentId,
        /// The body it is framing, or `None` for a pinned pose — which has no
        /// subject, and reporting one would invent a binding.
        subject: Option<ActorId>,
    },
}

impl CameraAuthority {
    /// Whether this authority is a scripted camera.
    #[must_use]
    pub const fn is_script(&self) -> bool {
        matches!(self, Self::Script { .. })
    }

    /// The body the camera is bound to, when it is bound to one.
    #[must_use]
    pub const fn subject(&self) -> Option<ActorId> {
        match self {
            Self::Player { .. } => None,
            Self::Script { subject, .. } => *subject,
        }
    }

    /// The authored camera that drew the frame, when a script did.
    #[must_use]
    pub const fn camera(&self) -> Option<&ContentId> {
        match self {
            Self::Player { .. } => None,
            Self::Script { camera, .. } => Some(camera),
        }
    }
}

impl fmt::Display for CameraAuthority {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Player { rig } => write!(f, "the player's {rig} view"),
            Self::Script { camera, subject } => match subject {
                Some(subject) => write!(f, "{camera} framing {subject}"),
                None => write!(f, "{camera} holding a pinned pose"),
            },
        }
    }
}

/// The camera a renderer draws this frame, whichever authority produced it.
///
/// It is the intersection of the rig's frame and a scripted camera's frame: the
/// pose, the projection, the aspect, the magnification, the mode kind and how
/// the pose came to be. A consumer that only draws — a renderer, a capture tool
/// — needs nothing else.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SessionView {
    /// The body the camera is following, when it follows one.
    pub subject: Option<ActorId>,
    /// The camera's world pose after smoothing and after any capture override.
    pub pose: CameraPose,
    /// The mode the camera runs under: a view kind, or
    /// [`CameraModeKind::AuthoredSequence`] for a scripted camera.
    pub mode: CameraModeKind,
    /// The projection to build the frustum from, at [`Self::aspect`].
    pub projection: LoweredProjection,
    /// The aspect this view was framed at.
    pub aspect: AspectRatio,
    /// The magnification the mode declares.
    pub magnification: Magnification,
    /// How the pose came to be this frame.
    pub smoothing: SmoothingState,
}

impl SessionView {
    /// The rig's own frame as a view, bound to `subject`.
    ///
    /// `subject` is passed in rather than read from the frame because
    /// [`RigFrame`] carries no subject: the rig knows which body it is bound to
    /// ([`CameraRig::subject`]), and the session is the one that reports a
    /// binding change, so it is the one that fills this in.
    #[must_use]
    pub const fn of_rig(frame: &RigFrame, subject: ActorId) -> Self {
        Self {
            subject: Some(subject),
            pose: frame.pose,
            mode: frame.mode,
            projection: frame.projection,
            aspect: frame.aspect,
            magnification: frame.magnification,
            smoothing: frame.smoothing,
        }
    }

    /// The viewport coordinate of a world-space point in this view.
    ///
    /// The view's own basis, its own projection and its own aspect, so a
    /// consumer cannot frame a point through a different frustum than the one on
    /// screen — the property F21-A's framing rule exists for.
    ///
    /// # Errors
    ///
    /// [`FramingError::BehindCamera`] when the point is at or behind the eye,
    /// and [`FramingError::Basis`] when the pose has no usable basis.
    pub fn framing_of(&self, target: WorldPosition) -> Result<Framing, FramingError> {
        self.projection
            .framing_of(self.aspect, self.pose.basis()?, target)
    }

    /// The capture target this view is, for a capture applied to it.
    ///
    /// `rig` is the player's rig when a player view produced the frame and
    /// `None` when a scripted camera did, which is the only authority a capture
    /// may not pin a view through: the install-time check in
    /// [`CameraSession::apply_capture`] refuses a view-pinning capture whose
    /// tick falls inside a running script's span.
    fn as_capture_target(&self, rig: Option<ViewRig>) -> CaptureTarget {
        CaptureTarget {
            rig,
            pose: self.pose,
            projection: self.projection,
            aspect: self.aspect,
            magnification: self.magnification,
        }
    }
}

/// Something the camera session did that a consumer must learn about.
///
/// Events are reported on the frame they happened, in the order they happened,
/// so a consumer that draws the frame also knows that the authority changed, that
/// the camera rebound to another body, or that a capture it had pending will
/// never be taken.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CameraEvent {
    /// A scripted camera drove the camera for the first time.
    ScriptStarted {
        /// The authored camera.
        camera: ContentId,
        /// The body it framed on its first frame.
        subject: Option<ActorId>,
    },
    /// A scripted camera stopped driving, and why.
    ScriptEnded {
        /// The authored camera.
        camera: ContentId,
        /// Why it stopped.
        reason: ScriptEndReason,
    },
    /// The body the camera was bound to changed this frame.
    SubjectRebound {
        /// The body it was bound to.
        from: ActorId,
        /// The body it is bound to now.
        to: ActorId,
    },
    /// A capture was pending for a tick the session has passed.
    CaptureMissed {
        /// The tick the capture asked for.
        requested: Tick,
        /// The tick the frame that found it late is at.
        at: Tick,
    },
    /// A pending capture was dropped without being taken.
    ///
    /// The end-of-session teardown ([`CameraSession::reset`]) drops a capture
    /// for a tick in a mission that is no longer running. The request is handed
    /// back only when the caller itself asks for it
    /// ([`CameraSession::clear_capture`]), so on this path the event is the only
    /// record that a capture the producer believed in will never be taken.
    CaptureDiscarded {
        /// The tick the dropped capture asked for.
        requested: Tick,
    },
}

impl fmt::Display for CameraEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ScriptStarted { camera, subject } => match subject {
                Some(subject) => write!(f, "{camera} started framing {subject}"),
                None => write!(f, "{camera} started holding a pinned pose"),
            },
            Self::ScriptEnded { camera, reason } => write!(f, "{camera} stopped: {reason}"),
            Self::SubjectRebound { from, to } => {
                write!(f, "the camera rebound from {from} to {to}")
            }
            Self::CaptureMissed { requested, at } => write!(
                f,
                "the capture for tick {} was missed: the session is at tick {}",
                requested.0, at.0
            ),
            Self::CaptureDiscarded { requested } => write!(
                f,
                "the capture for tick {} was discarded; it will not be taken",
                requested.0
            ),
        }
    }
}

/// Why a frame could not be produced.
///
/// Every variant is a *frame-time* refusal. None of them tears the session down:
/// the request that caused it is still installed and the next frame retries
/// with the same code, which is what makes a transient producer fault (a body
/// whose pose has not been published yet) recoverable rather than fatal.
#[derive(Clone, Debug, PartialEq)]
pub enum SessionError {
    /// The producer published no pose for the body the player flies.
    ///
    /// The camera cannot follow a body it has no pose for, and inventing one
    /// would place the eye in the world with no authority behind it. It is a
    /// producer that has not finished writing the frame, not a body that died,
    /// so the frame refuses and the next one retries — including when a script
    /// is framing the player role.
    PlayerPoseMissing {
        /// The body with no published pose.
        actor: ActorId,
    },
    /// The player's rig refused the frame.
    Rig(RigError),
    /// The session's own mode set has no authored-sequence mode, which
    /// [`CameraSession::request_script`] already refuses; reaching it means the
    /// set changed underneath a running session.
    Script(ScriptCameraError),
    /// The scripted camera's own smoother refused the step.
    ///
    /// The scripted camera smooths through the same
    /// [`PoseSmoother`](super::smoothing::PoseSmoother) law as the player's rigs
    /// and with the same response rate, so this is the same class of refusal
    /// [`RigError::Smoothing`](super::rig::RigError::Smoothing) is.
    Smoothing(SmoothingError),
    /// The frame's projection could not be pinned into a capture record.
    ///
    /// The capture stays installed and the next frame retries it; see
    /// [`CameraSession::frame`].
    ProjectionPin(ProjectionPinError),
}

impl fmt::Display for SessionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PlayerPoseMissing { actor } => write!(
                f,
                "the producer published no pose for {actor}, which the player is flying"
            ),
            Self::Rig(error) => write!(f, "{error}"),
            Self::Script(error) => write!(f, "{error}"),
            Self::Smoothing(error) => write!(f, "{error}"),
            Self::ProjectionPin(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for SessionError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::PlayerPoseMissing { .. } => None,
            Self::Rig(error) => Some(error),
            Self::Script(error) => Some(error),
            Self::Smoothing(error) => Some(error),
            Self::ProjectionPin(error) => Some(error),
        }
    }
}

impl From<RigError> for SessionError {
    fn from(error: RigError) -> Self {
        Self::Rig(error)
    }
}

impl From<ScriptCameraError> for SessionError {
    fn from(error: ScriptCameraError) -> Self {
        Self::Script(error)
    }
}

impl From<ProjectionPinError> for SessionError {
    fn from(error: ProjectionPinError) -> Self {
        Self::ProjectionPin(error)
    }
}

impl From<SmoothingError> for SessionError {
    fn from(error: SmoothingError) -> Self {
        Self::Smoothing(error)
    }
}

/// One frame the camera session produced.
///
/// Exactly one authority drew it, the camera a renderer draws is in
/// [`Self::view`], and everything that changed is in [`Self::events`].
#[derive(Clone, Debug, PartialEq)]
pub struct SessionFrame {
    /// The tick this frame renders.
    pub at: Tick,
    /// Who drew it.
    pub authority: CameraAuthority,
    /// The camera to draw.
    pub view: SessionView,
    /// The player's rig's own frame, when the player's rig produced the pose.
    ///
    /// It is the richer record — it carries the spyglass aim and the clamped
    /// look offset — so a consumer that needs those reads them here instead of
    /// re-deriving them from [`Self::view`].
    pub rig: Option<RigFrame>,
    /// The capture taken on this frame, when one applied.
    pub capture: Option<CaptureReport>,
    /// What the session did this frame, in the order it did it.
    pub events: Vec<CameraEvent>,
}

impl SessionFrame {
    /// Whether a scripted camera drew this frame.
    #[must_use]
    pub fn is_scripted(&self) -> bool {
        self.authority.is_script()
    }

    /// Whether an event of exactly this kind happened on this frame.
    #[must_use]
    pub fn carries(&self, event: &CameraEvent) -> bool {
        self.events.contains(event)
    }
}

/// The script that is driving the camera, and how far it has got.
#[derive(Clone, Debug, PartialEq)]
struct ActiveScript {
    request: ScriptCameraRequest,
    /// The body it framed last frame, so a rebound is a comparison and not a
    /// guess.
    subject: Option<ActorId>,
    /// Whether it has driven a frame yet.
    started: bool,
}

/// The player's live view: the mode that is up, and whether a free look is
/// layered on it.
///
/// [`ViewRig`] alone is not enough to put a view back. [`ViewRig::Look`] names
/// no mode — it is "whatever is up, turned" — so a capture that switched a
/// one-frame override in and out by rig alone would come back with the free look
/// up over the mode the *capture* named. This is the whole view, and
/// [`restore`](Self::restore) puts all of it back.
#[derive(Clone, Copy, Debug, PartialEq)]
struct LiveView {
    mode: CameraModeKind,
    looking: bool,
}

impl LiveView {
    /// The view `rig` is showing right now.
    fn of(rig: &CameraRig) -> Self {
        Self {
            mode: rig.mode(),
            looking: rig.is_looking(),
        }
    }

    /// Puts this view back on `rig`.
    fn restore(self, rig: &mut CameraRig) -> Result<(), RigError> {
        rig.restore_view(self.mode, self.looking)
    }
}

/// One session's camera: the player's rig, at most one scripted camera and at
/// most one pending capture.
///
/// A session is not an ECS resource and holds no world: it is fed
/// [`SessionFrameInputs`] values and produces [`SessionFrame`] values, so the
/// whole path is exercisable headless and every refusal is reachable.
#[derive(Clone, Debug, PartialEq)]
pub struct CameraSession {
    rig: CameraRig,
    script: Option<ActiveScript>,
    capture: Option<CaptureRequest>,
    script_smoother: PoseSmoother,
    bound: Option<ActorId>,
    /// Events that happened but have not reached a consumer yet.
    ///
    /// Everything the session does is a fact about the world that has already
    /// happened: a script ended, the camera rebound, a capture was retired. A
    /// frame that is *refused* produces no [`SessionFrame`] to carry them, so
    /// they wait here until the next frame that can be drawn — or until
    /// [`reset`](CameraSession::reset), which hands them back. Without this,
    /// one incomplete producer frame would swallow the one event a consumer
    /// needed, and the session would never report it again because its own state
    /// has already moved on.
    pending: Vec<CameraEvent>,
}

impl CameraSession {
    /// A session over `rig`, with no scripted camera and no pending capture.
    ///
    /// The scripted camera's smoother uses the rig's own response rate, so a
    /// script's lag is the same law as the player's and only one number is
    /// designed.
    ///
    /// # Errors
    ///
    /// [`SmoothingError::InvalidResponse`] when the rig's response rate is not
    /// finite and positive — which [`CameraRig`] already refuses at
    /// construction, so this is total in practice and stated rather than
    /// hidden.
    pub fn new(rig: CameraRig) -> Result<Self, SmoothingError> {
        let script_smoother = PoseSmoother::new(rig.smoother().response_per_s())?;
        Ok(Self {
            rig,
            script: None,
            capture: None,
            script_smoother,
            bound: None,
            pending: Vec::new(),
        })
    }

    /// The player's rig.
    #[must_use]
    pub const fn rig(&self) -> &CameraRig {
        &self.rig
    }

    /// The view the player's rig is in.
    #[must_use]
    pub fn view_rig(&self) -> ViewRig {
        self.rig.rig()
    }

    /// Selects the player's view: the producer's ordinary path into F21-B's
    /// rigs.
    ///
    /// # Errors
    ///
    /// [`RigError::ModeNotDeclared`] when the owner's mode set declares no mode
    /// for `rig`, and [`RigError::LookNotAvailable`] for a free look while the
    /// spyglass is up. Nothing is mutated on error.
    pub fn select_rig(&mut self, rig: ViewRig) -> Result<(), RigError> {
        self.rig.set_rig(rig)
    }

    /// The script driving the camera, when one is.
    #[must_use]
    pub fn script(&self) -> Option<&ScriptCameraRequest> {
        self.script.as_ref().map(|active| &active.request)
    }

    /// The body a running script is framing, when it frames one.
    #[must_use]
    pub const fn script_subject(&self) -> Option<ActorId> {
        match &self.script {
            Some(active) => active.subject,
            None => None,
        }
    }

    /// The events the session has reported but that have not reached a consumer
    /// yet.
    ///
    /// Normally empty: [`frame`](Self::frame) carries the events of the frame it
    /// produced. It is not empty after a frame that was **refused** — the events
    /// that frame had already earned wait for the next one — so a caller that
    /// wants them the moment they happen (a log, a test) can read them here.
    #[must_use]
    pub fn pending_events(&self) -> &[CameraEvent] {
        &self.pending
    }

    /// The pending capture, when one is installed.
    #[must_use]
    pub const fn capture(&self) -> Option<&CaptureRequest> {
        self.capture.as_ref()
    }

    /// The body the camera was bound to on the previous frame.
    #[must_use]
    pub const fn bound(&self) -> Option<ActorId> {
        self.bound
    }

    /// Installs a scripted camera request.
    ///
    /// # Errors
    ///
    /// * [`ScriptCameraError::AlreadyActive`] when a script is already driving;
    /// * [`ScriptCameraError::NoAuthoredMode`] when the owner's mode set
    ///   declares no authored-sequence mode, so there is no declared
    ///   projection, placement or magnification for a script to run under;
    /// * [`ScriptCameraError::CapturePinsView`] when a capture that pins a view
    ///   is pending for a tick this script would cover.
    ///
    /// Nothing is mutated on any of them, so a corrected request can be
    /// retried.
    pub fn request_script(
        &mut self,
        request: ScriptCameraRequest,
    ) -> Result<(), ScriptCameraError> {
        if let Some(active) = &self.script {
            return Err(ScriptCameraError::AlreadyActive {
                active: active.request.camera().clone(),
                requested: request.camera().clone(),
            });
        }
        if self
            .rig
            .modes()
            .get(CameraModeKind::AuthoredSequence)
            .is_none()
        {
            return Err(ScriptCameraError::NoAuthoredMode {
                camera: request.camera().clone(),
            });
        }
        if let Some(capture) = &self.capture
            && capture.rig().is_some()
            && request.covers(capture.tick())
        {
            return Err(ScriptCameraError::CapturePinsView {
                tick: capture.tick(),
            });
        }
        self.script = Some(ActiveScript {
            request,
            subject: None,
            started: false,
        });
        Ok(())
    }

    /// Releases the scripted camera before its span ran out.
    ///
    /// The teardown path: the returned request is what was released, and the
    /// next frame reports [`CameraEvent::ScriptEnded`] with
    /// [`ScriptEndReason::Released`], because a camera that quietly stops being
    /// a script is exactly the silent handover this seam rules out.
    ///
    /// A request that is released before it ever drove a frame reports nothing:
    /// there was no handover to announce, and an end with no
    /// [`CameraEvent::ScriptStarted`] would leave a consumer counting a script
    /// that never existed. Releasing when no script is running is not an error
    /// either — it returns `None` and changes nothing.
    pub fn release_script(&mut self) -> Option<ScriptCameraRequest> {
        self.script.take().map(|active| {
            if active.started {
                self.pending.push(CameraEvent::ScriptEnded {
                    camera: active.request.camera().clone(),
                    reason: ScriptEndReason::Released,
                });
            }
            active.request
        })
    }

    /// Installs a capture request for its tick.
    ///
    /// # Errors
    ///
    /// * [`CaptureError::UndeclaredRig`] when the request pins a view the
    ///   owner's mode set has no rig for, checked **now** so the switch at the
    ///   capture frame cannot fail on an undeclared mode later. The free-look rig
    ///   is the one a producer can still invalidate after this call — by raising
    ///   the spyglass in between — and that refusal is reported at the capture
    ///   frame as [`SessionError::Rig`] and retried like any other frame fault;
    /// * [`CaptureError::ScriptCoversTick`] when the request pins a view and a
    ///   running script covers its tick: a scripted camera is not a view a
    ///   capture can pin, and one of them must be released;
    /// * [`CaptureError::AlreadyPending`] when a capture is already installed.
    ///
    /// Nothing is mutated on any of them.
    pub fn apply_capture(&mut self, request: CaptureRequest) -> Result<(), CaptureError> {
        if let Some(pending) = &self.capture {
            return Err(CaptureError::AlreadyPending {
                requested: request.tick(),
                pending: pending.tick(),
            });
        }
        if let Some(rig) = request.rig()
            && !self.rig.can_select(rig)
        {
            return Err(CaptureError::UndeclaredRig {
                rig,
                kind: rig.mode_kind().map_or("free-look", |kind| kind.label()),
            });
        }
        if let Some(active) = &self.script
            && request.rig().is_some()
            && active.request.covers(request.tick())
        {
            return Err(CaptureError::ScriptCoversTick {
                camera: active.request.camera().clone(),
                tick: request.tick(),
            });
        }
        self.capture = Some(request);
        Ok(())
    }

    /// Drops a pending capture without taking it.
    ///
    /// The teardown path for a capture that will not be taken: the returned
    /// request is what was dropped.
    pub fn clear_capture(&mut self) -> Option<CaptureRequest> {
        self.capture.take()
    }

    /// Forgets everything about the previous session generation.
    ///
    /// The end-of-session path: the player's rig resets, the scripted camera
    /// ends with [`ScriptEndReason::SessionEnded`], the pending capture is
    /// dropped and both smoothers are cleared, so the next generation's first
    /// frame cannot open on the previous pilot's camera position, on a script
    /// from a mission that has ended, or on a capture for a tick in a mission
    /// that is no longer running.
    ///
    /// Everything this drops that a consumer must learn about is returned —
    /// the script's end, the discarded capture's tick, and whatever an earlier
    /// refused frame had already earned — because a teardown that reports
    /// nothing is a teardown nobody can see.
    pub fn reset(&mut self) -> Vec<CameraEvent> {
        let mut events = std::mem::take(&mut self.pending);
        if let Some(active) = self.script.take() {
            events.push(CameraEvent::ScriptEnded {
                camera: active.request.camera().clone(),
                reason: ScriptEndReason::SessionEnded,
            });
        }
        if let Some(capture) = self.capture.take() {
            events.push(CameraEvent::CaptureDiscarded {
                requested: capture.tick(),
            });
        }
        self.rig.reset();
        self.script_smoother.clear();
        self.bound = None;
        events
    }

    /// Produces this frame's camera.
    ///
    /// # Errors
    ///
    /// [`SessionError`] for a frame that cannot be drawn: no published pose for
    /// the player body, a refusal from the player's rig, or a projection that
    /// cannot be pinned into the capture record. **Nothing is torn down by an
    /// error**: the script, the capture and the view stay installed and the next
    /// frame retries with the same code. The single mutation an error can leave
    /// behind is one frame of the rig's own smoothing — which is what a frame
    /// is — plus, when a capture pinned a view, the view being switched back to
    /// what it was before the capture frame.
    ///
    /// Anything the session learned *before* it refused is not lost with the
    /// refused frame: the events stay in [`pending_events`](Self::pending_events)
    /// and are carried by the next frame that can be drawn. A rebound the
    /// session has already recorded cannot be reported twice, so a frame that
    /// dropped it would drop it for good.
    pub fn frame(&mut self, inputs: &SessionFrameInputs<'_>) -> Result<SessionFrame, SessionError> {
        // What an earlier refused frame had already earned comes first, so the
        // order a consumer reads is the order things happened.
        let mut events = std::mem::take(&mut self.pending);
        let resolved = self.draw_frame(inputs, &mut events);
        match resolved {
            Ok(mut frame) => {
                frame.events = events;
                Ok(frame)
            }
            Err(error) => {
                self.pending = events;
                Err(error)
            }
        }
    }

    /// Draws one frame, appending what it learned to `events`.
    fn draw_frame(
        &mut self,
        inputs: &SessionFrameInputs<'_>,
        events: &mut Vec<CameraEvent>,
    ) -> Result<SessionFrame, SessionError> {
        // A capture is for exactly one tick. One that is already behind can
        // never be taken, so it is retired with an event rather than held: a
        // pending capture is a promise about one frame, and a session that kept
        // it forever would hold it against every later mission.
        let mut capture = self.capture.clone();
        if let Some(pending) = &capture
            && pending.tick().0 < inputs.at.0
        {
            events.push(CameraEvent::CaptureMissed {
                requested: pending.tick(),
                at: inputs.at,
            });
            self.capture = None;
            capture = None;
        }
        let capture = capture.filter(|pending| pending.tick() == inputs.at);

        // A capture that pins a view switches the rig for exactly this frame,
        // and the rig is switched back before this function returns — on the
        // error paths too, which is why the restore happens between resolving
        // and propagating rather than at the end.
        //
        // What is recorded is the whole view, not just the rig that is up: a free
        // look is a rig layered on a mode, and restoring only the rig would
        // leave the player looking around in the mode the capture named.
        let capture_view = capture
            .as_ref()
            .and_then(|pending| pending.rig())
            .map(|requested| (requested, LiveView::of(&self.rig)));
        if let Some((requested, _)) = capture_view {
            self.rig.set_rig(requested)?;
        }

        let aspect = capture
            .as_ref()
            .and_then(|pending| pending.aspect())
            .unwrap_or(inputs.aspect);

        let resolved = self.resolve_frame(inputs, aspect, events);
        let restore = match capture_view {
            Some((_, previous)) => previous.restore(&mut self.rig),
            None => Ok(()),
        };
        let mut frame = resolved?;

        // A capture that pins a pose holds it exactly: the smoothing belongs to
        // the live camera, and a reproducible frame must not depend on the wall
        // time between frames. The rig's own state is left alone, so when the
        // capture retires the camera carries on from where it was.
        if let Some(pending) = &capture
            && let Some(pose) = pending.pose()
        {
            frame.view.pose = pose;
            frame.view.smoothing = SmoothingState::Reseated;
            if let Some(rig_frame) = frame.rig.as_mut() {
                rig_frame.pose = pose;
                rig_frame.smoothing = SmoothingState::Reseated;
            }
        }

        // The view the session goes back to once this frame is drawn. The restore
        // has already run, so this is the live view whether the capture switched
        // anything or not.
        let restore_to = self.rig.rig();
        let report = match &capture {
            Some(pending) => Some(
                pending.apply(
                    &frame
                        .view
                        .as_capture_target(frame.rig.as_ref().map(|rig_frame| rig_frame.rig)),
                    restore_to,
                )?,
            ),
            None => None,
        };
        restore?;

        // One frame, one capture: the request is retired here so a pinned pose,
        // view or aspect cannot leak into the frames around it.
        if capture.is_some() {
            self.capture = None;
        }

        frame.capture = report;
        // The events are attached by `frame`, which is also what decides what
        // happens to them if this frame is refused.
        Ok(frame)
    }

    /// Resolves the camera for a frame, with `aspect` already settled.
    fn resolve_frame(
        &mut self,
        inputs: &SessionFrameInputs<'_>,
        aspect: AspectRatio,
        events: &mut Vec<CameraEvent>,
    ) -> Result<SessionFrame, SessionError> {
        // The active request is cloned out because resolving the frame needs
        // `&mut self` while reading the span needs `&self.script`. The clone is
        // one small record and it only happens on frames where a script is
        // actually driving, which is a cinematic's length and nothing else.
        if let Some(active) = self.script.clone() {
            // Before its span: the script is pending and the player keeps the
            // camera. At or after `until`: the script has handed it back, and
            // the frame that discovers it is the frame that reports it.
            if inputs.at.0 >= active.request.until().0 {
                events.push(CameraEvent::ScriptEnded {
                    camera: active.request.camera().clone(),
                    reason: ScriptEndReason::SpanEnded { at: inputs.at },
                });
                self.script = None;
                self.script_smoother.clear();
            } else if active.request.covers(inputs.at)
                && let Some(view) = self.scripted_view(&active.request, inputs, aspect, events)?
            {
                let subject = view.subject;
                self.bound = subject;
                let mut started = false;
                if let Some(script) = self.script.as_mut() {
                    script.subject = subject;
                    started = !script.started;
                    script.started = true;
                }
                if started {
                    events.push(CameraEvent::ScriptStarted {
                        camera: active.request.camera().clone(),
                        subject,
                    });
                }
                return Ok(SessionFrame {
                    at: inputs.at,
                    authority: CameraAuthority::Script {
                        camera: active.request.camera().clone(),
                        subject,
                    },
                    view,
                    rig: None,
                    capture: None,
                    events: Vec::new(),
                });
            }
        }
        self.player_frame(inputs, aspect, events)
    }

    /// The player's own camera for a frame.
    fn player_frame(
        &mut self,
        inputs: &SessionFrameInputs<'_>,
        aspect: AspectRatio,
        events: &mut Vec<CameraEvent>,
    ) -> Result<SessionFrame, SessionError> {
        let aircraft = inputs
            .pose_of(inputs.player)
            .ok_or(SessionError::PlayerPoseMissing {
                actor: inputs.player,
            })?;
        self.report_rebound(inputs.player, events);
        let rig_frame = self.rig.resolve(&RigInputs {
            at: inputs.at,
            subject: inputs.player,
            aircraft,
            aspect,
            elapsed: inputs.elapsed,
            look: inputs.look,
            spyglass: inputs.spyglass,
            origin_change: inputs.origin_change,
        })?;
        self.bound = Some(inputs.player);
        Ok(SessionFrame {
            at: inputs.at,
            authority: CameraAuthority::Player { rig: rig_frame.rig },
            view: SessionView::of_rig(&rig_frame, inputs.player),
            rig: Some(rig_frame),
            capture: None,
            events: Vec::new(),
        })
    }

    /// A scripted camera's view for a frame, or `None` when the script has to
    /// end instead.
    fn scripted_view(
        &mut self,
        request: &ScriptCameraRequest,
        inputs: &SessionFrameInputs<'_>,
        aspect: AspectRatio,
        events: &mut Vec<CameraEvent>,
    ) -> Result<Option<SessionView>, SessionError> {
        // The mode is copied out of the set for the same reason the rig copies
        // it: the frame mutates this session, and one small record copy per
        // render frame is cheaper than a self-referential borrow.
        let mode = self
            .rig
            .modes()
            .get(CameraModeKind::AuthoredSequence)
            .cloned()
            .ok_or(ScriptCameraError::NoAuthoredMode {
                camera: request.camera().clone(),
            })?;

        let (subject, desired) = match request.shot() {
            ScriptedShot::Pinned { pose } => {
                // A pinned pose is held exactly: there is no lag to interpolate
                // and no body to rebind, so the frame-rate independent follow
                // has nothing to do this frame.
                self.script_smoother.snap(pose);
                return Ok(Some(SessionView {
                    subject: None,
                    pose,
                    mode: mode.kind(),
                    projection: mode.projection(),
                    aspect,
                    magnification: mode.magnification(),
                    smoothing: SmoothingState::Reseated,
                }));
            }
            ScriptedShot::Follows { subject: role } => {
                let subject = role.resolve(inputs.player);
                let Some(body) = inputs.pose_of(subject) else {
                    if subject == inputs.player {
                        // The producer says the player flies this body and
                        // published no pose for it. That is a producer that has
                        // not finished writing this frame, not a body that died:
                        // the player is stated every frame and a body the
                        // producer still names is still there. So the frame
                        // refuses and the next frame retries with the script
                        // still installed — ending it here would let one
                        // incomplete frame cut a cinematic short.
                        return Err(SessionError::PlayerPoseMissing { actor: subject });
                    }
                    // The body the shot named is not in this frame. The camera
                    // cannot follow a body it cannot see, and it must not keep
                    // drawing as though it could: the script ends here, the
                    // player takes the camera back on the same frame, and the
                    // reason is reported.
                    events.push(CameraEvent::ScriptEnded {
                        camera: request.camera().clone(),
                        reason: ScriptEndReason::SubjectGone { actor: subject },
                    });
                    self.script = None;
                    self.script_smoother.clear();
                    return Ok(None);
                };
                let (eye, rotation) = oriented_pose(mode.placement(), body)?;
                (subject, CameraPose::new(eye, rotation))
            }
        };

        // A rebound is a comparison against the body this script framed last
        // frame, so the swap is *named* rather than inferred from a smoothing
        // state. The player role re-resolves every frame, which is what makes an
        // aircraft swap a rebound instead of a stale binding (AC03).
        let rebound = self.script.as_ref().and_then(|active| active.subject);
        if let Some(previous) = rebound
            && previous != subject
        {
            events.push(CameraEvent::SubjectRebound {
                from: previous,
                to: subject,
            });
        }
        // A teleport is a world-space jump with no continuous path, and a first
        // frame (or a rebound) has nothing continuous to interpolate from. A
        // rebase is neither: canonical f64 world space is what this state is in,
        // so a rebase cannot move it.
        let reseat = inputs.origin_change == OriginChange::Teleport || rebound != Some(subject);
        let pose = if reseat {
            self.script_smoother.snap(desired);
            desired
        } else {
            self.script_smoother.advance(desired, inputs.elapsed)?
        };
        Ok(Some(SessionView {
            subject: Some(subject),
            pose,
            mode: mode.kind(),
            projection: mode.projection(),
            aspect,
            magnification: mode.magnification(),
            smoothing: self.script_smoother.state(),
        }))
    }

    /// Reports a change of the body the camera is bound to.
    ///
    /// A scripted camera holding a **pinned** pose is bound to no body, so
    /// handing that camera back is reported by
    /// [`CameraEvent::ScriptEnded`] and not by a rebound: there is no body to
    /// have rebound from.
    fn report_rebound(&mut self, subject: ActorId, events: &mut Vec<CameraEvent>) {
        if let Some(previous) = self.bound
            && previous != subject
        {
            events.push(CameraEvent::SubjectRebound {
                from: previous,
                to: subject,
            });
        }
        self.bound = Some(subject);
    }
}
