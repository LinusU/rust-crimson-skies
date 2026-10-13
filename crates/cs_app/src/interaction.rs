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
//! * [`apply_transfer_consumers`] (F36-C.2) — the one authoritative rebind of
//!   the input, HUD, spyglass, sound, weapons and network-ownership consumer
//!   bindings from an applied [`cs_sim::interaction::TransferReport`], as the
//!   single record [`TransferConsumers`]; a report the live session does not
//!   hold as **Completed** — an aborted, stale-generation or unknown
//!   interaction — is refused and changes nothing.
//! * [`teardown_transfer_consumers`] — the abort/retry/disconnect path that
//!   drops the published bindings for their own session generation and
//!   refuses a late teardown from a previous one.
//! * [`rebind_network_ownership`] — the server-side peer-to-actor ownership
//!   rebind the swap owes [`cs_net::validation::ActorOwnership`], refusing a
//!   peer that does not own the swapping actor and releasing the old aircraft
//!   only after the new one bound.
//!
//! Nothing here owns runtime behavior: the state machine, eligibility test and
//! transfer policy are `cs_sim::interaction`'s; this is the conversion and
//! binding record the ECS wiring consumes (F36-B/C).

use bevy::app::{App, Plugin, Update};
use bevy::ecs::component::Component;
use bevy::ecs::message::MessageReader;
use bevy::ecs::resource::Resource;
use bevy::ecs::system::ResMut;
use bevy::ecs::world::World;
use cs_content::interaction::{
    DeclaredCameraTransfer, DeclaredControlOwner, DeclaredEligibility, DeclaredInteraction,
    DeclaredInteractionKind, DeclaredInventoryTransfer, DeclaredPilotTransfer,
    DeclaredVelocityTransfer,
};
use cs_net::validation::{ActorOwnership, SessionViolation};
use cs_sim::damage::ActorId;
use cs_sim::interaction::{
    CameraTransfer, ControlOwner, EligibilityEnvelope, EnvelopeError, InteractionAbort,
    InteractionAuthorization, InteractionId, InteractionKind, InteractionState, InventoryTransfer,
    PilotTransfer, TransferPolicy, TransferReport, VelocityTransfer,
};
use cs_types::content::{ContentId, Resolved};
use cs_types::evidence::ClaimId;
use cs_types::net::PeerId;

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

// ------------------------------------------------- consumer rebinding (F36-C.2) ----

/// The session generation and interaction whose applied transfer the
/// published consumer bindings describe.
///
/// The binding is the `(session, interaction)` of the [`TransferReport`] the
/// bindings were derived from. A consumer reading a binding without this pair
/// cannot tell which transfer published it, which is why a teardown or a
/// rebind drops the pair rather than leaving a stale one behind
/// (`STATE-TRANSACTIONS`: "Retry restores the authored initial state ... No
/// actor identifier is reused within a session").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ConsumerBinding {
    /// The session generation the transfer committed in.
    pub session: u64,
    /// The interaction whose applied transfer published the bindings.
    pub interaction: InteractionId,
}

/// Resource: the input, HUD, spyglass, sound, weapons and network-ownership
/// bindings of one applied transfer, as one record (F36-C.2).
///
/// Spec: `specs/F36-docking-passenger-pickups-boarding-and-plane-swaps.md`,
/// non-negotiable behavior 5: "Airframe swaps can occur during a mission.
/// Input, HUD, spyglass, sounds, weapons, damage and networking all rebind to
/// the new actor in one authoritative transition." The six views are one
/// record derived from one [`TransferReport`], not six independent writes,
/// because the failure they share is disagreement: an input binding that
/// flies the new aircraft while the HUD still draws the old one's
/// instruments, or a network ownership entry that still lets a peer act for
/// an aircraft nobody flies.
///
/// [`apply_transfer_consumers`] is the only writer. The bindings follow the
/// report's own fields: input, HUD, sound, weapons and network ownership
/// follow [`TransferReport::control_actor`] (the aircraft that owns
/// pose/control — the airframe whose hardpoints and engine loop the weapons
/// and sound consumers present), and the spyglass follows
/// [`TransferReport::camera_actor`] (the camera binding the policy
/// transferred). This mapping is designed project behavior; the original
/// game's per-consumer rebind rules are unrecovered (recorded in
/// `docs/findings/2026-10-05-f36-c-interaction-consumers.md`).
///
/// Nothing is published before a transfer commits, and an aborted transfer
/// publishes nothing: an interaction that is not held as
/// [`InteractionState::Completed`] by the live [`InteractionRuntime`] is
/// refused and changes no binding.
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub struct TransferConsumers {
    bound: Option<ConsumerBinding>,
    input: Option<ActorId>,
    hud: Option<ActorId>,
    spyglass: Option<ActorId>,
    sound: Option<ActorId>,
    weapons: Option<ActorId>,
    network: Option<ActorId>,
}

