//! The mission host: the stage's records launched as one session and driven
//! through **one composed per-tick entry** (VS-M01-RT-MISSION-HOST, Rally
//! #1278 subtask `.01`).
//!
//! [`MissionHostSeed`] is what [`super::stage_for`] leaves on the stage: the
//! five records the window composition used to drop
//! (`docs/findings/2026-10-10-vs-m01-rt-window-composition.md`), the declared
//! control program, the objective program the F39 session launches from, the
//! cue table, the chain resolver and every absence the stage already knows
//! about. [`MissionHost::launch`] turns that seed into the live sessions, and
//! [`MissionHost::step`] advances them **in the documented order** for one
//! committed tick:
//!
//! 1. **environment** — [`EnvironmentSession::set_paused`] from
//!    [`PlaytestState`], then [`EnvironmentSession::advance_frame`] on the
//!    elapsed this step was handed;
//! 2. **world actors** — [`WorldActorSession::step`] to the host tick with no
//!    commands and no probes;
//! 3. **animations, markers and objectives** — exactly one
//!    [`step_mission_animations`] call, whose order is load-bearing (the
//!    record half runs before the mission half so a refusal can still be
//!    retried with the delivery it carries);
//! 4. **world observation** — the world's own report for the tick
//!    ([`MissionWorldObservation`], consumed so a report lives exactly one
//!    tick) merged with what the host itself observes — the player body's
//!    position and the record player's animation states — and fed to
//!    [`WorldFactTable::observe`];
//! 5. **objective lifecycles** — [`ObjectiveLifecycles::advance`], the
//!    measured per-frame timer pass, on the elapsed this step covers;
//! 6. **script host** — [`MissionSession::advance`] over the
//!    [`compose_mission_facts`] fold (which now carries the fresh lifecycle
//!    rows), then the completion pass: every `ObjectiveCompleted` event's
//!    block handed to [`ObjectiveLifecycles::complete`], so its
//!    `WAKE`/`NAP`/`KILL`/`SLEEP` effects run in the engine's own order. The
//!    two per-tick wake consumers follow: every `WAKE_ANIM` emission the
//!    animation ledger has not applied starts its named record
//!    ([`WakeAnimationLedger`]), and every `WAKEUP_ENEMIES` emission the enemy
//!    ledger has not applied releases the sleeping scripted aircraft it names
//!    ([`WakeEnemiesLedger`], M01-PLAY-AI-WAKE #1487).
//!
//! The answer is every record's **own** output for that tick
//! ([`MissionHostTick`]): the environment's committed ticks, the world-actor
//! tick, the animation [`TickReport`], the [`MarkerDelivery`], the objective
//! [`SessionTick`] and the control program's [`MissionTick`]. Nothing in that
//! struct is built by this module, and nothing is a default: a stage that
//! cannot produce one of them says so through [`MissionHostRefusal`] instead.
//!
//! # Absences are named, never filled in
//!
//! * The F39 objective declarations cannot be recovered for any original
//!   mission (`ObjectiveRecovery::program()` refuses unconditionally; Rally
//!   #1219 owns that measurement), so the seed carries the **empty**
//!   [`LoweredObjectives`] plus [`MissionHostRefusal::ObjectiveDeclarations`]
//!   quoting the reader's own message. No `ObjectiveSpec`, `CountCondition`,
//!   `MissionTimer`, `SweptTrigger` or spawn group is ever constructed here
//!   for an original mission (AGENTS.md rules 4 and 5).
//! * A stage that carries neither a declared [`BlockLifecycleTable`] nor a
//!   recovered [`EngineObjectives`] — because no production reader turned its
//!   record's `BEGIN_DORMANT` spellings into declarations (or that reader
//!   refused) — folds the empty table, its documented fail-closed read where
//!   every `Condition::ObjectiveAwake` answers `false`, under
//!   [`MissionHostRefusal::BlockLifecycles`]. A stage that carries the
//!   record's declared table (read through [`crate::control_lifecycles`])
//!   folds it, and a stage whose [`EngineObjectives`] recovered runs that
//!   instead; either raises no such refusal.
//! * The stage folds whatever the world reports this tick: a
//!   [`MissionWorldObservation`] the world's own observation system wrote
//!   this step (consumed on use) —
//!   [`super::observation::mission_world_observation`] is that system, the
//!   one [`install_mission_host`] installs before this entry — the player
//!   body's position and the record player's animation rows. A stage that
//!   never wrote one folds the empty report under
//!   [`MissionHostRefusal::WorldObservation`].
//! * A stage with no animation join, or whose scope lowered no world-actor
//!   program, is named by [`MissionHostRefusal::AnimationJoin`] and
//!   [`MissionHostRefusal::WorldActors`].
//!
//! `TickInput::terminal_requests` stays **empty** on every step: the only
//! `TerminalPrecedence` this build has is the designed conservative policy
//! `docs/contracts/SCRIPT-MISSION.md` reserves for synthetic tests, so a
//! retail terminal outcome comes from the control program's own
//! `INSTANTWIN`/`INSTANTLOSS` directives and never from a request this host
//! raises.
//!
//! # Ending a run
//!
//! A run ends on **one** terminal, from either of the two sources the
//! [`super::terminal`] module names, funnelled by
//! [`MissionHost::settle`]: the F39 objective session's own
//! `SessionTick::outcome` first (the source this stage names), then the
//! control program's own terminal state. The sequence, in this order: the
//! host stops stepping ([`MissionHostStepError::Settled`]; a settled run
//! advances nothing), the objective session's cue queue is drained (the count
//! on the [`MissionTerminal`] is what the session still owned, so cues the
//! player will never hear are named rather than dropped), the audio session's
//! pending queues are resolved, and the one report line is written. The
//! composed entry then sends [`MissionTerminal::exit`]'s `AppExit` as a
//! message, so the windowed run ends and the headless test can read it.
//!
//! # Restart: the authored initial state, rebuilt
//!
//! [`MissionHost::restart`] (VS-M01-RT-MISSION-HOST `.03`, Rally #1280)
//! rebuilds every record this host drives — and the world half beside them —
//! under a **fresh** [`SessionGeneration`] and [`SessionId`], in an order that
//! cannot leave the session half-rebuilt: the objective session's `retry`
//! runs first (its [`crate::objectives::TeardownReport`] names the wave
//! actors, cues, armed deadlines and settled outcome of the generation being
//! torn down, taken before the fresh runtime exists because the fresh
//! runtime reuses the old instance ids), then the marker consumer's `retry`,
//! the environment clock's `restart`, a fresh record player, a fresh
//! world-actor session and a fresh control session relaunched from the
//! stage's own stored programs, fresh fact tables, and only then the world
//! half — `unload_world` + `load_world` from the stage's definition,
//! instance and meshes — with the player body respawned from the stage's
//! start recipe so exactly one exists. A restart asked for the **live**
//! generation fails by name before anything is rebuilt
//! (`docs/contracts/IDENTITY-CONTENT.md`: no cross-session id reuse).
//!
//! Two triggers reach it through [`mission_host_restart`], the composed
//! `PostUpdate` system [`install_mission_host`] installs beside the per-tick
//! entry: the playtest's own meta reset (`R`, observed through
//! [`PlaytestState::resets`] — `perform_reset` runs in `Update`, so a latch
//! in `PostUpdate` is the first place the increment is visible), and a
//! [`MissionHostRestartRequest`] another system raises — the seam a terminal
//! handler latches a restart through.
//!
//! # What is not claimed
//!
//! No original executable ran; `retail` is read access to the owner's
//! installation. Nothing here is `verified_original`. A settled run now
//! exits with the outcome it maps to and a restart rebuilds the authored
//! initial state (`.02` and `.03` of VS-M01-RT-MISSION-HOST); the fresh
//! control session a restart launches starts `Running` again. M01's two
//! `Finish` blocks start dormant behind the `-1` sentinel and wake only
//! through the `NAP`/`WAKE`/`SLEEP` effects this host's completion pass now
//! applies — a run that reports no member presence cannot reach either one
//! (M01's whole wake graph passes through the three `INACTIVE` gates, a
//! measured property of the content), and a run that reports them can.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use avian3d::prelude::{LinearVelocity, Rotation};
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::prelude::{App, Entity, FixedPostUpdate, PostUpdate, Resource, Vec3, With, World};
use bevy::time::{Fixed, Time};
use cs_content::objectives_engine::EngineObjectives;
use cs_content::world::WorldObjectId;
use cs_script::ir::{AnimationState, MissionProgram};
use cs_script::runtime::{EventKind, SessionGeneration, TerminalState, TickError};
use cs_sim::mission::{BlockLifecycleTable, LaunchRefused, MissionSession, MissionTick};
use cs_sim::objectives::runtime::{ObjectiveEventKind, RuntimeLimits, TickInput};
use cs_sim::objectives::terminal::{TerminalOutcome, TerminalPrecedence};
use cs_sim::visibility::TimelineError;
use cs_sim::world_actors::runtime::WorldActorError;
use cs_types::Tick;
use cs_types::content::ContentId;
use cs_types::net::SessionId;

