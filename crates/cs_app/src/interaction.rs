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
//! * [`InteractionRuntime`] (F36-C) and the F36-C.1 consumer wiring —
//!   [`forward_actor_destroyed`] and [`InteractionSchedulePlugin`] forward
//!   the damage side's per-actor destroyed events
//!   ([`crate::damage::DamageActorDestroyed`]) into the runtime, so a
//!   carrier destroyed during a latch aborts in a running app, with every
//!   abort or refusal recorded in [`InteractionConsumerLog`].
//!
//! Nothing here owns runtime behavior: the state machine, eligibility test and
//! transfer policy are `cs_sim::interaction`'s; this is the conversion and
//! binding record the ECS wiring consumes (F36-B/C).

use bevy::app::{App, Plugin, Update};
use bevy::ecs::component::Component;
use bevy::ecs::message::MessageReader;
use bevy::ecs::resource::Resource;
use bevy::ecs::system::ResMut;
use cs_content::interaction::{
    DeclaredCameraTransfer, DeclaredControlOwner, DeclaredEligibility, DeclaredInteraction,
    DeclaredInteractionKind, DeclaredInventoryTransfer, DeclaredPilotTransfer,
    DeclaredVelocityTransfer,
};
use cs_sim::damage::ActorId;
use cs_sim::interaction::{
    CameraTransfer, ControlOwner, EligibilityEnvelope, EnvelopeError, InteractionAbort,
    InteractionAuthorization, InteractionId, InteractionKind, InventoryTransfer, PilotTransfer,
    TransferPolicy, VelocityTransfer,
};
use cs_types::content::{ContentId, Resolved};
use cs_types::evidence::ClaimId;

use crate::damage::DamageActorDestroyed;
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

// ------------------------------------------ the damage → interaction consumer ---
//
// F36-C.1 (task #658). [`DamageActorDestroyed`] is the damage side's
// per-actor destroyed event; this is the ECS system that forwards it to
// [`InteractionRuntime`], the record of what the forward produced, and the
// plugin that installs both into a running app. F36-C found the failure
// this closes: destroying the carrier while a latch held control left the
// latch controller holding a wreck forever, because nothing in the app
// could tell the session the actor was gone.
//
// The forward is idempotent by construction: `InteractionSession` aborts
// only active interactions, so a replayed destruction reaches a session
// whose transaction is already terminal and changes nothing. A world with
// no [`InteractionRuntime`] installed cannot abort anything — that is a
// wiring gap, not a gameplay case — and it is reported in
// [`InteractionConsumerLog`] rather than dropped, the way the damage
// consumer names its own refusals.

/// One update the damage → interaction forward applied, or one refusal.
#[derive(Clone, Debug, PartialEq)]
pub enum InteractionConsumerEvent {
    /// A forwarded destruction aborted one interaction. The abort's
    /// [`InteractionAbort::reason`] is the runtime's own
    /// [`AbortReason::TargetDestroyed`](cs_sim::interaction::AbortReason::TargetDestroyed):
    /// an actor destroyed mid-latch aborts from
    /// [`InteractionState::Latching`](cs_sim::interaction::InteractionState::Latching)
    /// with control resolving back to the surviving initiator.
    Aborted(InteractionAbort),
    /// A forwarded destruction found no [`InteractionRuntime`] installed,
    /// so no interaction could be aborted and any held interaction stayed
    /// live. Reported rather than dropped: a destruction that silently
    /// fails to reach the interaction session is the F36-C failure
    /// returning.
    NoRuntime {
        /// The destroyed actor whose abort could not be forwarded.
        actor: ActorId,
    },
}

/// Resource: the append-only record of what the damage → interaction
/// forward changed or refused, oldest first.
///
/// A log entry is written for a real abort or a refusal, never for a
/// destruction that found no active interaction — that is the normal case,
/// not a change and not a gap.
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub struct InteractionConsumerLog {
    events: Vec<InteractionConsumerEvent>,
}

impl InteractionConsumerLog {
    /// An empty log.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Appends one event.
    pub fn push(&mut self, event: InteractionConsumerEvent) {
        self.events.push(event);
    }

    /// Every event, oldest first.
    #[must_use]
    pub fn events(&self) -> &[InteractionConsumerEvent] {
        &self.events
    }

    /// The most recent event.
    #[must_use]
    pub fn last(&self) -> Option<&InteractionConsumerEvent> {
        self.events.last()
    }

    /// How many events the log holds.
    #[must_use]
    pub fn len(&self) -> usize {
        self.events.len()
    }

    /// Whether the log is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }
}

/// Forwards every [`DamageActorDestroyed`] event to the interaction runtime:
/// each destruction reaches [`InteractionRuntime::on_actor_destroyed`],
/// and every abort it produces lands in [`InteractionConsumerLog`].
///
/// A destruction that finds no active interaction logs nothing — the
/// session simply has nothing to abort — while a world with no
/// [`InteractionRuntime`] logs
/// [`InteractionConsumerEvent::NoRuntime`] instead of dropping the
/// destruction.
pub fn forward_actor_destroyed(
    mut destroyed: MessageReader<DamageActorDestroyed>,
    mut runtime: Option<ResMut<InteractionRuntime>>,
    mut log: ResMut<InteractionConsumerLog>,
) {
    for event in destroyed.read() {
        match runtime.as_deref_mut() {
            Some(runtime) => {
                for abort in runtime.on_actor_destroyed(event.actor) {
                    log.push(InteractionConsumerEvent::Aborted(abort));
                }
            }
            None => log.push(InteractionConsumerEvent::NoRuntime { actor: event.actor }),
        }
    }
}

/// Registers the damage → interaction consumer: the per-actor destroyed
/// event queue, the log, and [`forward_actor_destroyed`] in [`Update`].
///
/// This plugin owns the [`DamageActorDestroyed`] registration so the event
/// has exactly one maintenance pipeline; the damage side publishes through
/// [`publish_destroyed_actors`](crate::damage::publish_destroyed_actors),
/// which only needs the queue this plugin creates.
///
/// The plugin installs no session: a mission that opens an interaction
/// session inserts its own [`InteractionRuntime`]. Everything it installs
/// starts empty, so a session-less world forwards destructions into the
/// log as [`InteractionConsumerEvent::NoRuntime`] rather than losing them
/// or finding a stale session.
#[derive(Clone, Copy, Debug, Default)]
pub struct InteractionSchedulePlugin;

impl Plugin for InteractionSchedulePlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<DamageActorDestroyed>();
        app.init_resource::<InteractionConsumerLog>();
        app.add_systems(Update, forward_actor_destroyed);
    }
}
