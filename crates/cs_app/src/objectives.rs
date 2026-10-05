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
    MeasuredCountConditions, MeasuredDormantBlock, MeasuredRepeatedEffect, MeasuredTargetKinds,
    OBJECTIVE_INACTIVE_COUNT_KEY, ProgramActor, ProgramSymbol, UnmeasuredQuantity,
    is_objective_inactive_stage, is_optional_objective_key, measure_dormant_declarations,
    measured_category_evidence,
};
use cs_content::stunts::{
    OBJECTIVE_BLOCK_PREFIX, SCENARIO_OBJECTIVES_MEMBER, SCENARIO_TARGETS_MEMBER,
    TARGET_CATEGORY_KEY, TARGET_HELP_KEY, ZrdValue, objective_record, objective_record_count,
    objective_record_keys, zrd_directive_fields, zrd_field, zrd_flat_fields,
};
use cs_script::ir::{ActorId, SymbolId};
use cs_script::runtime::SessionGeneration;
use cs_sim::objectives::bailout::BailoutRefusal;
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
    /// A lifecycle transition produced no mission transition (F29-C.4). The
    /// actor's record keeps the transition it had already reached.
    ///
    /// Reported rather than dropped, because "a destruction arrived after a
    /// bailout and was refused" is a fact no consumer can recover by reading the
    /// counters: the counter simply never moved.
    Transition {
        /// Why nothing was recorded.
        reason: BailoutRefusal,
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
                // A confirmed pilot bailout is reported, not applied: the
                // airframe is still in the world (F30-A's measured contract is
                // that a bailout does not end targetability), so it stays in the
                // registry and this session's to tear down. What the mission
                // does with it is a declared [`CountReaction`]'s business, and
                // the unmeasured bailout policy requests nothing.
                ObjectiveEventKind::PilotBailedOut { .. } => {}
                // A refused transition is named, never dropped: the stream says
                // the world reported a destruction after a bailout and the
                // ledger kept the bailout, which is what a consumer can never
                // reconstruct by reading the counters alone.
                ObjectiveEventKind::TransitionRefused { reason, .. } => {
                    refusals.push(SessionRefusal::Transition { reason: *reason })
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
        let sites: Vec<MeasuredBranchSite> = zrd_directive_fields(block_value)
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
        // A block that spells **one** effect key twice is a different
        // unmeasured shape from a conflict — not "which of two effects wins"
        // but "what does the second site of *one* key do" — so it is recorded
        // per (block, kind) with every site in authored order, and gets its own
        // named verdict
        // ([`cs_content::objectives::UNMEASURED_REPEATED_EFFECT_KEY`]). It is
        // never folded into the multi-effect/conflict counting below, which is
        // over pairs of *different* effects. Measured zero over the whole
        // readable corpus (F39-E6): the 1338 mission blocks and every member of
        // the excluded reader archives spell it nowhere — so a repeat must be
        // told to an importer as an unresolved shape, not silently reported as
        // "two sites, one effect".
        for kind in BranchEffectKind::all() {
            let repeated: Vec<MeasuredBranchSite> = sites
                .iter()
                .filter(|site| site.kind == kind)
                .cloned()
                .collect();
            if repeated.len() >= 2 {
                measured.repeated_effects.push(MeasuredRepeatedEffect {
                    block: block.to_owned(),
                    kind,
                    sites: repeated,
                });
            }
        }
        // A **multi-effect** block declares two or more *different* effects, and
        // the whole question is which of two effects wins, so the counting below
        // is over pairs of different effects. The repeated-key reading above is
        // recorded beside it: a block that both repeats a key and declares
        // another effect keeps both questions, each under its own verdict.
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
        for (field, value) in zrd_directive_fields(block_value) {
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
///
/// The member's root is the list of records, exactly as
/// [`cs_content::stunts::objective_record_count`] already reads it: a member
/// whose root is not a list declares **no** records rather than being read as
/// one, so the reading never invents a record the member does not carry.
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

/// A repeated completion-effect key measured in one mission's record, named
/// with the mission it came from (F39-E6).
///
/// Kept beside [`RetailBranchConflict`] and deliberately not folded into it: a
/// repeat is one *kind* spelled twice in one block, a different unmeasured
/// shape from two different kinds sharing a target.
#[derive(Clone, Debug, PartialEq)]
pub struct RetailRepeatedEffect {
    /// The mission, as `zbd/<group>/<mission>`.
    pub mission: String,
    /// The condition measured inside that mission's record.
    pub repeated: MeasuredRepeatedEffect,
}

impl RetailRepeatedEffect {
    /// The mission's `zbd/<group>/<mission>` label with the block and kind,
    /// as one locatable string (`zbd/c3/m05 OBJECTIVE8: WAKE x2`).
    #[must_use]
    pub fn label(&self) -> String {
        format!("{} {}", self.mission, self.repeated.label())
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
    ///
    /// **What this is not.** It is the mission archive's own reading, not the
    /// mission's: F39-E3 measured the **installation-scope** readers
    /// ([`RetailScopeObjectiveCensus`]) and found the `c1c` **world-group**
    /// reader declaring five objective target records of its own — in the very
    /// world group of this one mission. Whether the mission resolves its group's
    /// or the install-wide reader is reader-archive precedence (F04/F06) and is
    /// unmeasured, so this `None` stays a measured absence in the mission archive
    /// and must not be read as "this mission has no objective targets".
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

    /// This row's measured repeated completion-effect keys, named with the
    /// mission (F39-E6).
    #[must_use]
    pub fn repeated_effects(&self) -> Vec<RetailRepeatedEffect> {
        self.branch_precedence
            .repeated_effects
            .iter()
            .cloned()
            .map(|repeated| RetailRepeatedEffect {
                mission: self.mission.clone(),
                repeated,
            })
            .collect()
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

    /// The missions whose **own archive** declares no `targets.zrd` member, so the
    /// F39-E4 target surface's denominator is stated rather than assumed.
    ///
    /// Measured over the owner's installation: `c1c/m01` carries an
    /// `objectives.zrd` with 24012 bytes of objective blocks and no target
    /// record, so **its own archive's** objective kinds are **unmeasured**, not
    /// empty.
    ///
    /// F39-E3 bounded this row from outside: the `c1c` world-group reader
    /// declares five objective target records of its own
    /// ([`RetailScopeObjectiveCensus::objective_target_scopes`]), and `c1c/m01`
    /// is a mission of that world group. So this list is the mission-scoped
    /// absence only; whether the mission inherits its group's record is
    /// reader-archive precedence (F04/F06), unmeasured, and it must not be read
    /// as "the mission has no objective targets".
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

    // ------------------------------------------------------------ F39-E6 ---

    /// Every measured repeated completion-effect key corpus-wide, named with
    /// the mission and the block it sits in, sorted by mission, block and
    /// kind.
    ///
    /// A different unmeasured shape from [`Self::conflicts`]: a repeat is one
    /// key spelled twice in one block, not two different effects sharing a
    /// target.
    #[must_use]
    pub fn repeated_effects(&self) -> Vec<RetailRepeatedEffect> {
        let mut repeated: Vec<RetailRepeatedEffect> = self
            .rows
            .iter()
            .flat_map(RetailObjectiveRow::repeated_effects)
            .collect();
        repeated.sort_by(|left, right| {
            left.mission
                .cmp(&right.mission)
                .then_with(|| left.repeated.block.cmp(&right.repeated.block))
                .then_with(|| left.repeated.kind.cmp(&right.repeated.kind))
        });
        repeated
    }

    /// How many **blocks** corpus-wide spell one completion-effect key two or
    /// more times.
    ///
    /// Blocks, not repeats: one block can spell two different keys twice each,
    /// so counting repeats instead would over-report the blocks an importer
    /// has to handle.
    #[must_use]
    pub fn repeated_effect_blocks(&self) -> u32 {
        self.rows
            .iter()
            .map(|row| row.branch_precedence.repeated_effect_blocks())
            .sum()
    }

    /// Whether the installation's objective records declare the repeated-key
    /// shape anywhere: one block spelling one completion-effect key twice.
    ///
    /// The corpus-wide form of
    /// [`MeasuredBranchPrecedence::needs_unmeasured_repeated_effect`]. Measured
    /// `false` over the owner's installation — the corpus never writes the
    /// shape, which bounds the corpus but is not evidence the original refuses
    /// it, so the verdict below stays a named unknown rather than becoming a
    /// "supported" reading.
    #[must_use]
    pub fn needs_unmeasured_repeated_effect(&self) -> bool {
        self.repeated_effect_blocks() > 0
    }

    /// Why a repeated completion-effect key is unresolved corpus-wide, by
    /// name, or `None` when no block spells one.
    #[must_use]
    pub fn unmeasured_repeated_effect_reason(&self) -> Option<&'static str> {
        self.needs_unmeasured_repeated_effect()
            .then_some(cs_content::objectives::UNMEASURED_REPEATED_EFFECT_KEY)
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
// F39-E6: the objective records outside the mission census
// ---------------------------------------------------------------------------
//
// F39-D's census is scoped by `mission_scope` — exactly
// `zbd/<group>/<mission>/zrdr.zbd` — so the shared reader `zbd/zrdr.zbd` and
// the eight world-group readers `zbd/<group>/zrdr.zbd` were never opened, and
// no `targets.zrd` member of any archive was either: the walk reads
// `objectives.zrd` only. F39-E6's question is whether the *repeated
// completion-effect key* shape hides in that excluded corpus, so this survey
// walks it with the same production readers the census uses:
//
// * every member of every `zrdr.zbd` archive `mission_scope` does not name is
//   decoded and measured — 9 archives and 612 members on the owner's
//   installation;
// * every `targets.zrd` member of every reader archive is decoded and its
//   objective records counted — 53 members and 332 records, including the
//   world-group `zbd/c1c/zrdr.zbd` one.
//
// Per member two measurements are taken: the key *spellings* anywhere in the
// decoded tree (a member does not have to be record-shaped to carry the
// shape), and the per-block [`measure_block_precedence`] reading for members
// that do declare `OBJECTIVE<N>` blocks. A member that cannot be located or
// decoded is an error, never a skipped row — a member vanishing from the
// denominator would look like a member that spells nothing.

/// Why a member is part of the excluded-corpus survey.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ExcludedObjectiveScope {
    /// A member of a reader archive `mission_scope` does not name: the shared
    /// reader `zbd/zrdr.zbd` or a world-group reader `zbd/<group>/zrdr.zbd`.
    OutsideMissionScope,
    /// A `targets.zrd` member of any reader archive — the objective record
    /// the `objectives.zrd` walk never opens, in whichever archive holds it.
    TargetsRecord,
}

/// The `targets.zrd` reading of one member: how many objective records it
/// declares and the complete key vocabulary those records use.
#[derive(Clone, Debug, PartialEq)]
pub struct ExcludedTargetsReading {
    /// How many objective records the member declares
    /// (`objective_record_count`).
    pub records: u32,
    /// The complete key inventory of those records, sorted by key, with the
    /// number of records each appears in (`objective_record_keys`).
    pub keys: Vec<(String, u32)>,
}

/// One `.zrd` member of the corpus outside the mission-scoped
/// `objectives.zrd` census, measured for completion-effect spellings.
#[derive(Clone, Debug, PartialEq)]
pub struct ExcludedObjectiveRow {
    /// The archive's installation spelling (`ZBD/zrdr.zbd`,
    /// `ZBD/C1C/zrdr.zbd`, `ZBD/C1/M02/zrdr.zbd`, …).
    pub container: String,
    /// SHA-256 of the whole archive, from production discovery.
    pub container_sha256: String,
    /// The member's name as the archive spells it.
    pub member: String,
    /// The absolute offset of the member's first byte inside the archive.
    pub member_offset: u64,
    /// The member's length in bytes.
    pub member_len: u64,
    /// SHA-256 of the member's own bytes.
    pub member_sha256: String,
    /// Why this member is in the census — a world-group `targets.zrd` member
    /// carries both scopes.
    pub scopes: BTreeSet<ExcludedObjectiveScope>,
    /// Completion-effect key spellings anywhere in the member's decoded tree
    /// ([`cs_content::objectives::BRANCH_EFFECT_KEY_VOCABULARY`]).
    ///
    /// Counted over *every* text node of the tree, not only flat-field
    /// position: a `0` here is a measurement that the member does not even
    /// spell a completion-effect key anywhere, not the narrower "no block
    /// carries one".
    pub effect_key_sites: u32,
    /// `TICK_DEPENDS_ON_OBJ` spellings anywhere in the member's decoded tree,
    /// on the same terms as [`Self::effect_key_sites`].
    pub order_key_sites: u32,
    /// The member's per-block completion-effect reading over the `OBJECTIVE<N>`
    /// blocks it declares — zero blocks for a member that is not an objectives
    /// record.
    pub precedence: MeasuredBranchPrecedence,
    /// The `targets.zrd` reading, present exactly when
    /// [`ExcludedObjectiveScope::TargetsRecord`] is in `scopes`.
    pub targets: Option<ExcludedTargetsReading>,
}

/// The measured `.zrd` members of the corpus outside the mission-scoped
/// `objectives.zrd` census (F39-E6).
#[derive(Clone, Debug, PartialEq)]
pub struct ExcludedObjectiveCensus {
    install_sha256: String,
    /// The reader archives the survey measured that `mission_scope` does not
    /// name, as installation spellings — the shared reader and the
    /// world-group readers.
    archives_outside_mission_scope: Vec<String>,
    rows: Vec<ExcludedObjectiveRow>,
}

impl ExcludedObjectiveCensus {
    /// SHA-256 of the whole installation manifest, from production discovery.
    #[must_use]
    pub fn install_sha256(&self) -> &str {
        &self.install_sha256
    }

    /// The reader archives measured that `mission_scope` does not name, as
    /// installation spellings, sorted.
    #[must_use]
    pub fn archives_outside_mission_scope(&self) -> &[String] {
        &self.archives_outside_mission_scope
    }

    /// The measured member rows, sorted by container and member.
    #[must_use]
    pub fn rows(&self) -> &[ExcludedObjectiveRow] {
        &self.rows
    }

    /// How many members the survey measured.
    #[must_use]
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// Whether the survey measured nothing.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// How many measured members carry `scope`.
    #[must_use]
    pub fn members_in_scope(&self, scope: ExcludedObjectiveScope) -> u32 {
        self.rows
            .iter()
            .filter(|row| row.scopes.contains(&scope))
            .count() as u32
    }

    /// Total completion-effect key spellings anywhere in every measured
    /// member's tree.
    #[must_use]
    pub fn effect_key_sites(&self) -> u32 {
        self.rows.iter().map(|row| row.effect_key_sites).sum()
    }

    /// Total `TICK_DEPENDS_ON_OBJ` spellings anywhere in every measured
    /// member's tree.
    #[must_use]
    pub fn order_key_sites(&self) -> u32 {
        self.rows.iter().map(|row| row.order_key_sites).sum()
    }

    /// How many `OBJECTIVE<N>` blocks the measured members declare in all.
    #[must_use]
    pub fn objective_blocks(&self) -> u32 {
        self.rows.iter().map(|row| row.precedence.blocks).sum()
    }

    /// How many measured members declare at least one `OBJECTIVE<N>` block.
    #[must_use]
    pub fn members_with_objective_blocks(&self) -> u32 {
        self.rows
            .iter()
            .filter(|row| row.precedence.blocks > 0)
            .count() as u32
    }

    /// How many blocks in the excluded corpus spell one completion-effect key
    /// two or more times.
    #[must_use]
    pub fn repeated_effect_blocks(&self) -> u32 {
        self.rows
            .iter()
            .map(|row| row.precedence.repeated_effect_blocks())
            .sum()
    }

    /// Whether the excluded corpus declares the repeated-key shape anywhere.
    ///
    /// Measured `false` over the owner's installation — see
    /// [`RetailObjectiveCensus::needs_unmeasured_repeated_effect`] for why the
    /// `false` bounds the corpus rather than settling the rule.
    #[must_use]
    pub fn needs_unmeasured_repeated_effect(&self) -> bool {
        self.repeated_effect_blocks() > 0
    }

    /// Why a repeated completion-effect key in this corpus is unresolved, by
    /// name, or `None` when no member spells one.
    #[must_use]
    pub fn unmeasured_repeated_effect_reason(&self) -> Option<&'static str> {
        self.needs_unmeasured_repeated_effect()
            .then_some(cs_content::objectives::UNMEASURED_REPEATED_EFFECT_KEY)
    }

    /// How many objective records the measured `targets.zrd` members declare
    /// in all.
    #[must_use]
    pub fn targets_records(&self) -> u32 {
        self.rows
            .iter()
            .filter_map(|row| row.targets.as_ref().map(|targets| targets.records))
            .sum()
    }
}

/// Measures the `.zrd` members outside the mission-scoped `objectives.zrd`
/// census: every member of the reader archives `mission_scope` does not name,
/// and every `targets.zrd` member of every reader archive.
///
/// Read-only, the same rule as [`survey_retail_objective_records`], and the
/// census **fails** rather than skipping a member that cannot be read or
/// decoded, for the same reason: a member silently absent from the denominator
/// would look like a member that spells nothing.
///
/// # Errors
///
/// [`ObjectiveCensusError::Discovery`] when the installation cannot be
/// discovered, [`ObjectiveCensusError::Read`] when an archive cannot be read
/// or one of its members could not be located, and
/// [`ObjectiveCensusError::Decode`] for the first member whose bytes do not
/// decode as `.zrd`.
pub fn survey_excluded_objective_records(
    install_root: &Path,
) -> Result<ExcludedObjectiveCensus, ObjectiveCensusError> {
    let found = cs_assets::install::discover(install_root)
        .map_err(|error| ObjectiveCensusError::Discovery(error.to_string()))?;
    let install_sha256 = cs_assets::install::fingerprint(&found.manifest).to_hex();

    let mut archives_outside_mission_scope = Vec::new();
    let mut rows = Vec::new();
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
        let outside_mission_scope = cs_formats::script_raw::mission_scope(&path).is_none();
        let bytes = std::fs::read(found.manifest.host_root.join(&spelling)).map_err(|error| {
            ObjectiveCensusError::Read {
                container: container_key.clone(),
                reason: error.to_string(),
            }
        })?;
        let discovery = cs_formats::script_raw::discover_container(&container_key, &path, &bytes);
        if let Some(finding) = discovery.findings().first() {
            return Err(ObjectiveCensusError::Read {
                container: container_key.clone(),
                reason: format!("a member could not be located: {finding}"),
            });
        }
        for program in discovery.programs() {
            let locator = program.locator();
            let member = locator.member().ok_or_else(|| ObjectiveCensusError::Read {
                container: container_key.clone(),
                reason: "a member was located without a name".to_owned(),
            })?;
            let mut scopes = BTreeSet::new();
            if outside_mission_scope {
                scopes.insert(ExcludedObjectiveScope::OutsideMissionScope);
            }
            if member.eq_ignore_ascii_case(SCENARIO_TARGETS_MEMBER) {
                scopes.insert(ExcludedObjectiveScope::TargetsRecord);
            }
            if scopes.is_empty() {
                continue;
            }
            let document = cs_content::stunts::decode_zrd(program.bytes()).map_err(|error| {
                ObjectiveCensusError::Decode {
                    container: container_key.clone(),
                    code: error.code(),
                    offset: error.offset(),
                }
            })?;
            let (effect_key_sites, order_key_sites) = branching_spellings(&document);
            let targets = scopes
                .contains(&ExcludedObjectiveScope::TargetsRecord)
                .then(|| ExcludedTargetsReading {
                    records: objective_record_count(&document),
                    keys: objective_record_keys(&document),
                });
            rows.push(ExcludedObjectiveRow {
                container: spelling.clone(),
                container_sha256: record.sha256.to_hex(),
                member: member.to_owned(),
                member_offset: locator.span().offset,
                member_len: locator.span().len,
                member_sha256: cs_assets::install::sha256(program.bytes()).to_hex(),
                scopes,
                effect_key_sites,
                order_key_sites,
                precedence: measure_block_precedence(&document),
                targets,
            });
        }
        if outside_mission_scope {
            archives_outside_mission_scope.push(spelling);
        }
    }
    archives_outside_mission_scope.sort();
    rows.sort_by(|left, right| {
        left.container
            .cmp(&right.container)
            .then_with(|| left.member.cmp(&right.member))
    });
    Ok(ExcludedObjectiveCensus {
        install_sha256,
        archives_outside_mission_scope,
        rows,
    })
}

/// How often a decoded member spells a measured branching key **anywhere** in
/// its tree: `(completion-effect sites, order-dependency sites)`.
///
/// The walk counts every text node, wherever it sits — a member need not be
/// record-shaped to be measured, so a zero here means the member does not even
/// spell the key, not merely that no `OBJECTIVE<N>` block carries it.
fn branching_spellings(document: &ZrdValue) -> (u32, u32) {
    let mut pending = vec![document];
    let (mut effects, mut orders) = (0u32, 0u32);
    while let Some(node) = pending.pop() {
        match node {
            ZrdValue::List(children) => pending.extend(children.iter()),
            ZrdValue::Text(text) if BRANCH_EFFECT_KEY_VOCABULARY.contains(&text.as_str()) => {
                effects += 1;
            }
            ZrdValue::Text(text) if text == BRANCH_ORDER_KEY => {
                orders += 1;
            }
            _ => {}
        }
    }
    (effects, orders)
}

// ---------------------------------------------------------------------------
// F39-E1: the dormant/reveal census
// ---------------------------------------------------------------------------
//
// F39-D counted the declarations (`BEGIN_DORMANT` in 1118 of 1338 blocks (F39-D published 1096 from a walk that swallowed the key after a bare directive; F39-D-COUNT), an
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

// ---------------------------------------------------------------------------
// F39-E7: the contract's six condition distinctions
// ---------------------------------------------------------------------------
//
// `docs/contracts/SCRIPT-MISSION.md` ("Objective event ordering") requires
// conditions to distinguish **six** things: "disabled, dead, captured, escaped,
// detached and despawned". The engine's counter vocabulary declares **five**
// ([`CountKind`], [`DeclaredCountKind`]) — `dead` is spelled `destroyed`, and
// `detached` has no entry at all. Nothing in this project had said where the
// sixth lives, which left the five-category vocabulary readable as if it
// satisfied the contract's six.
//
// This section answers that as a **query**, not a table:
// [`contract_condition_distinctions`] resolves each of the six against the
// engine and reports, for one, the vocabulary entry a `Condition::ActorIs` uses,
// the counter category it counts as, the lifecycle transition that produces that
// category, and — where there is no producer — a **named** reason from this
// module. Nothing is inferred: every field is read out of the enum it names,
// which is why the answer cannot go stale without a compile error in the table.
//
// # The two verdicts, and what they are not
//
// [`NO_LIFECYCLE_TRANSITION_REPORTS_THIS_CATEGORY`] is the structural fact for
// `disabled` and `escaped`: the runtime's only producer for a [`CountKind`] is
// the `TickInput::lifecycles` transition [`CountKind::from_lifecycle`] maps to
// it, and none maps to those two.
//
// [`DETACHED_IS_AN_EVENT_NOT_A_COUNTED_CATEGORY`] is F39-E7's measured verdict,
// and it is a claim about the **original's declarations**, not about the game:
// [`survey_retail_detached_declarations`] measures every reader archive in the
// installation — the 53 mission-scoped ones and the 9 shared/world-group ones
// F39-D's census excluded — and finds no counted condition and no objective kind
// spelling a detached category. It also measures that **all nine** shared and
// world-group readers declare no `objectives.zrd` member, so they contribute no
// counted condition and no block declaration — the measured answer to the wider
// denominator F39-D left open (unknown #5) for this question. The original *does*
// write detaches, in the vocabulary of an objective block's own triggers:
// `zbd/c2/m05 OBJECTIVE23` carries `WAKE_ANIM drop_paratroopers` with no stage
// and no threshold beside it, so the drop **completes** that objective rather
// than counting detached actors.
//
// A detached actor is therefore represented by the release/attach path, which
// keeps objective identity rather than ending the actor's life:
// [`cs_sim::world_actors::release::release_payload`] carries
// `objective: Option<SymbolId>` across the release (F34 non-negotiable 4), and
// the authored detach op is [`cs_content::animation::AttachmentOp::Detach`]
// (F20-C `AC03`). The lifecycle transition that removes an actor from mission
// accounting without counting it is
// [`cs_sim::damage::LifecycleKind::MissionRemoved`], which
// [`CountKind::from_lifecycle`] answers `None` for.
//
// **Not decided here.** Whether an original *record* may declare one of the five
// counted categories at all is decided in one place —
// `cs_content::objectives::original_count_category_refusal` (F39-E4) — from its
// own corpus census. This section does not restate that census and does not
// widen it; it only adds the sixth distinction the corpus was never asked about.

use cs_content::objectives::{
    DetachedVocabularySurface, MeasuredDetachedVocabulary, measure_detached_vocabulary,
};
use cs_script::ir::ActorState;
use cs_sim::damage::LifecycleKind;

/// No `LifecycleKind` transition reports the category, so nothing in the
/// runtime can count it.
///
/// This is [`CountKind::producer`] answering `None`, and it is the whole
/// structural reason: `disabled` and `escaped` are reachable only from a caller
/// that names the category itself. Which categories the original's records
/// *declare* is a separate, measured question that F39-E4 answers in
/// `cs_content::objectives`; this constant says nothing about it.
pub const NO_LIFECYCLE_TRANSITION_REPORTS_THIS_CATEGORY: &str =
    "no_lifecycle_transition_reports_this_category";

/// F39-E7's measured verdict: the original writes a detach as an **event**,
/// never as a counted category.
///
/// Measured over every reader archive in the owner's installation (53
/// mission-scoped plus the 9 shared/world-group readers F39-D's census
/// excluded), across the three surfaces
/// [`cs_content::objectives::DetachedVocabularySurface`]: no counted condition
/// (`INACTIVE<n>` beside an `INACTIVE_COMPLETION_COUNT`) and no `targets.zrd`
/// objective kind spells a detached category, over the published stem list
/// [`cs_content::objectives::DETACHED_SPELLING_STEMS`]. The corpus *does* write
/// the detach in its block-declaration vocabulary — `zbd/c2/m05 OBJECTIVE23`
/// carries `WAKE_ANIM drop_paratroopers`, with no stage and no threshold beside
/// it — and that block **completes** on the drop rather than counting detached
/// actors.
///
/// A negative over these surfaces is a measured absence of the *spelling*, not a
/// proof the original has no such notion: the compiled program behind each record
/// is undecoded (F13-B/C, F38 own the instruction table) and no original
/// executable has been run. Evidence, contrary hypotheses and the limits are in
/// `docs/findings/2026-10-04-f39-e7-detached-condition-vocabulary.md`.
pub const DETACHED_IS_AN_EVENT_NOT_A_COUNTED_CATEGORY: &str =
    "detached_is_an_event_not_a_counted_category";

/// One of the six distinctions `docs/contracts/SCRIPT-MISSION.md` requires
/// conditions to keep apart, in the contract's own order and spelling.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ContractDistinction {
    /// The contract's `disabled`.
    Disabled,
    /// The contract's `dead`, which the counter vocabulary spells `destroyed`.
    Dead,
    /// The contract's `captured`.
    Captured,
    /// The contract's `escaped`.
    Escaped,
    /// The contract's `detached`: the sixth, with no counter category.
    Detached,
    /// The contract's `despawned`.
    Despawned,
}

impl ContractDistinction {
    /// All six, in the contract's order.
    pub const ALL: &'static [Self] = &[
        Self::Disabled,
        Self::Dead,
        Self::Captured,
        Self::Escaped,
        Self::Detached,
        Self::Despawned,
    ];

    /// The contract's own lowercase word.
    #[must_use]
    pub const fn contract_spelling(self) -> &'static str {
        match self {
            Self::Disabled => "disabled",
            Self::Dead => "dead",
            Self::Captured => "captured",
            Self::Escaped => "escaped",
            Self::Detached => "detached",
            Self::Despawned => "despawned",
        }
    }

    /// The `cs_script::ir::ActorState` a `Condition::ActorIs` compares against,
    /// which is the vocabulary the contract's sentence is written about.
    ///
    /// All six distinctions resolve here, including [`Self::Detached`]: the
    /// script IR declares all six states. **Nothing in the engine writes one
    /// yet**, `detached` included — the states arrive as a
    /// `cs_script::runtime::MissionFacts` the simulation's caller supplies, and
    /// every caller in this repository passes an empty map, so a
    /// `Condition::ActorIs` cannot be satisfied for any of the six today. That
    /// is one limit on the whole vocabulary (populating `MissionFacts` is
    /// F37/F38's remaining mission-runtime work), not a property of the sixth,
    /// and it is why [`Self::unproduced_reason`] is about the **counted
    /// category** rather than about this state.
    #[must_use]
    pub const fn condition_state(self) -> ActorState {
        match self {
            Self::Disabled => ActorState::Disabled,
            Self::Dead => ActorState::Dead,
            Self::Captured => ActorState::Captured,
            Self::Escaped => ActorState::Escaped,
            Self::Detached => ActorState::Detached,
            Self::Despawned => ActorState::Despawned,
        }
    }

    /// The counter category this distinction counts as, when the declared
    /// vocabulary has one.
    ///
    /// [`Self::Detached`] returns `None`: the declared vocabulary declares five
    /// categories and none of them is a detach, which is what
    /// [`DETACHED_IS_AN_EVENT_NOT_A_COUNTED_CATEGORY`] measured.
    #[must_use]
    pub const fn declared_kind(self) -> Option<DeclaredCountKind> {
        match self {
            Self::Disabled => Some(DeclaredCountKind::Disabled),
            Self::Dead => Some(DeclaredCountKind::Destroyed),
            Self::Captured => Some(DeclaredCountKind::Captured),
            Self::Escaped => Some(DeclaredCountKind::Escaped),
            Self::Detached => None,
            Self::Despawned => Some(DeclaredCountKind::Despawned),
        }
    }

    /// The counted category this distinction counts as, or `None` when the
    /// counter vocabulary has none.
    ///
    /// Read through the production lowering, so this cannot claim a category
    /// the session could not actually count.
    #[must_use]
    pub fn counted_kind(self) -> Option<CountKind> {
        self.declared_kind().map(lower_kind)
    }

    /// The lifecycle transition that produces this distinction's counted
    /// category, or `None` when nothing does.
    ///
    /// Derived from [`CountKind::producer`], the only producer question the
    /// engine has.
    #[must_use]
    pub fn producer(self) -> Option<LifecycleKind> {
        self.counted_kind().and_then(CountKind::producer)
    }

    /// The **named** reason this distinction has no producer, or `None` when it
    /// has one.
    ///
    /// A `None` here is never a silent gap: it is either
    /// [`NO_LIFECYCLE_TRANSITION_REPORTS_THIS_CATEGORY`] or
    /// [`DETACHED_IS_AN_EVENT_NOT_A_COUNTED_CATEGORY`], and a caller that needs
    /// to print *why* prints the constant rather than the absence.
    #[must_use]
    pub const fn unproduced_reason(self) -> Option<&'static str> {
        match self {
            Self::Disabled | Self::Escaped => Some(NO_LIFECYCLE_TRANSITION_REPORTS_THIS_CATEGORY),
            Self::Detached => Some(DETACHED_IS_AN_EVENT_NOT_A_COUNTED_CATEGORY),
            Self::Dead | Self::Captured | Self::Despawned => None,
        }
    }

    /// Whether the engine has both a vocabulary entry and a producer for this
    /// distinction today.
    #[must_use]
    pub const fn is_counted_with_a_producer(self) -> bool {
        self.unproduced_reason().is_none()
    }
}