use super::compose::{MissionPlayerBody, MissionStage, spawn_player};
use super::landings::{LandingLedger, LandingTick, MissionLandings, step_landings};
use super::terminal::{MissionTerminal, MissionTerminalSource};
use crate::animation::mission::{AnimationNameIndex, MissionAnimationBinding, StartupAnimation};
use crate::audio::AudioSession;
use crate::environment::EnvironmentSession;
use crate::mission_ai::WakeEnemiesLedger;
use crate::mission_animations::{
    ArmedNode, CallAnimationLedger, MissionAnimationPlayer, MissionAnimationStepRefusal,
    PlayerError, TickReport, WakeAnimationLedger, armed_nodes_from_report, step_mission_animations,
};
use crate::mission_markers::{
    MarkerDelivery, MarkerTeardown, MarkerTeardownError, MissionMarkerBindings,
    MissionMarkerConsumer,
};
use crate::mission_session::SoundArchive;
use crate::objectives::{
    LoweredObjectives, ObjectiveSession, SessionLaunchError, SessionTick, TeardownReport,
};
use crate::physics::{BASELINE_FIXED_HZ, PhysicsTickLedger};
use crate::playtest::PlaytestState;
use crate::playtest::scene::SceneError;
use crate::score::SessionScoreLedger;
use crate::world::residency::{load_world, unload_world};
use crate::world_actors::{
    LoweredWorldActors, WorldActorLaunchError, WorldActorSession, WorldActorSessionTick,
    WorldActorTick,
};
use crate::world_facts::{
    MemberResolver, WorldFactTable, WorldObservation, WorldOperands, compose_mission_facts,
};

use super::lifecycles::{ObjectiveLifecycleTick, ObjectiveLifecycles};
use super::observation::mission_world_observation;
use super::radio::{MissionRadio, RadioLineOutcome};

/// The mission host's own session generation, minted from a process-wide
/// counter the way `SessionBuilder::open` mints a content session's: the
/// first host a process launches is generation 1 and every later one is
/// distinct, so two compositions in one process can never share a generation
/// (`docs/contracts/IDENTITY-CONTENT.md`).
static NEXT_GENERATION: AtomicU32 = AtomicU32::new(1);

/// Mints the session generation one [`MissionHost::launch`] serves.
///
/// A generation is a run parameter, not a recovered value: nothing in the
/// original installation records one. This derives it from process order
/// instead of declaring a magic number, so a mission evaluator's
/// generation-seeded RNG stream replays the same within one process and is
/// never shared between two sessions.
#[must_use]
pub fn mint_host_generation() -> SessionGeneration {
    SessionGeneration(NEXT_GENERATION.fetch_add(1, Ordering::Relaxed))
}

/// The [`SessionId`] a generation minted by [`mint_host_generation`] serves:
/// the animation record player and the marker consumer both key their
/// per-session state on it, and a live generation is never zero.
///
/// # Panics
///
/// Only for a generation of zero, which [`mint_host_generation`] never
/// returns.
#[must_use]
pub fn host_session_id(generation: SessionGeneration) -> SessionId {
    SessionId::new(u64::from(generation.0)).expect("a minted mission-host generation is never zero")
}

/// One absence the host runs over, named instead of filled in.
///
/// Every variant carries its detail as the refusing reader's or the
/// fail-closed table's **own** words, so a run always says what it did not
/// do (AGENTS.md rules 4 and 5).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MissionHostRefusal {
    /// The F39 objective declarations could not be recovered, so the objective
    /// session launches the empty program and no original objective runs.
    ObjectiveDeclarations {
        /// The recovery reader's own message, verbatim.
        detail: String,
    },
    /// The stage carries no animation join, so no startup row was offered to
    /// the record player.
    AnimationJoin {
        /// Why the join is absent.
        detail: String,
    },
    /// The scope lowered no world-actor program, so no world-actor session
    /// steps.
    WorldActors {
        /// The lowering's own verdict, or why the stage has none.
        detail: String,
    },
    /// No stage builds a `WorldObservation`, so the world fact fold answers
    /// from [`WorldFactTable`]'s empty, fail-closed report.
    WorldObservation {
        /// What the fold is reading instead of an observation.
        detail: String,
    },
    /// The stage carries neither a declared [`BlockLifecycleTable`] nor a
    /// recovered [`EngineObjectives`] — the record's lifecycle spellings were
    /// not read into either, or the readers refused — so the fold answers
    /// from the empty table, where every `Condition::ObjectiveAwake` answers
    /// `false` and no block can complete.
    BlockLifecycles {
        /// What the model is empty of, and whose job filling it is.
        detail: String,
    },
    /// The record's recovered objective declarations would not build an
    /// [`EngineObjectives`] — the engine's own range check refused — so only
    /// the declared dormancy table runs: no `WAKE`/`NAP`/`KILL`/`SLEEP`
    /// completion effects are applied.
    EngineObjectives {
        /// The engine constructor's own refusal.
        detail: String,
    },
    /// Mission-bound audio loops were still bound when the run ended.
    ///
    /// `cs_sim::audio_events::EmitterStopReason` measures no mission-end
    /// reason, so settling reports them rather than stopping them under a
    /// reason they did not have. The composition starts one — the player
    /// airframe's engine loop (#1313) — so on a machine that plays it the
    /// count is 1 at settle; [`super::teardown`] then despawns the emitter and
    /// the loop stops as `Despawned`, which is true of it.
    MissionAudioStillBound {
        /// How many loops the router still held.
        count: usize,
    },
    /// The session's kill-score ledger was missing, or still scored a
    /// torn-down generation, when the run settled.
    ///
    /// The ledger ([`crate::score::SessionScoreLedger`]) is the only store a
    /// `KillAwarded` event can move, so this run could not have recorded a
    /// score: the terminal's `score` is `0` — the honest answer for a run
    /// nothing could score — and this name says why, instead of a zero that
    /// looks like a run that scored nothing on its own.
    ScoreLedger {
        /// What the read-back found instead of this session's ledger.
        detail: String,
    },
}

impl fmt::Display for MissionHostRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ObjectiveDeclarations { detail } => {
                write!(f, "the objective declarations: {detail}")
            }
            Self::AnimationJoin { detail } => write!(f, "the animation join: {detail}"),
            Self::WorldActors { detail } => write!(f, "the world-actor program: {detail}"),
            Self::WorldObservation { detail } => {
                write!(f, "the world observation: {detail}")
            }
            Self::BlockLifecycles { detail } => {
                write!(f, "the block lifecycles: {detail}")
            }
            Self::EngineObjectives { detail } => {
                write!(f, "the engine objective lifecycle model: {detail}")
            }
            Self::MissionAudioStillBound { count } => write!(
                f,
                "the mission audio: {count} loop(s) were still bound when the run ended, and \
                 EmitterStopReason measures no mission-end reason to stop them under; the \
                 teardown stops them as despawned"
            ),
            Self::ScoreLedger { detail } => write!(f, "the kill score: {detail}"),
        }
    }
}

/// The records the mission host drives, read before a window, an app or an
/// entity exists and carried on the stage.
///
/// [`super::stage_for`] fills it from the [`super::MissionContent`] it
/// already prepares; the acceptance stages build one over the synthetic
/// harbor world to drive the same host without an installation.
#[derive(Clone, Debug)]
pub struct MissionHostSeed {
    /// The bound weather, running on this session's timeline and seeds.
    pub environment: EnvironmentSession,
    /// The scope's lowered world-actor program, when the scope lowered one.
    pub world_actors: Option<LoweredWorldActors>,
    /// The scope's animation join, when the stage read one.
    pub animation: Option<MissionAnimationBinding>,
    /// The mission's declared control program — the script host's own.
    pub control: MissionProgram,
    /// The control record's declared block lifecycles — its
    /// `BEGIN_DORMANT`/`TICK_DEPENDS_ON_OBJ` spellings read through
    /// [`crate::control_lifecycles`]. An empty table is the fail-closed
    /// read, named by [`MissionHostRefusal::BlockLifecycles`].
    pub block_lifecycles: BlockLifecycleTable,
    /// The recovered engine lifecycle model for the same record
    /// ([`crate::objectives::ObjectiveRecovery::engine_objectives`]), when
    /// every field the model needs loaded. A stage that carries one runs it
    /// as the `objectives` lifecycle — dormancy timers, `TICK_DEPENDS_ON_OBJ`,
    /// `WAKE`/`NAP`/`KILL`/`SLEEP` effects and all — and the declared table
    /// above stays only for the stage's own bookkeeping; a stage that
    /// carries neither folds the empty table's fail-closed read.
    pub engine_objectives: Option<EngineObjectives>,
    /// The sound-family archives in the mission's scope.
    pub sound_archives: Vec<SoundArchive>,
    /// The objective program the F39 session launches from. For every
    /// original mission today this is [`no_declared_objectives`] and the
    /// stage carries [`MissionHostRefusal::ObjectiveDeclarations`] with it.
    pub objectives: LoweredObjectives,
    /// The cue table the marker consumer resolves markers through. M01 ships
    /// none, so every gameplay cue is refused by name
    /// (`MarkerRefusal::UnboundCue`) — that refusal is the delivery's own,
    /// not a table this module invents.
    pub markers: MissionMarkerBindings,
    /// The chain resolver the world fact fold answers through.
    pub resolver: MemberResolver,
    /// Every absence the stage already knows about; [`MissionHost::launch`]
    /// appends the ones only it can see.
    pub refusals: Vec<MissionHostRefusal>,
    /// The reward ids the host may apply for this mission, resolved from the
    /// declared campaign content ([`crate::campaign::mission_reward_catalog`]).
    /// A stage with no declared campaign carries none, and a `GrantReward` in
    /// its program is then refused as `HostFault::UnknownReward` instead of
    /// being granted. An engine design, not an original rule.
    pub rewards: Vec<ContentId>,
    /// The mission's sound-group cues resolved to voice lines
    /// ([`super::radio`]): the group table, the members its declarations
    /// reach and each member's measured length, read at content preparation.
    /// A stage with no audio (the synthetic acceptance stages) carries
    /// [`MissionRadio::empty`], which answers every cue by refusing it.
    pub radio: MissionRadio,
    /// The mission's declared ground targets and the world group's authored
    /// damage set, bound against the mission's world container
    /// (M01-BRIDGE-CONVOY-SPAWN, #1566). The spawn host registers one
    /// damageable actor per bound target; a stage with no installation to read
    /// one from (the synthetic acceptance stages) carries the empty binding and
    /// spawns none.
    pub targets: crate::mission_targets::MissionTargetBinding,
}

