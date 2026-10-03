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
    DeclaredTimerStart, DeclaredVolume, DormantReadError, DormantReading, FAILURE_KEY_VOCABULARY,
    MeasuredBranchConflict, MeasuredBranchPrecedence, MeasuredBranchSite, MeasuredCategoryEvidence,
    MeasuredCountConditions, MeasuredDormantBlock, MeasuredTargetKinds,
    OBJECTIVE_INACTIVE_COUNT_KEY, ProgramActor, ProgramSymbol, UnmeasuredQuantity,
    is_objective_inactive_stage, is_optional_objective_key, measure_dormant_declarations,
    measured_category_evidence,
};
use cs_content::stunts::{
    OBJECTIVE_BLOCK_PREFIX, SCENARIO_OBJECTIVES_MEMBER, SCENARIO_TARGETS_MEMBER,
    TARGET_CATEGORY_KEY, TARGET_HELP_KEY, ZrdValue, objective_record, zrd_field, zrd_flat_fields,
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

// ---------------------------------------------------------------------------
// F39-E4: which counter categories the original declares
// ---------------------------------------------------------------------------
//
// F39 non-negotiable behavior 2 asks counters to distinguish destroyed,
// disabled, captured, escaped and despawned actors, and F39-A/B/C/D all recorded
// that two of the five had no producer at all. The two walks below are F39-E4's
// measurement of the surface where the original *could* declare one, and they
// are deliberately two, because the objective record and the target record are
// two members with two shapes and two jobs:
//
// * [`measure_count_conditions`] reads the `objectives.zrd` blocks' **counted
//   conditions** — the `INACTIVE<n>` sites beside their completion-count
//   threshold. This is the only counter the original's records actually write,
//   so it is where a counted category would have to appear. It names **part
//   states**, not actor end-states: 28 measured spellings, `healthy` (983) and
//   `panels` (194) the largest, none of which names a category.
// * [`measure_target_kinds`] reads the `targets.zrd` records' **localized
//   labels** — the objective kind each target is about. This is the only place
//   that reads as an actor end-state (`MSG_OBJ_DESTROY`, `MSG_OBJ_DISABLE`,
//   `MSG_OBJ_DISABLEENG`), and it is a **label**: resolving one to its displayed
//   text is F12/F51's string catalog and what it means at runtime is unmeasured.
//
// Both are pure functions over one decoded member, so a synthetic record is
// measured by the same code the census runs. Neither is a rule: a name match
// bounds the vocabulary and never produces a transition
// (`cs_content::objectives::UNMEASURED_COUNT_CATEGORY`).

/// Measures the counted conditions one decoded objective record declares.
///
/// Walks every numbered `OBJECTIVE<N>` block's fields and counts the
/// [`OBJECTIVE_INACTIVE_COUNT_KEY`] thresholds and the `INACTIVE<n>` stages
/// beside them, recording how many names each stage carries and what they are
/// spelled. Every name a stage carries is recorded, not only its last one, so a
/// negative reading is the strongest one this walk supports: *no name the
/// counted conditions write names this category*.
#[must_use]
pub fn measure_count_conditions(document: &ZrdValue) -> MeasuredCountConditions {
    let record = objective_record(document);
    let mut measured = MeasuredCountConditions::default();
    for (_, block_value) in zrd_flat_fields(record) {
        let mut has_threshold = false;
        let mut has_stage = false;
        for (field, value) in zrd_flat_fields(block_value) {
            if field == OBJECTIVE_INACTIVE_COUNT_KEY {
                measured.threshold_sites += 1;
                has_threshold = true;
                continue;
            }
            if !is_objective_inactive_stage(field) {
                continue;
            }
            measured.stage_sites += 1;
            has_stage = true;
            let Some(children) = value.as_list() else {
                // A stage that is not a list of names is counted under shape
                // `0`, so a shape this walk has never seen cannot vanish from
                // the census.
                *measured.shapes.entry(0).or_insert(0) += 1;
                continue;
            };
            *measured
                .shapes
                .entry(u32::try_from(children.len()).unwrap_or(u32::MAX))
                .or_insert(0) += 1;
            for name in children.iter().filter_map(ZrdValue::as_text) {
                *measured.names.entry(name.to_owned()).or_insert(0) += 1;
            }
        }
        if has_threshold && has_stage {
            measured.thresholded_blocks += 1;
        }
    }
    measured
}

/// Measures the objective kinds one decoded `targets.zrd` member declares.
///
/// Each record is counted once, and the labels it carries are recorded under
/// their own spelling: `help_label` is the objective's kind and `category_label`
/// the kind of thing it is about, and both are measured because both are places
/// the original spells what must happen to an actor.
#[must_use]
pub fn measure_target_kinds(document: &ZrdValue) -> MeasuredTargetKinds {
    let mut measured = MeasuredTargetKinds::default();
    for target in document.as_list().unwrap_or_default() {
        measured.records += 1;
        let help = zrd_field(target, TARGET_HELP_KEY).and_then(ZrdValue::as_text);
        if let Some(label) = help {
            *measured.names.entry(label.to_owned()).or_insert(0) += 1;
            measured.labelled += 1;
        }
        if let Some(label) = zrd_field(target, TARGET_CATEGORY_KEY).and_then(ZrdValue::as_text) {
            *measured.names.entry(label.to_owned()).or_insert(0) += 1;
        }
    }
    measured
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
    /// F39-E4's reading of this record's **counted conditions**, measured by
    /// [`measure_count_conditions`] on the same decoded member: the
    /// `INACTIVE<n>` stages, their completion-count threshold and the names
    /// they carry.
    pub count_conditions: MeasuredCountConditions,
    /// F39-E4's reading of this mission's **objective targets**, measured by
    /// [`measure_target_kinds`] on the archive's `targets.zrd` member: the
    /// localized kind each target is about.
    ///
    /// `None` when the archive declares **no** `targets.zrd` at all, which is a
    /// measured absence of the surface and not a defaulted reading: measured, one
    /// mission-scoped archive of the 53 (`c1c/m01`) carries an
    /// `objectives.zrd` and no target record, and a survey that reported "this
    /// mission declares no objective kind" for it would be reporting about a
    /// member nobody read.
    pub target_kinds: Option<MeasuredTargetKinds>,
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

    /// What this mission's own two measured surfaces declare about one counter
    /// category: its counted conditions' names and its targets' labels, merged.
    ///
    /// The mission-local form of [`RetailObjectiveCensus::category_evidence`],
    /// so a report can name the mission that carries a declaration instead of
    /// only the corpus-wide total. A mission with no target record contributes
    /// only its counted conditions' names.
    #[must_use]
    pub fn category_evidence(&self, kind: DeclaredCountKind) -> MeasuredCategoryEvidence {
        measured_category_evidence(&self.category_name_counts(), kind)
    }

    /// This row's measured names from both surfaces, merged: the counted
    /// conditions' names, plus its targets' labels when the archive declares a
    /// target record.
    #[must_use]
    pub fn category_name_counts(&self) -> BTreeMap<String, u32> {
        let mut names: BTreeMap<String, u32> = BTreeMap::new();
        let targets = self.target_kinds.iter().map(|kinds| &kinds.names);
        for source in std::iter::once(&self.count_conditions.names).chain(targets) {
            for (name, count) in source {
                *names.entry(name.clone()).or_insert(0) += *count;
            }
        }
        names
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

    // ------------------------------------------------------------ F39-E4 ---

    /// How many target records the measured missions declare, in all.
    ///
    /// Over the missions that declare a `targets.zrd` at all — see
    /// [`Self::missions_without_targets`] for the one that does not.
    #[must_use]
    pub fn target_records(&self) -> u32 {
        self.rows
            .iter()
            .filter_map(|row| row.target_kinds.as_ref())
            .map(|kinds| kinds.records)
            .sum()
    }

    /// How many of those records carry an objective kind (`help_label`).
    #[must_use]
    pub fn labelled_targets(&self) -> u32 {
        self.rows
            .iter()
            .filter_map(|row| row.target_kinds.as_ref())
            .map(|kinds| kinds.labelled)
            .sum()
    }

    /// The missions whose archive declares **no** `targets.zrd` member, so the
    /// F39-E4 target surface's denominator is stated rather than assumed.
    ///
    /// Measured over the owner's installation: `c1c/m01` carries an
    /// `objectives.zrd` with 24012 bytes of objective blocks and no target
    /// record, so its objective kinds are **unmeasured**, not empty.
    #[must_use]
    pub fn missions_without_targets(&self) -> Vec<&str> {
        self.rows
            .iter()
            .filter(|row| row.target_kinds.is_none())
            .map(|row| row.mission.as_str())
            .collect()
    }

    /// How many counted-condition stages the installation declares, in all.
    #[must_use]
    pub fn stage_sites(&self) -> u32 {
        self.rows
            .iter()
            .map(|row| row.count_conditions.stage_sites)
            .sum()
    }

    /// How many completion-count thresholds the installation declares, in all.
    #[must_use]
    pub fn threshold_sites(&self) -> u32 {
        self.rows
            .iter()
            .map(|row| row.count_conditions.threshold_sites)
            .sum()
    }

    /// How many blocks carry both a threshold and at least one stage: the
    /// counted conditions a consumer would have to read, in blocks.
    #[must_use]
    pub fn thresholded_blocks(&self) -> u32 {
        self.rows
            .iter()
            .map(|row| row.count_conditions.thresholded_blocks)
            .sum()
    }

    /// The union of the names the measured corpus writes, across **both** F39-E4
    /// surfaces — the counted conditions' names and the targets' labels —
    /// sorted, with the number of sites carrying each.
    ///
    /// Published whole, so the classification below can be read against the
    /// vocabulary it was drawn from instead of taken on trust. The counted
    /// conditions cover all 53 mission readers and the target labels the 52 that
    /// declare a `targets.zrd` (see [`Self::missions_without_targets`]), so the
    /// two surfaces have different denominators and neither is widened to the
    /// other.
    #[must_use]
    pub fn category_names(&self) -> Vec<(String, u32)> {
        let mut names: BTreeMap<String, u32> = BTreeMap::new();
        for row in &self.rows {
            for (name, count) in row.category_name_counts() {
                *names.entry(name).or_insert(0) += count;
            }
        }
        names.into_iter().collect()
    }

    /// What the installation's objective records declare about one counter
    /// category: the measured sites whose spelling matches
    /// [`DeclaredCountKind::name_stem`], with the spellings that matched.
    ///
    /// The corpus-wide form of [`RetailObjectiveRow::category_evidence`], and
    /// the measurement [`DeclaredCountKind::declared_by_original`] must agree
    /// with. An empty reading is a **bounded negative**: it says no name either
    /// surface writes spells this category, and never that the original has no
    /// such behaviour — the compiled program behind the records is undecoded.
    #[must_use]
    pub fn category_evidence(&self, kind: DeclaredCountKind) -> MeasuredCategoryEvidence {
        let names: BTreeMap<String, u32> = self.category_names().into_iter().collect();
        measured_category_evidence(&names, kind)
    }

    /// The missions that declare at least one site naming `kind`, with the
    /// measured sites each carries, so a report can name them.
    #[must_use]
    pub fn category_missions(&self, kind: DeclaredCountKind) -> Vec<(String, u32)> {
        self.rows
            .iter()
            .filter_map(|row| {
                let evidence = row.category_evidence(kind);
                (evidence.sites > 0).then_some((row.mission.clone(), evidence.sites))
            })
            .collect()
    }
}

/// Measures every mission-scoped objective record in `install_root`.
///
/// Read-only: the walk uses production discovery, so it never writes inside the
/// installation. The census **fails** rather than skipping a mission whose
/// archive cannot be read or whose record does not decode, because a mission
/// that silently vanished from the denominator would look like a mission with
/// no declared objectives.
///
/// # Errors
///
/// [`ObjectiveCensusError::Discovery`] when the installation cannot be
/// discovered, and [`ObjectiveCensusError::Read`] /
/// [`ObjectiveCensusError::Decode`] for the first mission that cannot be
/// measured.
pub fn survey_retail_objective_records(
    install_root: &Path,
) -> Result<RetailObjectiveCensus, ObjectiveCensusError> {
    let found = cs_assets::install::discover(install_root)
        .map_err(|error| ObjectiveCensusError::Discovery(error.to_string()))?;
    let install_sha256 = cs_assets::install::fingerprint(&found.manifest).to_hex();

    let mut rows: Vec<RetailObjectiveRow> = Vec::new();
    for record in locate_mission_objective_records(&found)? {
        let machine = cs_content::stunts::objective_state_machine(&record.document);
        let sites = |family: fn(&str) -> bool| -> u32 {
            machine
                .keys()
                .iter()
                .filter(|(key, _)| family(key))
                .map(|(_, count)| count)
                .sum()
        };
        // F39-E4's target surface, read only where the shared walk found a
        // `targets.zrd`. The decode error is this survey's own: a target record
        // that does not decode fails the census instead of reading as an empty
        // one.
        let target_kinds = record
            .targets_bytes
            .as_deref()
            .map(|bytes| {
                let targets = cs_content::stunts::decode_zrd(bytes).map_err(|error| {
                    ObjectiveCensusError::Decode {
                        container: record.container.clone(),
                        code: error.code(),
                        offset: error.offset(),
                    }
                })?;
                Ok(measure_target_kinds(&targets))
            })
            .transpose()?;
        rows.push(RetailObjectiveRow {
            mission: record.mission,
            container: record.container,
            container_sha256: record.container_sha256,
            member: record.member,
            member_offset: record.member_offset,
            member_len: record.member_len,
            member_sha256: record.member_sha256,
            blocks: machine.blocks(),
            keys: machine.keys().to_vec(),
            branching_sites: sites(|key| BRANCH_KEY_VOCABULARY.contains(&key)),
            completion_effect_sites: sites(|key| BRANCH_EFFECT_KEY_VOCABULARY.contains(&key)),
            order_dependency_sites: sites(|key| key == BRANCH_ORDER_KEY),
            optional_sites: sites(is_optional_objective_key),
            failure_sites: sites(|key| FAILURE_KEY_VOCABULARY.contains(&key)),
            branch_precedence: measure_block_precedence(&record.document),
            count_conditions: measure_count_conditions(&record.document),
            target_kinds,
        });
    }

    rows.sort_by(|left, right| left.mission.cmp(&right.mission));
    Ok(RetailObjectiveCensus {
        install_sha256,
        rows,
    })
}

/// One mission-scoped objective record, located and decoded once.
///
/// The shared walk of F39-D's census and F39-E1's
/// [`survey_retail_dormant_reveal`]: both need every mission's decoded
/// `objectives.zrd`, and two walks would mean two definitions of "every
/// mission". The decoded document is carried (not the member bytes) because
/// each survey walks it with its own reader.
///
/// Every record is held until the caller returns, so the walk decodes all 53
/// installation readers at once — a few megabytes of `.zrd` — rather than
/// streaming them. Calling this helper from both surveys therefore decodes the
/// installation twice per process, which is a deliberate simplicity trade for
/// one definition of "every mission", not a cache.
struct MissionObjectiveRecord {
    mission: String,
    container: String,
    container_sha256: String,
    member: String,
    member_offset: u64,
    member_len: u64,
    member_sha256: String,
    document: ZrdValue,
    /// F39-E4's second measured member: the archive's `targets.zrd` **bytes**,
    /// `None` when the archive declares no target record at all. Bytes and not a
    /// decoded document, so this shared walk stays the one that decodes only the
    /// objective record and the caller that asked for the target record decodes
    /// it. The absence is carried, not defaulted: it is a measured absence of the
    /// surface.
    targets_bytes: Option<Vec<u8>>,
}

/// Locates and decodes every mission-scoped objective record in `found`, sorted
/// by mission.
///
/// Mission scope is F13-B's own rule — exactly `zbd/<group>/<mission>`, so the
/// shared reader and the world-group readers are not missions — and a mission
/// archive with no objective record is a **refusal**, never a skipped row, so a
/// mission cannot vanish from a denominator.
///
/// # Errors
///
/// [`ObjectiveCensusError::Read`] for an archive that cannot be read or carries
/// no objective record, and [`ObjectiveCensusError::Decode`] for a record whose
/// bytes do not decode.
fn locate_mission_objective_records(
    found: &cs_assets::install::Discovery,
) -> Result<Vec<MissionObjectiveRecord>, ObjectiveCensusError> {
    let mut located = Vec::new();
    for record in &found.manifest.files {
        let container_key = record.relative_spelling.logical_key();
        if !container_key.ends_with(MISSION_READER_ARCHIVE) {
            continue;
        }
        let spelling = record.relative_spelling.as_str().to_owned();
        let path = RelativePath::new(&spelling.to_lowercase()).map_err(|error| {
            ObjectiveCensusError::Read {
                container: container_key.clone(),
                reason: error.to_string(),
            }
        })?;
        let Some(mission) = cs_formats::script_raw::mission_scope(&path) else {
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
        let locator = member.locator();
        let span = locator.span();
        // The member name the locator itself spells, so a row cannot carry an
        // empty name: the search above only accepted a program whose locator has
        // one.
        let member_name = locator.member().ok_or_else(|| ObjectiveCensusError::Read {
            container: container_key.clone(),
            reason: format!("the {SCENARIO_OBJECTIVES_MEMBER} member was located without a name"),
        })?;
        // F39-E4's second measured member: the objective **targets**, which is
        // where the original spells what must happen to an actor. Located by the
        // same F06 two-key dispatch, beside the objective record, and carried as
        // **bytes** rather than decoded here so a target member that fails to
        // decode cannot fail a survey that never reads it (F39-E1's dormant walk
        // calls this helper too). A mission whose archive declares **no** target
        // record is left as a measured absence (`None`), because refusing the
        // whole survey would discard F39-D's reading over one member F39-E4 added,
        // and defaulting it to an empty reading would report "this mission
        // declares no objective kind" about a member nobody read.
        let targets_bytes = discovery
            .programs()
            .iter()
            .find(|program| {
                program
                    .locator()
                    .member()
                    .is_some_and(|name| name.eq_ignore_ascii_case(SCENARIO_TARGETS_MEMBER))
            })
            .map(|targets_member| targets_member.bytes().to_vec());
        located.push(MissionObjectiveRecord {
            mission,
            container: spelling,
            container_sha256,
            member: member_name.to_owned(),
            member_offset: span.offset,
            member_len: span.len,
            member_sha256: cs_assets::install::sha256(member.bytes()).to_hex(),
            document,
            targets_bytes,
        });
    }
    located.sort_by(|left, right| left.mission.cmp(&right.mission));
    Ok(located)
}

// ---------------------------------------------------------------------------
// F39-E1: the dormant/reveal census
// ---------------------------------------------------------------------------
//
// F39-D counted the declarations (`BEGIN_DORMANT` in 1096 of 1338 blocks, an
// `INACTIVE<n>` stage in 1335, an `INACTIVE_COMPLETION_COUNT` in 130) and left
// what they *do* as its unknown #4. This census is the measurement half of that
// question: it reads every mission-scoped objective record through
// [`measure_dormant_declarations`] and publishes the *shape* of what the
// declarations carry, plus the two controlled conditions F39-E1 could isolate
// from the files alone.
//
// **What it is not.** It produces no reveal rule. The elapsed-time unit of
// `BEGIN_DORMANT`, what satisfying an `INACTIVE<n>` condition means, and whether
// a satisfied condition is monotone are all unmeasured; the inference, the
// contrary hypotheses and the verification that would settle them are in
// `docs/findings/2026-10-03-f39-e1-objective-dormant-reveal-lifecycle.md`, and
// `DeclaredSupport::Original` stays unplayable. No original executable was run.

/// Why the retail dormant/reveal census could not be produced.
#[derive(Clone, Debug, PartialEq)]
pub enum DormantCensusError {
    /// The installation could not be discovered.
    Discovery(String),
    /// A mission reader archive could not be read or carries no objective
    /// record.
    Read {
        /// The archive's logical key.
        container: String,
        /// Why it could not be read.
        reason: String,
    },
    /// A record's bytes do not decode.
    Decode {
        /// The archive's logical key.
        container: String,
        /// The decoder's stable code.
        code: &'static str,
        /// The offset the refusal was found at.
        offset: u64,
    },
    /// A block's dormant/reveal declarations are not a measured shape.
    Declaration {
        /// The mission the block belongs to.
        mission: String,
        /// The block's own key.
        block: String,
        /// Why it refused.
        reason: String,
    },
}

impl fmt::Display for DormantCensusError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Discovery(reason) => {
                write!(f, "the installation could not be discovered: {reason}")
            }
            Self::Read { container, reason } => {
                write!(f, "container {container} could not be read: {reason}")
            }
            Self::Decode {
                container,
                code,
                offset,
            } => write!(
                f,
                "container {container} objective record did not decode: {code} at offset {offset}"
            ),
            Self::Declaration {
                mission,
                block,
                reason,
            } => write!(f, "{mission} {block}: {reason}"),
        }
    }
}

