//! Trigger crossings: the gameplay stream a swept crossing decision is
//! delivered into — the spawn preflight's sensor record (task #415) and the
//! ordinary-flight pass over the world's sensor volumes (task #498).
//!
//! Spec: `specs/F39-objectives-triggers-timers-spawn-groups-and-dialogue-cues.md`
//! (F39 owns trigger semantics), stage `### F39-A` for the crossing
//! vocabulary this reuses, and
//! `specs/F23-avian-integration-collision-and-fixed-step-authority.md` for the
//! producer. Shared contracts: `docs/contracts/FLIGHT-PHYSICS.md`
//! ("Collision and ballistic tests": *"For interaction triggers use a swept
//! center/shape appropriate to the original rule"*) and
//! `docs/contracts/SCRIPT-MISSION.md` ("Objective event ordering").
//!
//! # The record, and the decision this module makes about it
//!
//! F23-D closed the **record** half of a measured engine hole and deliberately
//! left the **rule** half open. A body spawned inside one tick of travel of an
//! obstacle is not in the broad phase for its first tick (F23-B limitation 1),
//! and F23-C's criterion `accept_f23_c_preflight_never_stops_on_a_sensor`
//! forbids a sensor from stopping or delaying that spawn — so a fast body
//! leaves a thin trigger *inside* the very tick it is invisible to collision
//! detection, and the engine reports **no** overlap for it. Measured at
//! 120 Hz: a projectile spawned 0.4 ticks of travel short of a 2 cm trigger
//! crosses it and leaves at its fired speed with zero contact reports, at every
//! probed speed and rate.
//!
//! The second, non-blocking cast in
//! [`SpawnPreflightEvent::passed`](crate::physics::SpawnPreflightEvent::passed)
//! records that crossing, with the distance at which the body's own collider met
//! the volume, whether or not a solid obstacle stopped the same tick. F23-D
//! wrote down that *"whether gameplay consumes that field as a trigger crossing
//! is a rule F23-C's criterion does not decide"*. This module is that decision:
//!
//! > **The spawn-tick crossing is delivered as a gameplay crossing.** It is not
//! > a diagnostic. A trigger crossed inside the spawn tick reaches the
//! > authoritative event stream on the tick it happened, exactly once per
//! > `(actor, volume)` pair, and nothing about the spawn changes: the body
//! > keeps its velocity and a trigger still never stops or delays anything
//! > (F23-C's criterion is not weakened, and
//! > `accept_t415_a_the_delivery_does_not_move_or_delay_the_body` measures it).
//!
//! The alternative — declaring the field diagnostic-only — was rejected on the
//! affected content rather than on taste: the record's own consumers are
//! mission triggers, and a mission volume a body can cross and never report is
//! a mission that cannot complete. The full decision, the measurement behind it
//! and the boundaries of the claim are in
//! `docs/findings/2026-10-02-t415-spawn-tick-trigger-crossing.md`.
//!
//! # The crossing decision, made once
//!
//! The decision is not "the preflight said so". It is the rule the
//! ordinary-flight path of task #498 implements
//! ([`crate::world::sweep_volume_crossings`]), stated so the two cannot drift:
//!
//! * a crossing is decided from the **body's own motion over the tick** — a
//!   segment swept against the volume, or the engine's own time of impact
//!   along that segment — never from a sampled overlap, which is precisely
//!   what a body faster than the volume is thick cannot produce;
//! * the segment is the one the body would have swept, so the decision does not
//!   depend on the tick rate: a body that covers the volume in one tick at
//!   120 Hz covers it in one tick at 60 Hz, and the crossing is reported in
//!   both;
//! * each `(actor, volume)` pair enters the stream **once**: a body that stays
//!   inside a volume, or a record a multi-tick frame re-reads, produces no
//!   second entry. `duplicates` counts the ones that were refused rather than
//!   letting a second entry through;
//! * delivery is a *read*. `deliver_spawn_tick_crossings` takes the preflight
//!   log and this module's own resource and nothing else — no `Query`, no
//!   `Commands`, no mutable access to a pose or a velocity — so it cannot stop
//!   or delay a body even by accident, and a trigger is never an obstacle.
//!
//! # What this module does not do
//!
//! * **It does not name content.** A crossing names the volume's `Entity`; the
//!   `cs_script::ir::SymbolId` a mission program uses, and the
//!   `cs_sim::objectives::SweptTrigger` that pairs a symbol with an `ActorId`
//!   and a `Volume`, are F39-B's content binding (already named as deferred in
//!   `docs/findings/2026-10-01-f39-a-objective-trigger-spawn-semantics.md`).
//!   Inventing a symbol for a collider would be a guess about content, and a
//!   guessed content id in a mission's trigger table is worse than a missing
//!   one.
//! * **It does not apply an effect.** F18-C's
//!   [`OverlayTriggerRequests`](crate::world::OverlayTriggerRequests) is where
//!   a crossed volume opens a door; that hand-off is fed by the engine's
//!   `CollisionStart` stream, which reports nothing for a spawn-tick crossing,
//!   and the world-authored trigger volumes the mission would bind are not yet
//!   visible to the preflight's cast (they carry no
//!   [`BodyLayer`](crate::physics::BodyLayer)). Both gaps are composition
//!   defects in F18's and F23-C's paths, recorded in the finding and filed
//!   separately; wiring this channel into the overlay hand-off before they are
//!   closed would be an unreachable branch. The ordinary-flight producer of
//!   task #498 feeds the same hand-off for the world-authored volumes it can
//!   see ([`crate::world::sweep_volume_crossings`]); this producer still does
//!   not, for the same reason.
//! * **It claims nothing about the original game.** Whether the original
//!   reported a spawn-tick crossing at all, and with what delay, is **unknown**
//!   and is left to the calibration stage (F26). What is measured here is this
//!   project on the pinned pair (`bevy 0.19.1` / `avian3d 0.7.0`).
//!
//! # F39-C: the mission-program wiring
//!
//! The second half of this module is the F39-C integration stage
//! (`specs/F39-objectives-triggers-timers-spawn-groups-and-dialogue-cues.md`,
//! stage `### F39-C`; shared contract `docs/contracts/SCRIPT-MISSION.md`):
//!
//! * [`lower_program`] is the conversion boundary: a validated
//!   [`cs_content::objectives::DeclaredObjectiveProgram`] becomes the
//!   [`LoweredObjectives`] a session launches — every runtime declaration
//!   (`ObjectiveSpec`, `CountCondition`, `MissionTimer`, `SweptTrigger`) built
//!   field-wise from the declared record, every `Resolved::Unknown` refused by
//!   name, a non-gameplay timer domain refused by name, and the spawn-group
//!   symbols bound to the subject content a wave instantiates.
//! * [`ObjectiveSession`] is the wired producer→runtime→consumer path of one
//!   session. `step` hands the runtime one [`TickInput`] — the whole producer
//!   surface, filled by the mission host with the tick's lifecycle
//!   transitions, real movement segments, signals and declared requests — and
//!   dispatches the ordered `ObjectiveEvent` stream to the consumers:
//!   [`ObjectiveDisplay`] (the objective rows the UI shows), the pending
//!   [`EmittedCue`] queue the dialogue consumer drains exactly once, the
//!   [`SpawnDirective`]s a world instantiates (each naming its bound subject),
//!   the settled outcome, and a [`SessionRefusal`] record naming every refusal
//!   the stream reported, so error propagation is a queryable fact rather than
//!   a line buried in the trace.
//! * [`ObjectiveSession::retry`] is teardown/retry: it reports what the old
//!   session still owns (live spawned actors to despawn, undrained cues, armed
//!   deadlines, a settled outcome) and rebuilds a fresh runtime for the new
//!   [`SessionGeneration`] — which must differ from the live one — from the
//!   same lowered program, so no old timer, actor, counter, ledger key or cue
//!   survives (F39 AC03).
//!
//! The `TickInput` the host hands `step` is a trusted producer surface: a
//! signal it raises must name a mission signal — never the reserved
//! actor-event source or a symbol the program declares — or its
//! `SignalRaised` event would alias that declaration's own. The declared
//! schema refuses the colliding names it can see; what the host injects is
//! its contract to keep.
//!
//! Mid-mission save is **declared unsupported** for this runtime: a snapshot of
//! `ObjectiveRuntime` would need its own format, and the contract permits
//! declaring that unsupported rather than inventing one. Retry is the session
//! lifecycle this stage wires.
//!
//! # F39-E5: completion effects cross the boundary
//!
//! The third piece of this module is the lowering of the declared completion
//! effects ([`lower_effect`]): a `DeclaredObjective`'s
//! [`completion_effects`](cs_content::objectives::DeclaredObjective::completion_effects)
//! become the runtime's [`CompletionEffect`]s on the lowered [`ObjectiveSpec`],
//! field-wise like every other declared field. Two boundaries worth naming:
//!
//! * **The nap's number crosses as a number, never as a duration.** It becomes a
//!   [`UnmeasuredNumber`] and nothing downstream reads it as a time: the runtime
//!   applies the same move whatever it says. Carrying it rather than dropping it
//!   keeps the record's number available to a later measurement.
//! * **The one refused shape never reaches here.** A program whose two different
//!   effects name the same objective is refused by
//!   [`DeclaredObjectiveProgram::try_new`](cs_content::objectives::DeclaredObjectiveProgram::try_new)
//!   before a session exists, and the runtime refuses the same shape again at
//!   registration — no order is applied anywhere, because nothing measured which
//!   effect wins (F39-E2).
//!
//! The consumer side needs nothing new: an effect is an ordinary declared state
//! change, so the objective display moves from the same
//! [`ObjectiveEventKind::ObjectiveChanged`] event as every other state change
//! and no game state is read from the UI.

use std::collections::{BTreeMap, BTreeSet, HashSet, VecDeque};
use std::fmt;
use std::path::Path;

use avian3d::prelude::PhysicsSystems;
use bevy::{
    ecs::schedule::IntoScheduleConfigs,
    prelude::{App, Entity, FixedPostUpdate, Plugin, Res, ResMut, Resource},
};
use cs_content::objectives::{
    BRANCH_EFFECT_KEY_VOCABULARY, BRANCH_KEY_VOCABULARY, BRANCH_ORDER_KEY, BranchEffectKind,
    DeclaredCompletion, DeclaredCompletionEffect, DeclaredCountKind, DeclaredCountReaction,
    DeclaredObjectiveProgram, DeclaredObjectiveState, DeclaredPrecedence, DeclaredRevealRule,
    DeclaredTerminalOutcome, DeclaredTimeDomain, DeclaredTimer, DeclaredTimerAction,
    DeclaredTimerStart, DeclaredVolume, FAILURE_KEY_VOCABULARY, MeasuredBranchConflict,
    MeasuredBranchPrecedence, MeasuredBranchSite, ProgramActor, ProgramSymbol, UnmeasuredQuantity,
    is_optional_objective_key,
};
use cs_content::stunts::{
    OBJECTIVE_BLOCK_PREFIX, SCENARIO_OBJECTIVES_MEMBER, ZrdValue, objective_record, zrd_flat_fields,
};
use cs_script::ir::{ActorId, SymbolId};
use cs_script::runtime::SessionGeneration;
use cs_sim::objectives::counters::CountKind;
use cs_sim::objectives::runtime::{
    CompletionEffect, CompletionEffectKind, CountCondition, CountReaction, ObjectiveCompletion,
    ObjectiveEventKind, ObjectiveRuntime, ObjectiveSpec, ObjectiveTick, RevealRule, RuntimeError,
    RuntimeLimits, StopReason, TickInput, UnmeasuredNumber,
};
use cs_sim::objectives::spawn::IdempotencyKey;
use cs_sim::objectives::state::ObjectiveState;
use cs_sim::objectives::terminal::{TerminalOutcome, TerminalPrecedence};
use cs_sim::objectives::timer::{MissionTimer, TimerAction, TimerError, TimerStart};
use cs_sim::objectives::trigger::{CrossingKind, SweptTrigger, TriggerError, Volume};
use cs_sim::time::ClockPolicy;
use cs_types::Tick;
use cs_types::content::{ContentId, Resolved};
use cs_types::evidence::ClaimId;
use cs_types::install::RelativePath;

use crate::physics::{PhysicsTickLedger, SpawnPreflightLog};

/// Which producer decided a crossing.
///
/// A crossing carries its source so a consumer can tell a *swept* decision
/// (either of these) from a sampled overlap, and so a crossing from one
/// producer is never indistinguishable from the other's.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CrossingSource {
    /// The swept spawn preflight's non-blocking sensor cast
    /// ([`SpawnPreflightEvent::passed`](crate::physics::SpawnPreflightEvent::passed)):
    /// the crossing a body made inside the tick it was spawned in and is
    /// therefore invisible to collision detection.
    SpawnTickPreflight,
    /// The ordinary-flight swept crossing pass (task #498,
    /// [`crate::world::sweep_volume_crossings`]): the crossing a body made in
    /// ordinary flight, decided from the tick's own segment swept against the
    /// world's sensor volumes — the sampled-overlap hole task #401 measured.
    OrdinaryFlightSweep,
}