/// Launches the control session of `seed` for `generation`.
///
/// The evaluator stream comes from the run's mission seed
/// ([`RunSeeds::mission_seed`](crate::environment::RunSeeds::mission_seed), held
/// by the stage's environment) and the mission's content id, never from the
/// generation counter: path is run seed -> `cs_script::runtime::evaluator_seed`
/// -> the `MSN_EVAL` domain stream. The reward catalog is the stage's declared
/// one.
fn launch_script(
    seed: &MissionHostSeed,
    generation: SessionGeneration,
) -> Result<MissionSession, cs_sim::mission::LaunchRefused> {
    MissionSession::launch_seeded(
        seed.control.clone(),
        generation,
        seed.environment.seeds().mission_seed(),
        seed.rewards.iter().cloned(),
    )
}

/// The lifecycle model one stage runs: the recovered
/// [`EngineObjectives`] when the seed carries one, else the record's declared
/// [`BlockLifecycleTable`] — which may be the empty fail-closed read
/// [`stage_refusals`] names.
fn seed_lifecycles(seed: &MissionHostSeed) -> ObjectiveLifecycles {
    seed.engine_objectives.clone().map_or_else(
        || ObjectiveLifecycles::declared(seed.block_lifecycles.clone()),
        ObjectiveLifecycles::engine,
    )
}

/// The lowered objective program a stage carries when no declared program
/// could be recovered: no objectives, no count conditions, no timers, no
/// swept triggers and no spawn groups.
///
/// This is **not** a program this module authored — it is the absence of one,
/// and every stage that carries it also carries
/// [`MissionHostRefusal::ObjectiveDeclarations`] naming why no declared
/// program exists (for an original mission, the reader's own refusal). Its `precedence` field can only be
/// [`TerminalPrecedence::SyntheticConservative`], the one variant this build
/// has, which is why the composed entry never raises
/// `TickInput::terminal_requests`: with no declarations the field is never
/// consulted, and a retail terminal outcome belongs to the control program.
#[must_use]
pub fn no_declared_objectives() -> LoweredObjectives {
    LoweredObjectives {
        precedence: TerminalPrecedence::SyntheticConservative,
        limits: RuntimeLimits::default(),
        objectives: Vec::new(),
        conditions: Vec::new(),
        timers: Vec::new(),
        triggers: Vec::new(),
        spawn_groups: BTreeMap::new(),
    }
}

/// Why a stage's records could not be launched as a session.
///
/// Each variant carries the refusing constructor's own message
/// (`docs/contracts/CLI-EVIDENCE.md`).
#[derive(Debug)]
pub enum MissionHostLaunchError {
    /// The animation record player refused the host's timeline or session.
    Animation(PlayerError),
    /// The lowered world-actor program refused registration.
    WorldActors(WorldActorLaunchError),
    /// The objective session refused the declared program.
    Objectives(SessionLaunchError),
    /// The control program refused to validate and launch.
    Script(LaunchRefused),
}

impl fmt::Display for MissionHostLaunchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Animation(error) => write!(f, "the animation record player refuses: {error}"),
            Self::WorldActors(error) => {
                write!(f, "the world-actor session refuses to launch: {error}")
            }
            Self::Objectives(error) => {
                write!(f, "the objective session refuses to launch: {error}")
            }
            Self::Script(error) => {
                write!(f, "the control program refuses to launch: {}", error.error)
            }
        }
    }
}

impl std::error::Error for MissionHostLaunchError {}

/// Why one composed step did not run every stage.
///
/// Every variant names the record that refused; a refusal is never returned
/// as a half-successful answer (`docs/contracts/CLI-EVIDENCE.md`).
#[derive(Debug)]
pub enum MissionHostStepError {
    /// The world carries no [`PhysicsTickLedger`], so the host has no fixed
    /// timeline to step on.
    Timeline,
    /// The environment clock refused the frame.
    Environment(TimelineError),
    /// The world-actor session refused the asked-for tick.
    WorldActors(WorldActorError),
    /// One half of the composed animation/marker/objective step refused.
    Composed(MissionAnimationStepRefusal),
    /// The control program refused the tick.
    Script(TickError),
    /// This host has already settled on a terminal outcome: a settled run
    /// answers "already settled" and advances nothing.
    Settled,
}

impl fmt::Display for MissionHostStepError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Timeline => write!(
                f,
                "the world has no physics tick ledger, so the mission host has no fixed tick to \
                 advance on"
            ),
            Self::Environment(error) => write!(f, "the environment clock refuses: {error}"),
            Self::WorldActors(error) => write!(f, "the world-actor session refuses: {error:?}"),
            Self::Composed(error) => write!(f, "the composed animation step refuses: {error}"),
            Self::Script(TickError::NotAdvancing { last, given }) => write!(
                f,
                "the control program refuses a tick that does not advance: last {}, given {}",
                last.0, given.0
            ),
            Self::Settled => write!(
                f,
                "the mission host has already settled on a terminal outcome, so a settled run \
                 advances nothing"
            ),
        }
    }
}

impl std::error::Error for MissionHostStepError {}

/// What every record of one composed tick produced — the host's whole
/// answer, read off the records themselves.
///
/// Nothing here is constructed by the host: each field is the output of the
/// record named in its doc comment, so a test that asserts on this asserts on
/// production state.
#[derive(Debug)]
pub struct MissionHostTick {
    /// The committed fixed tick these answers belong to.
    pub tick: Tick,
    /// Whole ticks [`EnvironmentSession::advance_frame`] committed for this
    /// step's elapsed.
    pub environment_ticks: u64,
    /// What [`WorldActorSession::step`] answered, when the stage carries a
    /// world-actor program; `None` when it does not (named by
    /// [`MissionHostRefusal::WorldActors`]).
    pub world_actors: Option<WorldActorSessionTick>,
    /// What the animation record player published for this tick.
    pub animation: TickReport,
    /// What the marker consumer drained and admitted for this tick.
    pub markers: MarkerDelivery,
    /// What the objective session answered for this tick.
    pub objectives: SessionTick,
    /// What the objective lifecycle model applied on this tick: the timer
    /// pass's own report (woken / napped / slept) plus the blocks the
    /// completion pass completed, whose `WAKE`/`NAP`/`KILL`/`SLEEP` effects
    /// already ran.
    pub lifecycles: ObjectiveLifecycleTick,
    /// What the control program answered for this tick.
    pub script: MissionTick,
    /// What the mission's sound-group cues produced on this tick, in
    /// emission order: the voice lines handed to the audio session's radio
    /// queue, the music state requests, and every cue that was refused
    /// by name ([`super::radio`]). Empty when this tick emitted no cue.
    ///
    /// `Queued` is the offer, not the queue's own verdict: the radio queue
    /// answers with `RadioEvent`s on the session, so a refusal of its own (a
    /// full pending queue, a duplicate id, a foreign session) is named there
    /// and in the mixer's radio report rather than dropped.
    pub radio: Vec<RadioLineOutcome>,
}

/// The world's own report of what the mission's members, groups and
/// actors did — written by the world's observation system once per host
/// step and consumed by it, so a report describes exactly one tick.
///
/// The host merges this with what it observes itself — the record player's
/// measured animation states, keyed by each record's startup identity — and
/// feeds the merged rows to [`WorldFactTable::observe`]. A host step that
/// finds no report folds the members the stage could not see as absent
/// (fail-closed), which [`MissionHostRefusal::WorldObservation`] already
/// names for the stage that never builds one at all.
#[derive(Resource, Clone, Debug, Default)]
pub struct MissionWorldObservation {
    /// The world's rows for this step, in the observation's own vocabulary.
    pub observation: WorldObservation,
}

/// What the composed entry answered for one host tick, or the refusal that
/// stopped it.
///
/// [`mission_host_tick`] writes this after **every** attempt, so a world that
/// carries no report has run no composed step at all — which is exactly what
/// a test that deletes the composed entry observes.
#[derive(Resource, Debug)]
pub struct MissionHostReport {
    /// The host tick the step was asked for.
    pub tick: Tick,
    /// Every record's own answer, or the stage that refused.
    pub answer: Result<MissionHostTick, MissionHostStepError>,
}

/// Why a restart could not rebuild the authored initial state.
///
/// Every variant names the record that refused, in the order the restart
/// runs them; a refusal is never returned as a half-successful answer
/// (`docs/contracts/CLI-EVIDENCE.md`).
#[derive(Debug)]
pub enum MissionHostRestartError {
    /// The generation asked for is the live one, so the rebuilt sessions
    /// would carry the very stamp the torn-down artifacts already do
    /// (`IDENTITY-CONTENT`: no cross-session id reuse). Refused before
    /// anything was rebuilt.
    SameGeneration {
        /// The generation that was asked for and already serves.
        session: SessionGeneration,
    },
    /// The objective session refused to relaunch the declared program.
    Objectives(SessionLaunchError),
    /// The marker consumer refused the new session id.
    Markers(MarkerTeardownError),
    /// The environment clock refused to rebuild from the authored
    /// definition.
    Environment(TimelineError),
    /// The animation record player refused the host's session or timeline.
    Animation(PlayerError),
    /// The world-actor session refused to relaunch the lowered program.
    WorldActors(WorldActorLaunchError),
    /// The control program refused to validate and launch again.
    Script(LaunchRefused),
    /// The world half refused: the reload would not load from the stage's
    /// own definition, instance and meshes (the unload is a no-op when
    /// nothing is resident, and reports so through an empty despawn list).
    World(crate::world::WorldLoadError),
    /// The player body could not be respawned through the production
    /// physics path.
    Player(SceneError),
}