impl std::error::Error for DormantCensusError {}

/// One mission's measured dormant/reveal declarations.
#[derive(Clone, Debug, PartialEq)]
pub struct DormantRevealRow {
    /// The mission, as `zbd/<group>/<mission>`.
    pub mission: String,
    /// The reader archive's relative spelling.
    pub container: String,
    /// SHA-256 of that whole archive, from production discovery.
    pub container_sha256: String,
    /// The objective member's name.
    pub member: String,
    /// The member's SHA-256, hex.
    pub member_sha256: String,
    /// Every numbered `OBJECTIVE<N>` block, in declaration order.
    pub blocks: Vec<MeasuredDormantBlock>,
}

impl DormantRevealRow {
    /// How many numbered blocks the mission declares.
    #[must_use]
    pub fn block_count(&self) -> usize {
        self.blocks.len()
    }
}

/// The measured dormant/reveal declarations of every mission-scoped objective
/// record in an installation.
#[derive(Clone, Debug, PartialEq)]
pub struct DormantRevealCensus {
    install_sha256: String,
    rows: Vec<DormantRevealRow>,
}

impl DormantRevealCensus {
    /// SHA-256 of the whole installation manifest, from production discovery.
    #[must_use]
    pub fn install_sha256(&self) -> &str {
        &self.install_sha256
    }

