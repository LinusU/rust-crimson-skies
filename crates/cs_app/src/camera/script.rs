//! The scripted camera request and its refusals (F21-C).
//!
//! Spec: `specs/F21-cameras-cockpit-views-and-spyglass.md`, stage
//! `### F21-C`. Shared contract: `docs/contracts/UI-NETWORK.md`.
//!
//! A *script camera* is what a mission script, a cinematic or a capture tool
//! asks the camera to do for a bounded span. This module is the **producer
//! half** of that conversation: the typed request a producer hands the
//! [`CameraSession`](super::session::CameraSession), and the refusals it can
//! get back before anything is applied. The consumer half — which authority
//! produced a frame, when the script stops driving and what the camera does
//! instead — is [`super::session`].
//!
//! # What this stage does not own
//!
//! F40 owns authored camera *timelines*: decoding them, choosing the pose at
//! each tick and playing them back (`specs/F40-cutscenes-video-scripted-cameras-and-transitions.md`,
//! stages `### F40-A` and `### F40-B`, whose owner paths are
//! `crates/cs_app/src/cinematics/` and `crates/cs_content/src/cinematics.rs`).
//! This module deliberately carries **no keyframes, no interpolation and no
//! playback clock**: a request is one span of ticks plus one shot. A timeline
//! that has been decoded drives the camera by asking for a shot per span, which
//! is why this seam can exist before it.
//!
//! # Two shots, and why a script cannot invent a frustum
//!
//! [`ScriptedShot::Pinned`] holds one exact pose. [`ScriptedShot::Follows`]
//! frames a body through the owner's **declared**
//! [`AuthoredSequence`](cs_content::cameras::CameraModeKind::AuthoredSequence)
//! mode — its projection, its placement and its magnification — so a scripted
//! camera looks the way the content says a scripted camera looks and never
//! through a frustum a producer invented. A set that declares no authored-sequence
//! mode refuses the request ([`ScriptCameraError::NoAuthoredMode`]) instead of
//! falling back to the chase view, which is the refusal F21-B left in place by
//! name.
//!
//! # The subject is a role, not an instance
//!
//! [`ScriptSubject::Player`] names *whoever the player currently flies*, so an
//! aircraft swap rebinds the camera in the same frame — that is the whole of
//! AC03. [`ScriptSubject::Actor`] names one body, and a body that is not in the
//! producer's frame ends the script with
//! [`ScriptEndReason::SubjectGone`] instead of leaving a camera pointed at a
//! body that no longer exists. There is no third option: a script cannot hold a
//! body across its destruction, and it cannot silently start following
//! somebody else.

use std::fmt;

use cs_sim::damage::ActorId;
use cs_types::Tick;
use cs_types::content::{ContentId, ContentKind};

use super::pose::CameraPose;

/// What a producer asks the camera to do for a bounded span.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ScriptedShot {
    /// Hold one exact world pose.
    ///
    /// The pose is held **exactly**: the frame-rate independent follow has
    /// nothing to interpolate, and a scripted camera that lagged would make the
    /// same request at the same tick draw two different frames. The capture
    /// report says so in [`CaptureOverride::Smoothing`](super::capture::CaptureOverride::Smoothing).
    Pinned {
        /// The pose to hold, in canonical world space.
        pose: CameraPose,
    },
    /// Frame a body through the owner's declared authored-sequence mode.
    Follows {
        /// Which body, or which role.
        subject: ScriptSubject,
    },
}

/// Which body a following shot frames.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ScriptSubject {
    /// The body the player currently flies.
    ///
    /// Resolved from the producer's frame every time, never cached: this is
    /// what makes an aircraft swap rebind the camera rather than leave it on a
    /// despawned body (AC03).
    Player,
    /// One specific body, which must still exist.
    Actor(ActorId),
}

impl ScriptSubject {
    /// The body this subject names for a frame whose player body is `player`.
    ///
    /// A specific [`ActorId`] answers itself; [`Self::Player`] answers whatever
    /// the producer says the player flies this frame.
    #[must_use]
    pub const fn resolve(self, player: ActorId) -> ActorId {
        match self {
            Self::Player => player,
            Self::Actor(actor) => actor,
        }
    }