impl std::fmt::Display for CrossingSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SpawnTickPreflight => f.write_str("spawn_tick_preflight"),
            Self::OrdinaryFlightSweep => f.write_str("ordinary_flight_sweep"),
        }
    }
}

/// One trigger crossing, delivered to gameplay.
///
/// The `kind` is the crossing's own direction; the pair enters the stream once
/// (see [`TriggerCrossings::record`]), so a consumer may treat an entry as an
/// event and never as a level.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TriggerCrossing {
    /// The swept body that crossed the volume.
    pub actor: Entity,
    /// The sensor volume it crossed.
    pub volume: Entity,
    /// The fixed tick the crossing was decided on, from the adapter's ledger.
    pub tick: u64,
    /// Whether this is the entry or the exit.
    pub kind: CrossingKind,
    /// Which producer decided it.
    pub source: CrossingSource,
    /// How far along the tick's own travel the body's collider met the volume,
    /// in meters, measured from the segment's start. For a spawn-tick crossing
    /// that start is the position it spawned at — the same reference the
    /// clamp's distance uses, so a consumer can order a crossing against a
    /// solid stop on the same tick; for an ordinary-flight crossing it is the
    /// pose the body ended the previous tick at.
    pub distance_m: f32,
}

impl TriggerCrossing {
    /// Whether this is an entry.
    #[must_use]
    pub const fn is_entry(&self) -> bool {
        matches!(self.kind, CrossingKind::Entry)
    }

    /// Whether this is an exit.
    #[must_use]
    pub const fn is_exit(&self) -> bool {
        matches!(self.kind, CrossingKind::Exit)
    }

    /// Whether `actor` is the body that crossed.
    #[must_use]
    pub fn is_by(&self, actor: Entity) -> bool {
        self.actor == actor
    }

    /// Whether `volume` is the volume that was crossed.
    #[must_use]
    pub fn is_of(&self, volume: Entity) -> bool {
        self.volume == volume
    }
}

/// The crossings delivered to gameplay, in the order they were decided.
///
/// A drain-what-you-read buffer in the shape of [`SpawnPreflightLog`] and
/// [`ContactReports`](crate::physics::ContactReports): a crossing is appended
/// once and a consumer takes the whole batch, so a crossing can never fire
/// twice or be silently dropped. It reads the preflight log **without**
/// draining it, so the session's own per-frame hand-off of the same records
/// ([`SessionFrame::spawn_events`](crate::physics::SessionFrame::spawn_events))
/// is unaffected.
#[derive(Resource, Debug, Default)]
pub struct TriggerCrossings {
    crossings: Vec<TriggerCrossing>,
    delivered: u64,
    duplicates: u64,
    /// The `(actor, volume)` pairs already in the stream.
    pairs: HashSet<[Entity; 2]>,
}

impl TriggerCrossings {
    /// The undelivered-to-consumer crossings, in decision order.
    #[must_use]
    pub fn crossings(&self) -> &[TriggerCrossing] {
        &self.crossings
    }

    /// The crossings of one actor, in decision order.
    #[must_use]
    pub fn crossings_by(&self, actor: Entity) -> Vec<&TriggerCrossing> {
        self.crossings
            .iter()
            .filter(|crossing| crossing.is_by(actor))
            .collect()
    }

    /// Crossings delivered since the resource was built or cleared.
    #[must_use]
    pub fn delivered(&self) -> u64 {
        self.delivered
    }

    /// Records refused because the pair was already in the stream.
    ///
    /// A record is re-read on every tick between the one that produced it and
    /// the one that drains it — a multi-tick render frame reads the same
    /// preflight record on each of its ticks — so this counter is what makes
    /// "exactly once" a property of the consumer rather than of the frame
    /// length.
    #[must_use]
    pub fn duplicates(&self) -> u64 {
        self.duplicates
    }

    /// Takes every recorded crossing, leaving the counters and the ledger.
    pub fn take(&mut self) -> Vec<TriggerCrossing> {
        core::mem::take(&mut self.crossings)
    }

    /// Drops the crossings and the counters, keeping the pair ledger.
    ///
    /// A consumer that has taken its batch does not want the same crossing
    /// again, so taking and clearing are different acts: only this one is for a
    /// world that is being reset.
    pub fn clear(&mut self) {
        self.crossings.clear();
        self.delivered = 0;
        self.duplicates = 0;
    }

    /// Drops the crossings, the counters and the pair ledger.
    ///
    /// The ledger is per world, and a body can preflight only once in a
    /// world's life, so this belongs to a teardown rather than to a consumer
    /// pass: keeping it across a reset would refuse every crossing of an entity
    /// id the new world reused.
    pub fn reset(&mut self) {
        self.clear();
        self.pairs.clear();
    }

    /// The one place a crossing enters the stream.
    ///
    /// This is the **producer** seam, not a consumer's: a crossing is decided
    /// by the producer that measured the swept segment, and gameplay reads the
    /// stream with [`take`](Self::take). It is public because a second producer
    /// — the ordinary-flight path of task #498
    /// ([`crate::world::sweep_volume_crossings`]) — records into the same
    /// per-pair ledger, or the two would each fire their own duplicate for a
    /// pair the other already reported.
    ///
    /// Returns whether it was delivered. A `(actor, volume)` pair that is
    /// already in the stream is a duplicate: it is counted and refused, so a
    /// crossing fires exactly once however many times its record is read.
    pub fn record(&mut self, crossing: TriggerCrossing) -> bool {
        if !self.pairs.insert([crossing.actor, crossing.volume]) {
            self.duplicates += 1;
            return false;
        }
        self.delivered += 1;
        self.crossings.push(crossing);
        true
    }
}

/// Turns the swept spawn preflight's sensor record into gameplay crossings.
///
/// Runs in `FixedPostUpdate` after `PhysicsSystems::StepSimulation`, which is
/// after the preflight itself (it is scheduled before
/// `PhysicsSystems::Prepare`), so a record is delivered on the tick that
/// produced it — a projectile that crossed a trigger and was then stopped by a
/// wall in the same tick gets both facts on that tick, in the tick's order.
///
/// It is a pure read of two resources. That is the whole non-blocking claim:
/// the system holds no `Query`, issues no `Commands` and takes no mutable
/// access to a pose or a velocity, so a delivery cannot stop a body, delay it
/// or move it. A trigger never stops or delays anything (F23-C), and the
/// preflight's own second cast is non-blocking for the same reason.
///
/// A record that names a sensor without the distance the preflight measured it
/// at is refused rather than turned into a crossing with an invented geometry.
/// The two fields come from one `Option` in the producer, so a record with
/// only one of them is a producer defect, not a gameplay case.
fn deliver_spawn_tick_crossings(
    preflight: Res<SpawnPreflightLog>,
    ledger: Option<Res<PhysicsTickLedger>>,
    mut crossings: ResMut<TriggerCrossings>,
) {
    let tick = ledger.map_or(0, |ledger| ledger.ticks);
    for event in preflight.events() {
        let (Some(volume), Some(distance_m)) = (event.passed, event.passed_distance_m) else {
            continue;
        };
        crossings.record(TriggerCrossing {
            actor: event.body,
            volume,
            tick,
            // A body that was outside its volume and reached it inside one tick
            // entered it. A brand-new body's state is outside by construction:
            // the preflight cast is the decision, and there is no prior
            // observation for a turn-away or a return to suppress.
            kind: CrossingKind::Entry,
            source: CrossingSource::SpawnTickPreflight,
            distance_m,
        });
    }
}

/// Installs the spawn-tick trigger-crossing consumer.
///
/// Add it to the world that runs the preflight — the production
/// [`PhysicsSession`](crate::physics::PhysicsSession), whose
/// [`configure`](crate::physics::PhysicsSessionBuilder::configure) seam is the
/// place an app composes one. Without it the preflight still records the
/// crossing and gameplay still sees nothing, which is the gap this task closed.
///
/// It **requires** the preflight: the delivery reads
/// [`SpawnPreflightLog`], which
/// [`PhysicsBodiesPlugin`](crate::physics::PhysicsBodiesPlugin) owns. A world
/// that installs this plugin without one panics on its first fixed tick rather
/// than delivering nothing, which is the failure a composition mistake should
/// make. `world_app()` is such a world today — the finding records that gap and
/// the task that owns it.
///
/// The plugin needs [`PhysicsTickLedger`] to stamp a crossing with its tick and
/// uses `0` when it is absent, like the contact reporter does: an undated
/// crossing is still a crossing, and a missing ledger is not a reason to drop
/// one.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SpawnTickTriggerPlugin;

impl Plugin for SpawnTickTriggerPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<TriggerCrossings>().add_systems(
            FixedPostUpdate,
            deliver_spawn_tick_crossings.after(PhysicsSystems::StepSimulation),
        );
    }
}

// ---------------------------------------------------------------------------
// F39-C: the mission-program wiring
// ---------------------------------------------------------------------------
//
// The three pieces below are the whole stage:
//
// 1. [`lower_program`] — the declared→lowered boundary. A
//    `cs_content::objectives::DeclaredObjectiveProgram` (its own vocabulary:
//    `ProgramSymbol`, `ProgramActor`, declared enums) becomes the
//    [`LoweredObjectives`] a session launches. `cs_content` cannot depend on
//    `cs_sim`/`cs_script`, so the boundary maps field-wise: `ProgramSymbol(u)`
//    → `SymbolId(u)`, `ProgramActor(u)` → `ActorId(u)`, declared variants →
//    runtime variants. What lowering refuses is named:
//    [`ProgramLowerError::UnknownPrecedence`], [`ProgramLowerError::NonGameplayDomain`]
//    and the constructor failures of `CountCondition::new`, `MissionTimer::new`
//    and `SweptTrigger::new` — each reported with the declared symbol.
// 2. [`ObjectiveSession`] — the one place a session owns the runtime and every
//    per-session consumer state: the objective display the UI reads, the
//    pending cue queue the dialogue consumer drains once, the live wave
//    registry a teardown despawns, and the outcome once settled.
// 3. [`ObjectiveSession::retry`] — the teardown/retry contract the acceptance
//    case is about. It answers "what did the old session still own" as data
//    ([`TeardownReport`]) *before* rebuilding, so the world despawns the old
//    wave's actors — which the fresh session's instance counter will reuse —
//    instead of meeting them twice.

/// A spawn group's lowered binding: what one instance of a wave is built
/// from. The group symbol is the program's identity; the subject is the
/// content the world instantiates for each admitted instance id.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoweredSpawnGroup {
    /// The group's program symbol.
    pub group: SymbolId,
    /// The content one instance is built from.
    pub subject: ContentId,
}

/// What [`lower_program`] produces: the runtime declarations plus the
/// spawn-group content bindings, ready for [`ObjectiveSession::launch`].
///
/// The lists keep authored order, which is also registration order; identity
/// is always the symbol, never the position.
#[derive(Clone, Debug, PartialEq)]
pub struct LoweredObjectives {
    /// The declared terminal precedence.
    pub precedence: TerminalPrecedence,
    /// The bounds the session's runtime runs under. `RuntimeLimits::default()`
    /// unless overridden through [`LoweredObjectives::with_limits`].
    pub limits: RuntimeLimits,
    /// The lowered objectives, in authored order.
    pub objectives: Vec<ObjectiveSpec>,
    /// The lowered count conditions with their reactions, in authored order.
    pub conditions: Vec<(CountCondition, CountReaction)>,
    /// The lowered timers, in authored order.
    pub timers: Vec<MissionTimer>,
    /// The lowered swept triggers, in authored order.
    pub triggers: Vec<SweptTrigger>,
    /// The lowered spawn groups, keyed by group symbol.
    pub spawn_groups: BTreeMap<SymbolId, LoweredSpawnGroup>,
}

impl LoweredObjectives {
    /// Overrides the runtime bounds; [`RuntimeLimits::default`] otherwise.
    #[must_use]
    pub fn with_limits(mut self, limits: RuntimeLimits) -> Self {
        self.limits = limits;
        self
    }
}