impl fmt::Display for MissionHostRestartError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SameGeneration { session } => write!(
                f,
                "generation {session:?} is the live mission-host generation, so a restart that \
                 reused it would re-stamp the cues, waves and events of a session that is still \
                 running (IDENTITY-CONTENT: no cross-session id reuse)"
            ),
            Self::Objectives(error) => {
                write!(f, "the objective session refuses to relaunch: {error}")
            }
            Self::Markers(error) => {
                write!(f, "the marker consumer refuses the new session: {error}")
            }
            Self::Environment(error) => write!(f, "the environment clock refuses: {error}"),
            Self::Animation(error) => {
                write!(f, "the animation record player refuses: {error}")
            }
            Self::WorldActors(error) => {
                write!(f, "the world-actor session refuses to relaunch: {error}")
            }
            Self::Script(error) => {
                write!(
                    f,
                    "the control program refuses to launch again: {}",
                    error.error
                )
            }
            Self::World(error) => write!(f, "the mission's world refuses to reload: {error}"),
            Self::Player(error) => write!(f, "the player's body refuses to respawn: {error}"),
        }
    }
}

impl std::error::Error for MissionHostRestartError {}

/// What one restart tore down and rebuilt — the previous generation's
/// remaining ownership as data a caller can assert on.
///
/// Nothing here is narrated: `objectives` is the objective session's own
/// [`TeardownReport`] (the generation it tore down, the wave actors to
/// despawn, the cues that will now never play, the deadlines that were still
/// armed, the outcome it settled), `markers` is the marker ledger's own
/// release, and the two world lists are the residency's own despawn and
/// respawn answers for the stage's instance.
#[derive(Resource, Clone, Debug)]
pub struct MissionHostRestartReport {
    /// The generation whose sessions were torn down.
    pub torn_down: SessionGeneration,
    /// The generation the rebuilt sessions now serve.
    pub started: SessionGeneration,
    /// The session id the record player and the marker consumer now serve.
    pub served: SessionId,
    /// What the objective session's `retry` reported before the fresh
    /// runtime replaced it.
    pub objectives: TeardownReport,
    /// What the marker consumer's `retry` released.
    pub markers: MarkerTeardown,
    /// The world objects the unload despawned, in the order the residency
    /// named them; empty when nothing was resident.
    pub world_despawned: Vec<WorldObjectId>,
    /// The world objects the reload respawned from the stage's own instance.
    pub world_respawned: Vec<WorldObjectId>,
}

/// A restart asked for by another system, latched until
/// [`mission_host_restart`] consumes it.
///
/// This is the seam a terminal handler latches a restart through ("a restart
/// requested after a terminal state"): inserting the resource is a request,
/// the composed `PostUpdate` system takes it, and the fresh control session
/// the restart launches starts `Running` again. The playtest's own meta
/// reset (`R`) reaches the same system through [`PlaytestState::resets`]
/// without going through here.
#[derive(Resource, Debug, Default)]
pub struct MissionHostRestartRequest;

/// A restart the composed system could not perform, kept so a run says what
/// it did not do rather than silently keeping the old sessions
/// (`docs/contracts/CLI-EVIDENCE.md`).
#[derive(Resource, Debug)]
pub struct MissionHostRestartFailure {
    /// The refusal itself, named by the record that produced it.
    pub error: MissionHostRestartError,
}

/// The mission host of one composition: every record the stage carries, plus
/// the sessions launched from them.
///
/// It is a plain value the fixed-tick schedule owns as a resource;
/// [`mission_host_tick`] takes it out of the world for the duration of one
/// step, because [`step_mission_animations`] needs `&mut World` while these
/// records live inside it.
#[derive(Resource, Debug)]
pub struct MissionHost {
    environment: EnvironmentSession,
    world_actors: Option<WorldActorSession>,
    animation: MissionAnimationPlayer,
    /// The mission scope's measured animation **name → carrier record** index:
    /// the join a `WAKE_ANIM` directive's name resolves through. Empty when the
    /// stage carries no animation join, in which case every name such a
    /// directive spells is refused by name.
    names: AnimationNameIndex,
    wake: WakeAnimationLedger,
    /// The `CALL_ANIMATION` consumer's ledger: every spelled call target a
    /// published statement named and what it started, in publish order.
    /// Cleared on restart beside `wake` — a fresh generation's calls are its
    /// own.
    calls: CallAnimationLedger,
    /// The `WAKEUP_ENEMIES` consumer's ledger (M01-PLAY-AI-WAKE, #1487):
    /// which directive keys were applied, which sleeping scripted aircraft
    /// they released, and every name that woke nothing and why.
    enemies: WakeEnemiesLedger,
    armed: Vec<ArmedNode>,
    /// The landing path's own record of one generation (M01-PLAY-TRIGGERS-
    /// ANIMS-LAND, #1498): which rows a landing has already attempted, what
    /// those landings started or refused, and the call chains they opened.
    /// Cleared on restart beside `wake` and `armed` — a fresh generation's
    /// landings are its own.
    landings: LandingLedger,
    markers: MissionMarkerConsumer,
    objectives: ObjectiveSession,
    script: MissionSession,
    lifecycles: ObjectiveLifecycles,
    world_facts: WorldFactTable,
    operands: WorldOperands,
    sound_archives: Vec<SoundArchive>,
    radio: MissionRadio,
    /// The music state selector behind this generation's `MusicDirector`
    /// requests (M01-PLAY-MUSIC, #1463): the mission seed's own draw stream
    /// and the resolved music groups' weights and counters, armed at launch
    /// and rebuilt with every restart so one generation's picks never bleed
    /// into the next's.
    music: crate::audio::MusicSelector,
    refusals: Vec<MissionHostRefusal>,
    generation: SessionGeneration,
    served: SessionId,
    stepped: Option<Tick>,
    settled: Option<MissionTerminal>,
}

impl MissionHost {
    /// Launches one host from `stage`'s seed for `generation`, serving
    /// `served`.
    ///
    /// Every record is launched through its own production constructor — the
    /// world-actor session, the record player, the marker consumer, the
    /// objective session and the control program — and a refusal from any of
    /// them is returned by name rather than worked around. The stage's own
    /// refusals are carried over and the ones only this launch can see (no
    /// animation join, no world-actor program, the unobserved fact fold, and
    /// — for a stage that carries no declared [`BlockLifecycleTable`] and
    /// has not named that absence itself — the undeclared block lifecycles)
    /// are appended, so [`Self::refusals`] is the complete list a run
    /// reports.
    ///
    /// # Errors
    ///
    /// [`MissionHostLaunchError`] naming the constructor that refused.
    pub fn launch(
        stage: &MissionStage,
        generation: SessionGeneration,
        served: SessionId,
    ) -> Result<Self, MissionHostLaunchError> {
        let seed = &stage.host;

        let environment = seed.environment.clone();

        let world_actors = match &seed.world_actors {
            Some(lowered) => Some(
                WorldActorSession::launch(lowered.clone(), generation)
                    .map_err(MissionHostLaunchError::WorldActors)?,
            ),
            None => None,
        };

        let mut animation = MissionAnimationPlayer::new(served, BASELINE_FIXED_HZ)
            .map_err(MissionHostLaunchError::Animation)?;
        if let Some(binding) = &seed.animation {
            offer_startup_rows(&mut animation, binding);
        }
        let names = seed
            .animation
            .as_ref()
            .map_or_else(AnimationNameIndex::default, |binding| {
                binding.names().clone()
            });
        let markers = MissionMarkerConsumer::new(served, seed.markers.clone());
        let objectives = ObjectiveSession::launch(seed.objectives.clone(), generation)
            .map_err(MissionHostLaunchError::Objectives)?;
        let script = launch_script(seed, generation).map_err(MissionHostLaunchError::Script)?;

        let refusals = stage_refusals(seed);
        let lifecycles = seed_lifecycles(seed);

        Ok(Self {
            environment,
            world_actors,
            animation,
            names,
            wake: WakeAnimationLedger::default(),
            calls: CallAnimationLedger::default(),
            enemies: WakeEnemiesLedger::default(),
            armed: Vec::new(),
            landings: LandingLedger::default(),
            markers,
            objectives,
            script,
            lifecycles,
            world_facts: WorldFactTable::new(seed.resolver.clone()),
            operands: WorldOperands::of(&seed.control),
            sound_archives: seed.sound_archives.clone(),
            radio: seed.radio.clone(),
            music: crate::audio::MusicSelector::launch(
                seed.environment.seeds().mission_seed(),
                seed.radio.music_arms(),
            ),
            refusals,
            generation,
            served,
            stepped: None,
            settled: None,
        })
    }