    /// The measured rows, one per mission, sorted by mission.
    #[must_use]
    pub fn rows(&self) -> &[DormantRevealRow] {
        &self.rows
    }

    /// Every measured block, in row then declaration order.
    pub fn blocks(&self) -> impl Iterator<Item = (&str, &MeasuredDormantBlock)> {
        self.rows.iter().flat_map(|row| {
            row.blocks
                .iter()
                .map(move |block| (row.mission.as_str(), block))
        })
    }

    /// How many mission-scoped readers were measured.
    #[must_use]
    pub fn readers(&self) -> usize {
        self.rows.len()
    }

    /// How many blocks were measured in total.
    #[must_use]
    pub fn block_count(&self) -> usize {
        self.rows.iter().map(DormantRevealRow::block_count).sum()
    }

    /// Every positive `BEGIN_DORMANT` argument the installation declares,
    /// sorted and deduplicated.
    ///
    /// This is the measured *domain* of the declaration: F39-E1 records it so a
    /// later stage can check a value against what the original actually wrote
    /// instead of against an assumed unit.
    #[must_use]
    pub fn dated_arguments(&self) -> Vec<f32> {
        let mut arguments: Vec<f32> = self
            .blocks()
            .filter_map(|(_, block)| match block.dormant {
                Some(DormantReading::ElapsedTime(value)) => Some(value),
                Some(DormantReading::Sentinel) | None => None,
            })
            .collect();
        arguments.sort_by(f32::total_cmp);
        arguments.dedup_by(|left, right| left.total_cmp(right).is_eq());
        arguments
    }