/// Why a declared objective program could not be lowered.
#[derive(Clone, Debug, PartialEq)]
pub enum ProgramLowerError {
    /// The record's [`DeclaredSupport`](cs_content::objectives::DeclaredSupport)
    /// says its objective semantics are not recovered, so a designed
    /// progression must not be run under this mission's id
    /// (`docs/contracts/SCRIPT-MISSION.md`: *"If the actual program is
    /// unavailable or cannot be decoded, the mission remains Unsupported."*).
    ///
    /// This is the gate, not a warning. A mission whose branching, optional and
    /// failure conditions were not recovered has no objectives a session may
    /// complete, so there is nothing to launch.
    UnsupportedProgram {
        /// The mission the record belongs to.
        subject: ContentId,
        /// Why the semantics are not recovered, as the record states it.
        reason: String,
    },
    /// The declared precedence is `Resolved::Unknown`: the original rule is
    /// unmeasured and a session must not pick one in the record's place.
    UnknownPrecedence {
        /// The claim the unknown is recorded under.
        claim_id: ClaimId,
        /// Why the precedence is unknown.
        reason: String,
    },
    /// A timer declared a domain that is not gameplay (`UiWall` or
    /// `MediaUnscaled`): a menu frame or a cutscene would advance a mission
    /// deadline, which is not a mission timer.
    NonGameplayDomain {
        /// The declared timer.
        timer: ProgramSymbol,
        /// The domain it declared.
        domain: DeclaredTimeDomain,
    },
    /// `MissionTimer::new` refused the lowered declaration.
    Timer {
        /// The declared timer.
        timer: ProgramSymbol,
        /// The constructor's refusal.
        error: TimerError,
    },
    /// `SweptTrigger::new` refused the lowered declaration.
    Trigger {
        /// The declared trigger.
        trigger: ProgramSymbol,
        /// The constructor's refusal.
        error: TriggerError,
    },
    /// `CountCondition::new` refused the lowered declaration.
    Condition {
        /// The declared condition.
        condition: ProgramSymbol,
        /// The constructor's refusal.
        error: RuntimeError,
    },
}

impl std::fmt::Display for ProgramLowerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnsupportedProgram { subject, reason } => {
                write!(f, "mission {subject} has no recovered objectives: {reason}")
            }
            Self::UnknownPrecedence { reason, .. } => {
                write!(f, "the terminal precedence is unknown: {reason}")
            }
            Self::NonGameplayDomain { timer, domain } => write!(
                f,
                "timer {timer} declares a non-gameplay domain ({domain:?})"
            ),
            Self::Timer { timer, error } => {
                write!(f, "timer {timer} cannot be declared: {error}")
            }
            Self::Trigger { trigger, error } => {
                write!(f, "trigger {trigger} cannot be declared: {error}")
            }
            Self::Condition { condition, error } => {
                write!(f, "count condition {condition} cannot be declared: {error}")
            }
        }
    }
}

impl std::error::Error for ProgramLowerError {}

const fn lower_symbol(symbol: ProgramSymbol) -> SymbolId {
    SymbolId(symbol.0)
}

const fn lower_actor(actor: ProgramActor) -> ActorId {
    ActorId(actor.0)
}

const fn lower_state(state: DeclaredObjectiveState) -> ObjectiveState {
    match state {
        DeclaredObjectiveState::Hidden => ObjectiveState::Hidden,
        DeclaredObjectiveState::Pending => ObjectiveState::Pending,
        DeclaredObjectiveState::Active => ObjectiveState::Active,
        DeclaredObjectiveState::Succeeded => ObjectiveState::Succeeded,
        DeclaredObjectiveState::Failed => ObjectiveState::Failed,
        DeclaredObjectiveState::Optional => ObjectiveState::Optional,
        DeclaredObjectiveState::Superseded => ObjectiveState::Superseded,
    }
}

const fn lower_outcome(outcome: DeclaredTerminalOutcome) -> TerminalOutcome {
    match outcome {
        DeclaredTerminalOutcome::Success => TerminalOutcome::Success,
        DeclaredTerminalOutcome::Extraction => TerminalOutcome::Extraction,
        DeclaredTerminalOutcome::Failure => TerminalOutcome::Failure,
    }
}

const fn lower_precedence(precedence: DeclaredPrecedence) -> TerminalPrecedence {
    match precedence {
        DeclaredPrecedence::SyntheticConservative => TerminalPrecedence::SyntheticConservative,
    }
}

const fn lower_reveal(reveal: DeclaredRevealRule) -> RevealRule {
    match reveal {
        DeclaredRevealRule::Immediate => RevealRule::Immediate,
        DeclaredRevealRule::OnCondition(condition) => RevealRule::OnCondition {
            condition: lower_symbol(condition),
        },
        DeclaredRevealRule::OnTimer(timer) => RevealRule::OnTimer {
            timer: lower_symbol(timer),
        },
        DeclaredRevealRule::OnSignal(signal) => RevealRule::OnSignal {
            signal: lower_symbol(signal),
        },
        DeclaredRevealRule::OnObjectiveState { objective, state } => RevealRule::OnObjectiveState {
            objective: lower_symbol(objective),
            state: lower_state(state),
        },
    }
}

const fn lower_completion(completion: DeclaredCompletion) -> ObjectiveCompletion {
    match completion {
        DeclaredCompletion::Continue => ObjectiveCompletion::Continue,
        DeclaredCompletion::Requests(outcome) => {
            ObjectiveCompletion::Requests(lower_outcome(outcome))
        }
    }
}

const fn lower_effect_kind(kind: BranchEffectKind) -> CompletionEffectKind {
    match kind {
        BranchEffectKind::Wake => CompletionEffectKind::Wake,
        BranchEffectKind::Nap => CompletionEffectKind::Nap,
        BranchEffectKind::Kill => CompletionEffectKind::Kill,
        BranchEffectKind::Wakeup => CompletionEffectKind::Wakeup,
    }
}

/// Lowers one declared completion effect field-wise.
///
/// The nap's number crosses the boundary as a
/// [`UnmeasuredNumber`] — **not** as a duration, a weight or a threshold. Nothing
/// here divides by it, compares it or stores it in a clock, and the runtime's own
/// [`CompletionEffectKind::moves_to`] never looks at it: a nap moves its target
/// whatever the number says, because what the number measures is unmeasured.
/// Carrying it rather than dropping it keeps the record's number visible to a
/// later measurement instead of losing it at the boundary.
///
/// The declared shape (a nap carries a number, nothing else does) is already
/// enforced by the schema, and `CompletionEffect`'s own constructor re-checks it
/// so a struct literal cannot skip the rule.
fn lower_effect(effect: &DeclaredCompletionEffect) -> CompletionEffect {
    CompletionEffect {
        kind: lower_effect_kind(effect.kind),
        target: lower_symbol(effect.objective),
        // A declared quantity is finite by construction, so this is the identity
        // in practice; if a future declared form ever carries a non-finite
        // number the mapping drops it here and the runtime's own shape check
        // refuses the declaration by name instead of running it with a number it
        // could not read.
        argument: effect
            .argument
            .map(UnmeasuredQuantity::value)
            .and_then(UnmeasuredNumber::new),
    }
}

const fn lower_kind(kind: DeclaredCountKind) -> CountKind {
    match kind {
        DeclaredCountKind::Destroyed => CountKind::Destroyed,
        DeclaredCountKind::Disabled => CountKind::Disabled,
        DeclaredCountKind::Captured => CountKind::Captured,
        DeclaredCountKind::Escaped => CountKind::Escaped,
        DeclaredCountKind::Despawned => CountKind::Despawned,
    }
}

const fn lower_reaction(reaction: DeclaredCountReaction) -> CountReaction {
    match reaction {
        DeclaredCountReaction::ReportOnly => CountReaction::ReportOnly,
        DeclaredCountReaction::SetObjectiveState { objective, state } => {
            CountReaction::SetObjectiveState {
                objective: lower_symbol(objective),
                state: lower_state(state),
            }
        }
        DeclaredCountReaction::Finish(outcome) => CountReaction::Finish(lower_outcome(outcome)),
    }
}

const fn lower_start(start: DeclaredTimerStart) -> TimerStart {
    match start {
        DeclaredTimerStart::OnArm => TimerStart::OnArm,
        DeclaredTimerStart::AtTick(tick) => TimerStart::AtTick(Tick(tick)),
        DeclaredTimerStart::OnSignal(signal) => TimerStart::OnSignal(lower_symbol(signal)),
        DeclaredTimerStart::OnObjectiveState { objective, state } => TimerStart::OnObjectiveState {
            objective: lower_symbol(objective),
            state: lower_state(state),
        },
        DeclaredTimerStart::Never => TimerStart::Never,
    }
}

/// Maps the declared domain onto the session's clock policy for that domain.
///
/// A deadline's pause and speed-up rules are *the domain's*, not a second
/// declaration: `Simulation` is the single-player simulation clock and
/// `AuthoritativeGameplay` the authoritative one — both freeze on pause. A
/// `UiWall`/`MediaUnscaled` deadline is refused, because a clock that keeps
/// running through a pause is not a mission timer.
fn lower_policy(timer: &DeclaredTimer) -> Result<ClockPolicy, ProgramLowerError> {
    match timer.domain {
        DeclaredTimeDomain::Simulation => Ok(ClockPolicy::single_player_simulation()),
        DeclaredTimeDomain::AuthoritativeGameplay => Ok(ClockPolicy::authoritative_gameplay()),
        domain => Err(ProgramLowerError::NonGameplayDomain {
            timer: timer.symbol,
            domain,
        }),
    }
}

fn lower_action(action: &DeclaredTimerAction) -> TimerAction {
    match action {
        DeclaredTimerAction::SetObjectiveState { objective, state } => {
            TimerAction::SetObjectiveState {
                objective: lower_symbol(*objective),
                state: lower_state(*state),
            }
        }
        DeclaredTimerAction::Signal(signal) => TimerAction::Signal(lower_symbol(*signal)),
        DeclaredTimerAction::SpawnGroup { key, group, count } => TimerAction::SpawnGroup {
            key: IdempotencyKey(key.clone()),
            group: lower_symbol(*group),
            count: *count,
        },
        DeclaredTimerAction::Cue { key, dialogue } => TimerAction::Cue {
            key: IdempotencyKey(key.clone()),
            dialogue: dialogue.clone(),
        },
        DeclaredTimerAction::GrantOptionalReward { reward } => TimerAction::GrantOptionalReward {
            reward: reward.clone(),
        },
        DeclaredTimerAction::Finish(outcome) => TimerAction::Finish(lower_outcome(*outcome)),
    }
}

const fn lower_volume(volume: DeclaredVolume) -> Volume {
    match volume {
        DeclaredVolume::Sphere { center_m, radius_m } => Volume::Sphere { center_m, radius_m },
        DeclaredVolume::Aabb { min_m, max_m } => Volume::Aabb { min_m, max_m },
    }
}

/// Lowers a validated declared objective program into the runtime records a
/// session launches from.
///
/// A record whose [`DeclaredSupport`](cs_content::objectives::DeclaredSupport)
/// says its semantics were not recovered is refused **first**, by name, with
/// the reason the record carries: there are no supported objectives to lower,
/// and a session that ran them would be substituting a designed progression for
/// an original mission's branching, optional and failure conditions.
///
/// Every `Resolved::Unknown` refuses by name rather than becoming a default,
/// and every constructor refusal of the runtime layer is reported with the
/// declared symbol that caused it — the same boundary discipline as
/// [`crate::roster::lower_roster`].
///
/// # Errors
///
/// [`ProgramLowerError::UnsupportedProgram`] for an unrecovered record, then
/// [`ProgramLowerError::UnknownPrecedence`] and the runtime constructor
/// refusals, naming the first declaration that could not lower.
pub fn lower_program(
    declared: &DeclaredObjectiveProgram,
) -> Result<LoweredObjectives, ProgramLowerError> {
    if let Some(reason) = declared.support().refusal() {
        return Err(ProgramLowerError::UnsupportedProgram {
            subject: declared.subject().clone(),
            reason: reason.to_owned(),
        });
    }
    let precedence = match declared.precedence() {
        Resolved::Known(known) => lower_precedence(known.value),
        Resolved::Unknown { claim_id, reason } => {
            return Err(ProgramLowerError::UnknownPrecedence {
                claim_id: claim_id.clone(),
                reason: reason.clone(),
            });
        }
    };

    let objectives = declared
        .objectives()
        .iter()
        .map(|objective| ObjectiveSpec {
            id: lower_symbol(objective.symbol),
            content: objective.content.clone(),
            initial: lower_state(objective.initial),
            reveal: lower_reveal(objective.reveal),
            on_complete: lower_completion(objective.on_complete),
            completion_effects: objective
                .completion_effects
                .iter()
                .map(lower_effect)
                .collect(),
        })
        .collect();

    let mut conditions = Vec::with_capacity(declared.conditions().len());
    for declared_condition in declared.conditions() {
        let condition = CountCondition::new(
            lower_symbol(declared_condition.symbol),
            lower_kind(declared_condition.kind),
            declared_condition
                .roster
                .iter()
                .map(|actor| lower_actor(*actor)),
            declared_condition.required,
        )
        .map_err(|error| ProgramLowerError::Condition {
            condition: declared_condition.symbol,
            error,
        })?;
        conditions.push((condition, lower_reaction(declared_condition.reaction)));
    }

    let mut timers = Vec::with_capacity(declared.timers().len());
    for declared_timer in declared.timers() {
        let timer = MissionTimer::new(
            lower_symbol(declared_timer.symbol),
            lower_policy(declared_timer)?,
            lower_start(declared_timer.start),
            declared_timer.period_ticks,
            lower_action(&declared_timer.action),
        )
        .map_err(|error| ProgramLowerError::Timer {
            timer: declared_timer.symbol,
            error,
        })?;
        timers.push(timer);
    }

    let mut triggers = Vec::with_capacity(declared.triggers().len());
    for declared_trigger in declared.triggers() {
        let trigger = SweptTrigger::new(
            lower_symbol(declared_trigger.symbol),
            lower_actor(declared_trigger.actor),
            lower_volume(declared_trigger.volume),
        )
        .map_err(|error| ProgramLowerError::Trigger {
            trigger: declared_trigger.symbol,
            error,
        })?;
        triggers.push(trigger);
    }

    let spawn_groups = declared
        .spawn_groups()
        .iter()
        .map(|group| {
            (
                lower_symbol(group.symbol),
                LoweredSpawnGroup {
                    group: lower_symbol(group.symbol),
                    subject: group.subject.clone(),
                },
            )
        })
        .collect();

    Ok(LoweredObjectives {
        precedence,
        limits: RuntimeLimits::default(),
        objectives,
        conditions,
        timers,
        triggers,
        spawn_groups,
    })
}