    /// Rebuilds the authored initial state of `stage` under `generation`,
    /// leaving nothing of the previous session behind.
    ///
    /// The order is the one that cannot leave the session half-rebuilt:
    ///
    /// 1. [`ObjectiveSession::retry`] **first** — its
    ///    [`TeardownReport`] names the wave actors to despawn, the cues that
    ///    will never play, the armed deadlines and the settled outcome, taken
    ///    before the fresh runtime exists because the fresh runtime reuses
    ///    the old instance ids;
    /// 2. [`MissionMarkerConsumer::retry`] — the marker ledger's releases;
    /// 3. [`EnvironmentSession::restart`] — the authored weather at tick
    ///    zero again, keeping the run's seeds;
    /// 4. a fresh [`MissionAnimationPlayer`] with the stage's join rows
    ///    offered again at its tick zero;
    /// 5. a fresh [`WorldActorSession`] relaunched from the stage's stored
    ///    [`LoweredWorldActors`];
    /// 6. a fresh [`MissionSession`] relaunched from the stage's stored
    ///    control program — its state is `Running` again, so whatever
    ///    terminal the previous generation settled is cleared with the
    ///    session that carried it;
    /// 7. the seed's lifecycle model restored fresh — the recovered
    ///    [`EngineObjectives`] at its own load state, or the declared
    ///    [`BlockLifecycleTable`]'s spelled starting states, never anything
    ///    the torn-down run advanced — a fresh [`WorldFactTable`], and the
    ///    complete refusal list recomputed from the stage's seed;
    /// 8. the host's own records cleared: `last_tick()` is `None` (the
    ///    world's fixed ledger is the physics timeline and keeps counting;
    ///    this host keeps no clock of its own) and [`Self::terminal`] is
    ///    `None` — the terminal the previous generation settled is this
    ///    host's own mark of a run that has ended, and the fresh session
    ///    starts `Running`, so a rebuilt host is never already settled;
    /// 9. the world half: [`unload_world`] then [`load_world`] from the
    ///    stage's own definition, instance and meshes;
    /// 10. the player body respawned from the stage's start recipe, so
    ///     exactly one exists.
    ///
    /// `SessionGeneration` and `SessionId` both advance: a restart asked for
    /// the **live** generation fails by name before anything is rebuilt
    /// (`docs/contracts/IDENTITY-CONTENT.md` — a rebuilt session's cues,
    /// waves and events would carry the very stamp the torn-down artifacts
    /// already do).
    ///
    /// The world half reaches the residency through a scratch-`App` world
    /// swap: [`load_world`]/[`unload_world`] take `&mut App` while this
    /// method has only `&mut World`, and every `app.` access those two make
    /// is `app.world()`/`app.world_mut()` (measured across
    /// `world/residency.rs` and `world/spawn.rs`), so an `App` whose main
    /// world *is* the live one behaves exactly as the composition's own.
    /// `docs/findings/2026-10-11-vs-m01-rt-mission-host-restart.md` records
    /// the choice and the two rejected options.
    ///
    /// # Errors
    ///
    /// [`MissionHostRestartError`] naming the record that refused. Each
    /// constructor is all-or-nothing (the retry paths build the fresh value
    /// before releasing the old one), and the world half is
    /// [`load_world`]'s own transaction — but a refusal after the sessions
    /// were rebuilt leaves the fresh sessions standing with the error
    /// reported by name, never a silent half-restart.
    pub fn restart(
        &mut self,
        world: &mut World,
        stage: &MissionStage,
        generation: SessionGeneration,
    ) -> Result<MissionHostRestartReport, MissionHostRestartError> {
        if generation == self.generation {
            return Err(MissionHostRestartError::SameGeneration {
                session: generation,
            });
        }
        let served = host_session_id(generation);
        let seed = &stage.host;

        let objectives = self
            .objectives
            .retry(generation)
            .map_err(MissionHostRestartError::Objectives)?;
        let markers = self
            .markers
            .retry(served)
            .map_err(MissionHostRestartError::Markers)?;
        self.environment
            .restart()
            .map_err(MissionHostRestartError::Environment)?;

        let mut animation = MissionAnimationPlayer::new(served, BASELINE_FIXED_HZ)
            .map_err(MissionHostRestartError::Animation)?;
        if let Some(binding) = &seed.animation {
            offer_startup_rows(&mut animation, binding);
        }
        self.animation = animation;
        // The name index and the wake ledgers follow the generation: a
        // rebuilt host resolves names through the same measured join and has
        // applied no directive of the fresh session yet.
        self.names = seed
            .animation
            .as_ref()
            .map_or_else(AnimationNameIndex::default, |binding| {
                binding.names().clone()
            });
        self.wake = WakeAnimationLedger::default();
        self.calls = CallAnimationLedger::default();
        self.enemies = WakeEnemiesLedger::default();
        self.armed.clear();
        self.landings = LandingLedger::default();
        // `markers.retry(served)` above already released the old ledger and
        // stamped the new session on the live consumer, whose declared cue
        // table is the stage's own — nothing else about it changes.
        self.world_actors = match &seed.world_actors {
            Some(lowered) => Some(
                WorldActorSession::launch(lowered.clone(), generation)
                    .map_err(MissionHostRestartError::WorldActors)?,
            ),
            None => None,
        };
        self.script = launch_script(seed, generation).map_err(MissionHostRestartError::Script)?;
        self.lifecycles = seed_lifecycles(seed);
        self.world_facts = WorldFactTable::new(seed.resolver.clone());
        self.operands = WorldOperands::of(&seed.control);
        self.sound_archives = seed.sound_archives.clone();
        // The selector goes with its generation: the fresh session arms the
        // same mission seed's stream from its first draw, with the authored
        // weights and the shared objective counter at zero — a torn-down
        // generation's picks never bleed into the rebuilt one's.
        self.music = crate::audio::MusicSelector::launch(
            seed.environment.seeds().mission_seed(),
            seed.radio.music_arms(),
        );
        self.refusals = stage_refusals(seed);
        self.stepped = None;
        // The terminal is this host's own record of a run that has ended
        // (`.02`'s `settled` mark, which `step` and the composed entry read
        // as "advance nothing"). The fresh control session starts `Running`,
        // so the mark of the generation just torn down goes with it: a
        // rebuilt host is never already settled.
        self.settled = None;
        self.generation = generation;
        self.served = served;

        let world_despawned: Vec<WorldObjectId> = with_app_world(world, |app| {
            unload_world(app).map_or_else(Vec::new, |load| load.despawned)
        });
        let respawned = with_app_world(world, |app| {
            load_world(app, &stage.world, &stage.instance, &stage.meshes)
        })
        .map_err(MissionHostRestartError::World)?;
        let world_respawned: Vec<WorldObjectId> = respawned
            .objects()
            .iter()
            .map(|spawned| spawned.object.clone())
            .collect();

        let bodies: Vec<Entity> = {
            let mut query = world.query_filtered::<Entity, With<MissionPlayerBody>>();
            query.iter(world).collect()
        };
        for body in bodies {
            let _ = world.despawn(body);
        }
        spawn_player(world).map_err(MissionHostRestartError::Player)?;

        // The scripted actors go with their generation: despawned before
        // the respawn so a restart leaves exactly the roster's set, and
        // respawned under the new `served` session — their damage actors are
        // re-registered on the fresh resolver `respawn_mission_actors`
        // installs for that session (M01-PLAY-ACTORS, #1435). The host is
        // mid-restart here — not yet a world resource — so it is handed
        // over by reference.
        crate::mission_actors::despawn_mission_actors(world);
        crate::mission_actors::respawn_mission_actors(world, self, stage);

        // The previous composed step's report describes the generation just
        // torn down; it is removed so this restart leaves no stale answer and
        // the next step writes this generation's own.
        world.remove_resource::<MissionHostReport>();

        Ok(MissionHostRestartReport {
            torn_down: objectives.session,
            started: generation,
            served,
            objectives,
            markers,
            world_despawned,
            world_respawned,
        })
    }