/// One of the six, resolved against the engine. Every field is read out of the
/// enum it names; none of them is a restatement of another.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ContractDistinctionReading {
    /// Which of the six this is.
    pub distinction: ContractDistinction,
    /// The contract's own word.
    pub contract_spelling: &'static str,
    /// The `ActorState` a `Condition::ActorIs` compares against.
    pub condition_state: ActorState,
    /// The declared counter category, when there is one.
    pub declared: Option<DeclaredCountKind>,
    /// The lowered counter category the session would count, when there is one.
    pub counted: Option<CountKind>,
    /// The lifecycle transition that produces it, when one does.
    pub producer: Option<LifecycleKind>,
    /// The named reason there is no producer, or `None`.
    pub unproduced_reason: Option<&'static str>,
}

/// The contract's six distinctions, each with a vocabulary entry and a producer
/// or a named reason it has none.
///
/// The single queryable answer to "does the engine implement the contract's six
/// distinctions?".
///
/// Exhaustiveness is a **compile-time** property, and it comes from the
/// per-distinction queries rather than from anything here:
/// [`ContractDistinction::contract_spelling`],
/// [`ContractDistinction::condition_state`],
/// [`ContractDistinction::declared_kind`] and
/// [`ContractDistinction::unproduced_reason`] are `match`es over the enum with
/// no catch-all arm, so a seventh variant cannot be added without deciding its
/// contract word, its condition state, its declared category and its reason —
/// and every cell of the returned table is read out of one of those queries, so
/// a cell cannot go stale either. [`ContractDistinction::ALL`] is the order the
/// table is built in, and its length is the contract's six; the acceptance
/// suite pins that against the six words the contract's sentence spells.
#[must_use]
pub fn contract_condition_distinctions() -> Vec<ContractDistinctionReading> {
    ContractDistinction::ALL
        .iter()
        .map(|distinction| {
            let declared = distinction.declared_kind();
            let counted = distinction.counted_kind();
            ContractDistinctionReading {
                distinction: *distinction,
                contract_spelling: distinction.contract_spelling(),
                condition_state: distinction.condition_state(),
                declared,
                counted,
                producer: counted.and_then(CountKind::producer),
                unproduced_reason: distinction.unproduced_reason(),
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// F39-E7: the measured vocabulary census
// ---------------------------------------------------------------------------

/// Why the detached-vocabulary census could not be produced.
#[derive(Clone, Debug, PartialEq)]
pub enum DetachedCensusError {
    /// The installation could not be discovered.
    Discovery(String),
    /// A reader archive could not be read or does not decode.
    Read {
        /// The archive's logical key.
        archive: String,
        /// Why it refused.
        reason: String,
    },
    /// A member's bytes do not decode.
    Decode {
        /// The archive's logical key.
        archive: String,
        /// The member that refused.
        member: String,
        /// The decoder's code.
        code: String,
        /// The byte offset it refused at.
        offset: u64,
    },
}

impl std::fmt::Display for DetachedCensusError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Discovery(reason) => write!(f, "installation discovery failed: {reason}"),
            Self::Read { archive, reason } => write!(f, "{archive}: {reason}"),
            Self::Decode {
                archive,
                member,
                code,
                offset,
            } => write!(
                f,
                "{archive}/{member}: does not decode ({code} at byte {offset})"
            ),
        }
    }
}

impl std::error::Error for DetachedCensusError {}

/// Whether a reader archive holds one mission's records or the shared ones.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ReaderScope {
    /// Exactly `zbd/<group>/<mission>` (F13-B's rule): this archive is one
    /// mission's.
    Mission(String),
    /// The shared reader (`zbd/zrdr.zbd`) or a world-group reader
    /// (`zbd/<group>/zrdr.zbd`). F39-D's census excluded both, so they are
    /// counted and reported separately rather than folded into the mission
    /// denominator.
    Shared(String),
}