// ---------------------------------------------------------------------------
// The session: producer → runtime → consumers
// ---------------------------------------------------------------------------

/// Why a session could not launch (or a retry could not rebuild).
#[derive(Clone, Debug, PartialEq)]
pub enum SessionLaunchError {
    /// A lowered declaration the runtime refused at registration — a defect
    /// in the lowered program, named by the symbol that failed.
    Declaration {
        /// The declaration's symbol.
        declaration: SymbolId,
        /// The runtime's refusal.
        error: RuntimeError,
    },
    /// A retry asked for the live generation. Every artifact the session
    /// hands out carries its generation so a stale one can never be confused
    /// with the current session's — which only holds when the generations
    /// differ.
    SameGeneration {
        /// The generation passed to a session already running it.
        session: SessionGeneration,
    },
}

impl std::fmt::Display for SessionLaunchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Declaration { declaration, error } => {
                write!(f, "declaration {declaration:?} was refused: {error}")
            }
            Self::SameGeneration { session } => write!(
                f,
                "a retry must advance the generation, not rebuild {session:?} in place"
            ),
        }
    }
}

impl std::error::Error for SessionLaunchError {}

/// A wave the runtime admitted, for the world to instantiate.
///
/// `subject` is the content the group's binding names — the part of the
/// directive the event stream alone does not carry. `session` travels with
/// the directive so a stale wave can never be confused with the current
/// generation's, and `instances` are the stable per-session ids the runtime
/// allocated: the world spawns exactly these, never its own count.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpawnDirective {
    /// The session that admitted the wave.
    pub session: SessionGeneration,
    /// The tick it was admitted on.
    pub tick: Tick,
    /// The idempotency key the wave was admitted under.
    pub key: IdempotencyKey,
    /// The spawn group's symbol.
    pub group: SymbolId,
    /// The content each instance is built from.
    pub subject: ContentId,
    /// The session-stable instance ids to instantiate.
    pub instances: Vec<ActorId>,
}

/// One dialogue cue waiting to be played once.
///
/// The queue is the dialogue consumer's input: [`ObjectiveSession::drain_cues`]
/// hands over every pending cue exactly once, and a retry drops what was
/// never played into the [`TeardownReport`]. `session` travels with the cue so
/// a stale one can never be confused with the current generation's.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EmittedCue {
    /// The session that emitted the cue.
    pub session: SessionGeneration,
    /// The tick it was emitted on.
    pub tick: Tick,
    /// The idempotency key the cue was admitted under.
    pub key: IdempotencyKey,
    /// The dialogue content to play.
    pub dialogue: ContentId,
}

/// A refusal the event stream reported, lifted into one typed list.
///
/// The runtime never drops a refused request silently — this is where those
/// reports land for a consumer that does not want to re-walk the stream.
/// Nothing here is a mission failure; each entry names what was asked and why
/// nothing was applied.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SessionRefusal {
    /// A declared objective state change was refused; the objective kept its
    /// state.
    ObjectiveChange {
        /// The objective.
        objective: SymbolId,
        /// The state it keeps.
        from: ObjectiveState,
        /// The state that was refused.
        to: ObjectiveState,
    },
    /// A timer request or start was refused; the timer kept its state.
    Timer {
        /// The timer.
        timer: SymbolId,
        /// Why it was refused.
        reason: TimerError,
    },
    /// A request named a declaration the runtime does not have.
    Request {
        /// The named declaration.
        request: SymbolId,
        /// Why it was refused.
        reason: RuntimeError,
    },
    /// A repeated spawn key was refused; `instances` are the ids the first
    /// admission allocated, so a wave is never spawned twice under one key.
    Spawn {
        /// The idempotency key.
        key: IdempotencyKey,
        /// The group.
        group: SymbolId,
        /// The ids the first admission took.
        instances: Vec<ActorId>,
    },
    /// A spawn admission whose group symbol is bound to no content — the wave
    /// has ids and nothing to build them from. Unreachable from a lowered
    /// program (the schema closes the reference), reported anyway.
    UnboundSpawn {
        /// The group.
        group: SymbolId,
        /// The ids the admission took.
        instances: Vec<ActorId>,
    },
    /// A repeated cue key was refused; the dialogue is not repeated.
    Cue {
        /// The idempotency key.
        key: IdempotencyKey,
        /// The dialogue that was not played.
        dialogue: ContentId,
    },
}

/// One row of the objective display the UI reads.
///
/// The row is a fact about the *stream*, not about the runtime: `revealed` is
/// set by the reveal event, `state` by change events, so a display driven by
/// this model can only show what the ordered stream reported — F39
/// non-negotiable behavior 5 on the consumer side.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DisplayedObjective {
    /// The objective's program symbol.
    pub symbol: SymbolId,
    /// The content id it was authored as.
    pub content: ContentId,
    /// Its state as last reported.
    pub state: ObjectiveState,
    /// Whether its reveal rule has fired.
    pub revealed: bool,
}

/// The objective display the UI consumes: one row per declared objective, in
/// authored order, updated only from the ordered event stream.
///
/// The model is **event-driven**, not runtime-queried: [`visible`](Self::visible)
/// filters `revealed && state.is_visible()`, and both fields only ever move
/// when an `ObjectiveRevealed` or `ObjectiveChanged` event reports them. A
/// refused change therefore moves nothing on the display either — the same
/// fact the runtime reported, seen by the player.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ObjectiveDisplay {
    rows: Vec<DisplayedObjective>,
}

impl ObjectiveDisplay {
    /// Seeds the display from the declared specs at launch (or retry): an
    /// `Immediate` objective is born revealed — its registration emits no
    /// event — every other row waits for its rule.
    fn seed(objectives: &[ObjectiveSpec]) -> Self {
        Self {
            rows: objectives
                .iter()
                .map(|spec| DisplayedObjective {
                    symbol: spec.id,
                    content: spec.content.clone(),
                    state: spec.initial,
                    revealed: spec.reveal == RevealRule::Immediate,
                })
                .collect(),
        }
    }

    /// Applies one stream event. Returns whether any row moved.
    fn apply(&mut self, event: &ObjectiveEventKind) -> bool {
        match event {
            ObjectiveEventKind::ObjectiveRevealed { objective, state } => {
                let Some(row) = self.rows.iter_mut().find(|row| row.symbol == *objective) else {
                    return false;
                };
                let changed = !row.revealed || row.state != *state;
                row.revealed = true;
                row.state = *state;
                changed
            }
            ObjectiveEventKind::ObjectiveChanged { objective, to, .. } => {
                let Some(row) = self.rows.iter_mut().find(|row| row.symbol == *objective) else {
                    return false;
                };
                let changed = row.state != *to;
                row.state = *to;
                changed
            }
            _ => false,
        }
    }

    /// The rows the player may be shown, in authored order.
    #[must_use]
    pub fn visible(&self) -> Vec<DisplayedObjective> {
        self.rows
            .iter()
            .filter(|row| row.revealed && row.state.is_visible())
            .cloned()
            .collect()
    }

    /// One row by symbol, whether or not it is visible.
    #[must_use]
    pub fn row(&self, symbol: SymbolId) -> Option<DisplayedObjective> {
        self.rows.iter().find(|row| row.symbol == symbol).cloned()
    }

    /// How many objectives the display tracks.
    #[must_use]
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// Whether the display tracks nothing.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }
}

/// What [`ObjectiveSession::step`] answered with.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionTick {
    /// The runtime's ordered event stream for the tick — the authoritative
    /// trace every consumer answer below was dispatched from.
    pub tick: ObjectiveTick,
    /// The waves admitted this tick, for the world to instantiate.
    pub spawns: Vec<SpawnDirective>,
    /// Every refusal the stream reported, named.
    pub refusals: Vec<SessionRefusal>,
    /// The session's outcome once settled.
    pub outcome: Option<TerminalOutcome>,
    /// Set when a bound stopped the tick before any effect applied.
    pub stop: Option<StopReason>,
    /// Whether any display row moved this tick.
    pub display_changed: bool,
}

/// What a retry's teardown reports: everything the old session still owned.
///
/// This is data, not a narrative: `actors` are the wave instances the world
/// must despawn (the new session's instance counter will reuse those same
/// ids, which is why the report exists and why it precedes the new session's
/// first step), `cues` are the emitted-but-never-drained lines that will now
/// never play, `armed_timers` the deadlines that were still counting, and
/// `session`/`outcome` stamp which generation this report belongs to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TeardownReport {
    /// The generation that was torn down.
    pub session: SessionGeneration,
    /// The live wave instances the world must despawn.
    pub actors: Vec<ActorId>,
    /// The cues emitted but never drained — dropped, never played.
    pub cues: Vec<EmittedCue>,
    /// The declared timers still armed at teardown.
    pub armed_timers: Vec<SymbolId>,
    /// The outcome the old session settled, if it did.
    pub outcome: Option<TerminalOutcome>,
}

/// The wired objective session of one mission generation: the producer→
/// runtime→consumer path the stage exists to wire.
///
/// One session owns one [`ObjectiveRuntime`] and every piece of per-session
/// consumer state. `step` is the only way in: it hands the runtime one
/// [`TickInput`] — the whole producer surface — and dispatches the ordered
/// stream to the consumers. `retry` is the only way out that keeps the
/// session object: it reports what the old generation still owned and
/// rebuilds a fresh runtime for the new one.
#[derive(Debug)]
pub struct ObjectiveSession {
    /// The lowered program this session launches and relaunches from. Kept so
    /// a retry rebuilds the authored declarations rather than carrying the
    /// failed generation's state.
    program: LoweredObjectives,
    /// The live runtime of the current generation.
    runtime: ObjectiveRuntime,
    /// The objective display the UI reads.
    display: ObjectiveDisplay,
    /// Dialogue cues emitted and not yet drained.
    pending_cues: VecDeque<EmittedCue>,
    /// The wave instances admitted this generation and still live, per group
    /// symbol, in admission order. An actor the stream counts in a
    /// gone-category (`Destroyed`, `Captured`, `Escaped`, `Despawned` — not
    /// `Disabled`, which still exists in the world) leaves the registry, so a
    /// teardown names exactly the actors the world still has to remove.
    live: BTreeMap<SymbolId, Vec<ActorId>>,
}

impl ObjectiveSession {
    /// Builds a runtime from the lowered program: every declaration
    /// registered in authored order, each refusal named by the symbol that
    /// caused it.
    fn build_runtime(
        program: &LoweredObjectives,
        session: SessionGeneration,
    ) -> Result<ObjectiveRuntime, SessionLaunchError> {
        let mut runtime = ObjectiveRuntime::new(session, program.precedence, program.limits);
        for spec in &program.objectives {
            runtime.add_objective(spec.clone()).map_err(|error| {
                SessionLaunchError::Declaration {
                    declaration: spec.id,
                    error,
                }
            })?;
        }
        for (condition, reaction) in &program.conditions {
            runtime
                .add_condition(condition.clone(), *reaction)
                .map_err(|error| SessionLaunchError::Declaration {
                    declaration: condition.key,
                    error,
                })?;
        }
        for trigger in &program.triggers {
            runtime.add_trigger(trigger.clone()).map_err(|error| {
                SessionLaunchError::Declaration {
                    declaration: trigger.id(),
                    error,
                }
            })?;
        }
        for timer in &program.timers {
            runtime
                .add_timer(timer.clone())
                .map_err(|error| SessionLaunchError::Declaration {
                    declaration: timer.id(),
                    error,
                })?;
        }
        Ok(runtime)
    }

