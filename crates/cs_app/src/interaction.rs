//! The interaction lowering boundary (F36-A).
//!
//! Spec: `specs/F36-docking-passenger-pickups-boarding-and-plane-swaps.md`,
//! stage `### F36-A`. Shared contract:
//! `docs/contracts/STATE-TRANSACTIONS.md`.
//!
//! This module sits between the declared interaction schema
//! ([`cs_content::interaction`]) and the runtime contract
//! ([`cs_sim::interaction`]), which cannot see each other — `cs_sim` must not
//! depend on `cs_content` (`docs/01-ARCHITECTURE.md`). It contains:
//!
//! * [`lower_interaction`] — the conversion boundary: a validated
//!   [`cs_content::interaction::DeclaredInteraction`] becomes an
//!   [`InteractionPlan`] with its kind, authorization, eligibility envelope
//!   and transfer policy mapped field-wise.
//! * [`InteractionLowerError::UnknownAuthorization`] and
//!   [`InteractionLowerError::UnknownEligibility`] — the mandatory values: an
//!   interaction cannot latch under a guessed objective or a guessed capture
//!   radius, so the boundary refuses instead of substituting a default.
//! * [`InteractionActorBinding`] — the ECS record tying an entity to its actor
//!   and catalog subject, generation-stamped like
//!   [`crate::scene::SceneNodeBinding`] and
//!   [`crate::capital::CapitalActorBinding`] so a reload can never leave a
//!   stale binding looking live.
//!
//! Nothing here owns runtime behavior: the state machine, eligibility test and
//! transfer policy are `cs_sim::interaction`'s; this is the conversion and
//! binding record the ECS wiring consumes (F36-B/C).

use bevy::ecs::component::Component;
use cs_content::interaction::{
    DeclaredCameraTransfer, DeclaredControlOwner, DeclaredEligibility, DeclaredInteraction,
    DeclaredInteractionKind, DeclaredInventoryTransfer, DeclaredPilotTransfer,
    DeclaredVelocityTransfer,
};
use cs_sim::damage::ActorId;
use cs_sim::interaction::{
    CameraTransfer, ControlOwner, EligibilityEnvelope, EnvelopeError, InteractionAuthorization,
    InteractionId, InteractionKind, InventoryTransfer, PilotTransfer, TransferPolicy,
    VelocityTransfer,
};
use cs_types::content::{ContentId, Resolved};
use cs_types::evidence::ClaimId;

use crate::scene::SceneGeneration;

/// Why a declared interaction could not be lowered to the runtime contract.
#[derive(Clone, Debug, PartialEq)]
pub enum InteractionLowerError {
    /// The authorizing objective is `Resolved::Unknown`: an interaction cannot
    /// latch or complete under a guessed mission phase.
    UnknownAuthorization {
        /// The claim the unknown is recorded under.
        claim_id: ClaimId,
        /// Why the objective is unknown.
        reason: String,
    },
    /// An eligibility field is `Resolved::Unknown`: a capture envelope cannot
    /// be evaluated against an invented value.
    UnknownEligibility {
        /// The declared field name.
        field: &'static str,
        /// The claim the unknown is recorded under.
        claim_id: ClaimId,
        /// Why the value is unknown.
        reason: String,
    },
    /// The runtime refused the assembled envelope.
    Envelope(EnvelopeError),
}

impl std::fmt::Display for InteractionLowerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownAuthorization { claim_id, reason } => write!(
                f,
                "the interaction authorization is unknown ({}: {reason}) and cannot be lowered",
                claim_id.as_str()
            ),
            Self::UnknownEligibility {
                field,
                claim_id,
                reason,
            } => write!(
                f,
                "eligibility field {field} is unknown ({}: {reason}) and cannot be lowered",
                claim_id.as_str()
            ),
            Self::Envelope(source) => write!(f, "the runtime refused the envelope: {source}"),
        }
    }
}

impl std::error::Error for InteractionLowerError {}

/// The lowered runtime contract of one interaction, ready for the F36-B
/// runtime to drive.
#[derive(Clone, Debug, PartialEq)]
pub struct InteractionPlan {
    /// The stable interaction identity.
    pub id: InteractionId,
    /// The interaction kind.
    pub kind: InteractionKind,
    /// The mission authorization.
    pub authorization: InteractionAuthorization,
    /// The swept eligibility envelope.
    pub envelope: EligibilityEnvelope,
    /// The per-transition transfer policy.
    pub policy: TransferPolicy,
}