impl TransferConsumers {
    /// The `(session, interaction)` the published bindings belong to; `None`
    /// when nothing is published.
    #[must_use]
    pub const fn bound(&self) -> Option<ConsumerBinding> {
        self.bound
    }

    /// The actor input flies, when a transfer published one.
    #[must_use]
    pub const fn input(&self) -> Option<ActorId> {
        self.input
    }

    /// The actor whose instruments the HUD presents, when a transfer
    /// published one.
    #[must_use]
    pub const fn hud(&self) -> Option<ActorId> {
        self.hud
    }

    /// The actor the spyglass camera binds to, when a transfer published one.
    #[must_use]
    pub const fn spyglass(&self) -> Option<ActorId> {
        self.spyglass
    }

    /// The actor whose engine loop the sound consumer follows, when a
    /// transfer published one.
    #[must_use]
    pub const fn sound(&self) -> Option<ActorId> {
        self.sound
    }

    /// The actor whose hardpoints the weapons consumer presents, when a
    /// transfer published one.
    #[must_use]
    pub const fn weapons(&self) -> Option<ActorId> {
        self.weapons
    }

    /// The actor network ownership must move to, when a transfer published
    /// one.
    #[must_use]
    pub const fn network(&self) -> Option<ActorId> {
        self.network
    }

    /// Drops every published binding, so no consumer can read a record
    /// belonging to a transfer or session that no longer stands.
    pub fn clear(&mut self) {
        self.bound = None;
        self.input = None;
        self.hud = None;
        self.spyglass = None;
        self.sound = None;
        self.weapons = None;
        self.network = None;
    }
}

/// Why a transfer report could not rebind the consumers. A refused report
/// changes nothing except where its own documentation says the published
/// bindings were dropped.
#[derive(Clone, Debug, PartialEq)]
pub enum TransferConsumerError {
    /// No [`InteractionRuntime`] is installed, so no live session can stand
    /// behind the report. The previously published bindings are dropped:
    /// they describe a session that is gone (the F30-C unbind discipline).
    NoRuntime,
    /// The report belongs to a different session generation than the live
    /// one — a retry replaced the session, so the report's transfer is not
    /// part of the world these consumers fly.
    StaleSession {
        /// The live session generation.
        live: u64,
        /// The generation the report claimed.
        report: u64,
    },
    /// The live session does not hold this interaction as completed: it
    /// aborted, was never opened, or was dropped by a retry. An aborted
    /// transfer never rebinds.
    NotCompleted {
        /// The interaction the report claimed.
        id: InteractionId,
        /// The stage the live session holds it at, if it knows it at all.
        state: Option<InteractionState>,
    },
}

impl std::fmt::Display for TransferConsumerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoRuntime => write!(
                f,
                "no interaction runtime is installed to stand behind the report"
            ),
            Self::StaleSession { live, report } => write!(
                f,
                "the report's transfer belongs to session {report}, the live session is {live}"
            ),
            Self::NotCompleted { id, state } => match state {
                Some(state) => write!(f, "the live session holds {id} at {state}, not completed"),
                None => write!(f, "the live session does not know {id}"),
            },
        }
    }
}

impl std::error::Error for TransferConsumerError {}

/// What one [`apply_transfer_consumers`] pass published.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ConsumerRebindReport {
    /// The `(session, interaction)` the bindings now describe.
    pub binding: ConsumerBinding,
    /// The pass rebound the consumers to a different transfer than the one
    /// previously bound, so the previous transfer's actors were dropped
    /// rather than carried over.
    pub rebound: bool,
}