    /// Launches one session of the lowered program.
    ///
    /// # Errors
    ///
    /// [`SessionLaunchError::Declaration`] naming the first declaration the
    /// runtime refused.
    pub fn launch(
        program: LoweredObjectives,
        session: SessionGeneration,
    ) -> Result<Self, SessionLaunchError> {
        let runtime = Self::build_runtime(&program, session)?;
        let display = ObjectiveDisplay::seed(&program.objectives);
        Ok(Self {
            program,
            runtime,
            display,
            pending_cues: VecDeque::new(),
            live: BTreeMap::new(),
        })
    }

    /// The session generation this session owns.
    #[must_use]
    pub const fn session(&self) -> SessionGeneration {
        self.runtime.session()
    }

    /// The lowered program this session runs.
    #[must_use]
    pub const fn program(&self) -> &LoweredObjectives {
        &self.program
    }

    /// The live runtime, for state inspection (timer states, counters,
    /// `is_visible`).
    #[must_use]
    pub const fn runtime(&self) -> &ObjectiveRuntime {
        &self.runtime
    }

    /// The objective display the UI reads.
    #[must_use]
    pub const fn display(&self) -> &ObjectiveDisplay {
        &self.display
    }

    /// The session's outcome once settled.
    #[must_use]
    pub const fn outcome(&self) -> Option<TerminalOutcome> {
        self.runtime.outcome()
    }

    /// The subject content a spawn group is bound to, when it is bound.
    #[must_use]
    pub fn group_subject(&self, group: SymbolId) -> Option<&ContentId> {
        self.program.spawn_groups.get(&group).map(|g| &g.subject)
    }

    /// Every wave instance admitted this generation and still live, in
    /// (group, admission) order.
    #[must_use]
    pub fn live_actors(&self) -> Vec<ActorId> {
        self.live.values().flatten().copied().collect()
    }

    /// The live instances of one group.
    #[must_use]
    pub fn live_wave(&self, group: SymbolId) -> &[ActorId] {
        self.live.get(&group).map_or(&[], Vec::as_slice)
    }

    /// How many cues are waiting to be drained.
    #[must_use]
    pub fn pending_cues(&self) -> usize {
        self.pending_cues.len()
    }

    /// Hands over every pending cue, in emission order, exactly once.
    ///
    /// This is the dialogue consumer's only read: a drained cue is gone from
    /// the session, so a line plays once — and a cue still pending when the
    /// session is retried is reported in the [`TeardownReport`], never played.
    pub fn drain_cues(&mut self) -> Vec<EmittedCue> {
        self.pending_cues.drain(..).collect()
    }

    /// Applies one tick's facts and dispatches the ordered stream to the
    /// consumers.
    ///
    /// # Errors
    ///
    /// [`RuntimeError`] when the runtime refuses the tick — a non-advancing
    /// tick or a refused movement. The error is propagated unchanged and
    /// nothing was applied anywhere: the display, the cue queue and the wave
    /// registry are untouched, matching the runtime's own "a refused tick
    /// changed nothing".
    pub fn step(&mut self, input: &TickInput<'_>) -> Result<SessionTick, RuntimeError> {
        let tick = self.runtime.step(input)?;
        let mut spawns = Vec::new();
        let mut refusals = Vec::new();
        let mut display_changed = false;
        for event in &tick.events {
            match &event.kind {
                ObjectiveEventKind::ObjectiveRevealed { .. }
                | ObjectiveEventKind::ObjectiveChanged { .. } => {
                    display_changed |= self.display.apply(&event.kind);
                }
                ObjectiveEventKind::ObjectiveChangeRefused {
                    objective,
                    from,
                    to,
                } => {
                    refusals.push(SessionRefusal::ObjectiveChange {
                        objective: *objective,
                        from: *from,
                        to: *to,
                    });
                }
                ObjectiveEventKind::TimerRefused { timer, reason } => {
                    refusals.push(SessionRefusal::Timer {
                        timer: *timer,
                        reason: *reason,
                    });
                }
                ObjectiveEventKind::RequestRefused { request, reason } => {
                    refusals.push(SessionRefusal::Request {
                        request: *request,
                        reason: reason.clone(),
                    });
                }
                ObjectiveEventKind::SpawnAdmitted {
                    key,
                    group,
                    instances,
                } => match self.group_subject(*group).cloned() {
                    Some(subject) => {
                        self.live.entry(*group).or_default().extend(instances);
                        spawns.push(SpawnDirective {
                            session: self.runtime.session(),
                            tick: tick.tick,
                            key: key.clone(),
                            group: *group,
                            subject,
                            instances: instances.clone(),
                        });
                    }
                    None => refusals.push(SessionRefusal::UnboundSpawn {
                        group: *group,
                        instances: instances.clone(),
                    }),
                },
                ObjectiveEventKind::SpawnRefused {
                    key,
                    group,
                    instances,
                } => {
                    refusals.push(SessionRefusal::Spawn {
                        key: key.clone(),
                        group: *group,
                        instances: instances.clone(),
                    });
                }
                ObjectiveEventKind::CueEmitted { key, dialogue } => {
                    self.pending_cues.push_back(EmittedCue {
                        session: self.runtime.session(),
                        tick: tick.tick,
                        key: key.clone(),
                        dialogue: dialogue.clone(),
                    });
                }
                ObjectiveEventKind::CueRefused { key, dialogue } => {
                    refusals.push(SessionRefusal::Cue {
                        key: key.clone(),
                        dialogue: dialogue.clone(),
                    });
                }
                ObjectiveEventKind::Counted { actor, kind } => {
                    // An actor in a gone-category leaves the wave registry:
                    // `Disabled` is not gone — the actor still exists in the
                    // world and is still this session's to tear down.
                    if !matches!(kind, CountKind::Disabled) {
                        for instances in self.live.values_mut() {
                            instances.retain(|instance| *instance != *actor);
                        }
                    }
                }
                ObjectiveEventKind::OutcomeSettled { .. }
                | ObjectiveEventKind::ConditionMet { .. }
                | ObjectiveEventKind::TriggerCrossed(_)
                | ObjectiveEventKind::SignalRaised { .. }
                | ObjectiveEventKind::TimerArmed { .. }
                | ObjectiveEventKind::TimerExpired { .. }
                | ObjectiveEventKind::OptionalReward { .. } => {}
            }
        }
        Ok(SessionTick {
            stop: tick.stop,
            outcome: tick.outcome,
            tick,
            spawns,
            refusals,
            display_changed,
        })
    }

    /// Tears the current generation down and relaunches the same program
    /// under a new session generation.
    ///
    /// The [`TeardownReport`] is built *first* and names everything the old
    /// session still owned — the live wave actors to despawn, the cues that
    /// will now never play, the deadlines that were still armed, the settled
    /// outcome — because the fresh runtime's instance counter reuses the same
    /// ids, and a world that misses the report meets the old wave twice.
    ///
    /// The fresh runtime is built before the old one is released, so a launch
    /// defect can never leave the session half-torn-down.
    ///
    /// # Errors
    ///
    /// [`SessionLaunchError::SameGeneration`] when `session` is the live
    /// generation — the rebuilt session's cues, waves and events would carry
    /// the very stamp the torn-down artifacts already do — and
    /// [`SessionLaunchError::Declaration`] when the same program that launched
    /// before cannot be registered again — unreachable for a program that was
    /// lowered, but reported rather than assumed.
    pub fn retry(
        &mut self,
        session: SessionGeneration,
    ) -> Result<TeardownReport, SessionLaunchError> {
        if session == self.runtime.session() {
            return Err(SessionLaunchError::SameGeneration { session });
        }
        let fresh = Self::build_runtime(&self.program, session)?;
        let report = TeardownReport {
            session: self.runtime.session(),
            actors: self.live_actors(),
            cues: self.pending_cues.drain(..).collect(),
            armed_timers: self
                .program
                .timers
                .iter()
                .map(MissionTimer::id)
                .filter(|timer| {
                    self.runtime
                        .timer_state(*timer)
                        .is_some_and(|state| state.is_armed())
                })
                .collect(),
            outcome: self.runtime.outcome(),
        };
        self.runtime = fresh;
        self.display = ObjectiveDisplay::seed(&self.program.objectives);
        self.live.clear();
        Ok(report)
    }
}

// ---------------------------------------------------------------------------
// F39-D: the retail objective-record census
// ---------------------------------------------------------------------------
//
// The stage's own question — "validate original branching, optional and
// failure conditions" — is not answerable from design, so this half measures
// the original's own objective records on the read-only installation.
//
// **What is measured.** Every mission-scoped reader archive
// (`zbd/<group>/<mission>/zrdr.zbd`) is opened with the F06 two-key dispatch,
// its `objectives.zrd` member is decoded with the production `.zrd` reader, and
// the numbered `OBJECTIVE<N>` blocks are read through
// [`cs_content::stunts::objective_state_machine`]. The census reports, per
// mission: the member's provenance span, how many blocks the mission declares,
// the **complete** key vocabulary inside those blocks with the number of blocks
// each key occurs in, and which keys match a declared search vocabulary for
// branching, for optionality and for failure. F39-E2 adds a per-block reading of
// the branching keys' *targets* below.
//
// **The denominator's bound (F39-E3).** The mission rows are the denominator
// only if no reader outside mission scope declares the same vocabulary, so the
// census measures every `zrdr.zbd` the mission-scope rule does *not* claim —
// the shared reader (`zbd/zrdr.zbd`) and the world-group readers
// (`zbd/<group>/zrdr.zbd`) — as [`RetailObjectiveReaderRow`]s. Every member of
// each is decoded with the production `.zrd` reader and read through the same
// [`cs_content::stunts::objective_state_machine`], so a member that hid an
// `OBJECTIVE<N>` block under an unrelated name would still be counted; a
// member that cannot be decoded **fails the census** rather than vanishing
// from the bound. On the owner's installation every one of the 612 members
// declares zero blocks: the mission-scoped rows are the complete denominator.
// Exactly one member outside mission scope carries an objective-record name —
// `zbd/c1c/zrdr.zbd`'s `targets.zrd`, 5 shared target records the C1C missions
// can inherit — and it is published the same way (F39-E3's measured answer to
// F39-D's unknown #5).
//
// **What is not measured, and why.** A census of key *names* is a vocabulary
// measurement, not a decoded behaviour: it says which declarations the original
// writes, never what one does, and a negative result ("no such key occurs")
// bounds the vocabulary, not the game's behaviour. F13's mission-language
// instruction table is still unmeasured, so the compiled program behind these
// records is not decoded at all; nothing here reads an opcode. And no original
// executable was run, so nothing here is evidence of how the game behaves —
// only of what its files declare.
//
// **Why it matters to the engine.** F39 AC04 speaks of completing *supported*
// objectives. This census is where "supported" gets its content: whatever the
// original's objective records do and do not declare is what a declared
// program may claim to have recovered, and
// [`cs_content::objectives::DeclaredSupport`] carries the verdict. On the
// owner's installation the measured answer is that no original mission declares
// a branching, optional or failure condition this stage can read — so every
// original mission is Unsupported for exactly the semantics F39 names, and
// [`lower_program`] refuses such a record by name rather than substituting a
// designed progression for it.

/// The reader archive a mission's objective record lives in.
const MISSION_READER_ARCHIVE: &str = "zrdr.zbd";

// ---------------------------------------------------------------------------
// F39-E2: precedence between completion effects in one block
// ---------------------------------------------------------------------------
//
// F39-D counted the branching *sites* per mission, which answers "does the
// original declare branching at all" and nothing finer. F39-E2's question is the
// per-block one: when a block declares more than one completion effect, which
// one applies to the objective they name in common, and in which order? That
// needs the **targets**, not the key names, so it needs a per-block walk.
//
// [`measure_block_precedence`] is that walk, and it is deliberately a pure
// function over one decoded member: no filesystem, no installation, so a
// synthetic record can be measured with the same production code the census
// runs. It reads each numbered `OBJECTIVE<N>` block's fields **in the record's own
// order** (`zrd_flat_fields` preserves it), which is the only ordering the bytes
// carry — and which is measured *not* to be a format invariant, because every
// effect pair the corpus writes more than once is written both ways round (see
// `MeasuredBranchPrecedence::declared_order`). So the authored order is recorded
// on each [`MeasuredBranchConflict`] and never ranked by it.
//
// Two facts the walk measures that a key census cannot:
//
// * a **target** is an objective of the same mission: every measured target
//   names a block the same record declares, and none names its own block, so no
//   cross-record or self reference needs a namespace the engine does not have;
// * a `NAP` site carries a **second number** that is not an objective number, so
//   the effect's value is not a bare target list.