impl ReaderScope {
    /// The archive's logical key.
    #[must_use]
    pub fn archive(&self) -> &str {
        match self {
            Self::Mission(mission) => mission,
            Self::Shared(archive) => archive,
        }
    }
}

/// One reader archive's measured vocabulary.
#[derive(Clone, Debug, PartialEq)]
pub struct DetachedVocabularyRow {
    /// Which scope this archive is.
    pub scope: ReaderScope,
    /// The archive's spelling as inventoried.
    pub container: String,
    /// The archive's SHA-256, as production discovery measured it.
    pub container_sha256: String,
    /// The archive's decoded `objectives.zrd` member SHA-256, or `None` when
    /// the archive declares **no** objective record at all — a measured
    /// absence for a shared/world-group reader, never a defaulted empty
    /// reading. A *mission* reader in that state is a refusal, not a row.
    pub objectives_sha256: Option<String>,
    /// The decoded `targets.zrd` member SHA-256, or `None` when the archive
    /// declares none — a measured absence, named by
    /// [`DetachedVocabularyCensus::archives_without_targets`].
    pub targets_sha256: Option<String>,
    /// What the archive's declarations spell.
    pub vocabulary: MeasuredDetachedVocabulary,
}

/// The installation's measured detached vocabulary, over every reader archive.
#[derive(Clone, Debug, PartialEq)]
pub struct DetachedVocabularyCensus {
    install_sha256: String,
    rows: Vec<DetachedVocabularyRow>,
}