    /// How many blocks declare that they begin dormant.
    #[must_use]
    pub fn dormant_blocks(&self) -> usize {
        self.blocks().filter(|(_, b)| b.begins_dormant()).count()
    }

    /// How many dormant blocks carry the measured sentinel `-1`.
    #[must_use]
    pub fn sentinel_blocks(&self) -> usize {
        self.blocks()
            .filter(|(_, b)| b.dormant.is_some_and(DormantReading::is_sentinel))
            .count()
    }

    /// How many dormant blocks carry a positive argument.
    #[must_use]
    pub fn dated_blocks(&self) -> usize {
        self.dormant_blocks() - self.sentinel_blocks()
    }

    /// How many blocks declare at least one `INACTIVE<n>` condition.
    #[must_use]
    pub fn condition_blocks(&self) -> usize {
        self.blocks()
            .filter(|(_, b)| b.condition_count() > 0)
            .count()
    }

    /// How many `INACTIVE<n>` declarations were read in total.
    #[must_use]
    pub fn condition_count(&self) -> usize {
        self.blocks().map(|(_, b)| b.condition_count()).sum()
    }

    /// How many blocks declare an `INACTIVE_COMPLETION_COUNT`.
    #[must_use]
    pub fn completion_count_blocks(&self) -> usize {
        self.blocks()
            .filter(|(_, b)| b.completion_count.is_some())
            .count()
    }