    /// Advances every record this host owns for one committed tick, in the
    /// documented order, and answers with what each of them produced.
    ///
    /// `elapsed` is the gameplay time this step covers — the composition
    /// hands it one fixed tick's timestep, so the environment commits one
    /// authored weather tick per host tick on the composition's own
    /// `physics::BASELINE_FIXED_HZ` timeline. `world` is the ECS world the
    /// animation/markers/objectives half drains its markers from and steps
    /// its objective session against; the host itself must not be a resource
    /// of it while this runs (see [`mission_host_tick`]).
    ///
    /// The tick comes from [`PhysicsTickLedger`], the world's authoritative
    /// count of committed fixed ticks — this host keeps no clock of its own.
    ///
    /// # Errors
    ///
    /// [`MissionHostStepError`] naming the record that refused. A refusal
    /// stops the step: the stages before it have run and the stages after it
    /// have not, and the report says so instead of answering with a partial
    /// success.
    pub fn step(
        &mut self,
        world: &mut World,
        elapsed: Duration,
    ) -> Result<MissionHostTick, MissionHostStepError> {
        if self.settled.is_some() {
            return Err(MissionHostStepError::Settled);
        }
        let tick = {
            let Some(ledger) = world.get_resource::<PhysicsTickLedger>() else {
                return Err(MissionHostStepError::Timeline);
            };
            Tick(ledger.ticks)
        };
        // The tick is recorded before anything runs: a stage that advanced and
        // then refused must not be offered the same tick twice, because the
        // animation record player and the objective session both refuse a
        // repeated tick.
        self.stepped = Some(tick);
        let paused = world
            .get_resource::<PlaytestState>()
            .is_some_and(|state| state.paused);

        // 1. Environment: the pause reaches the authored weather first, then
        //    the frame's elapsed advances it.
        self.environment.set_paused(paused);
        let environment_ticks = self
            .environment
            .advance_frame(elapsed)
            .map_err(MissionHostStepError::Environment)?;

        // 2. World actors: to the host tick, with no commands and no probes —
        //    this stage schedules nothing on its behalf.
        let world_actors = match &mut self.world_actors {
            Some(session) => Some(
                session
                    .step(&WorldActorTick {
                        to: tick,
                        commands: Vec::new(),
                        probes: Vec::new(),
                    })
                    .map_err(MissionHostStepError::WorldActors)?,
            ),
            None => None,
        };

        // 3. Animations, markers and objectives: exactly one composed call.
        //    A paused frame commits zero gameplay ticks, so the objective
        //    session's timers do not advance while the playtest is paused.
        let input = TickInput {
            committed_ticks: if paused { 0 } else { 1 },
            ..TickInput::at(tick)
        };
        let composed = step_mission_animations(
            world,
            &mut self.animation,
            &mut self.markers,
            &mut self.objectives,
            &input,
        )
        .map_err(MissionHostStepError::Composed)?;

        // 3b. Call animations: every `CALL_ANIMATION` statement this tick's
        //     record report published names the record it calls (a measured
        //     payload field). Starting it here — inside the animation stage,
        //     before the stage-4 observation — is what lets a call's target
        //     report `RUNNING` on the tick its caller's statement published,
        //     which is the only observation a zero-duration record can ever
        //     report `RUNNING` in.
        self.calls
            .consume(&composed.records, &self.names, &mut self.animation, tick);

        // 4. World observation: the world's own report for this step —
        //    consumed, so it describes exactly this tick — merged with what
        //    the host itself observes: the record player's measured
        //    animation states, keyed by each record's startup identity, the
        //    only name the player honestly holds.
        let mut observation = world
            .remove_resource::<MissionWorldObservation>()
            .map_or_else(WorldObservation::new, |report| report.observation);
        for record in self.animation.running() {
            observation =
                observation.animation(record.identity().to_owned(), AnimationState::Running.code());
        }
        for record in self.animation.finished() {
            observation = observation.animation(
                record.identity().to_owned(),
                AnimationState::Executed.code(),
            );
        }
        self.world_facts.observe(observation);

        // 5. Objective lifecycles: the measured per-frame timer pass on the
        //    elapsed this step covers, before the control program reads the
        //    fresh lifecycle rows — a paused frame covers no gameplay time,
        //    so its mission clock does not advance.
        let lifecycles_advanced =
            self.lifecycles
                .advance(if paused { 0.0 } else { elapsed.as_secs_f64() });

        // 6. Script host: the production fact fold over the fresh lifecycle
        //    rows, then the program's own answer for this tick; the
        //    completion pass then hands every completed block to the
        //    lifecycle model, whose WAKE/NAP/KILL/SLEEP effects run in the
        //    engine's own order.
        let lifecycle_facts = self.lifecycles.facts();
        let facts = compose_mission_facts(
            &self.script,
            &lifecycle_facts,
            &self.world_facts,
            &self.operands,
        );
        let script = self
            .script
            .advance(&facts, tick)
            .map_err(MissionHostStepError::Script)?;
        let mut completed = Vec::new();
        for event in &script.events {
            if event.kind == EventKind::ObjectiveCompleted {
                completed.push(event.key.source.0);
            }
        }
        for index in &completed {
            self.lifecycles.complete(*index);
        }
        // 6b. Wake animations: every `WAKE_ANIM` emission this host has not
        //     already applied is resolved through the scope's own measured
        //     name index and started through the record player at the tick
        //     being stepped. The script advance has already run, so this
        //     consumes the emissions of this tick and no earlier one (the
        //     ledger deduplicates by execution key, the same rule the
        //     countdown applies to the timer directives). A record started
        //     here is not observed until the next step's stage 4: the fold
        //     reads the player's state after this tick's `advance` already
        //     ran, and the record's own statements publish on the `advance`
        //     after its start tick.
        let _woken = self
            .wake
            .consume(self.script.state(), &self.names, &mut self.animation, tick);
        //     The designed arming input: the active-state statements this
        //     tick's report published, with the publishing record's own node
        //     names (see `OBJECT_ACTIVE_ARMING_CLAIM`).
        self.armed
            .extend(armed_nodes_from_report(&composed.records, &self.names));
        // 6c. Wake enemies: every `WAKEUP_ENEMIES` emission this host has not
        //     already applied releases the sleeping scripted aircraft it names
        //     (M01-PLAY-AI-WAKE, #1487). The release swaps the held kinematic
        //     body for the dynamic AI body the awake records spawn as; the
        //     next fixed tick's arm/decide pass then flies it, so a release
        //     made on this tick is visible to the AI on the following one.
        let _woken_enemies = self.enemies.consume(world, self.script.state(), tick);
        // 6d. Landings (M01-PLAY-TRIGGERS-ANIMS-LAND, #1498): the designed
        //     landing path reads this tick's stage-4 observation — the sole
        //     authority for the player's pose and for each node's composed
        //     position — plus the armed set just extended above, and starts
        //     every row a landing satisfies through this host's own player.
        //     The records it starts publish their statements on the next
        //     script advance, so the stage-4 fold of the **next** step is
        //     the first place they (and the gates their calls satisfy) are
        //     observed, exactly as a `WAKE_ANIM`-started record behaves.
        self.step_landings(world, tick);
        let lifecycles = ObjectiveLifecycleTick {
            woken: lifecycles_advanced.woken,
            napped: lifecycles_advanced.napped,
            slept: lifecycles_advanced.slept,
            completed,
        };

        // 7. Sound: every sound-group cue this tick's lifecycle transitions
        //    emitted, taken from the model so each is offered exactly once,
        //    resolved through the mission's group table and offered to the
        //    audio session's radio queue. A world with no audio session
        //    answers every cue by naming it, never by dropping it.
        let cues = self.lifecycles.take_cues();
        let mut session = world.get_resource_mut::<AudioSession>();
        let radio =
            super::radio::offer_cues(&cues, &self.radio, &mut self.music, session.as_deref_mut());

        Ok(MissionHostTick {
            tick,
            environment_ticks,
            world_actors,
            animation: composed.records,
            markers: composed.mission.markers,
            objectives: composed.mission.tick,
            lifecycles,
            script,
            radio,
        })
    }

    /// Runs the terminal sequence over one stepped tick, when either of the
    /// two sources has settled the run, and keeps the terminal on the host.
    ///
    /// **Two sources, one funnel.** The F39 objective session's own
    /// [`SessionTick::outcome`] is consulted first — the source this stage
    /// names, and the one an original mission's declared conditions, timers
    /// and count reactions would settle through — and then the control
    /// program's own terminal state (`MissionTick::terminal`, which for M01
    /// is its measured `INSTANTWIN`/`INSTANTLOSS` lowering). The record that
    /// settles first ends the run, and [`MissionTerminal::source`] says which.
    /// Nothing here *raises* an outcome: the designed conservative
    /// `TerminalPrecedence` is reserved by
    /// `docs/contracts/SCRIPT-MISSION.md` for synthetic tests, so this host
    /// never fills `TickInput::terminal_requests` for a retail-derived
    /// program.
    ///
    /// The sequence, in order:
    ///
    /// 1. the host is marked settled, so [`Self::step`] answers
    ///    [`MissionHostStepError::Settled`] and a settled run advances
    ///    nothing;
    /// 2. the objective session's cue queue is drained — the count on the
    ///    terminal is what the session **still owned**, so cues the player
    ///    will never hear are named rather than dropped;
    /// 3. the audio session's pending queues are resolved
    ///    (`AudioSession::drain`/`drain_radio`), and whatever loops are
    ///    still bound are counted and named under
    ///    [`MissionHostRefusal::MissionAudioStillBound`] — the router's
    ///    `EmitterStopReason` measures no mission-end reason, so they are
    ///    reported rather than stopped under a reason they did not have (the
    ///    teardown that follows despawns their emitters, which stops them as
    ///    `Despawned`);
    /// 4. the session's kill-score ledger
    ///    ([`crate::score::SessionScoreLedger`]) is read back for the score
    ///    this run recorded — the store only a `KillAwarded` event can move,
    ///    so a world without it for this generation answers `0` and names the
    ///    absence under [`MissionHostRefusal::ScoreLedger`];
    /// 5. the one report line the run writes is produced and printed.
    ///
    /// Returns the terminal the first call settled, and `None` on every later
    /// call and on a tick neither source settled.
    pub fn settle(
        &mut self,
        world: &mut World,
        produced: &MissionHostTick,
    ) -> Option<MissionTerminal> {
        if self.settled.is_some() {
            return None;
        }
        let (outcome, source, requested_by) = if let Some(outcome) = produced.objectives.outcome {
            (
                outcome,
                MissionTerminalSource::Objectives,
                produced.objectives.tick.events.iter().find_map(|event| {
                    matches!(event.kind, ObjectiveEventKind::OutcomeSettled { .. })
                        .then_some(event.key.source)
                }),
            )
        } else if produced.script.terminal == TerminalState::Running {
            return None;
        } else {
            (
                outcome_of(produced.script.terminal),
                MissionTerminalSource::ControlProgram,
                produced.script.events.iter().find_map(|event| {
                    matches!(event.kind, EventKind::TerminalRequested(_))
                        .then_some(event.key.source)
                }),
            )
        };

        // 1./2. The cue queue: the count is what the session still owned.
        //       (Step 1 is the assignment at the end of this function, which
        //       is what makes `step` answer "already settled" from now on.)
        let undrained_cues = self.objectives.pending_cues();
        let _never_heard = self.objectives.drain_cues();

        // 3. The audio session's pending queues, and what is still bound.
        let mut audio_loops_bound = 0;
        if let Some(mut audio) = world.get_resource_mut::<AudioSession>() {
            let _outcomes = audio.drain();
            let _radio = audio.drain_radio();
            audio_loops_bound = audio.router.active_loop_count();
        }
        if audio_loops_bound > 0 {
            self.refusals
                .push(MissionHostRefusal::MissionAudioStillBound {
                    count: audio_loops_bound,
                });
        }

        // 4. The score: read back from this session's own kill-score ledger,
        //    because nothing else in the process could have scored this run —
        //    a world without the ledger for this generation answers 0, and
        //    the absence is named rather than left to look like a run that
        //    scored nothing by itself.
        let score = match world.get_resource::<SessionScoreLedger>() {
            Some(ledger) if ledger.session() == self.served => ledger.score(),
            found => {
                let detail = match found {
                    Some(ledger) => format!(
                        "the installed ledger scores session {}, but this run serves session {}",
                        ledger.session().get(),
                        self.served.get(),
                    ),
                    None => format!(
                        "the world carries no kill-score ledger for session {}",
                        self.served.get(),
                    ),
                };
                self.refusals
                    .push(MissionHostRefusal::ScoreLedger { detail });
                0
            }
        };

        // 5. The one line, from the terminal's own record of it.
        let terminal = MissionTerminal::new(
            outcome,
            source,
            produced.tick,
            self.generation,
            requested_by,
            undrained_cues,
            audio_loops_bound,
            score,
        );
        println!("{}", terminal.report_line);
        self.settled = Some(terminal.clone());
        Some(terminal)
    }