impl DetachedVocabularyCensus {
    /// The installation fingerprint the walk measured.
    #[must_use]
    pub fn install_sha256(&self) -> &str {
        &self.install_sha256
    }

    /// Every reader archive the walk read, sorted by scope then archive.
    #[must_use]
    pub fn rows(&self) -> &[DetachedVocabularyRow] {
        &self.rows
    }

    /// How many reader archives were read.
    #[must_use]
    pub fn readers(&self) -> usize {
        self.rows.len()
    }

    /// How many **mission-scoped** archives were read.
    #[must_use]
    pub fn mission_readers(&self) -> usize {
        self.rows
            .iter()
            .filter(|row| matches!(row.scope, ReaderScope::Mission(_)))
            .count()
    }

    /// How many shared/world-group archives were read — the denominator F39-D's
    /// census did not have.
    ///
    /// **Measured: none of them declares an `objectives.zrd` member**, so they
    /// contribute no counted condition and no block declaration. One of them
    /// (`zbd/c1c/zrdr.zbd`) does declare a `targets.zrd`, so the objective-kind
    /// surface is not mission-scoped.
    #[must_use]
    pub fn shared_readers(&self) -> usize {
        self.rows
            .iter()
            .filter(|row| matches!(row.scope, ReaderScope::Shared(_)))
            .count()
    }

    /// The archives that declare no `targets.zrd`, named.
    #[must_use]
    pub fn archives_without_targets(&self) -> Vec<&str> {
        self.rows
            .iter()
            .filter(|row| row.targets_sha256.is_none())
            .map(|row| row.scope.archive())
            .collect()
    }

    /// The archives that declare no `objectives.zrd` at all, named.
    ///
    /// **Measured: every shared and world-group reader.** The shared reader and
    /// the nine world-group readers hold animation, sound and motion records and
    /// none declares an objective record, so they contribute **no** counted
    /// condition and **no** block declaration. That is the measured answer to
    /// F39-D's unknown #5 *for this question* — the objective-declaration
    /// vocabulary is a mission-scoped one — and it is why
    /// [`Self::shared_readers`] is reported beside [`Self::mission_readers`]
    /// instead of being folded into one number. One shared reader does declare a
    /// `targets.zrd`, so the objective-kind surface is not mission-scoped. A
    /// *mission* reader with no objective record is a refusal, never a row.
    #[must_use]
    pub fn archives_without_objectives(&self) -> Vec<&str> {
        self.rows
            .iter()
            .filter(|row| row.objectives_sha256.is_none())
            .map(|row| row.scope.archive())
            .collect()
    }

    /// Every name every archive declared, across all of them.
    fn names(&self) -> impl Iterator<Item = &cs_content::objectives::DetachedSpellingSite> {
        self.rows.iter().flat_map(|row| row.vocabulary.names.iter())
    }

    /// How many **distinct** names one surface spelled over the whole
    /// installation.
    #[must_use]
    pub fn distinct_names(&self, surface: DetachedVocabularySurface) -> usize {
        let names: std::collections::BTreeSet<&str> = self
            .names()
            .filter(|site| site.surface == surface)
            .map(|site| site.name.as_str())
            .collect();
        names.len()
    }

    /// How many sites one surface contributed over the whole installation.
    #[must_use]
    pub fn sites(&self, surface: DetachedVocabularySurface) -> usize {
        self.names().filter(|site| site.surface == surface).count()
    }

    /// How many sites on one surface matched one family.
    #[must_use]
    pub fn family_sites(&self, surface: DetachedVocabularySurface, family: &str) -> usize {
        self.names()
            .filter(|site| site.surface == surface && site.family == Some(family))
            .count()
    }

    /// How many sites on **any** surface matched one family.
    #[must_use]
    pub fn sites_in_family(&self, family: &str) -> usize {
        self.names()
            .filter(|site| site.family == Some(family))
            .count()
    }

    /// Every site that matched one family, as `(archive, surface, block, name)`.
    ///
    /// The measured sites, so "no mission declares a detached count" and "the
    /// release family does appear, here" are both answerable from the census.
    #[must_use]
    pub fn sites_of_family(
        &self,
        family: &str,
    ) -> Vec<(String, DetachedVocabularySurface, Option<String>, String)> {
        let mut sites: Vec<(String, DetachedVocabularySurface, Option<String>, String)> = self
            .rows
            .iter()
            .flat_map(|row| {
                row.vocabulary
                    .names
                    .iter()
                    .filter(move |site| site.family == Some(family))
                    .map(move |site| {
                        (
                            row.scope.archive().to_owned(),
                            site.surface,
                            site.block.clone(),
                            site.name.clone(),
                        )
                    })
            })
            .collect();
        sites.sort();
        sites
    }

    /// How many counted conditions the whole installation declares.
    #[must_use]
    pub fn counted_conditions(&self) -> usize {
        self.sites(DetachedVocabularySurface::CountedCondition)
    }

    /// How many objective kinds the whole installation declares.
    #[must_use]
    pub fn objective_kinds(&self) -> usize {
        self.sites(DetachedVocabularySurface::ObjectiveKind)
    }

    /// How many sites on the two surfaces that could **declare a category** spell
    /// the contract's own `detach` stem.
    ///
    /// The measured absence F39-E7 turns on: it counts the counted-condition and
    /// objective-kind surfaces only, never the block-declaration surface, because
    /// a declaration in the objective's own trigger vocabulary is evidence of the
    /// *event*, not of a category.
    #[must_use]
    pub fn detached_category_sites(&self) -> usize {
        DetachedVocabularySurface::ALL
            .iter()
            .filter(|surface| **surface != DetachedVocabularySurface::BlockDeclaration)
            .map(|surface| self.family_sites(*surface, DETACH_CONTRACT_STEM))
            .sum()
    }

    /// How many **block declarations** spell the detach family, that is any stem
    /// of [`cs_content::objectives::DETACHED_SPELLING_STEMS`] — the measured
    /// non-zero that keeps the absence above from being vacuous.
    ///
    /// Named for the family, not for one stem: over the owner's installation the
    /// measured sites are `DROP` (7), `LAUNCH` (16) and `FREE` (1), and the
    /// contract's own `DETACH` stem contributes none. `sites_of_family` answers
    /// the same question per stem and names every site.
    #[must_use]
    pub fn detach_family_declaration_sites(&self) -> usize {
        self.names()
            .filter(|site| {
                site.surface == DetachedVocabularySurface::BlockDeclaration && site.family.is_some()
            })
            .count()
    }

    /// Every distinct family stem that matched at least one site, with its
    /// per-surface counts.
    #[must_use]
    pub fn family_counts(&self) -> Vec<(&'static str, Vec<(DetachedVocabularySurface, usize)>)> {
        cs_content::objectives::DETACHED_SPELLING_STEMS
            .iter()
            .copied()
            .map(|stem| {
                let per_surface = DetachedVocabularySurface::ALL
                    .iter()
                    .map(|surface| (*surface, self.family_sites(*surface, stem)))
                    .collect();
                (stem, per_surface)
            })
            .collect()
    }
}

/// The contract's own spelling stem, re-exported here so a census reader does
/// not have to import two modules to ask the question.
pub const DETACH_CONTRACT_STEM: &str = cs_content::objectives::DETACH_STEM;