    /// The declared `INACTIVE_COMPLETION_COUNT` values, sorted and deduplicated.
    #[must_use]
    pub fn completion_counts(&self) -> Vec<u32> {
        let mut counts: Vec<u32> = self
            .blocks()
            .filter_map(|(_, b)| b.completion_count)
            .collect();
        counts.sort_unstable();
        counts.dedup();
        counts
    }

    /// How many blocks declare a count **equal** to their condition count.
    #[must_use]
    pub fn counts_matching_conditions(&self) -> usize {
        self.blocks()
            .filter(|(_, b)| {
                b.completion_count
                    .is_some_and(|count| count as usize == b.condition_count())
            })
            .count()
    }

    /// How many blocks declare a count **larger** than their condition count —
    /// a threshold their own conditions cannot reach.
    #[must_use]
    pub fn counts_above_conditions(&self) -> usize {
        self.blocks()
            .filter(|(_, b)| b.count_exceeds_conditions())
            .count()
    }

    /// How many blocks declare a count and no condition at all.
    #[must_use]
    pub fn counts_without_conditions(&self) -> usize {
        self.blocks()
            .filter(|(_, b)| b.count_without_conditions())
            .count()
    }

    /// How many blocks carry at least one display identity.
    #[must_use]
    pub fn identity_blocks(&self) -> usize {
        self.blocks()
            .filter(|(_, b)| !b.identities.is_empty())
            .count()
    }