    /// Whether this subject can change body without a new request.
    ///
    /// Only the player role can: it is re-resolved every frame, so a swap is a
    /// rebound and not an ending.
    #[must_use]
    pub const fn is_role(self) -> bool {
        matches!(self, Self::Player)
    }
}

impl fmt::Display for ScriptSubject {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Player => f.write_str("the player's aircraft"),
            Self::Actor(actor) => write!(f, "{actor}"),
        }
    }
}

/// Why a scripted camera stopped driving the session's camera.
///
/// Every ending is reported on the frame it happens
/// ([`CameraEvent::ScriptEnded`](super::session::CameraEvent::ScriptEnded)), so a
/// consumer learns that the camera is back under the player's control in the
/// same frame it draws without one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ScriptEndReason {
    /// The span of ticks ran out.
    SpanEnded {
        /// The first tick the request no longer covers.
        at: Tick,
    },
    /// A body the shot was framing is not in the producer's frame any more.
    SubjectGone {
        /// The body that went away.
        actor: ActorId,
    },
    /// The producer released the camera before its span ran out.
    Released,
    /// The session was torn down for a new session generation.
    SessionEnded,
}

impl fmt::Display for ScriptEndReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SpanEnded { at } => write!(f, "its span ran out at tick {}", at.0),
            Self::SubjectGone { actor } => write!(f, "{actor} is no longer in the frame"),
            Self::Released => f.write_str("the producer released it"),
            Self::SessionEnded => f.write_str("the session was torn down"),
        }
    }
}

/// Why a scripted camera request was refused.
///
/// Every variant is a refusal at the request boundary, **before** the session
/// has changed anything: an install that fails leaves the session exactly as it
/// was, the previous script still driving if there was one, so the producer can
/// correct the request and retry.
#[derive(Clone, Debug, PartialEq)]
pub enum ScriptCameraError {
    /// The request named a content id that is not a camera track.
    ///
    /// The identity of an authored camera is a `camera_track` id
    /// (`cs_content::cinematics` addresses an in-engine camera that way), and a
    /// mission or an airframe id in this field would make one id address two
    /// unrelated records.
    NotACameraTrack {
        /// What the request named.
        id: ContentId,
    },
    /// The span of ticks is empty: `until` is not after `from`.
    EmptySpan {
        /// The first tick the request covers.
        from: Tick,
        /// The first tick it does not cover.
        until: Tick,
    },
    /// The owner declares no authored-sequence mode, so there is no declared
    /// projection, placement or magnification for a scripted camera to run
    /// under.
    ///
    /// This is the refusal F21-B left in place by name: a scripted camera with
    /// no declared mode must not silently behave like the chase view.
    NoAuthoredMode {
        /// The camera that asked.
        camera: ContentId,
    },
    /// A scripted camera is already driving the session's camera.
    AlreadyActive {
        /// The camera that is driving now.
        active: ContentId,
        /// The camera that asked.
        requested: ContentId,
    },
    /// A capture that pins a **view** is pending for a tick this request would
    /// cover.
    ///
    /// One camera has one authority. A capture that names the view it draws
    /// through and a scripted camera that names the pose it draws are both
    /// claims on the same frame, and neither can be honoured without silently
    /// dropping the other, so the session refuses the second one and the
    /// producer releases or retires the first. A capture that pins nothing but
    /// a mission, a tick, a pose or settings has no such claim and coexists
    /// with a script.
    CapturePinsView {
        /// The tick the pending capture asked for.
        tick: Tick,
    },
}

impl fmt::Display for ScriptCameraError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotACameraTrack { id } => write!(
                f,
                "a scripted camera names an authored camera track, and {id} is a {} id",
                id.kind()
            ),
            Self::EmptySpan { from, until } => write!(
                f,
                "a scripted camera must cover at least one tick, and ticks {}..{} covers none",
                from.0, until.0
            ),
            Self::NoAuthoredMode { camera } => write!(
                f,
                "{camera} cannot drive the camera: this aircraft's camera modes declare no \
                 authored sequence, so there is no projection, placement or magnification to \
                 run it under"
            ),
            Self::AlreadyActive { active, requested } => write!(
                f,
                "{active} is already driving the camera, so {requested} was not installed"
            ),
            Self::CapturePinsView { tick } => write!(
                f,
                "a capture that pins a view is pending for tick {}, which this request would \
                 cover; release or retire the capture first",
                tick.0
            ),
        }
    }
}