/// Lowers a declared interaction into the runtime contract.
///
/// Kind, authorization, the eligibility envelope and the transfer policy map
/// field-wise. An unknown objective or an unknown eligibility field is
/// refused by claim; the boundary never invents a value.
///
/// # Errors
///
/// [`InteractionLowerError`] naming the first unresolved field or the runtime
/// envelope refusal.
pub fn lower_interaction(
    id: InteractionId,
    declared: &DeclaredInteraction,
) -> Result<InteractionPlan, InteractionLowerError> {
    let kind = lower_kind(declared.kind());
    let authorization = match declared.authorization() {
        Resolved::Known(known) => {
            InteractionAuthorization::new(id.session, kind, known.value.clone())
        }
        Resolved::Unknown { claim_id, reason } => {
            return Err(InteractionLowerError::UnknownAuthorization {
                claim_id: claim_id.clone(),
                reason: reason.clone(),
            });
        }
    };

    let envelope = lower_envelope(declared.envelope())?;
    let policy = lower_policy(declared.transfer());

    Ok(InteractionPlan {
        id,
        kind,
        authorization,
        envelope,
        policy,
    })
}

fn lower_envelope(
    envelope: &DeclaredEligibility,
) -> Result<EligibilityEnvelope, InteractionLowerError> {
    EligibilityEnvelope::try_new(
        required_f64(&envelope.capture_radius_m, "capture_radius_m")?,
        required_f64(&envelope.max_relative_speed_m_s, "max_relative_speed_m_s")?,
        required_f64(&envelope.min_closing_speed_m_s, "min_closing_speed_m_s")?,
        required_f64(&envelope.max_approach_angle_deg, "max_approach_angle_deg")?,
        required_axis(&envelope.approach_axis_local)?,
    )
    .map_err(InteractionLowerError::Envelope)
}

fn required_f64(value: &Resolved<f64>, field: &'static str) -> Result<f64, InteractionLowerError> {
    match value {
        Resolved::Known(known) => Ok(known.value),
        Resolved::Unknown { claim_id, reason } => Err(InteractionLowerError::UnknownEligibility {
            field,
            claim_id: claim_id.clone(),
            reason: reason.clone(),
        }),
    }
}

fn required_axis(value: &Resolved<[f64; 3]>) -> Result<[f64; 3], InteractionLowerError> {
    match value {
        Resolved::Known(known) => Ok(known.value),
        Resolved::Unknown { claim_id, reason } => Err(InteractionLowerError::UnknownEligibility {
            field: "approach_axis_local",
            claim_id: claim_id.clone(),
            reason: reason.clone(),
        }),
    }
}

fn lower_kind(kind: DeclaredInteractionKind) -> InteractionKind {
    match kind {
        DeclaredInteractionKind::Docking => InteractionKind::Docking,
        DeclaredInteractionKind::PassengerPickup => InteractionKind::PassengerPickup,
        DeclaredInteractionKind::Boarding => InteractionKind::Boarding,
        DeclaredInteractionKind::AircraftSwap => InteractionKind::AircraftSwap,
    }
}

fn lower_policy(policy: &cs_content::interaction::DeclaredTransferPolicy) -> TransferPolicy {
    TransferPolicy {
        velocity: lower_velocity(policy.velocity),
        pilot: lower_pilot(policy.pilot),
        inventory: lower_inventory(policy.inventory),
        camera: lower_camera(policy.camera),
        control_after_release: lower_control(policy.control_after_release),
    }
}

fn lower_velocity(velocity: DeclaredVelocityTransfer) -> VelocityTransfer {
    match velocity {
        DeclaredVelocityTransfer::MatchTarget => VelocityTransfer::MatchTarget,
        DeclaredVelocityTransfer::PreserveInitiator => VelocityTransfer::PreserveInitiator,
    }
}

fn lower_pilot(pilot: DeclaredPilotTransfer) -> PilotTransfer {
    match pilot {
        DeclaredPilotTransfer::MoveToTarget => PilotTransfer::MoveToTarget,
        DeclaredPilotTransfer::None => PilotTransfer::None,
    }
}