    /// How many `IDENTITY` declarations were read in total.
    ///
    /// Measured: this is **one** more than [`Self::identity_blocks`], because one
    /// block of the installation declares two identities and which one the
    /// original honours is unmeasured.
    #[must_use]
    pub fn identity_declarations(&self) -> usize {
        self.blocks().map(|(_, b)| b.identities.len()).sum()
    }

    /// The distinct identity roles, with the number of declarations naming each.
    #[must_use]
    pub fn identity_roles(&self) -> Vec<(String, usize)> {
        let mut roles: BTreeMap<String, usize> = BTreeMap::new();
        for (_, block) in self.blocks() {
            for identity in &block.identities {
                *roles.entry(identity.role.clone()).or_insert(0) += 1;
            }
        }
        roles.into_iter().collect()
    }

    /// How many identity declarations name a message id.
    #[must_use]
    pub fn identity_messages(&self) -> usize {
        self.blocks()
            .flat_map(|(_, b)| b.identities.iter())
            .filter(|identity| identity.message.is_some())
            .count()
    }

    /// How many blocks both begin dormant **and** carry a display identity.
    ///
    /// F39-E1's visibility measurement: the original's display role is declared
    /// independently of its dormancy, so a dormant block can be the one the
    /// player is shown. Whether the original shows it *while* dormant is
    /// unmeasured and stays so.
    #[must_use]
    pub fn dormant_identity_blocks(&self) -> usize {
        self.blocks()
            .filter(|(_, b)| b.begins_dormant() && !b.identities.is_empty())
            .count()
    }

    /// Every subject any `INACTIVE<n>` condition names, with how many
    /// declarations name it.
    #[must_use]
    pub fn condition_subjects(&self) -> Vec<(String, usize)> {
        let mut subjects: BTreeMap<String, usize> = BTreeMap::new();
        for (_, block) in self.blocks() {
            for condition in &block.conditions {
                *subjects.entry(condition.subject.clone()).or_insert(0) += 1;
            }
        }
        subjects.into_iter().collect()
    }

    /// Every attribute any `INACTIVE<n>` condition names, with how many
    /// declarations name it.
    ///
    /// Measured over the installation: exactly two spellings occur — `healthy`
    /// and `panels` — which is why the reader keeps them as text and no enum.
    #[must_use]
    pub fn condition_attributes(&self) -> Vec<(String, usize)> {
        let mut attributes: BTreeMap<String, usize> = BTreeMap::new();
        for (_, block) in self.blocks() {
            for condition in &block.conditions {
                if let Some(attribute) = &condition.attribute {
                    *attributes.entry(attribute.clone()).or_insert(0) += 1;
                }
            }
        }
        attributes.into_iter().collect()
    }

    /// Every part any `INACTIVE<n>` condition names, with how many declarations
    /// name it.
    ///
    /// Measured over the installation: 88 distinct spellings, which include the
    /// engine and gasbag node names of the actors' own airframe records next to
    /// the same words used without a part. None of them is decoded, so they stay
    /// text.
    #[must_use]
    pub fn condition_parts(&self) -> Vec<(String, usize)> {
        let mut parts: BTreeMap<String, usize> = BTreeMap::new();
        for (_, block) in self.blocks() {
            for condition in &block.conditions {
                if let Some(part) = &condition.part {
                    *parts.entry(part.clone()).or_insert(0) += 1;
                }
            }
        }
        parts.into_iter().collect()
    }

    /// The declared arities, with how many `INACTIVE<n>` declarations hold each.
    #[must_use]
    pub fn condition_arities(&self) -> Vec<(usize, usize)> {
        let mut arities: BTreeMap<usize, usize> = BTreeMap::new();
        for (_, block) in self.blocks() {
            for condition in &block.conditions {
                *arities.entry(condition.arity).or_insert(0) += 1;
            }
        }
        arities.into_iter().collect()
    }

    /// How many blocks declare the sound group they play when the objective
    /// activates.
    ///
    /// This is the population
    /// [`cue_ordered_dated_blocks`](Self::cue_ordered_dated_blocks) draws its
    /// controlled condition from, and it is what pins the measured spelling:
    /// a wrong key would read no block and quietly empty the controlled
    /// condition rather than fail.
    #[must_use]
    pub fn wakeup_sound_group_blocks(&self) -> usize {
        self.blocks()
            .filter(|(_, block)| block.wakeup_sound_group.is_some())
            .count()
    }