    /// The settled terminal, once the run has ended on one.
    #[must_use]
    pub const fn terminal(&self) -> Option<&MissionTerminal> {
        self.settled.as_ref()
    }

    /// The tick this host last stepped, when it has stepped one.
    #[must_use]
    pub const fn last_tick(&self) -> Option<Tick> {
        self.stepped
    }

    /// Every absence this host runs over, in the order they were named.
    #[must_use]
    pub fn refusals(&self) -> &[MissionHostRefusal] {
        &self.refusals
    }

    /// The environment session this host advances.
    #[must_use]
    pub const fn environment(&self) -> &EnvironmentSession {
        &self.environment
    }

    /// The world-actor session, when the stage carries a world-actor program.
    #[must_use]
    pub fn world_actors(&self) -> Option<&WorldActorSession> {
        self.world_actors.as_ref()
    }

    /// The animation record player.
    #[must_use]
    pub const fn animation(&self) -> &MissionAnimationPlayer {
        &self.animation
    }

    /// The mission scope's animation **name → carrier record** index: the join
    /// every `WAKE_ANIM` name resolves through. Empty when the stage carries no
    /// animation join.
    #[must_use]
    pub const fn animation_names(&self) -> &AnimationNameIndex {
        &self.names
    }

    /// The `WAKE_ANIM` consumer's ledger: which directive keys were applied,
    /// which records they started and which names started nothing.
    #[must_use]
    pub const fn wake_animations(&self) -> &WakeAnimationLedger {
        &self.wake
    }

    /// The `WAKEUP_ENEMIES` consumer's ledger (M01-PLAY-AI-WAKE, #1487):
    /// which directive keys were applied, which sleeping scripted aircraft
    /// they released and which names woke nothing.
    #[must_use]
    pub const fn wake_enemies(&self) -> &WakeEnemiesLedger {
        &self.enemies
    }

    /// The `CALL_ANIMATION` consumer's ledger: every spelled call target a
    /// published statement named, what it started and which names started
    /// nothing.
    #[must_use]
    pub const fn call_animations(&self) -> &CallAnimationLedger {
        &self.calls
    }

    /// The designed node-arming input: every active-state statement a running
    /// record has published so far, with the publishing record's own node names
    /// ([`ArmedNode`]).
    #[must_use]
    pub fn armed_nodes(&self) -> &[ArmedNode] {
        &self.armed
    }

    /// The landing path's ledger: what this generation's landings have
    /// attempted, started or refused (M01-PLAY-TRIGGERS-ANIMS-LAND, #1498).
    #[must_use]
    pub fn landings(&self) -> &LandingLedger {
        &self.landings
    }

    /// One tick of the landing path — the host half of stage 6d.
    ///
    /// Reads the player body's committed velocity and tilt from the live
    /// world: exactly **one** body carries them, the same one-body rule the
    /// observation writer applies to the pose, so zero or two bodies mean
    /// no landing (fail-closed, not a refusal). Everything else the
    /// evaluation needs — the table, the stage-4 observation, the armed set
    /// and the name index — is already a record of this host, so the
    /// [`super::landings::step_landings`] rule borrows each field once and
    /// mutates only the ledger and the player.
    ///
    /// A world with no landing table runs nothing: the composition always
    /// installs one (an absent archive is its named absence), so this arm
    /// only covers a world built without the composition — and runs nothing
    /// there, by design.
    fn step_landings(&mut self, world: &mut World, tick: Tick) {
        let (velocity, tilt_deg) = {
            let mut query =
                world.query_filtered::<(&LinearVelocity, &Rotation), With<MissionPlayerBody>>();
            let bodies: Vec<([f64; 3], f64)> = query
                .iter(world)
                .map(|(velocity, rotation)| {
                    let [vx, vy, vz] = velocity.0.to_array();
                    let up = rotation.0 * Vec3::Y;
                    let tilt = f64::from(up.y).clamp(-1.0, 1.0).acos().to_degrees();
                    ([f64::from(vx), f64::from(vy), f64::from(vz)], tilt)
                })
                .collect();
            let [one] = bodies.as_slice() else {
                return;
            };
            *one
        };
        let Some(table) = world.get_resource::<MissionLandings>() else {
            return;
        };
        let input = LandingTick {
            table,
            observation: self.world_facts.observation(),
            armed: &self.armed,
            names: &self.names,
            velocity,
            tilt_deg,
        };
        step_landings(&mut self.landings, &mut self.animation, tick, &input);
    }

    /// The marker consumer.
    #[must_use]
    pub const fn markers(&self) -> &MissionMarkerConsumer {
        &self.markers
    }

    /// The objective session.
    #[must_use]
    pub const fn objectives(&self) -> &ObjectiveSession {
        &self.objectives
    }

    /// The control program's session.
    #[must_use]
    pub const fn script(&self) -> &MissionSession {
        &self.script
    }

    /// The objective lifecycle model the fact fold answers through — the
    /// recovered engine model when the stage carries one, else the declared
    /// dormancy table (possibly the empty, fail-closed read).
    #[must_use]
    pub const fn lifecycles(&self) -> &ObjectiveLifecycles {
        &self.lifecycles
    }

    /// The world fact table the fold answers through.
    #[must_use]
    pub const fn world_facts(&self) -> &WorldFactTable {
        &self.world_facts
    }

    /// The world-side operands the fold was collected for.
    #[must_use]
    pub const fn world_operands(&self) -> &WorldOperands {
        &self.operands
    }

    /// The sound-family archives the stage carries.
    #[must_use]
    pub fn sound_archives(&self) -> &[SoundArchive] {
        &self.sound_archives
    }

    /// The mission's sound-group cues resolved to voice lines, and the PCM
    /// its prepare step decoded for each — what
    /// [`publish_prepared_voices`](super::radio::publish_prepared_voices)
    /// hands to the device's sample library (M01-RADIO-SAMPLE-WIRE, #1466).
    #[must_use]
    pub const fn radio(&self) -> &MissionRadio {
        &self.radio
    }

    /// The generation every session this host launched serves.
    #[must_use]
    pub const fn generation(&self) -> SessionGeneration {
        self.generation
    }

    /// The session id the record player and the marker consumer serve.
    #[must_use]
    pub const fn served(&self) -> SessionId {
        self.served
    }
}

/// The complete refusal list a launch or a restart runs over: the stage
/// seed's own, then the absences only the seed can see (no world-actor
/// program, no animation join), then the ones every host names (the world
/// observation port and the missing lifecycle models).
///
/// Launch and restart share this one function so a rebuilt host reports
/// exactly the absences the launched one did.
fn stage_refusals(seed: &MissionHostSeed) -> Vec<MissionHostRefusal> {
    let mut refusals = seed.refusals.clone();
    if seed.world_actors.is_none() {
        refusals.push(MissionHostRefusal::WorldActors {
            detail: "this stage carries no lowered world-actor program, so no world-actor \
                     session steps"
                .to_owned(),
        });
    }
    if seed.animation.is_none() {
        refusals.push(MissionHostRefusal::AnimationJoin {
            detail: "this stage carries no animation join, so no startup row was offered to the \
                     record player"
                .to_owned(),
        });
    }
    refusals.push(MissionHostRefusal::WorldObservation {
        detail: "the world fact table folds only what this step observed: the \
                 MissionWorldObservation port (consumed per step) plus the record player's \
                 animation rows — a member or group nobody reported stays absent and every \
                 condition reading it answers false"
            .to_owned(),
    });
    // A stage that carries the record's lifecycle model — recovered engine
    // objectives, or at least the declared dormancy table — folds it and
    // raises no refusal; a stage that carries neither runs over the empty
    // fail-closed read and says so — unless it already named the absence
    // itself (the reader's own refusals travel on the seed), which is the
    // same naming and is not repeated here.
    if seed.block_lifecycles.is_empty()
        && seed.engine_objectives.is_none()
        && !seed
            .refusals
            .iter()
            .any(|refusal| matches!(refusal, MissionHostRefusal::BlockLifecycles { .. }))
    {
        refusals.push(MissionHostRefusal::BlockLifecycles {
            detail: "this stage carries neither a declared block lifecycle table nor a \
                     recovered engine model, so every Condition::ObjectiveAwake answers false"
                .to_owned(),
        });
    }
    refusals
}