/// Measures what every reader archive in `install_root` spells about a detached
/// category.
///
/// **Every** reader archive is read, not only the mission-scoped ones: F39-D's
/// census (and F39-E4's) covered exactly `zbd/<group>/<mission>` and left the
/// shared reader (`zbd/zrdr.zbd`, 220 members) and the nine world-group readers
/// outside its denominator (F39-D unknown #5). This walk reads them too and
/// keeps them in their own [`ReaderScope::Shared`] rows, because a detach is
/// exactly the kind of thing a shared reader would spell. **Measured: none of
/// them declares an `objectives.zrd` member**, so they contribute no counted
/// condition and no block declaration; one of them declares a `targets.zrd` and
/// does contribute objective kinds. That is itself the answer to the wider
/// denominator for this question, and the reason the two scopes are reported
/// side by side rather than as one number.
///
/// A reader archive with no `targets.zrd` is a **measured absence**, kept as a
/// row with `targets_sha256: None`: `archives_without_targets` names it, so
/// "this archive declares no objective kind" is never reported about a member
/// nobody read. A **mission** archive with no `objectives.zrd` is a refusal
/// instead, so a mission cannot vanish from a denominator.
///
/// # Errors
///
/// [`DetachedCensusError::Discovery`] when the installation cannot be
/// discovered, [`DetachedCensusError::Read`] when a reader archive cannot be
/// read, and [`DetachedCensusError::Decode`] when a member does not decode. A
/// reader that refuses is an error, never a skipped row, so an archive cannot
/// vanish from a denominator.
pub fn survey_retail_detached_declarations(
    install_root: &Path,
) -> Result<DetachedVocabularyCensus, DetachedCensusError> {
    let found = cs_assets::install::discover(install_root)
        .map_err(|error| DetachedCensusError::Discovery(error.to_string()))?;
    let install_sha256 = cs_assets::install::fingerprint(&found.manifest).to_hex();

    let mut rows: Vec<DetachedVocabularyRow> = Vec::new();
    for record in &found.manifest.files {
        let container_key = record.relative_spelling.logical_key();
        if !container_key.ends_with(MISSION_READER_ARCHIVE) {
            continue;
        }
        let spelling = record.relative_spelling.as_str().to_owned();
        let path = RelativePath::new(&spelling.to_lowercase()).map_err(|error| {
            DetachedCensusError::Read {
                archive: container_key.clone(),
                reason: error.to_string(),
            }
        })?;
        let bytes = std::fs::read(found.manifest.host_root.join(&spelling)).map_err(|error| {
            DetachedCensusError::Read {
                archive: container_key.clone(),
                reason: error.to_string(),
            }
        })?;
        let discovery = cs_formats::script_raw::discover_container(&container_key, &path, &bytes);
        let member = |wanted: &str| {
            discovery.programs().iter().find(|program| {
                program
                    .locator()
                    .member()
                    .is_some_and(|name| name.eq_ignore_ascii_case(wanted))
            })
        };
        let scope = match cs_formats::script_raw::mission_scope(&path) {
            Some(mission) => ReaderScope::Mission(mission),
            None => ReaderScope::Shared(container_key.clone()),
        };
        let decode = |member: &str, bytes: &[u8]| {
            cs_content::stunts::decode_zrd(bytes).map_err(|error| DetachedCensusError::Decode {
                archive: container_key.clone(),
                member: member.to_owned(),
                code: error.code().to_owned(),
                offset: error.offset(),
            })
        };
        // A mission reader with no objective record is a **refusal** (F39-D's
        // rule: a mission cannot vanish from a denominator). A shared or
        // world-group reader without one declares no objective record at all —
        // they hold animation, sound and motion records — so it is a measured
        // absence and the row says so.
        let objectives = match member(SCENARIO_OBJECTIVES_MEMBER) {
            Some(objectives) => Some((
                cs_assets::install::sha256(objectives.bytes()).to_hex(),
                decode(SCENARIO_OBJECTIVES_MEMBER, objectives.bytes())?,
            )),
            None if matches!(scope, ReaderScope::Mission(_)) => {
                return Err(DetachedCensusError::Read {
                    archive: container_key.clone(),
                    reason: format!(
                        "the mission archive declares no {} objective record",
                        SCENARIO_OBJECTIVES_MEMBER
                    ),
                });
            }
            None => None,
        };
        let (objectives_sha256, objectives_document) = match objectives {
            Some((digest, document)) => (Some(digest), Some(document)),
            None => (None, None),
        };
        // A missing `targets.zrd` is a measured absence in either scope: the row
        // keeps `None` and `archives_without_targets` names it.
        let (targets_sha256, targets_document) = match member(SCENARIO_TARGETS_MEMBER) {
            Some(targets) => (
                Some(cs_assets::install::sha256(targets.bytes()).to_hex()),
                Some(decode(SCENARIO_TARGETS_MEMBER, targets.bytes())?),
            ),
            None => (None, None),
        };
        // An archive that declares no objective record measures nothing: its row
        // exists so the archive is in the denominator, and
        // `archives_without_objectives` names it, so the empty reading is a
        // measured absence and not a silent skip.
        let vocabulary = match &objectives_document {
            Some(document) => measure_detached_vocabulary(document, targets_document.as_ref()),
            None => cs_content::objectives::MeasuredDetachedVocabulary::default(),
        };
        rows.push(DetachedVocabularyRow {
            scope,
            container: spelling,
            container_sha256: record.sha256.to_hex(),
            objectives_sha256,
            targets_sha256,
            vocabulary,
        });
    }

    rows.sort_by(|left, right| left.scope.archive().cmp(right.scope.archive()));
    Ok(DetachedVocabularyCensus {
        install_sha256,
        rows,
    })
}

// ---------------------------------------------------------------------------
// F39-E3: the installation-scope objective census
// ---------------------------------------------------------------------------
//
// F39-D's census walks **mission-scoped** reader archives only
// (`zbd/<group>/<mission>/zrdr.zbd`, F13-B's `mission_scope` rule) and left its
// own denominator open as unknown #5: the install-wide reader `ZBD/zrdr.zbd` and
// the world-group readers `ZBD/<group>/zrdr.zbd` are outside it, so a mission may
// inherit objective declarations the census does not see. This section is that
// measurement.
//
// **What is measured.** Every reader archive the installation holds that is *not*
// mission-scoped — the same production walk, the same `mission_scope` rule — is
// opened with the F06 two-key dispatch, classified by F14-D.1's own member rules
// ([`cs_content::catalog::reader_dirs::classify_installation_scope`]), and every
// member it declares is decoded with the production `.zrd` reader. Each member is
// then read for three surfaces:
//
// * **numbered objective blocks**, through
//   [`cs_content::stunts::objective_state_machine`] — the same reader F39-D
//   measured mission records with, so "0 here" and "1338 there" are one
//   measurement of one surface over two denominators;
// * **objective target records**, through the F39-E4 surface
//   ([`measure_target_kinds`]) for a member the archive names `targets.zrd`, the
//   name F39-E4's census located by;
// * **every objective-named spelling anywhere in the member**, through
//   [`measure_scope_member`]'s complete text inventory — the search list that
//   bounds the negative result. It searches keys *and* values, so a declaration
//   that names an objective in either position is counted.
//
// **What is not measured, and why.** Nothing here decodes a rule: a spelling is a
// spelling. Whether the install-wide reader's `OBJECTIVESLIST` dialog primitive
// and `MSG_BRF_DLG_OBJECTIVES` message id are ever resolved for a mission, and
// whether a mission without its own `targets.zrd` sees its world group's, are
// reader-archive **precedence** questions — F04/F06 own the resolution order and
// nothing measured here observes it. No original executable was run.

/// The measured objective surface of one decoded installation-scope member.
///
/// Three surfaces, each measured whole: the numbered `OBJECTIVE<N>` blocks, the
/// objective target records (for a member the archive names `targets.zrd`), and
/// the **complete** set of spellings that name an objective anywhere in the
/// member — keys and values alike.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MeasuredScopeMember {
    /// How many numbered `OBJECTIVE<N>` blocks the member declares, read with
    /// the same reader F39-D's mission rows use.
    pub objective_blocks: u32,
    /// How many objective target records the member declares, or `None` when the
    /// member is not named `targets.zrd` — a measured **absence of the named
    /// surface**, never a default of zero records. The name is the selector
    /// because it is the name F39-E4's census located mission target records by,
    /// and because reading any member's root as a record list would count a
    /// member's ordinary fields as records (`ZBD/C2`'s `game_targets.zrd` holds
    /// animation definitions, not objectives).
    pub target_records: Option<MeasuredTargetKinds>,
    /// Every spelling in the member that names an objective, sorted, with the
    /// number of times it occurs. The **complete** inventory for this surface:
    /// this is the list a negative result is drawn from, so a spelling this walk
    /// never saw cannot be argued away.
    pub objective_spellings: Vec<(String, u32)>,
    /// The member's complete key vocabulary, sorted by key, with the number of
    /// times each key occurs. Published whole beside the objective surface so the
    /// classification can be read against the vocabulary it was drawn from.
    pub keys: Vec<(String, u32)>,
    /// How many text nodes the walks stopped short of, because the document nested
    /// deeper than [`SCOPE_WALK_DEPTH`].
    ///
    /// **Zero for every document the production `.zrd` decoder produced**, which is
    /// what makes both inventories above complete. It is reported rather than
    /// assumed because these inventories are the *search list* a negative result is
    /// drawn from: a walk that stopped early would leave the stopped-at subtree
    /// unread while still reporting "no spelling names an objective", which is the
    /// one failure mode a bounded walk must never have. The census refuses a member
    /// that reports a non-zero count (see
    /// [`ScopeObjectiveCensusError::WalkTruncated`]) instead of publishing a
    /// partial inventory as a complete one.
    pub truncated_nodes: u32,
}

/// The spelling that makes a text node part of the objective search list: any
/// spelling containing `objective`, case-insensitively.
///
/// Measured over the installation's whole reader layer this matches the mission
/// objective blocks' own family (`OBJECTIVE<n>`, `WAKE_OBJECTIVE_WHEN_I_COMPLETE`,
/// `REMOVE_OBJECTIVE_TARGET`, …), the mission readers' `objective` /
/// `objective_numbers` / `OBJECTIVE_DELAY` keys, and the install-wide reader's
/// `OBJECTIVESLIST` / `Objective` / `MSG_BRF_DLG_OBJECTIVES` dialog spellings. It
/// is a **substring** rule on purpose: a tighter rule could miss a spelling the
/// original uses, and the point of this census is to bound what a mission may
/// inherit, not to classify what it means.
const OBJECTIVE_SPELLING_NEEDLE: &str = "objective";

/// Measures one decoded installation-scope member's objective surface.
///
/// `member` is the member's name as the archive declares it; it selects the
/// objective target records and is never interpreted beyond that selection.
///
/// The spelling inventory is a **whole-tree** text walk rather than a walk of the
/// record's fields only: an objective declaration may name an objective in a
/// value (`"MSG_OBJ_DEFEND"`, a node name) as easily as in a key, and a walk that
/// read keys alone would report "nothing names an objective" about a member whose
/// value does.
#[must_use]
pub fn measure_scope_member(document: &ZrdValue, member: &str) -> MeasuredScopeMember {
    let objective_blocks = cs_content::stunts::objective_state_machine(document).blocks();
    let is_target_member = member.eq_ignore_ascii_case(SCENARIO_TARGETS_MEMBER);
    let target_records = is_target_member.then(|| measure_target_kinds(document));
    let mut spellings: BTreeMap<String, u32> = BTreeMap::new();
    let mut truncated = 0u32;
    collect_objective_spellings(document, 0, &mut spellings, &mut truncated);
    let keys = scope_member_keys(document, is_target_member, &mut truncated);
    MeasuredScopeMember {
        objective_blocks,
        target_records,
        objective_spellings: spellings.into_iter().collect(),
        keys,
        truncated_nodes: truncated,
    }
}