    /// How many blocks declare the sound group they play when the objective
    /// completes.
    ///
    /// Measured, and pinned like [`Self::wakeup_sound_group_blocks`]: the
    /// completion cue is a *declared* cue and nothing else, so a wrong spelling
    /// here would read as "no block declares one" instead of failing.
    #[must_use]
    pub fn completed_sound_group_blocks(&self) -> usize {
        self.blocks()
            .filter(|(_, block)| block.completed_sound_group.is_some())
            .count()
    }

    /// How many of the dated blocks also name the cue they play on activation.
    ///
    /// Measured: this is the population the two cue-ordered families are drawn
    /// from, and it is how many dated declarations are cross-checked by the
    /// original's own sound numbering.
    #[must_use]
    pub fn dated_wakeup_sound_group_blocks(&self) -> usize {
        self.blocks()
            .filter(|(_, block)| {
                block.dormant.is_some_and(|reading| !reading.is_sentinel())
                    && block.wakeup_sound_group.is_some()
            })
            .count()
    }

    /// Controlled condition B: the **condition ladders**.
    ///
    /// A ladder is two or more blocks in one mission whose `INACTIVE<n>`
    /// declarations are *identical* subject/part/attribute tuples while their
    /// `INACTIVE_COMPLETION_COUNT`s differ. Two blocks cannot share a condition
    /// set by accident, so the pair is the controlled condition for
    /// "`INACTIVE_COMPLETION_COUNT` is a threshold over *this* block's own
    /// conditions": if the count named anything else (a mission-wide counter, a
    /// stage index, another block's threshold) a designer would have had no
    /// reason to write the same fourteen conditions on four blocks with four
    /// different counts.
    ///
    /// This measures a *co-occurrence*, not a rule. The units the conditions
    /// count, whether satisfying one is monotone, and whether the ladder ends in
    /// the block that carries the display identity are all unmeasured.
    #[must_use]
    pub fn condition_ladders(&self) -> Vec<ConditionLadder> {
        let mut families: BTreeMap<
            (String, ConditionSignature),
            Vec<(&str, &MeasuredDormantBlock)>,
        > = BTreeMap::new();
        for (mission, block) in self.blocks() {
            if block.condition_count() == 0 {
                continue;
            }
            families
                .entry((mission.to_owned(), block.condition_signature()))
                .or_default()
                .push((mission, block));
        }
        families
            .into_iter()
            .filter(|(_, blocks)| blocks.len() >= 2)
            .map(|((mission, signature), blocks)| ConditionLadder {
                mission,
                condition_count: signature.len(),
                signature,
                rungs: blocks
                    .into_iter()
                    .map(|(_, block)| LadderRung {
                        block: block.block.clone(),
                        completion_count: block.completion_count,
                        begins_dormant: block.begins_dormant(),
                        carries_identity: !block.identities.is_empty(),
                    })
                    .collect(),
            })
            .collect()
    }

    /// Controlled condition A: the **cue-ordered dated blocks**.
    ///
    /// F39-E1's controlled condition for "`BEGIN_DORMANT`'s positive argument is
    /// an elapsed-time quantity". Two blocks in one mission whose
    /// [`OBJECTIVE_WAKEUP_SOUND_GROUP`](cs_content::objectives::OBJECTIVE_WAKEUP_SOUND_GROUP_KEY)
    /// names differ only in a trailing number are two cues the original's own
    /// sound library numbers in sequence; if the dormant argument ordered them
    /// the same way, it orders those blocks the way the designers numbered them,
    /// which a *count* of anything would not do. Only cue names with a common
    /// prefix and a trailing number are compared, so the test never invents an
    /// ordering for names that carry none.
    ///
    /// The population this is drawn from is measured, not assumed:
    /// 123 blocks declare an activation cue and 37 of the dated blocks declare
    /// one, and both figures are asserted by the retail suite.
    #[must_use]
    pub fn cue_ordered_dated_blocks(&self) -> Vec<CueOrderedFamily> {
        let mut groups: BTreeMap<(String, String), Vec<CueOrderedEntry>> = BTreeMap::new();
        for (mission, block) in self.blocks() {
            let (Some(cue), Some(DormantReading::ElapsedTime(argument))) =
                (&block.wakeup_sound_group, block.dormant)
            else {
                continue;
            };
            let Some((prefix, index)) = split_trailing_index(cue) else {
                continue;
            };
            groups
                .entry((mission.to_owned(), prefix.to_owned()))
                .or_default()
                .push(CueOrderedEntry {
                    index,
                    argument,
                    block: block.block.clone(),
                    cue: cue.clone(),
                });
        }
        groups
            .into_iter()
            .filter(|(_, entries)| entries.len() >= 2)
            .map(|((mission, prefix), mut entries)| {
                entries.sort_by_key(|entry| entry.index);
                let by_index: Vec<f32> = entries.iter().map(|entry| entry.argument).collect();
                let mut by_argument = entries.clone();
                by_argument.sort_by(|left, right| left.argument.total_cmp(&right.argument));
                let order_agrees = by_argument
                    .iter()
                    .map(|entry| entry.index)
                    .collect::<Vec<_>>()
                    == entries.iter().map(|entry| entry.index).collect::<Vec<_>>();
                CueOrderedFamily {
                    mission,
                    prefix,
                    entries,
                    order_agrees,
                    by_index,
                }
            })
            .collect()
    }
}