/// Measures the completion-effect declarations of one decoded objective record,
/// per block.
///
/// A census of declaration sites and their targets, never a rule: it says which
/// effects a block declares, which objectives each names, and which blocks name
/// the same objective twice — the only shape in which the original's order would
/// have to decide an outcome. What any effect *does*, and which of two effects on
/// one objective the original applies, stay unmeasured
/// ([`cs_content::objectives::UNMEASURED_BLOCK_PRECEDENCE`]).
#[must_use]
pub fn measure_block_precedence(document: &ZrdValue) -> MeasuredBranchPrecedence {
    // Two passes, because "does this record declare objective 68?" is a question
    // about the whole record: a target is only dangling if no block names it.
    let record = objective_record(document);
    let declared: BTreeSet<u32> = zrd_flat_fields(record)
        .into_iter()
        .filter(|(key, _)| is_objective_block(key))
        .filter_map(|(key, _)| objective_block_number(key))
        .collect();

    let mut measured = MeasuredBranchPrecedence::default();
    for (block, block_value) in zrd_flat_fields(record) {
        let Some(number) = objective_block_number(block) else {
            continue;
        };
        measured.blocks += 1;
        let sites: Vec<MeasuredBranchSite> = zrd_flat_fields(block_value)
            .into_iter()
            .filter_map(|(field, value)| {
                let kind = BranchEffectKind::from_measured_key(field)?;
                Some(MeasuredBranchSite {
                    kind,
                    targets: measured_numbers(value, |entry| match entry {
                        ZrdValue::Int(target) => Some(*target),
                        _ => None,
                    }),
                    arguments: measured_numbers(value, |entry| match entry {
                        ZrdValue::Float(number) => Some(*number),
                        _ => None,
                    }),
                })
            })
            .collect();
        if sites.is_empty() {
            continue;
        }
        measured.effect_blocks += 1;
        measured.effect_sites += u32::try_from(sites.len()).unwrap_or(u32::MAX);
        measured.argument_sites += u32::try_from(
            sites
                .iter()
                .filter(|site| !site.arguments.is_empty())
                .count(),
        )
        .unwrap_or(u32::MAX);
        for site in &sites {
            measured.targets += u32::try_from(site.targets.len()).unwrap_or(u32::MAX);
            measured.widest_site = measured
                .widest_site
                .max(u32::try_from(site.targets.len()).unwrap_or(u32::MAX));
            if site.names(number) {
                measured.self_referencing_sites += 1;
            }
            if site.targets.iter().any(|target| !declared.contains(target)) {
                measured.dangling_sites += 1;
            }
        }
        if sites.len() < 2 {
            continue;
        }
        // A **multi-effect** block declares two or more *different* effects, and
        // the whole question is which of two effects wins, so the counting below
        // is over pairs of different effects. A record that spelled the same key
        // twice in one block declares the same effect twice, which is a different
        // (and separately unmeasured) shape and is never counted as an ordering
        // question. Measured: no block in the installation spells an effect key
        // twice, so this distinction changes no measured number — it is here so
        // the counter keeps meaning what
        // [`MeasuredBranchPrecedence::multi_effect_blocks`] says it means, and
        // what the repeated-key shape would mean is filed as **F39-E6** rather
        // than decided here.
        let kinds: BTreeSet<BranchEffectKind> = sites.iter().map(|site| site.kind).collect();
        if kinds.len() < 2 {
            continue;
        }
        measured.multi_effect_blocks += 1;
        // The block's own declared order, counted for every pair of *different*
        // effects: the measurement that shows the field order is authored per
        // block rather than imposed by the format.
        for (index, site) in sites.iter().enumerate() {
            for later in sites.iter().skip(index + 1) {
                if site.kind == later.kind {
                    continue;
                }
                *measured
                    .authored_orders
                    .entry((site.kind, later.kind))
                    .or_insert(0) += 1;
            }
        }
        // Two *different* effects can only collide if their target sets overlap;
        // the disjoint blocks are measured as such so the isolated conditions
        // below can never be read as "every multi-effect block conflicts".
        let mut disjoint = true;
        for (index, site) in sites.iter().enumerate() {
            for later in sites.iter().skip(index + 1) {
                if site.kind == later.kind {
                    continue;
                }
                if site.targets.iter().any(|target| later.names(*target)) {
                    disjoint = false;
                }
            }
        }
        if disjoint {
            measured.disjoint_multi_effect_blocks += 1;
            continue;
        }
        // One condition per shared objective, in authored order and sorted by
        // target so the reading of a block does not depend on hash order.
        let mut targets: Vec<u32> = sites
            .iter()
            .flat_map(|site| site.targets.iter().copied())
            .collect();
        targets.sort_unstable();
        targets.dedup();
        for target in targets {
            let conflicting: Vec<MeasuredBranchSite> = sites
                .iter()
                .filter(|site| site.names(target))
                .cloned()
                .collect();
            // Two *different* effects on one objective, never the same effect
            // spelled twice: a conflict is a precedence question, and one effect
            // repeated is not two of them.
            if conflicting
                .iter()
                .map(|site| site.kind)
                .collect::<BTreeSet<_>>()
                .len()
                < 2
            {
                continue;
            }
            measured.conflicts.push(MeasuredBranchConflict {
                block: block.to_owned(),
                target,
                sites: conflicting,
            });
        }
    }
    measured
}

/// `OBJECTIVE17` → `true`; `OBJECTIVE_X` → `false`. F13's own block rule, the
/// same one `objective_state_machine` counts blocks with.
fn is_objective_block(key: &str) -> bool {
    objective_block_number(key).is_some()
}

/// `OBJECTIVE17` → `Some(17)`; anything else → `None`.
///
/// The number is an **objective index**: F39-E2 measured every completion-effect
/// target naming a block of the same record, which is what makes it an index
/// into the numbered blocks rather than a name.
fn objective_block_number(key: &str) -> Option<u32> {
    key.strip_prefix(OBJECTIVE_BLOCK_PREFIX)
        .filter(|digits| !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit()))
        .and_then(|digits| digits.parse().ok())
}

/// The numbers of a `.zrd` value a measurement accepts, keeping the record's own
/// order. A value the original never writes (a bare number, a text node) yields
/// nothing rather than a guessed reading.
fn measured_numbers<T: Copy>(value: &ZrdValue, accept: fn(&ZrdValue) -> Option<T>) -> Vec<T> {
    match value {
        ZrdValue::List(children) => children.iter().filter_map(accept).collect(),
        _ => Vec::new(),
    }
}

/// A conflict measured in one mission's record, named with the mission it came
/// from.
///
/// The census flattens the per-record conflicts into these so a corpus-wide
/// answer can name every instance, while each [`MeasuredBranchConflict`] stays
/// the record's own reading.
#[derive(Clone, Debug, PartialEq)]
pub struct RetailBranchConflict {
    /// The mission, as `zbd/<group>/<mission>`.
    pub mission: String,
    /// The condition measured inside that mission's record.
    pub conflict: MeasuredBranchConflict,
}

impl RetailBranchConflict {
    /// The mission's `zbd/<group>/<mission>` label with the block it sits in,
    /// as one locatable string (`zbd/c3/m05 OBJECTIVE8`).
    #[must_use]
    pub fn label(&self) -> String {
        format!("{} {}", self.mission, self.conflict.block)
    }
}

/// Why the retail objective-record census could not be produced.
#[derive(Clone, Debug, PartialEq)]
pub enum ObjectiveCensusError {
    /// The installation could not be discovered. The reason is rendered rather
    /// than carried as the discovery error type, so the census's own error
    /// stays comparable and printable on its own.
    Discovery(String),
    /// A mission reader archive could not be read from disk or is missing from
    /// the inventory.
    Read {
        /// The archive's logical key.
        container: String,
        /// Why the read failed.
        reason: String,
    },
    /// A mission's objective record did not decode as `.zrd`.
    Decode {
        /// The archive's logical key.
        container: String,
        /// The decoder's refusal code.
        code: &'static str,
        /// Offset of the refusal inside the member.
        offset: u64,
    },
}

impl fmt::Display for ObjectiveCensusError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Discovery(reason) => {
                write!(f, "the installation could not be discovered: {reason}")
            }
            Self::Read { container, reason } => {
                write!(f, "reader archive {container} could not be read: {reason}")
            }
            Self::Decode {
                container,
                code,
                offset,
            } => write!(
                f,
                "{container}'s objective record did not decode: {code} at offset {offset}"
            ),
        }
    }
}

impl std::error::Error for ObjectiveCensusError {}

/// One mission's measured objective record.
///
/// `Eq` is not derived: F39-E2's `branch_precedence` carries the measured numbers
/// beside the completion-effect targets, and a float is not `Eq`.
#[derive(Clone, Debug, PartialEq)]
pub struct RetailObjectiveRow {
    /// The mission, as `zbd/<group>/<mission>`.
    pub mission: String,
    /// The reader archive's installation spelling, as production discovery
    /// spells it (`ZBD/<GROUP>/<MISSION>/zrdr.zbd`). Its
    /// [`RelativePath::logical_key`](cs_types::install::RelativePath::logical_key)
    /// is `<mission>/zrdr.zbd`.
    pub container: String,
    /// SHA-256 of that whole archive, from production discovery.
    pub container_sha256: String,
    /// The objective member's name (`objectives.zrd`).
    pub member: String,
    /// The absolute offset of the member's first byte inside the archive.
    pub member_offset: u64,
    /// The member's length in bytes.
    pub member_len: u64,
    /// SHA-256 of the member's own bytes.
    pub member_sha256: String,
    /// How many numbered `OBJECTIVE<N>` blocks the mission declares. This is
    /// the measured count of *declared* objectives, in blocks, not a decoded
    /// objective graph.
    pub blocks: u32,
    /// The complete key vocabulary inside those blocks, sorted by key, with the
    /// number of blocks each key occurs in.
    pub keys: Vec<(String, u32)>,
    /// Occurrences of [`BRANCH_KEY_VOCABULARY`] keys across those blocks.
    pub branching_sites: u32,
    /// The part of `branching_sites` that declares a **completion effect** on
    /// another objective
    /// ([`cs_content::objectives::BRANCH_EFFECT_KEY_VOCABULARY`]). F39-E2 splits
    /// it out because it is the family whose *targets* decide whether an ordering
    /// question exists at all.
    pub completion_effect_sites: u32,
    /// The part of `branching_sites` that declares an explicit **order
    /// dependency** (`TICK_DEPENDS_ON_OBJ`), which names an objective this one is
    /// sequenced behind and is never a completion effect.
    pub order_dependency_sites: u32,
    /// Occurrences of the optionality keys across those blocks: the
    /// `INACTIVE<n>` stages and `INACTIVE_COMPLETION_COUNT`.
    pub optional_sites: u32,
    /// Occurrences of [`FAILURE_KEY_VOCABULARY`] keys across those blocks.
    pub failure_sites: u32,
    /// F39-E2's per-block reading of this record's completion effects, measured by
    /// [`measure_block_precedence`] on the same decoded member.
    pub branch_precedence: MeasuredBranchPrecedence,
}

impl RetailObjectiveRow {
    /// The census's per-mission measurement, as the record's own type.
    ///
    /// The same numbers under the same names, so a refusal can carry the census
    /// without a second translation of them.
    #[must_use]
    pub fn measured(&self) -> cs_content::objectives::MeasuredObjectiveRecord {
        cs_content::objectives::MeasuredObjectiveRecord {
            container: self.container.clone(),
            member: self.member.clone(),
            sha256: self.member_sha256.clone(),
            byte_len: self.member_len,
            blocks: self.blocks,
            branching_sites: self.branching_sites,
            optional_sites: self.optional_sites,
            failure_sites: self.failure_sites,
            branch_precedence: self.branch_precedence.clone(),
        }
    }

    /// This row's measured completion-effect conflicts, named with the mission.
    #[must_use]
    pub fn conflicts(&self) -> Vec<RetailBranchConflict> {
        self.branch_precedence
            .conflicts
            .iter()
            .cloned()
            .map(|conflict| RetailBranchConflict {
                mission: self.mission.clone(),
                conflict,
            })
            .collect()
    }
}