/// Rebinds input, HUD, spyglass, sound, weapons and network ownership from
/// one applied [`TransferReport`] — the F36-C.2 consumer entry.
///
/// This is the **only** writer of [`TransferConsumers`], and it derives all
/// six bindings from the one report, so the input binding, the HUD's
/// instruments, the spyglass camera, the engine-loop sound, the weapon
/// hardpoints and the network ownership entry can never disagree about which
/// aircraft the pilot now flies (spec F36 non-negotiable behavior 5: "one
/// authoritative transition").
///
/// The report is checked against the live [`InteractionRuntime`] before any
/// binding moves, which is what makes an aborted or stale transfer unable to
/// rebind anything:
///
/// * no installed runtime is [`TransferConsumerError::NoRuntime`], and the
///   previously published bindings are dropped — they described a session
///   nobody can stand behind any more;
/// * a report from another generation is
///   [`TransferConsumerError::StaleSession`] (a retry replaced the session,
///   `STATE-TRANSACTIONS`: "Retry restores the authored initial state");
/// * an interaction the live session does not hold as
///   [`InteractionState::Completed`] — an aborted, unknown or dropped one —
///   is [`TransferConsumerError::NotCompleted`]. The session produces no
///   report for those, so a report claiming one is stale or fabricated, and
///   a fabricated report must not move the player's aircraft bindings.
///
/// Both refusals other than [`TransferConsumerError::NoRuntime`] leave the
/// published bindings untouched: they still describe the last transfer that
/// actually committed, which remains true after an unrelated abort.
///
/// # Errors
///
/// [`TransferConsumerError`] as above; no binding changes on
/// [`TransferConsumerError::StaleSession`] or
/// [`TransferConsumerError::NotCompleted`].
pub fn apply_transfer_consumers(
    world: &mut World,
    report: &TransferReport,
) -> Result<ConsumerRebindReport, TransferConsumerError> {
    // Read the previous binding before the mutable work: a pass for a
    // different transfer rebinds, and the rebind must drop the previous
    // transfer's actors rather than carry them across.
    let rebound = world
        .get_resource::<TransferConsumers>()
        .and_then(TransferConsumers::bound)
        .is_some_and(|bound| bound.interaction != report.id);

    // The live session is the authority the report is checked against. The
    // checks are collected before any write so a refusal cannot leave a
    // half-rebound record.
    let Some(runtime) = world.get_resource::<InteractionRuntime>() else {
        // A bindings record that outlives the runtime it was derived from
        // must not stay readable: unbind, then report.
        match world.get_resource_mut::<TransferConsumers>() {
            Some(mut consumers) => consumers.clear(),
            None => {
                world.insert_resource(TransferConsumers::default());
            }
        }
        return Err(TransferConsumerError::NoRuntime);
    };
    let live = runtime.0.session();
    if report.id.session != live {
        return Err(TransferConsumerError::StaleSession {
            live,
            report: report.id.session,
        });
    }
    let state = runtime.0.state(report.id);
    if state != Some(InteractionState::Completed) {
        return Err(TransferConsumerError::NotCompleted {
            id: report.id,
            state,
        });
    }

    // Commit: nothing below can fail. One record, all six bindings, from the
    // one report.
    let binding = ConsumerBinding {
        session: report.id.session,
        interaction: report.id,
    };
    world.insert_resource(TransferConsumers {
        bound: Some(binding),
        input: Some(report.control_actor),
        hud: Some(report.control_actor),
        spyglass: Some(report.camera_actor),
        sound: Some(report.control_actor),
        weapons: Some(report.control_actor),
        network: Some(report.control_actor),
    });
    Ok(ConsumerRebindReport { binding, rebound })
}

/// Drops the published consumer bindings for `session` — the explicit
/// abort/retry/disconnect path (F36-C.2).
///
/// A caller that knows a generation is over calls it so no consumer reads a
/// binding from a transfer the retry replaced (`STATE-TRANSACTIONS`: "Retry
/// restores the authored initial state, not a mutated copy of the just-
/// failed world"). It refuses to clear bindings published by a **different**
/// session, so a late teardown for the previous generation cannot blank the
/// current one, and reports whether it cleared anything.
///
/// Returns `false` (changing nothing) when nothing is bound at all, so the
/// caller is told a teardown that appeared to do work did not.
pub fn teardown_transfer_consumers(world: &mut World, session: u64) -> bool {
    let Some(mut consumers) = world.get_resource_mut::<TransferConsumers>() else {
        return false;
    };
    if consumers
        .bound()
        .is_some_and(|bound| bound.session != session)
    {
        return false;
    }
    if consumers.bound().is_none() {
        return false;
    }
    consumers.clear();
    true
}