/// Offers a join's startup rows to `animation`, grouped by their own event
/// and stamped at the tick the player is about to advance (its tick zero).
///
/// The grouping is the one the launch surface already runs
/// (`crate::mission_launch::started_rows`): one `start` per declared startup
/// event, rows in stored order, and a row the join refused stays a refusal of
/// the player rather than a silently skipped row.
fn offer_startup_rows(animation: &mut MissionAnimationPlayer, binding: &MissionAnimationBinding) {
    let mut events: Vec<(String, Vec<StartupAnimation>)> = Vec::new();
    for row in binding.startup() {
        match events.iter_mut().find(|(event, _)| event == row.event()) {
            Some((_, group)) => group.push(row.clone()),
            None => events.push((row.event().to_owned(), vec![row.clone()])),
        }
    }
    for (event, group) in &events {
        let _ = animation.start(event, Tick(0), group);
    }
}

/// The outcome a settled control-program terminal state maps to.
///
/// `Succeeded` is the declared success. `Failed` and `Aborted` are both
/// failures: `Aborted` is not a mission outcome this engine reaches from a
/// record, and a run that somehow carries one must exit nonzero rather than
/// be swallowed as the success it was not
/// (`docs/contracts/CLI-EVIDENCE.md`). `Unsupported` is reached only through
/// a launch refusal, so it cannot arrive from evaluation; it is mapped to
/// failure for the same reason. `Running` is not an outcome and is never
/// passed here by the funnel.
fn outcome_of(state: TerminalState) -> TerminalOutcome {
    match state {
        TerminalState::Succeeded => TerminalOutcome::Success,
        TerminalState::Failed | TerminalState::Aborted | TerminalState::Unsupported => {
            TerminalOutcome::Failure
        }
        TerminalState::Running => TerminalOutcome::Failure,
    }
}

/// Runs `f` with an `App` whose main world **is** `world`, so the `&mut App`
/// residency entry points ([`load_world`]/[`unload_world`]) act on the live
/// composition.
///
/// This is the fact-9 option the restart chose, and it is safe for exactly
/// one measured reason: every `app.` access those two make — and every one
/// their helper `world/spawn.rs` makes — is `app.world()` or
/// `app.world_mut()` (measured across both files on 2026-10-11, recorded in
/// `docs/findings/2026-10-11-vs-m01-rt-mission-host-restart.md`), so they
/// behave identically on an `App` that owns nothing but a world handle. The
/// scratch app is dropped with the placeholder world it was built with; the
/// live world — resources, entities and all — comes back exactly where it
/// was, mutated only by the call in between.
fn with_app_world<R>(world: &mut World, f: impl FnOnce(&mut App) -> R) -> R {
    let mut scratch = App::empty();
    let placeholder = std::mem::replace(scratch.world_mut(), std::mem::take(world));
    let result = f(&mut scratch);
    *world = std::mem::replace(scratch.world_mut(), placeholder);
    result
}

/// The composed per-tick entry: one step of the mission host per committed
/// fixed tick.
///
/// It runs in [`FixedPostUpdate`] after `PhysicsSystems::StepSimulation`, so
/// the tick it reads from [`PhysicsTickLedger`] is the tick whose physics has
/// already run, and it takes the host **out** of the world for the step —
/// [`step_mission_animations`] needs `&mut World` while these records live
/// inside it as a resource.
///
/// A tick this host has already stepped is not stepped again: the record
/// player and the objective session both refuse a repeated tick, and a report
/// already stands for that tick. A world with no fixed clock or no host runs
/// nothing at all, which is the "no session, no host" rule the rest of the
/// composition follows.
///
/// A **settled** host runs nothing at all either: the entry checks
/// [`MissionHost::terminal`] before stepping, so a run that already ended on
/// one terminal never advances another tick and never writes a second
/// terminal. When the step it did run settles the host, the entry sends the
/// terminal's [`MissionExit`](super::terminal::MissionExit) as an
/// [`AppExit`](bevy::app::AppExit) message, so the windowed run ends and the
/// headless test reads the very message the process would exit with.
pub fn mission_host_tick(world: &mut World) {
    let Some(elapsed) = world
        .get_resource::<Time<Fixed>>()
        .map(|time| time.timestep())
    else {
        return;
    };
    let Some(tick) = world
        .get_resource::<PhysicsTickLedger>()
        .map(|ledger| Tick(ledger.ticks))
    else {
        return;
    };
    if world
        .get_resource::<MissionHost>()
        .is_some_and(|host| host.terminal().is_some() || host.last_tick() == Some(tick))
    {
        return;
    }
    let Some(mut host) = world.remove_resource::<MissionHost>() else {
        return;
    };
    let answer = host.step(world, elapsed);
    if let Ok(produced) = &answer {
        host.settle(world, produced);
    }
    let tick = host.last_tick().unwrap_or(tick);
    if let Some(terminal) = host.terminal() {
        world.write_message(terminal.exit.app_exit());
    }
    world.insert_resource(host);
    world.insert_resource(MissionHostReport { tick, answer });
}

/// How many playtest resets the composed restart has already observed.
///
/// `perform_reset` runs in `Update` and increments
/// [`PlaytestState::resets`] there; the restart system runs in
/// [`PostUpdate`], the first schedule after it, and compares against this
/// mark — the fact-11 seam.
///
/// It is `pub(crate)` only so [`super::compose::teardown`] can remove it
/// beside every other restart resource: a torn-down composition leaves no
/// restart bookkeeping behind either.
#[derive(Resource, Debug)]
pub(crate) struct MissionHostRestartWatch {
    /// The last observed `PlaytestState::resets`.
    resets: u32,
}

/// The composed restart entry: one full rebuild per requested restart.
///
/// Two triggers reach it, and both are consumed exactly once:
///
/// * the playtest's own meta reset (`R`/[`PlaytestRequests`](crate::playtest::PlaytestRequests)
///   → `perform_reset` in `Update`), observed through
///   [`PlaytestState::resets`] against [`MissionHostRestartWatch`];
/// * a [`MissionHostRestartRequest`] another system inserted — the seam a
///   terminal handler latches "restart after this terminal" through.
///
/// It takes the host **and** the stage out of the world for the restart
/// (both are put back whatever happens), mints the next generation from the
/// process-wide counter, and stores the [`MissionHostRestartReport`] — or,
/// when a constructor refused, the [`MissionHostRestartFailure`] naming it.
/// A world with no host, or with no stage resource, runs nothing: "no
/// session, no host" as everywhere else in this composition.
pub fn mission_host_restart(world: &mut World) {
    let requested = world
        .remove_resource::<MissionHostRestartRequest>()
        .is_some();
    let resets = world
        .get_resource::<PlaytestState>()
        .map(|state| state.resets);
    let mut reset = false;
    if let Some(resets) = resets
        && let Some(mut watch) = world.get_resource_mut::<MissionHostRestartWatch>()
        && watch.resets != resets
    {
        watch.resets = resets;
        reset = true;
    }
    if !(requested || reset) {
        return;
    }
    let Some(mut host) = world.remove_resource::<MissionHost>() else {
        return;
    };
    let Some(stage) = world.remove_resource::<MissionStage>() else {
        world.insert_resource(host);
        return;
    };
    let generation = mint_host_generation();
    let result = host.restart(world, &stage, generation);
    world.insert_resource(host);
    world.insert_resource(stage);
    // The score follows the session the host serves: a generation that was
    // rebuilt gets a fresh ledger at zero, and a restart that refused before
    // the sessions were rebuilt keeps the ledger (and its tally) of the
    // generation still standing. Either way the two can never disagree about
    // which session the score belongs to
    // (`docs/contracts/STATE-TRANSACTIONS.md`: retry restores the authored
    // initial state, results are always generation-qualified).
    let served = world.resource::<MissionHost>().served();
    if world
        .get_resource::<SessionScoreLedger>()
        .map(SessionScoreLedger::session)
        != Some(served)
    {
        world.insert_resource(SessionScoreLedger::open(served));
    }
    match result {
        Ok(report) => {
            world.remove_resource::<MissionHostRestartFailure>();
            world.insert_resource(report);
        }
        Err(error) => {
            world.remove_resource::<MissionHostRestartReport>();
            world.insert_resource(MissionHostRestartFailure { error });
        }
    }
}

/// Installs [`mission_host_tick`] in the fixed-tick schedule, after the
/// physics step of the same tick **and after that tick's damage consumers**
/// ([`crate::damage::drive_damage_schedule`]) — so a run that settles on a
/// tick settles on the score that tick's kills resolved, never on the tick
/// before them — and [`mission_host_restart`] in
/// [`PostUpdate`] — after the playtest's `Update`-schedule reset has run and
/// incremented [`PlaytestState::resets`].
///
/// The world's own report for the same tick is installed here too:
/// [`mission_world_observation`] runs in the fixed-tick schedule after the
/// physics step and **before** [`mission_host_tick`], so the
/// [`MissionWorldObservation`] port stage 4 consumes already carries this
/// tick's player, world-actor and group rows (M01-PLAY-TRIGGERS-OBSERVE,
/// #1480). A composition that installs this installs the writer and its
/// consumer as one pair, in that order.
///
/// This is the half the windowed composition wires: it inserts the
/// [`MissionHost`] resource and then calls here, so a composition installs
/// nothing at all when its stage carries no records.
pub fn install_mission_host(app: &mut App) {
    use avian3d::prelude::PhysicsSystems;

    app.add_systems(
        FixedPostUpdate,
        mission_world_observation
            .after(PhysicsSystems::StepSimulation)
            .before(mission_host_tick),
    );
    app.add_systems(
        FixedPostUpdate,
        mission_host_tick
            .after(PhysicsSystems::StepSimulation)
            .after(crate::damage::drive_damage_schedule),
    );
    app.add_systems(PostUpdate, mission_host_restart);
    let resets = app
        .world()
        .get_resource::<PlaytestState>()
        .map_or(0, |state| state.resets);
    app.insert_resource(MissionHostRestartWatch { resets });
}