/// The deepest the whole-tree walks descend, one step past the production `.zrd`
/// decoder's own `MAX_ZRD_DEPTH`.
///
/// The decoder **refuses** a member nested deeper than that bound, so a decoded
/// document cannot reach this one: the guard here is a stack-safety backstop for a
/// caller that hands [`measure_scope_member`] a hand-built tree, and
/// [`MeasuredScopeMember::truncated_nodes`] is what makes "the walks saw
/// everything" a measured fact rather than an assumption. The census refuses a
/// non-zero count, so a truncated walk can never publish a partial inventory as a
/// complete one — which is the whole basis of the negative result.
const SCOPE_WALK_DEPTH: usize = 65;

fn collect_objective_spellings(
    node: &ZrdValue,
    depth: usize,
    spellings: &mut BTreeMap<String, u32>,
    truncated: &mut u32,
) {
    if depth >= SCOPE_WALK_DEPTH {
        *truncated = truncated.saturating_add(1);
        return;
    }
    match node {
        ZrdValue::Text(text) => {
            if text
                .to_ascii_lowercase()
                .contains(OBJECTIVE_SPELLING_NEEDLE)
            {
                *spellings.entry(text.clone()).or_insert(0) += 1;
            }
        }
        ZrdValue::List(children) => {
            for child in children {
                collect_objective_spellings(child, depth + 1, spellings, truncated);
            }
        }
        ZrdValue::Int(_) | ZrdValue::Float(_) => {}
    }
}

/// One member's complete key vocabulary: the fields of the member's own record
/// and of every nested record under it.
///
/// Each member is read with the shape its name selects, which is the discipline
/// `#463` measured and this repository's own readers follow:
///
/// * every member is read as a **flat alternating record** ([`zrd_flat_fields`])
///   below its unwrapped record ([`objective_record`]), which is the shape
///   `objectives.zrd`, `escape.zrd` and every dialog record uses;
/// * a `targets.zrd` member is **additionally** read as a list of `[key, value]`
///   pairs ([`cs_content::stunts::objective_record_keys`]), which is the shape
///   every objective target record uses.
///
/// The second reading is deliberately restricted to the member name that carries
/// it. A shape-agnostic walk would read any two-element list whose first element
/// is text as a field and invent keys out of the data — `["fuel_truck01",
/// "tank"]` would become two keys — which is the failure the pair reader's own
/// doc forbids. A member in some third shape would show up as an **empty**
/// inventory here rather than as invented keys, and the spelling inventory above
/// is shape-agnostic precisely so such a member cannot hide.
///
/// It is an inventory of **field names**, never a reading of what a field means.
fn scope_member_keys(
    document: &ZrdValue,
    is_target_member: bool,
    truncated: &mut u32,
) -> Vec<(String, u32)> {
    let mut keys: BTreeMap<String, u32> = BTreeMap::new();
    collect_scope_keys(objective_record(document), 0, &mut keys, truncated);
    if is_target_member {
        for (key, count) in cs_content::stunts::objective_record_keys(document) {
            *keys.entry(key).or_insert(0) += count;
        }
    }
    keys.into_iter().collect()
}

fn collect_scope_keys(
    node: &ZrdValue,
    depth: usize,
    keys: &mut BTreeMap<String, u32>,
    truncated: &mut u32,
) {
    if depth >= SCOPE_WALK_DEPTH {
        *truncated = truncated.saturating_add(1);
        return;
    }
    for (key, value) in zrd_flat_fields(node) {
        *keys.entry(key.to_owned()).or_insert(0) += 1;
        collect_scope_keys(value, depth + 1, keys, truncated);
    }
}

/// Why the installation-scope objective census could not be produced.
#[derive(Clone, Debug, PartialEq)]
pub enum ScopeObjectiveCensusError {
    /// The installation could not be discovered, or its campaign layout could not
    /// be walked (the world-group set F14-D.1's classification needs).
    Discovery(String),
    /// A scope reader archive could not be read from disk.
    Read {
        /// The archive's logical key.
        container: String,
        /// Why the read failed.
        reason: String,
    },
    /// A member's bytes did not decode as `.zrd`.
    Decode {
        /// The archive's logical key.
        container: String,
        /// The member's name, as the archive declares it.
        member: String,
        /// The decoder's refusal code.
        code: &'static str,
        /// Offset of the refusal inside the member.
        offset: u64,
    },
    /// A reader archive that is not mission-scoped fits neither installation-scope
    /// rule.
    ///
    /// A **refusal**, never a skipped row: a scope archive this census cannot
    /// classify is an archive whose contents it has not measured, and dropping it
    /// would turn "we did not look" into "there was nothing there".
    Unclassified {
        /// The archive's logical key.
        container: String,
        /// Why the members did not decide a role.
        reason: String,
    },
    /// A member nested deeper than the whole-tree walks descend, so its spelling
    /// and key inventories are partial.
    ///
    /// A **refusal** rather than a published row: the inventories are the search
    /// list the "no installation-scope reader declares an objective block" result
    /// is drawn from, and a walk that stopped early would report "nothing names an
    /// objective" about a subtree it never read. The production `.zrd` decoder
    /// refuses a member this deep, so nothing measured here can produce one.
    WalkTruncated {
        /// The archive's logical key.
        container: String,
        /// The member's name, as the archive declares it.
        member: String,
        /// How many text nodes the walks stopped short of.
        nodes: u32,
    },
}

impl fmt::Display for ScopeObjectiveCensusError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Discovery(reason) => {
                write!(f, "the installation could not be discovered: {reason}")
            }
            Self::Read { container, reason } => {
                write!(
                    f,
                    "scope reader archive {container} could not be read: {reason}"
                )
            }
            Self::Decode {
                container,
                member,
                code,
                offset,
            } => write!(
                f,
                "{container}'s {member} member did not decode: {code} at offset {offset}"
            ),
            Self::Unclassified { container, reason } => {
                write!(
                    f,
                    "scope reader archive {container} is unclassified: {reason}"
                )
            }
            Self::WalkTruncated {
                container,
                member,
                nodes,
            } => {
                write!(
                    f,
                    "{container}'s {member} member nests deeper than the census walks descend, \
                     leaving {nodes} text nodes unread: its inventories are partial, not empty"
                )
            }
        }
    }
}

impl std::error::Error for ScopeObjectiveCensusError {}

/// One installation-scope reader archive's measured objective surface.
#[derive(Clone, Debug, PartialEq)]
pub struct RetailScopeObjectiveRow {
    /// The scope the archive is, as the directory that holds it: `zbd` for the
    /// install-wide reader, `zbd/<world group>` for a world group's.
    pub scope: String,
    /// What F14-D.1's own member rules decide the archive is.
    pub role: cs_content::catalog::reader_dirs::ReaderDirRole,
    /// The reader archive's installation spelling, as production discovery spells
    /// it (`ZBD/zrdr.zbd`, `ZBD/C1C/zrdr.zbd`).
    pub container: String,
    /// SHA-256 of that whole archive, from production discovery.
    pub container_sha256: String,
    /// The member names that corroborated the role, as
    /// [`ClassifiedReaderDir::evidence`](cs_content::catalog::reader_dirs::ClassifiedReaderDir)
    /// publishes them.
    pub evidence: Vec<&'static str>,
    /// How many members the archive's own index declares, counting a name listed
    /// twice twice: the owner's `ZBD/zrdr.zbd` declares 221 and lists
    /// `player.zrd` twice, so it is 220 [`Self::distinct_members`] and this.
    pub declared_members: usize,
    /// How many **distinct** lowercase member names the archive lists.
    pub distinct_members: usize,
    /// How many of those members decoded as `.zrd`. Every measured member does;
    /// a member that does not decode is a refusal, never a skipped row.
    pub decoded_members: usize,
    /// How many numbered `OBJECTIVE<N>` blocks the whole archive declares.
    pub objective_blocks: u32,
    /// How many objective target records the archive declares across its
    /// `targets.zrd` members, or `None` when it declares no such member — a
    /// measured absence of the named surface.
    pub target_records: Option<MeasuredTargetKinds>,
    /// Every objective-named spelling the archive carries, sorted, with the number
    /// of occurrences.
    pub objective_spellings: Vec<(String, u32)>,
    /// The archive's complete member key vocabulary, sorted by key.
    pub keys: Vec<(String, u32)>,
    /// Every member name the archive lists, lowercased, sorted and deduplicated.
    ///
    /// Member names are not field names, so they are carried beside
    /// [`Self::keys`] rather than inside it: the key vocabulary is what the
    /// members' *records* say, and this is what the archive *lists*.
    pub member_names: Vec<String>,
}

impl RetailScopeObjectiveRow {
    /// Whether this row's archive declares any numbered objective block.
    ///
    /// A measurement of declarations, never of rules: see
    /// [`RetailObjectiveCensus::declares_branching`] for the same distinction on
    /// the mission-scoped side.
    #[must_use]
    pub const fn declares_objective_blocks(&self) -> bool {
        self.objective_blocks > 0
    }
}

/// The measured objective surface of every installation-scope reader archive.
#[derive(Clone, Debug, PartialEq)]
pub struct RetailScopeObjectiveCensus {
    install_sha256: String,
    rows: Vec<RetailScopeObjectiveRow>,
}

impl RetailScopeObjectiveCensus {
    /// SHA-256 of the whole installation manifest, from production discovery.
    #[must_use]
    pub fn install_sha256(&self) -> &str {
        &self.install_sha256
    }

    /// The measured rows, one per installation-scope reader archive, sorted by
    /// scope.
    #[must_use]
    pub fn rows(&self) -> &[RetailScopeObjectiveRow] {
        &self.rows
    }

    /// How many installation-scope reader archives the census measured.
    #[must_use]
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// Whether the census measured nothing.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// The row for one scope, by its `zbd` / `zbd/<group>` label.
    #[must_use]
    pub fn row(&self, scope: &str) -> Option<&RetailScopeObjectiveRow> {
        self.rows.iter().find(|row| row.scope == scope)
    }

    /// How many members the measured archives declare in all, duplicates counted.
    #[must_use]
    pub fn declared_members(&self) -> usize {
        self.rows.iter().map(|row| row.declared_members).sum()
    }

    /// How many members decoded across the measured archives.
    #[must_use]
    pub fn decoded_members(&self) -> usize {
        self.rows.iter().map(|row| row.decoded_members).sum()
    }

    /// How many **distinct** lowercase member names the measured archives carry in
    /// all: the union, not the sum, so a shared member name in two world groups
    /// is one name.
    #[must_use]
    pub fn distinct_member_names(&self) -> usize {
        let mut names: BTreeSet<String> = BTreeSet::new();
        for row in &self.rows {
            for name in &row.member_names {
                names.insert(name.clone());
            }
        }
        names.len()
    }