/// The member names a mission-scoped reader uses for its control records:
/// measured over the campaign and scenario readers of the installation
/// (F14-D.1, F13-B — the per-mission members are `map.zrd`, `aiv.zrd`,
/// `objectives.zrd` and `egen.zrd`, instant-action readers add `ia.zrd`,
/// networked readers `net.zrd`, and `targets.zrd` holds the objective target
/// records). Outside mission scope a member carrying one of these names is a
/// place a mission can inherit a declaration **by name** — measured over the
/// owner's installation exactly one occurs (`zbd/c1c/zrdr.zbd`'s `targets.zrd`).
const MISSION_CONTROL_MEMBER_NAMES: [&str; 7] = [
    "aiv.zrd",
    "egen.zrd",
    "ia.zrd",
    "map.zrd",
    "net.zrd",
    "objectives.zrd",
    "targets.zrd",
];

/// The member that carries a mission's objective *target* records — the
/// records [`cs_content::stunts::objective_record_count`] and
/// [`cs_content::stunts::objective_record_keys`] measure.
const OBJECTIVE_TARGETS_MEMBER: &str = "targets.zrd";

/// One member of a non-mission-scoped reader archive, measured for objective
/// declarations.
///
/// Every member of a measured reader gets a row — the member list is the
/// searched set the negative bound covers, so nothing is summarized away.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RetailObjectiveMemberRow {
    /// The member's name as the archive's index spells it.
    pub member: String,
    /// The absolute offset of the member's first byte inside the archive.
    pub member_offset: u64,
    /// The member's length in bytes.
    pub member_len: u64,
    /// SHA-256 of the member's own bytes.
    pub member_sha256: String,
    /// How many numbered `OBJECTIVE<N>` blocks the decoded member declares —
    /// `0` for every member the owner's installation holds.
    pub blocks: u32,
    /// The complete key vocabulary inside those blocks (empty when there are
    /// none, which is every member measured).
    pub keys: Vec<(String, u32)>,
    /// Whether the member's name is a [`MISSION_CONTROL_MEMBER_NAMES`] name —
    /// a place a mission can inherit a declaration by name.
    pub mission_control: bool,
    /// When the member is a `targets.zrd` record list: how many objective
    /// target records it declares. `None` for every other member — a member
    /// not named `targets.zrd` is not read through the targets record shape,
    /// because that reading is only defined for the member the original names
    /// (a record count over an arbitrary document is a number, not a
    /// measurement).
    pub records: Option<u32>,
    /// The complete key vocabulary of the target records (empty when
    /// [`Self::records`] is `None`).
    pub record_keys: Vec<(String, u32)>,
}

/// One non-mission-scoped reader archive, measured for the census's bound.
///
/// The census's denominator is mission-scoped; these rows are what proves the
/// denominator complete: every member decoded, every `OBJECTIVE<N>` block in
/// any member counted, every mission-control member name flagged. On the
/// owner's installation every row reports zero blocks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RetailObjectiveReaderRow {
    /// The reader's scope: `zbd` for the shared reader, `zbd/<group>` for a
    /// world-group reader.
    pub scope: String,
    /// The archive's installation spelling (`ZBD/zrdr.zbd`,
    /// `ZBD/C1C/zrdr.zbd`). Its
    /// [`RelativePath::logical_key`](cs_types::install::RelativePath::logical_key)
    /// is `<scope>/zrdr.zbd`.
    pub container: String,
    /// SHA-256 of that whole archive, from production discovery.
    pub container_sha256: String,
    /// Every member the archive's index declares, one row per index entry,
    /// sorted by name — the complete measured set.
    pub members: Vec<RetailObjectiveMemberRow>,
    /// How many `OBJECTIVE<N>` blocks the archive's members declare in total.
    pub blocks: u32,
    /// The union of every member's in-block key vocabulary, sorted by key,
    /// with the number of blocks each key occurs in.
    pub keys: Vec<(String, u32)>,
}

/// The scope a non-mission-scoped `zrdr.zbd` archive covers, or `None` when
/// the path is not a reader archive outside mission scope.
///
/// The logical key is matched against the observed layout: `zbd/zrdr.zbd` is
/// the shared reader (scope `zbd`) and `zbd/<group>/zrdr.zbd` a world-group
/// reader (scope `zbd/<group>`). `zbd/<group>/<mission>/zrdr.zbd` is
/// mission-scoped — [`cs_formats::script_raw::mission_scope`] owns it — and
/// anything else names no reader at all.
#[must_use]
pub fn non_mission_reader_scope(path: &RelativePath) -> Option<String> {
    let key = path.logical_key();
    let archive = format!("/{MISSION_READER_ARCHIVE}");
    let scope = key.strip_suffix(archive.as_str())?;
    if scope == "zbd" {
        Some(scope.to_owned())
    } else if scope.starts_with("zbd/") && !scope.ends_with('/') {
        let segments = scope.split('/').count();
        (segments == 2).then(|| scope.to_owned())
    } else {
        None
    }
}

/// The objective-declaration surface the census measures in one decoded
/// reader member.
///
/// This is the measurement the census applies to every member of every
/// non-mission-scoped reader — extracted so the discriminating tests can
/// exercise it on authored documents without an installation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReaderMemberMeasurement {
    /// How many numbered `OBJECTIVE<N>` blocks the document declares.
    pub blocks: u32,
    /// The complete key vocabulary inside those blocks.
    pub keys: Vec<(String, u32)>,
    /// Whether the member's name is a measured mission-control member name.
    pub mission_control: bool,
    /// For a `targets.zrd` member: the declared target records.
    pub records: Option<u32>,
    /// The complete key vocabulary of the target records.
    pub record_keys: Vec<(String, u32)>,
}

/// Measures the objective declarations of one decoded reader member.
///
/// `blocks`/`keys` come from the same
/// [`cs_content::stunts::objective_state_machine`] the mission rows use, so the
/// numbers are comparable: an `OBJECTIVE<N>` block counts the same in a shared
/// reader as in a mission reader. `mission_control` is name-based — the
/// member names a mission can inherit by. `records`/`record_keys` are read
/// only for a `targets.zrd` member, the shape the target-record readers are
/// defined for.
#[must_use]
pub fn reader_member_measurement(
    member: &str,
    document: &cs_content::stunts::ZrdValue,
) -> ReaderMemberMeasurement {
    let machine = cs_content::stunts::objective_state_machine(document);
    let lower = member.to_ascii_lowercase();
    let (records, record_keys) = if lower == OBJECTIVE_TARGETS_MEMBER {
        (
            Some(cs_content::stunts::objective_record_count(document)),
            cs_content::stunts::objective_record_keys(document),
        )
    } else {
        (None, Vec::new())
    };
    ReaderMemberMeasurement {
        blocks: machine.blocks(),
        keys: machine.keys().to_vec(),
        mission_control: MISSION_CONTROL_MEMBER_NAMES.contains(&lower.as_str()),
        records,
        record_keys,
    }
}

/// The measured objective records of every mission-scoped reader archive.
///
/// `Eq` is not derived, for the same reason [`RetailObjectiveRow`] does not
/// derive it.
#[derive(Clone, Debug, PartialEq)]
pub struct RetailObjectiveCensus {
    install_sha256: String,
    rows: Vec<RetailObjectiveRow>,
    readers: Vec<RetailObjectiveReaderRow>,
}

impl RetailObjectiveCensus {
    /// SHA-256 of the whole installation manifest, from production discovery.
    #[must_use]
    pub fn install_sha256(&self) -> &str {
        &self.install_sha256
    }

    /// The measured rows, one per mission, sorted by mission.
    #[must_use]
    pub fn rows(&self) -> &[RetailObjectiveRow] {
        &self.rows
    }

    /// How many missions the census measured.
    #[must_use]
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// Whether the census measured nothing.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// The row for one mission, by its `zbd/<group>/<mission>` label.
    #[must_use]
    pub fn row(&self, mission: &str) -> Option<&RetailObjectiveRow> {
        self.rows.iter().find(|row| row.mission == mission)
    }

    /// How many `OBJECTIVE<N>` blocks the installation declares in total.
    #[must_use]
    pub fn blocks(&self) -> u32 {
        self.rows.iter().map(|row| row.blocks).sum()
    }

    /// The union of every measured key vocabulary, sorted, with the total
    /// number of blocks each key occurs in.
    ///
    /// The census publishes this beside every classification, so the families
    /// below can be read against the vocabulary they were drawn from instead of
    /// taken on trust.
    #[must_use]
    pub fn vocabulary(&self) -> Vec<(String, u32)> {
        let mut totals: BTreeMap<String, u32> = BTreeMap::new();
        for row in &self.rows {
            for (key, count) in &row.keys {
                *totals.entry(key.clone()).or_insert(0) += count;
            }
        }
        totals.into_iter().collect()
    }

    /// Total occurrences of the measured branching keys across every mission.
    #[must_use]
    pub fn branching_sites(&self) -> u32 {
        self.rows.iter().map(|row| row.branching_sites).sum()
    }

    /// Total occurrences of the measured optionality keys across every mission.
    #[must_use]
    pub fn optional_sites(&self) -> u32 {
        self.rows.iter().map(|row| row.optional_sites).sum()
    }

    /// Total occurrences of the measured outcome keys across every mission.
    #[must_use]
    pub fn failure_sites(&self) -> u32 {
        self.rows.iter().map(|row| row.failure_sites).sum()
    }

    /// Whether the original's objective records declare **any** branching site:
    /// an objective block naming what another objective does when this one
    /// completes, or an explicit order.
    ///
    /// A measurement of declaration sites, not of rules: `true` says the
    /// original writes such a declaration, never what it does. Which is why
    /// [`cs_content::objectives::DeclaredSupport::Original`] stays unplayable
    /// either way.
    #[must_use]
    pub fn declares_branching(&self) -> bool {
        self.branching_sites() > 0
    }

    /// See [`Self::declares_branching`] for what a `true` means.
    #[must_use]
    pub fn declares_optionality(&self) -> bool {
        self.optional_sites() > 0
    }

    /// See [`Self::declares_branching`] for what a `true` means.
    #[must_use]
    pub fn declares_outcome(&self) -> bool {
        self.failure_sites() > 0
    }

    /// The missions that declare at least one branching site, with their counts.
    #[must_use]
    pub fn branching_missions(&self) -> BTreeMap<&str, u32> {
        self.missions_with_sites(|row| row.branching_sites)
    }

    /// The missions that declare at least one outcome site, with their counts.
    #[must_use]
    pub fn outcome_missions(&self) -> BTreeMap<&str, u32> {
        self.missions_with_sites(|row| row.failure_sites)
    }

    /// The non-mission-scoped reader archives the census measured, one row
    /// each, sorted by scope — the shared reader and the world-group readers.
    /// These are the bound on the mission denominator, not part of it.
    #[must_use]
    pub fn readers(&self) -> &[RetailObjectiveReaderRow] {
        &self.readers
    }

    /// The row for one non-mission reader, by scope (`zbd` or `zbd/<group>`).
    #[must_use]
    pub fn reader(&self, scope: &str) -> Option<&RetailObjectiveReaderRow> {
        self.readers.iter().find(|row| row.scope == scope)
    }

    /// How many member rows the non-mission readers hold in total — the size
    /// of the searched set.
    #[must_use]
    pub fn reader_members(&self) -> usize {
        self.readers.iter().map(|row| row.members.len()).sum()
    }

    /// How many `OBJECTIVE<N>` blocks every non-mission-scoped reader declares
    /// in total — `0` on the owner's installation, which is what makes the
    /// mission-scoped rows the complete denominator.
    #[must_use]
    pub fn reader_blocks(&self) -> u32 {
        self.readers.iter().map(|row| row.blocks).sum()
    }

    /// The union of every non-mission reader's in-block key vocabulary,
    /// sorted, with the total number of blocks each key occurs in — the same
    /// publication the mission rows make through [`Self::vocabulary`], so the
    /// negative bound reads against the vocabulary it was drawn from.
    #[must_use]
    pub fn reader_vocabulary(&self) -> Vec<(String, u32)> {
        let mut totals: BTreeMap<String, u32> = BTreeMap::new();
        for row in &self.readers {
            for (key, count) in &row.keys {
                *totals.entry(key.clone()).or_insert(0) += count;
            }
        }
        totals.into_iter().collect()
    }

    /// Every member outside mission scope whose name is a mission-control
    /// member name — the members a mission can inherit a declaration from,
    /// each as `(reader scope, member row)`. On the owner's installation this
    /// is exactly one member (`zbd/c1c`'s `targets.zrd`).
    #[must_use]
    pub fn inherited_members(&self) -> Vec<(&str, &RetailObjectiveMemberRow)> {
        let mut found = Vec::new();
        for row in &self.readers {
            for member in &row.members {
                if member.mission_control {
                    found.push((row.scope.as_str(), member));
                }
            }
        }
        found
    }