fn lower_inventory(inventory: DeclaredInventoryTransfer) -> InventoryTransfer {
    match inventory {
        DeclaredInventoryTransfer::MoveToTarget => InventoryTransfer::MoveToTarget,
        DeclaredInventoryTransfer::None => InventoryTransfer::None,
    }
}

fn lower_camera(camera: DeclaredCameraTransfer) -> CameraTransfer {
    match camera {
        DeclaredCameraTransfer::FollowTarget => CameraTransfer::FollowTarget,
        DeclaredCameraTransfer::FollowInitiator => CameraTransfer::FollowInitiator,
    }
}

fn lower_control(control: DeclaredControlOwner) -> ControlOwner {
    match control {
        DeclaredControlOwner::Initiator => ControlOwner::Initiator,
        DeclaredControlOwner::Target => ControlOwner::Target,
        DeclaredControlOwner::LatchController => ControlOwner::LatchController,
    }
}

/// Component: marks an entity as the interaction face of one actor.
///
/// `actor` is the actor's [`ActorId`], `subject` the catalog subject the
/// interaction was declared for and `generation` the scene generation that
/// spawned the binding — so a reload stamps new bindings and stale ones are
/// identified by mismatch, never by surviving pointers (the
/// `STATE-TRANSACTIONS` session-generation discipline).
#[derive(Component, Clone, Debug, PartialEq)]
pub struct InteractionActorBinding {
    /// The actor this entity presents.
    pub actor: ActorId,
    /// The interaction catalog subject.
    pub subject: ContentId,
    /// The scene generation that spawned the binding.
    pub generation: SceneGeneration,
}

/// Why an initiator's spatial record could not supply an interaction motion.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnchorMotionError {
    /// The record has no swept segment: it was just spawned or teleported, so
    /// no continuous path exists and no velocity may be inferred (a
    /// teleport is not a rebase, F16 non-negotiable behavior 5).
    NoContinuousPath,
    /// The segment could not form a motion.
    Motion(cs_sim::interaction::MotionError),
}

impl std::fmt::Display for AnchorMotionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoContinuousPath => {
                write!(f, "the initiator has no continuous path to sweep")
            }
            Self::Motion(source) => write!(f, "the initiator motion is invalid: {source}"),
        }
    }
}

impl std::error::Error for AnchorMotionError {}

/// Builds the initiator's one-tick world-frame motion from its spatial record.
///
/// Both endpoints are the record's f64 world positions, which an
/// [`OriginShift`](crate::origin::OriginShift) never changes, so a rebase
/// between the two samples cannot show up as speed. The f32 local cache is
/// deliberately not read.
///
/// # Errors
///
/// [`AnchorMotionError::NoContinuousPath`] without a swept segment, or
/// [`AnchorMotionError::Motion`] for a non-finite position or zero rate.
pub fn initiator_motion(
    anchor: &crate::origin::SpatialAnchor,
    ticks_per_second: u32,
) -> Result<cs_sim::interaction::InitiatorMotion, AnchorMotionError> {
    let sweep = anchor.sweep().ok_or(AnchorMotionError::NoContinuousPath)?;
    cs_sim::interaction::InitiatorMotion::try_new(
        sweep.from_world().to_array(),
        anchor.world().to_array(),
        1,
        ticks_per_second,
    )
    .map_err(AnchorMotionError::Motion)
}

/// Resource: the interaction session of the running mission (F36-C).
///
/// Wraps [`cs_sim::interaction::InteractionSession`] so the app's destroyed-
/// actor, pause and retry paths reach one owner; the resource holds no state
/// of its own.
#[derive(bevy::ecs::resource::Resource, Clone, Debug, PartialEq)]
pub struct InteractionRuntime(pub cs_sim::interaction::InteractionSession);

impl InteractionRuntime {
    /// Reports that an actor's last damage zone was destroyed. Every active
    /// interaction involving it aborts and control resolves back to the
    /// surviving initiator.
    pub fn on_actor_destroyed(
        &mut self,
        actor: ActorId,
    ) -> Vec<cs_sim::interaction::InteractionAbort> {
        self.0.actor_destroyed(actor)
    }

    /// Retries the mission under a new session generation.
    pub fn on_retry(&mut self, session: u64) -> Vec<cs_sim::interaction::InteractionAbort> {
        self.0.retry(session)
    }
}