impl std::error::Error for ScriptCameraError {}

/// A scripted camera request: which authored camera, for which span, doing
/// what.
///
/// A request is a **value** with a bounded lifetime. It is installed with
/// [`CameraSession::request_script`](super::session::CameraSession::request_script),
/// runs until its span ends or the producer releases it, and cannot outlive the
/// session generation that installed it.
#[derive(Clone, Debug, PartialEq)]
pub struct ScriptCameraRequest {
    camera: ContentId,
    from: Tick,
    until: Tick,
    shot: ScriptedShot,
}

impl ScriptCameraRequest {
    /// A request that runs `shot` over the half-open span of ticks
    /// `from..until`.
    ///
    /// # Errors
    ///
    /// [`ScriptCameraError::NotACameraTrack`] when `camera` is not a
    /// `camera_track` id, and [`ScriptCameraError::EmptySpan`] when the span
    /// covers no tick at all.
    pub fn new(
        camera: ContentId,
        from: Tick,
        until: Tick,
        shot: ScriptedShot,
    ) -> Result<Self, ScriptCameraError> {
        if camera.kind() != ContentKind::CameraTrack {
            return Err(ScriptCameraError::NotACameraTrack { id: camera });
        }
        if until.0 <= from.0 {
            return Err(ScriptCameraError::EmptySpan { from, until });
        }
        Ok(Self {
            camera,
            from,
            until,
            shot,
        })
    }

    /// A request that holds one exact pose over the span.
    ///
    /// # Errors
    ///
    /// As [`Self::new`].
    pub fn pinned(
        camera: ContentId,
        from: Tick,
        until: Tick,
        pose: CameraPose,
    ) -> Result<Self, ScriptCameraError> {
        Self::new(camera, from, until, ScriptedShot::Pinned { pose })
    }

    /// A request that frames a subject over the span.
    ///
    /// # Errors
    ///
    /// As [`Self::new`].
    pub fn follows(
        camera: ContentId,
        from: Tick,
        until: Tick,
        subject: ScriptSubject,
    ) -> Result<Self, ScriptCameraError> {
        Self::new(camera, from, until, ScriptedShot::Follows { subject })
    }

    /// The authored camera that asked.
    #[must_use]
    pub const fn camera(&self) -> &ContentId {
        &self.camera
    }

    /// The first tick the request covers.
    #[must_use]
    pub const fn from(&self) -> Tick {
        self.from
    }

    /// The first tick the request does not cover.
    #[must_use]
    pub const fn until(&self) -> Tick {
        self.until
    }

    /// What the camera is asked to do.
    #[must_use]
    pub const fn shot(&self) -> ScriptedShot {
        self.shot
    }

    /// Whether this request covers `at`.
    ///
    /// The span is half-open, so a request that ends at tick `n` has handed the
    /// camera back before the frame at `n` is drawn and never overlaps the next
    /// one.
    #[must_use]
    pub const fn covers(&self, at: Tick) -> bool {
        at.0 >= self.from.0 && at.0 < self.until.0
    }

    /// The body this request frames at `at`, for a frame whose player body is
    /// `player`.
    ///
    /// `None` for a [`ScriptedShot::Pinned`] request: a pinned pose has no
    /// subject, and a consumer that asked must not be handed one.
    #[must_use]
    pub const fn subject_of(&self, player: ActorId) -> Option<ActorId> {
        match self.shot {
            ScriptedShot::Pinned { .. } => None,
            ScriptedShot::Follows { subject } => Some(subject.resolve(player)),
        }
    }
}

impl fmt::Display for ScriptCameraRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} over ticks {}..{}",
            self.camera, self.from.0, self.until.0
        )?;
        match self.shot {
            ScriptedShot::Pinned { pose } => write!(f, " holding {pose:?}"),
            ScriptedShot::Follows { subject } => write!(f, " following {subject}"),
        }
    }
}