/// The trailing number of `name`, or `None` when there is none.
fn split_trailing_index(name: &str) -> Option<(&str, u32)> {
    let split = name
        .char_indices()
        .rev()
        .take_while(|(_, character)| character.is_ascii_digit())
        .last()
        .map(|(index, _)| index)?;
    if split == 0 {
        return None;
    }
    let (prefix, digits) = name.split_at(split);
    Some((prefix, digits.parse().ok()?))
}

/// One condition's measured subject, part and attribute, as the signature of a
/// [`ConditionLadder`].
pub type ConditionSignature = Vec<(String, Option<String>, Option<String>)>;

/// One rung of a [`ConditionLadder`]: a block that declares the ladder's shared
/// conditions with its own threshold.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LadderRung {
    /// The block's own key.
    pub block: String,
    /// The threshold it declares, when it declares one.
    pub completion_count: Option<u32>,
    /// Whether the block declares that it begins dormant.
    pub begins_dormant: bool,
    /// Whether the block carries a display identity.
    pub carries_identity: bool,
}

/// Controlled condition B's family: blocks sharing one condition set at
/// different thresholds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConditionLadder {
    /// The mission the family belongs to.
    pub mission: String,
    /// How many conditions the shared set holds.
    pub condition_count: usize,
    /// The shared condition set, in stage order.
    pub signature: ConditionSignature,
    /// The blocks declaring it, in the order the mission declares them (measured
    /// to be ascending block number).
    pub rungs: Vec<LadderRung>,
}

impl ConditionLadder {
    /// The distinct thresholds the family declares, sorted.
    #[must_use]
    pub fn thresholds(&self) -> Vec<u32> {
        let mut counts: Vec<u32> = self
            .rungs
            .iter()
            .filter_map(|rung| rung.completion_count)
            .collect();
        counts.sort_unstable();
        counts.dedup();
        counts
    }

    /// Whether every declared threshold is a real count over the shared set —
    /// no rung asks for more conditions than the set holds.
    #[must_use]
    pub fn thresholds_are_reachable(&self) -> bool {
        self.rungs.iter().all(|rung| {
            rung.completion_count
                .is_none_or(|count| count as usize <= self.condition_count)
        })
    }
}

/// One dated block inside a [`CueOrderedFamily`].
#[derive(Clone, Debug, PartialEq)]
pub struct CueOrderedEntry {
    /// The trailing number of the cue name.
    pub index: u32,
    /// The block's dormant argument.
    pub argument: f32,
    /// The block's own key.
    pub block: String,
    /// The cue name as measured.
    pub cue: String,
}

/// Controlled condition A's family: dated blocks whose cues are numbered in the
/// same order their arguments are.
#[derive(Clone, Debug, PartialEq)]
pub struct CueOrderedFamily {
    /// The mission the family belongs to.
    pub mission: String,
    /// The cue names' common prefix, without the trailing number.
    pub prefix: String,
    /// The blocks, ordered by their cue's trailing number.
    pub entries: Vec<CueOrderedEntry>,
    /// Whether that order is also the order of the dormant arguments.
    pub order_agrees: bool,
    /// The arguments in cue-index order, which is what the family compares.
    pub by_index: Vec<f32>,
}

/// Measures every mission-scoped objective record's dormant/reveal
/// declarations in `install_root`.
///
/// Read-only: the walk is production discovery, so nothing inside the
/// installation is written. A mission whose archive cannot be read, whose
/// record does not decode, or whose block declares a shape F39-E1 never
/// measured is a **named refusal**, never a skipped row.
///
/// # Errors
///
/// [`DormantCensusError`] in every case; see each variant.
pub fn survey_retail_dormant_reveal(
    install_root: &Path,
) -> Result<DormantRevealCensus, DormantCensusError> {
    let found = cs_assets::install::discover(install_root)
        .map_err(|error| DormantCensusError::Discovery(error.to_string()))?;
    let install_sha256 = cs_assets::install::fingerprint(&found.manifest).to_hex();
    let located = locate_mission_objective_records(&found).map_err(|error| match error {
        ObjectiveCensusError::Discovery(reason) => DormantCensusError::Discovery(reason),
        ObjectiveCensusError::Read { container, reason } => {
            DormantCensusError::Read { container, reason }
        }
        ObjectiveCensusError::Decode {
            container,
            code,
            offset,
        } => DormantCensusError::Decode {
            container,
            code,
            offset,
        },
    })?;

    let mut rows: Vec<DormantRevealRow> = Vec::new();
    for record in located {
        let blocks =
            measure_dormant_declarations(&record.document).map_err(|error: DormantReadError| {
                DormantCensusError::Declaration {
                    mission: record.mission.clone(),
                    block: error.block().to_owned(),
                    reason: error.to_string(),
                }
            })?;
        rows.push(DormantRevealRow {
            mission: record.mission,
            container: record.container,
            container_sha256: record.container_sha256,
            member: record.member,
            member_sha256: record.member_sha256,
            blocks,
        });
    }
    rows.sort_by(|left, right| left.mission.cmp(&right.mission));
    Ok(DormantRevealCensus {
        install_sha256,
        rows,
    })
}