    fn missions_with_sites(&self, sites: fn(&RetailObjectiveRow) -> u32) -> BTreeMap<&str, u32> {
        self.rows
            .iter()
            .filter_map(|row| {
                let count = sites(row);
                (count > 0).then_some((row.mission.as_str(), count))
            })
            .collect()
    }

    // ------------------------------------------------------------ F39-E2 ---

    /// Total completion-effect sites across every mission: the part of
    /// [`Self::branching_sites`] that declares what happens to another objective
    /// when this one completes.
    #[must_use]
    pub fn completion_effect_sites(&self) -> u32 {
        self.rows
            .iter()
            .map(|row| row.completion_effect_sites)
            .sum()
    }

    /// Total explicit order-dependency sites across every mission, kept apart
    /// from the completion effects because it names a sequencing relationship and
    /// never a second effect on one objective.
    #[must_use]
    pub fn order_dependency_sites(&self) -> u32 {
        self.rows.iter().map(|row| row.order_dependency_sites).sum()
    }

    /// How many blocks, corpus-wide, declare two or more **different**
    /// completion effects for the same completion event.
    #[must_use]
    pub fn multi_effect_blocks(&self) -> u32 {
        self.rows
            .iter()
            .map(|row| row.branch_precedence.multi_effect_blocks)
            .sum()
    }

    /// How many of those blocks name **disjoint** target sets, so no two of their
    /// effects can act on one objective and no ordering question arises in them
    /// whatever rule the original uses.
    #[must_use]
    pub fn disjoint_multi_effect_blocks(&self) -> u32 {
        self.rows
            .iter()
            .map(|row| row.branch_precedence.disjoint_multi_effect_blocks)
            .sum()
    }

    /// Every measured conflict corpus-wide, named with the mission and the block
    /// it sits in, sorted by mission and then by block and target.
    #[must_use]
    pub fn conflicts(&self) -> Vec<RetailBranchConflict> {
        let mut conflicts: Vec<RetailBranchConflict> = self
            .rows
            .iter()
            .flat_map(RetailObjectiveRow::conflicts)
            .collect();
        conflicts.sort_by(|left, right| {
            left.mission
                .cmp(&right.mission)
                .then_with(|| left.conflict.block.cmp(&right.conflict.block))
                .then_with(|| left.conflict.target.cmp(&right.conflict.target))
        });
        conflicts
    }

    /// How many **blocks** corpus-wide name a common objective with two
    /// different completion effects: the only shape in which the original's order
    /// between them would decide anything.
    ///
    /// Blocks, not conflicts and not missions: one block can name two objectives
    /// in common and one mission can hold several conflicting blocks, so counting
    /// either of those instead would under-report the blocks an importer has to
    /// handle.
    #[must_use]
    pub fn conflicting_blocks(&self) -> u32 {
        self.rows
            .iter()
            .map(|row| row.branch_precedence.conflicting_blocks())
            .sum()
    }

    /// Whether the installation's objective records need a completion-effect
    /// ordering rule that is not measured.
    ///
    /// The corpus-wide form of
    /// [`MeasuredBranchPrecedence::needs_unmeasured_order`]: `true` says the
    /// original's own records declare two effects for one objective somewhere, so
    /// any importer recovering those declarations must refuse or name that case.
    /// It never says what the original does about it.
    #[must_use]
    pub fn needs_unmeasured_order(&self) -> bool {
        self.conflicting_blocks() > 0
    }

    /// The measured combination of every conflict, sorted, with how many
    /// conflicts carry it (`"WAKE+NAP"`, `"NAP+KILL"`, …).
    #[must_use]
    pub fn conflict_combinations(&self) -> Vec<(String, u32)> {
        let mut counts: BTreeMap<String, u32> = BTreeMap::new();
        for conflict in self.conflicts() {
            *counts.entry(conflict.conflict.combination()).or_insert(0) += 1;
        }
        counts.into_iter().collect()
    }

    /// How many blocks corpus-wide spell `first` before `second`, and how many
    /// spell it the other way round.
    ///
    /// The corpus-wide declared-order measurement. Both counts positive means the
    /// corpus imposes no order on the pair — see
    /// [`MeasuredBranchPrecedence::declared_order`].
    #[must_use]
    pub fn declared_order(&self, first: BranchEffectKind, second: BranchEffectKind) -> (u32, u32) {
        self.rows.iter().fold((0, 0), |(forward, back), row| {
            let (row_forward, row_back) = row.branch_precedence.declared_order(first, second);
            (forward + row_forward, back + row_back)
        })
    }
}

/// Measures every mission-scoped objective record in `install_root`, and
/// every `zrdr.zbd` outside mission scope as the denominator's bound (F39-E3).
///
/// Read-only: the walk uses production discovery, so it never writes inside the
/// installation. The census **fails** rather than skipping a mission whose
/// archive cannot be read or whose record does not decode, because a mission
/// that silently vanished from the denominator would look like a mission with
/// no declared objectives. The same rule covers the bound: a shared or
/// world-group reader whose member index cannot be read, or a member that
/// cannot be decoded, fails the census rather than leaving an unmeasured gap
/// an `OBJECTIVE<N>` block could hide inside.
///
/// # Errors
///
/// [`ObjectiveCensusError::Discovery`] when the installation cannot be
/// discovered, and [`ObjectiveCensusError::Read`] /
/// [`ObjectiveCensusError::Decode`] for the first mission or reader member that
/// cannot be measured.
pub fn survey_retail_objective_records(
    install_root: &Path,
) -> Result<RetailObjectiveCensus, ObjectiveCensusError> {
    let found = cs_assets::install::discover(install_root)
        .map_err(|error| ObjectiveCensusError::Discovery(error.to_string()))?;
    let install_sha256 = cs_assets::install::fingerprint(&found.manifest).to_hex();

    let mut rows: Vec<RetailObjectiveRow> = Vec::new();
    let mut readers: Vec<RetailObjectiveReaderRow> = Vec::new();
    for record in &found.manifest.files {
        let container_key = record.relative_spelling.logical_key();
        if !container_key.ends_with(MISSION_READER_ARCHIVE) {
            continue;
        }
        // Mission scope is F13-B's own rule: exactly `zbd/<group>/<mission>`, so
        // the shared reader and the world-group readers are not missions and
        // never enter the denominator.
        let spelling = record.relative_spelling.as_str().to_owned();
        let path = RelativePath::new(&spelling.to_lowercase()).map_err(|error| {
            ObjectiveCensusError::Read {
                container: container_key.clone(),
                reason: error.to_string(),
            }
        })?;
        let Some(mission) = cs_formats::script_raw::mission_scope(&path) else {
            let scope =
                non_mission_reader_scope(&path).ok_or_else(|| ObjectiveCensusError::Read {
                    container: container_key.clone(),
                    reason: "the reader archive is neither mission-scoped nor a \
                             measured shared or world-group reader"
                        .to_owned(),
                })?;
            readers.push(measure_reader(
                scope,
                &spelling,
                record.sha256.to_hex(),
                &container_key,
                &path,
                &std::fs::read(found.manifest.host_root.join(&spelling)).map_err(|error| {
                    ObjectiveCensusError::Read {
                        container: container_key.clone(),
                        reason: error.to_string(),
                    }
                })?,
            )?);
            continue;
        };
        let container_sha256 = record.sha256.to_hex();
        let bytes = std::fs::read(found.manifest.host_root.join(&spelling)).map_err(|error| {
            ObjectiveCensusError::Read {
                container: container_key.clone(),
                reason: error.to_string(),
            }
        })?;
        let discovery = cs_formats::script_raw::discover_container(&container_key, &path, &bytes);
        let member = discovery
            .programs()
            .iter()
            .find(|program| {
                program
                    .locator()
                    .member()
                    .is_some_and(|name| name.eq_ignore_ascii_case(SCENARIO_OBJECTIVES_MEMBER))
            })
            .ok_or_else(|| ObjectiveCensusError::Read {
                container: container_key.clone(),
                reason: format!(
                    "the archive declares no {} objective record",
                    SCENARIO_OBJECTIVES_MEMBER
                ),
            })?;
        let document = cs_content::stunts::decode_zrd(member.bytes()).map_err(|error| {
            ObjectiveCensusError::Decode {
                container: container_key.clone(),
                code: error.code(),
                offset: error.offset(),
            }
        })?;
        let machine = cs_content::stunts::objective_state_machine(&document);
        let locator = member.locator();
        let span = locator.span();
        // The member name the locator itself spells, so the row cannot carry an
        // empty name: the search above only accepted a program whose locator has
        // one.
        let member_name = locator.member().ok_or_else(|| ObjectiveCensusError::Read {
            container: container_key.clone(),
            reason: format!("the {SCENARIO_OBJECTIVES_MEMBER} member was located without a name"),
        })?;
        let sites = |family: fn(&str) -> bool| -> u32 {
            machine
                .keys()
                .iter()
                .filter(|(key, _)| family(key))
                .map(|(_, count)| count)
                .sum()
        };
        rows.push(RetailObjectiveRow {
            mission,
            container: spelling,
            container_sha256,
            member: member_name.to_owned(),
            member_offset: span.offset,
            member_len: span.len,
            member_sha256: cs_assets::install::sha256(member.bytes()).to_hex(),
            blocks: machine.blocks(),
            keys: machine.keys().to_vec(),
            branching_sites: sites(|key| BRANCH_KEY_VOCABULARY.contains(&key)),
            completion_effect_sites: sites(|key| BRANCH_EFFECT_KEY_VOCABULARY.contains(&key)),
            order_dependency_sites: sites(|key| key == BRANCH_ORDER_KEY),
            optional_sites: sites(is_optional_objective_key),
            failure_sites: sites(|key| FAILURE_KEY_VOCABULARY.contains(&key)),
            branch_precedence: measure_block_precedence(&document),
        });
    }

    rows.sort_by(|left, right| left.mission.cmp(&right.mission));
    readers.sort_by(|left, right| left.scope.cmp(&right.scope));
    Ok(RetailObjectiveCensus {
        install_sha256,
        rows,
        readers,
    })
}

/// Measures one non-mission-scoped reader archive for the census's bound.
///
/// Every member the archive's index declares becomes a
/// [`RetailObjectiveMemberRow`]: decoded with the production `.zrd` reader and
/// measured through [`reader_member_measurement`], so the row is the complete
/// searched set rather than a summary. A member that cannot be decoded, or an
/// archive whose member index cannot be read, fails the census rather than
/// leaving a gap an `OBJECTIVE<N>` block could hide inside.
fn measure_reader(
    scope: String,
    spelling: &str,
    container_sha256: String,
    container_key: &str,
    path: &RelativePath,
    bytes: &[u8],
) -> Result<RetailObjectiveReaderRow, ObjectiveCensusError> {
    let discovery = cs_formats::script_raw::discover_container(container_key, path, bytes);
    if let Some(finding) = discovery.findings().first() {
        return Err(ObjectiveCensusError::Read {
            container: container_key.to_owned(),
            reason: format!("the member index could not be read in full: {finding:?}"),
        });
    }
    let mut members = Vec::with_capacity(discovery.len());
    for program in discovery.programs() {
        let locator = program.locator();
        let member_name = locator
            .member()
            .ok_or_else(|| ObjectiveCensusError::Read {
                container: container_key.to_owned(),
                reason: "a member was located without a name".to_owned(),
            })?
            .to_owned();
        let document = cs_content::stunts::decode_zrd(program.bytes()).map_err(|error| {
            ObjectiveCensusError::Decode {
                container: format!("{container_key}::{member_name}"),
                code: error.code(),
                offset: error.offset(),
            }
        })?;
        let measured = reader_member_measurement(&member_name, &document);
        let span = locator.span();
        members.push(RetailObjectiveMemberRow {
            member: member_name,
            member_offset: span.offset,
            member_len: span.len,
            member_sha256: cs_assets::install::sha256(program.bytes()).to_hex(),
            blocks: measured.blocks,
            keys: measured.keys,
            mission_control: measured.mission_control,
            records: measured.records,
            record_keys: measured.record_keys,
        });
    }
    members.sort_by(|left, right| left.member.cmp(&right.member));
    let mut totals: BTreeMap<String, u32> = BTreeMap::new();
    let mut blocks = 0u32;
    for member in &members {
        blocks += member.blocks;
        for (key, count) in &member.keys {
            *totals.entry(key.clone()).or_insert(0) += count;
        }
    }
    Ok(RetailObjectiveReaderRow {
        scope,
        container: spelling.to_owned(),
        container_sha256,
        members,
        blocks,
        keys: totals.into_iter().collect(),
    })
}