    /// How many numbered `OBJECTIVE<N>` blocks the installation declares outside
    /// its mission-scoped readers.
    #[must_use]
    pub fn objective_blocks(&self) -> u32 {
        self.rows.iter().map(|row| row.objective_blocks).sum()
    }

    /// Whether any installation-scope reader declares a numbered objective block.
    ///
    /// A measurement of declarations: `false` says no scope reader carries one,
    /// never that the original has no shared objectives. The inheritance
    /// **precedence** question — whether a mission resolves a member of its world
    /// group's or the install-wide reader at all — is not measured here.
    #[must_use]
    pub fn declares_objective_blocks(&self) -> bool {
        self.objective_blocks() > 0
    }

    /// How many objective target records the installation declares outside its
    /// mission-scoped readers.
    #[must_use]
    pub fn target_records(&self) -> u32 {
        self.rows
            .iter()
            .filter_map(|row| row.target_records.as_ref())
            .map(|kinds| kinds.records)
            .sum()
    }

    /// How many of those records carry an objective kind.
    #[must_use]
    pub fn labelled_targets(&self) -> u32 {
        self.rows
            .iter()
            .filter_map(|row| row.target_records.as_ref())
            .map(|kinds| kinds.labelled)
            .sum()
    }

    /// The scopes whose archive declares an objective target record, sorted.
    #[must_use]
    pub fn objective_target_scopes(&self) -> Vec<&str> {
        self.rows
            .iter()
            .filter(|row| {
                row.target_records
                    .as_ref()
                    .is_some_and(|kinds| kinds.records > 0)
            })
            .map(|row| row.scope.as_str())
            .collect()
    }

    /// The union of the objective target labels the measured archives declare,
    /// sorted, with the number of records carrying each.
    #[must_use]
    pub fn target_names(&self) -> BTreeMap<String, u32> {
        let mut names: BTreeMap<String, u32> = BTreeMap::new();
        for kinds in self
            .rows
            .iter()
            .filter_map(|row| row.target_records.as_ref())
        {
            for (name, count) in &kinds.names {
                *names.entry(name.clone()).or_insert(0) += count;
            }
        }
        names
    }

    /// The union of every objective-named spelling the measured archives carry,
    /// sorted, with the number of occurrences. The complete search list the
    /// negative result above is drawn from.
    #[must_use]
    pub fn objective_spellings(&self) -> Vec<(String, u32)> {
        let mut totals: BTreeMap<String, u32> = BTreeMap::new();
        for row in &self.rows {
            for (spelling, count) in &row.objective_spellings {
                *totals.entry(spelling.clone()).or_insert(0) += count;
            }
        }
        totals.into_iter().collect()
    }

    /// The union of every key the measured archives' members use, sorted, with the
    /// number of occurrences. Published whole, as F39-D published the mission
    /// vocabulary: a reader can check the whole inventory instead of trusting the
    /// objective classification drawn from it.
    #[must_use]
    pub fn keys(&self) -> Vec<(String, u32)> {
        let mut totals: BTreeMap<String, u32> = BTreeMap::new();
        for row in &self.rows {
            for (key, count) in &row.keys {
                *totals.entry(key.clone()).or_insert(0) += count;
            }
        }
        totals.into_iter().collect()
    }

    /// The install-wide reader's row, when the census measured one.
    #[must_use]
    pub fn shared_reader(&self) -> Option<&RetailScopeObjectiveRow> {
        self.rows
            .iter()
            .find(|row| row.role == cs_content::catalog::reader_dirs::ReaderDirRole::SharedReader)
    }

    /// The world-group readers' rows, sorted by scope.
    #[must_use]
    pub fn world_group_readers(&self) -> Vec<&RetailScopeObjectiveRow> {
        self.rows
            .iter()
            .filter(|row| {
                row.role == cs_content::catalog::reader_dirs::ReaderDirRole::WorldGroupReader
            })
            .collect()
    }
}

impl RetailScopeObjectiveRow {
    /// The member names this row's archive lists, sorted and deduplicated.
    ///
    /// The union census-wide is [`RetailScopeObjectiveCensus::distinct_member_names`],
    /// which counts distinct names across archives rather than summing rows, so a
    /// shared name is one name.
    #[must_use]
    pub fn members(&self) -> &[String] {
        &self.member_names
    }
}

/// Measures every installation-scope objective declaration in `install_root`.
///
/// Read-only: the walk is production discovery plus production reader-archive
/// dispatch, so nothing inside the installation is written. It **fails** rather
/// than skipping an archive it cannot read, decode or classify, for the reason
/// [`survey_retail_objective_records`] gives: an archive that silently vanished
/// from the denominator would read as an archive with no objective declarations.
///
/// The denominator is stated rather than assumed: the *non-mission-scoped*
/// `zrdr.zbd` archives, which is the complement of F39-D's mission-scoped walk
/// under the same [`mission_scope`](cs_formats::script_raw::mission_scope) rule.
/// Together the two walks cover every reader archive the installation holds.
///
/// # Errors
///
/// [`ScopeObjectiveCensusError::Discovery`] when the installation or its
/// campaign layout cannot be read, [`ScopeObjectiveCensusError::Read`] /
/// [`ScopeObjectiveCensusError::Decode`] for the first archive or member that
/// cannot be measured, and [`ScopeObjectiveCensusError::Unclassified`] for a
/// non-mission-scoped archive that fits neither installation-scope rule.
pub fn survey_retail_scope_objective_records(
    install_root: &Path,
) -> Result<RetailScopeObjectiveCensus, ScopeObjectiveCensusError> {
    let found = cs_assets::install::discover(install_root)
        .map_err(|error| ScopeObjectiveCensusError::Discovery(error.to_string()))?;
    let install_sha256 = cs_assets::install::fingerprint(&found.manifest).to_hex();
    // F14-D.1's classification only decides a `ZBD/<group>/zrdr.zbd` archive when
    // the campaign layout declares that world group, so the census needs the same
    // world-group set the campaign walk derives.
    let world_groups: BTreeSet<String> =
        cs_content::campaign_bindings::campaign_layout(install_root)
            .map_err(|error| ScopeObjectiveCensusError::Discovery(error.to_string()))?
            .into_iter()
            .map(|entry| entry.mission.world_group.to_ascii_lowercase())
            .collect();
    if world_groups.is_empty() {
        return Err(ScopeObjectiveCensusError::Discovery(
            "the campaign layout declares no world group".to_owned(),
        ));
    }
    let present: BTreeSet<String> = found
        .manifest
        .files
        .iter()
        .map(|record| record.relative_spelling.logical_key())
        .collect();

    let mut rows: Vec<RetailScopeObjectiveRow> = Vec::new();
    for record in &found.manifest.files {
        let container_key = record.relative_spelling.logical_key();
        if !container_key.ends_with(MISSION_READER_ARCHIVE) {
            continue;
        }
        let spelling = record.relative_spelling.as_str().to_owned();
        let path = RelativePath::new(&spelling.to_lowercase()).map_err(|error| {
            ScopeObjectiveCensusError::Read {
                container: container_key.clone(),
                reason: error.to_string(),
            }
        })?;
        // F13-B's own rule, used the same way F39-D uses it: a mission-scoped
        // archive belongs to the mission census, and everything left over is an
        // installation-scope reader.
        if cs_formats::script_raw::mission_scope(&path).is_some() {
            continue;
        }
        let scope = scope_label(&container_key);
        let bytes = std::fs::read(found.manifest.host_root.join(&spelling)).map_err(|error| {
            ScopeObjectiveCensusError::Read {
                container: container_key.clone(),
                reason: error.to_string(),
            }
        })?;
        let discovery = cs_formats::script_raw::discover_container(&container_key, &path, &bytes);
        let names: BTreeSet<String> = discovery
            .programs()
            .iter()
            .filter_map(|program| program.locator().member())
            .map(|name| name.to_ascii_lowercase())
            .collect();
        let directory = match container_key.rsplit_once('/') {
            Some((directory, _)) => directory.to_owned(),
            None => String::new(),
        };
        let mis_anim = format!("{directory}/mis_anim.zbd").to_ascii_lowercase();
        let (role, evidence) = cs_content::catalog::reader_dirs::classify_installation_scope(
            &names,
            present.contains(&mis_anim),
        )
        .ok_or_else(|| ScopeObjectiveCensusError::Unclassified {
            container: container_key.clone(),
            reason: format!(
                "{} member names and no mis_anim.zbd fit neither the world-group nor the \
                 install-wide rule",
                names.len()
            ),
        })?;
        // F14-D.1 decides a `ZBD/<group>/zrdr.zbd` archive only when the campaign layout
        // declares that world group, so a group-named scope the layout does not
        // claim is a refusal rather than a measured row. The install-wide reader
        // has no group component and is not subject to the check.
        let components: Vec<&str> = container_key.split('/').collect();
        if components.len() == 3
            && let Some(group) = components.get(1)
            && !group.is_empty()
            && !world_groups.contains(&group.to_ascii_lowercase())
        {
            return Err(ScopeObjectiveCensusError::Unclassified {
                container: container_key.clone(),
                reason: format!(
                    "the campaign layout declares no world group named {group:?}, so F14-D.1's \
                     rule does not decide this archive's scope"
                ),
            });
        }

        let mut row = RetailScopeObjectiveRow {
            scope,
            role,
            container: spelling.clone(),
            container_sha256: record.sha256.to_hex(),
            evidence,
            declared_members: discovery.programs().len(),
            distinct_members: names.len(),
            decoded_members: 0,
            objective_blocks: 0,
            target_records: None,
            objective_spellings: Vec::new(),
            keys: Vec::new(),
            member_names: Vec::new(),
        };
        let mut spellings: BTreeMap<String, u32> = BTreeMap::new();
        let mut keys: BTreeMap<String, u32> = BTreeMap::new();
        let mut member_names: BTreeSet<String> = BTreeSet::new();
        for program in discovery.programs() {
            let member =
                program
                    .locator()
                    .member()
                    .ok_or_else(|| ScopeObjectiveCensusError::Read {
                        container: container_key.clone(),
                        reason: "the archive indexes a member without a name".to_owned(),
                    })?;
            member_names.insert(member.to_ascii_lowercase());
            let document = cs_content::stunts::decode_zrd(program.bytes()).map_err(|error| {
                ScopeObjectiveCensusError::Decode {
                    container: container_key.clone(),
                    member: member.to_owned(),
                    code: error.code(),
                    offset: error.offset(),
                }
            })?;
            row.decoded_members += 1;
            let measured = measure_scope_member(&document, member);
            if measured.truncated_nodes > 0 {
                return Err(ScopeObjectiveCensusError::WalkTruncated {
                    container: container_key.clone(),
                    member: member.to_owned(),
                    nodes: measured.truncated_nodes,
                });
            }
            row.objective_blocks += measured.objective_blocks;
            for (spelling, count) in measured.objective_spellings {
                *spellings.entry(spelling).or_insert(0) += count;
            }
            for (key, count) in measured.keys {
                *keys.entry(key).or_insert(0) += count;
            }
            if let Some(kinds) = measured.target_records {
                let entry = row
                    .target_records
                    .get_or_insert_with(MeasuredTargetKinds::default);
                entry.records += kinds.records;
                entry.labelled += kinds.labelled;
                for (name, count) in kinds.names {
                    *entry.names.entry(name).or_insert(0) += count;
                }
            }
        }
        row.objective_spellings = spellings.into_iter().collect();
        row.keys = keys.into_iter().collect();
        row.member_names = member_names.into_iter().collect();
        rows.push(row);
    }

    rows.sort_by(|left, right| left.scope.cmp(&right.scope));
    Ok(RetailScopeObjectiveCensus {
        install_sha256,
        rows,
    })
}