/// Why a network-ownership rebind was refused. A refused rebind changes
/// nothing: the peer keeps the aircraft it owned.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NetworkRebindError {
    /// The peer does not own the actor the transfer moved control away from,
    /// so it is not the pilot this swap rebinds.
    PeerDoesNotOwnInitiator {
        /// The peer that asked for the rebind.
        peer: PeerId,
        /// The actor the report moved control away from.
        actor: ActorId,
    },
    /// Another peer already owns the destination aircraft: "A peer never
    /// takes an aircraft from another pilot" (`cs_net` ownership table).
    DestinationOwned {
        /// The peer that asked for the rebind.
        peer: PeerId,
        /// The destination aircraft.
        actor: ActorId,
        /// The peer that owns it.
        owner: PeerId,
    },
}

impl std::fmt::Display for NetworkRebindError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PeerDoesNotOwnInitiator { peer, actor } => write!(
                f,
                "{peer} does not own the aircraft {actor} it is swapping from"
            ),
            Self::DestinationOwned { peer, actor, owner } => {
                write!(f, "{peer} cannot take {actor}: {owner} already owns it")
            }
        }
    }
}

impl std::error::Error for NetworkRebindError {}

/// What one [`rebind_network_ownership`] did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NetworkRebind {
    /// The actor the peer now owns.
    pub bound: ActorId,
    /// The aircraft the peer released, when it owned a different one.
    pub released: Option<ActorId>,
}

/// Moves the server's peer-to-actor ownership entry along an applied
/// [`TransferReport`] (F36-C.2).
///
/// The swap moves the pilot from the report's initiator to its
/// [`TransferReport::control_actor`], and the ownership table
/// ([`ActorOwnership`], `docs/contracts/UI-NETWORK.md`) must follow so the
/// peer's intents are authorized for the aircraft it now flies and refused
/// for the one it left.
///
/// The order is deliberate: the new binding is established **first** and the
/// old aircraft is released only after it bound, so a refusal at any point
/// leaves the peer owning exactly what it owned before — never nothing, and
/// never both. When control did not move (`control_actor` is the initiator)
/// there is nothing to rebind and the current binding is reported unchanged.
///
/// # Errors
///
/// [`NetworkRebindError::PeerDoesNotOwnInitiator`] when the peer is not the
/// pilot of the aircraft the report moved control away from, and
/// [`NetworkRebindError::DestinationOwned`] when another peer already owns
/// the destination. The table is unchanged in both cases.
pub fn rebind_network_ownership(
    ownership: &mut ActorOwnership,
    peer: PeerId,
    report: &TransferReport,
) -> Result<NetworkRebind, NetworkRebindError> {
    let from = report.id.initiator;
    let to = report.control_actor;
    if to == from {
        // Control did not move: the peer's binding already describes the
        // aircraft it still flies.
        return Ok(NetworkRebind {
            bound: to,
            released: None,
        });
    }
    if ownership.actor_of(peer) != Some(from) {
        return Err(NetworkRebindError::PeerDoesNotOwnInitiator { peer, actor: from });
    }
    // Bind the destination first: `bind` refuses exactly when another pilot
    // already owns it ("A peer never takes an aircraft from another pilot"),
    // and that refusal is reported against the table's own answer. The old
    // aircraft is released only after the new one bound, so a refusal left
    // the peer with its original binding.
    if let Err(violation) = ownership.bind(peer, to) {
        let owner = match violation {
            SessionViolation::ActorNotOwned {
                owner: Some(owner), ..
            } => owner,
            // `bind` documents exactly one refusal and carries the blocking
            // owner; any other answer is reported against the asking peer
            // rather than unwrapped.
            _ => peer,
        };
        return Err(NetworkRebindError::DestinationOwned {
            peer,
            actor: to,
            owner,
        });
    }
    ownership.release(from);
    Ok(NetworkRebind {
        bound: to,
        released: Some(from),
    })
}