/// The scope label of an installation-scope reader archive: `<root>/<world group>`
/// for a world group's reader, `<root>` for the install-wide one.
///
/// Splitting the archive's own logical key is F14-D.1's path shape rather than a
/// second naming rule: which of the two a non-mission-scoped reader archive is
/// follows from the key's own depth, and the label is **derived from the key**,
/// not written down. A `zrdr.zbd` under some other root (`sounds/x/zrdr.zbd`) is
/// labelled with that root rather than with `zbd`, so a row cannot claim a
/// directory it is not in; F13-B's [`mission_scope`] rule, which only ever names a
/// `zbd` root, is what makes `zbd` the root of every row measured on the owner's
/// installation — a fact the census reads back through its own rows, not an
/// assumption made here.
fn scope_label(container_key: &str) -> String {
    let mut components = container_key.split('/');
    let root = components.next().unwrap_or_default();
    match components.next() {
        // `<root>/<archive>` is the install-wide reader: there is no world-group
        // component at all, which is what separates it from a group's reader.
        Some(_) if components.next().is_none() => root.to_owned(),
        Some(group) => format!("{root}/{group}"),
        None => root.to_owned(),
    }
}

/// Which measured family a declared objective-record field belongs to.
///
/// A family is a *spelling* class, not a meaning: it says which F39 measurement
/// saw the key, so a refusal can point at the finding that left it unresolved.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum RecordFieldFamily {
    /// `WAKE`/`NAP`/`KILL`/`WAKEUP_OBJECTIVE_WHEN_I_COMPLETE` (F39-E2/E5/E6).
    CompletionEffect,
    /// `TICK_DEPENDS_ON_OBJ` (F39-E2).
    OrderDependency,
    /// `INSTANTWIN`/`INSTANTLOSS` (F39-D).
    Outcome,
    /// `BEGIN_DORMANT` (F39-E1).
    Dormancy,
    /// An `INACTIVE<n>` stage condition (F39-E1/E7).
    InactiveStage,
    /// `INACTIVE_COMPLETION_COUNT` (F39-E1).
    CompletionCount,
    /// The display `IDENTITY` (F39-E1).
    Identity,
    /// `WAKEUP_SOUND_GROUP`/`COMPLETED_SOUND_GROUP` (F39-E1).
    SoundCue,
    /// An optionality key (F39-D).
    Optionality,
    /// A key no F39 stage measured.
    Unmeasured,
}

impl RecordFieldFamily {
    /// The family a measured key spelling belongs to.
    #[must_use]
    pub fn of_key(key: &str) -> Self {
        if BRANCH_EFFECT_KEY_VOCABULARY.contains(&key) {
            Self::CompletionEffect
        } else if key == BRANCH_ORDER_KEY {
            Self::OrderDependency
        } else if FAILURE_KEY_VOCABULARY.contains(&key) {
            Self::Outcome
        } else if key == cs_content::objectives::OBJECTIVE_DORMANT_KEY {
            Self::Dormancy
        } else if key == OBJECTIVE_INACTIVE_COUNT_KEY {
            Self::CompletionCount
        } else if is_objective_inactive_stage(key) {
            Self::InactiveStage
        } else if key == cs_content::objectives::OBJECTIVE_IDENTITY_KEY {
            Self::Identity
        } else if key == cs_content::objectives::OBJECTIVE_WAKEUP_SOUND_GROUP_KEY
            || key == cs_content::objectives::OBJECTIVE_COMPLETED_SOUND_GROUP_KEY
        {
            Self::SoundCue
        } else if is_optional_objective_key(key) {
            Self::Optionality
        } else {
            Self::Unmeasured
        }
    }

    /// Why a field of this family cannot be recovered into a declared program,
    /// by name.
    #[must_use]
    pub const fn reason(self) -> &'static str {
        match self {
            Self::CompletionEffect => {
                "the spelling is measured but what the effect does is an inference, and precedence between effects is unmeasured (F39-E2, E5, E6)"
            }
            Self::OrderDependency => {
                "the objective it sequences behind is named but the ordering rule is undecoded (F39-E2)"
            }
            Self::Outcome => "the terminal precedence between win and loss is unmeasured (F39-D)",
            Self::Dormancy => {
                "the unit of the argument and the reveal rule are unmeasured (F39-E1)"
            }
            Self::InactiveStage => {
                "what satisfying the named actor/part/attribute condition means is unmeasured (F39-E1, E7)"
            }
            Self::CompletionCount => {
                "the threshold's reachability and monotonicity are unmeasured (F39-E1)"
            }
            Self::Identity => {
                "the message ids are not resolvable to text and the reveal timing is unmeasured (F39-E1)"
            }
            Self::SoundCue => "when the cue is emitted is unmeasured (F39-E1)",
            Self::Optionality => "what an optional declaration changes is unmeasured (F39-D)",
            Self::Unmeasured => {
                "no F39 stage measured this key; the mission-language instruction table is undecoded (F13-B/C, F38)"
            }
        }
    }
}

/// One field of an original objective record that was read and could **not** be
/// recovered into a [`DeclaredObjectiveProgram`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnrecoveredField {
    /// The `OBJECTIVE<N>` block number, or `None` for a field declared at the
    /// record's top level.
    pub block: Option<u32>,
    /// The key as the original wrote it.
    pub key: String,
    /// The measured family of the key.
    pub family: RecordFieldFamily,
}

impl UnrecoveredField {
    /// Why this field is unrecovered.
    #[must_use]
    pub const fn reason(&self) -> &'static str {
        self.family.reason()
    }
}

/// What reading one mission's `objectives.zrd` recovered, and everything it
/// could not.
///
/// The producer walks **every** field of every `OBJECTIVE<N>` block and of the
/// record's top level, so nothing is silently dropped:
/// `fields_read() == fields_recovered() + unrecovered().len()` by construction.
/// Today no field is recoverable — the mission-language table and the objective
/// semantics are unmeasured, and AGENTS.md rule 4 forbids guessing them — so
/// [`program`](Self::program) is the refusal naming each gap, never a program
/// built from guessed semantics.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObjectiveRecovery {
    mission: String,
    blocks: Vec<u32>,
    fields_read: usize,
    unrecovered: Vec<UnrecoveredField>,
}

impl ObjectiveRecovery {
    /// Walks a decoded `objectives.zrd` document.
    #[must_use]
    pub fn read(mission: &str, document: &ZrdValue) -> Self {
        let mut blocks = Vec::new();
        let mut fields_read = 0;
        let mut unrecovered = Vec::new();
        for (key, value) in zrd_flat_fields(objective_record(document)) {
            if let Some(number) = cs_content::objectives::objective_block_number(key) {
                blocks.push(number);
                for (field, _) in zrd_directive_fields(value) {
                    fields_read += 1;
                    unrecovered.push(UnrecoveredField {
                        block: Some(number),
                        key: field.to_owned(),
                        family: RecordFieldFamily::of_key(field),
                    });
                }
            } else {
                fields_read += 1;
                unrecovered.push(UnrecoveredField {
                    block: None,
                    key: key.to_owned(),
                    family: RecordFieldFamily::of_key(key),
                });
            }
        }
        Self {
            mission: mission.to_owned(),
            blocks,
            fields_read,
            unrecovered,
        }
    }

    /// The mission this record belongs to.
    #[must_use]
    pub fn mission(&self) -> &str {
        &self.mission
    }

    /// The declared `OBJECTIVE<N>` block numbers, in record order.
    #[must_use]
    pub fn blocks(&self) -> &[u32] {
        &self.blocks
    }

    /// How many fields were read, blocks and top level together.
    #[must_use]
    pub const fn fields_read(&self) -> usize {
        self.fields_read
    }

    /// How many fields were recovered into a declared program.
    #[must_use]
    pub const fn fields_recovered(&self) -> usize {
        self.fields_read - self.unrecovered.len()
    }

    /// Every field that could not be recovered.
    #[must_use]
    pub fn unrecovered(&self) -> &[UnrecoveredField] {
        &self.unrecovered
    }

    /// The unrecovered field counts per family.
    #[must_use]
    pub fn unrecovered_by_family(&self) -> BTreeMap<RecordFieldFamily, usize> {
        let mut counts = BTreeMap::new();
        for field in &self.unrecovered {
            *counts.entry(field.family).or_insert(0) += 1;
        }
        counts
    }

    /// The declared program, or the refusal naming what cannot be recovered.
    ///
    /// # Errors
    ///
    /// [`ObjectiveRecoveryRefusal`], always today: no field family has a measured
    /// recovery, so a program would be built from guessed semantics.
    pub fn program(&self) -> Result<DeclaredObjectiveProgram, ObjectiveRecoveryRefusal> {
        Err(ObjectiveRecoveryRefusal {
            mission: self.mission.clone(),
            families: self.unrecovered_by_family(),
        })
    }
}

/// The named reason a mission's objective record yields no declared program.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObjectiveRecoveryRefusal {
    mission: String,
    families: BTreeMap<RecordFieldFamily, usize>,
}

impl ObjectiveRecoveryRefusal {
    /// The unrecovered field counts per family.
    #[must_use]
    pub const fn families(&self) -> &BTreeMap<RecordFieldFamily, usize> {
        &self.families
    }
}

impl fmt::Display for ObjectiveRecoveryRefusal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{}: no declared objective program can be recovered; unrecovered",
            self.mission
        )?;
        for (family, count) in &self.families {
            write!(formatter, " {family:?} x{count} ({})", family.reason())?;
        }
        Ok(())
    }
}

impl std::error::Error for ObjectiveRecoveryRefusal {}

/// Reads one installed mission's `objectives.zrd` and reports what is and is not
/// recoverable.
///
/// # Errors
///
/// [`ObjectiveCensusError`] when the installation cannot be walked, the mission
/// is not among its readers, or its record does not decode.
pub fn recover_retail_objectives(
    install_root: &Path,
    mission: &str,
) -> Result<ObjectiveRecovery, ObjectiveCensusError> {
    let found = cs_assets::install::discover(install_root)
        .map_err(|error| ObjectiveCensusError::Discovery(error.to_string()))?;
    let record = locate_mission_objective_records(&found)?
        .into_iter()
        .find(|record| record.mission.eq_ignore_ascii_case(mission))
        .ok_or_else(|| ObjectiveCensusError::Read {
            container: mission.to_owned(),
            reason: "the installation has no such mission reader".to_owned(),
        })?;
    Ok(ObjectiveRecovery::read(&record.mission, &record.document))
}
