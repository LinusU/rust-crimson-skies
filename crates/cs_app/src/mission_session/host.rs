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
//! 4. **script host** — [`MissionSession::advance`] over the
//!    [`compose_mission_facts`] fold.
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
//! * No production reader turns a record's `BEGIN_DORMANT` spellings into
//!   [`BlockLifecycleTable`] declarations yet, so the fold runs over an empty
//!   table — its documented fail-closed read, where every
//!   `Condition::ObjectiveAwake` answers `false` — under
//!   [`MissionHostRefusal::BlockLifecycles`].
//! * No stage builds a [`crate::world_facts::WorldObservation`] out of the ECS
//!   world, so [`WorldFactTable`] answers from its empty report under
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
//! # What is not claimed
//!
//! No original executable ran; `retail` is read access to the owner's
//! installation. Nothing here is `verified_original`. The composed entry
//! stops a run on nothing yet: a terminal outcome and a restart belong to
//! VS-M01-RT-MISSION-HOST `.02` and `.03`, so this stage neither exits nor
//! reloads anything.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::prelude::{App, FixedPostUpdate, Resource, World};
use bevy::time::{Fixed, Time};
use cs_script::ir::MissionProgram;
use cs_script::runtime::{SessionGeneration, TickError};
use cs_sim::mission::{BlockLifecycleTable, LaunchRefused, MissionSession, MissionTick};
use cs_sim::objectives::runtime::{RuntimeLimits, TickInput};
use cs_sim::objectives::terminal::TerminalPrecedence;
use cs_sim::visibility::TimelineError;
use cs_sim::world_actors::runtime::WorldActorError;
use cs_types::Tick;
use cs_types::net::SessionId;

use super::compose::MissionStage;
use crate::animation::mission::{MissionAnimationBinding, StartupAnimation};
use crate::environment::EnvironmentSession;
use crate::mission_animations::{
    MissionAnimationPlayer, MissionAnimationStepRefusal, PlayerError, TickReport,
    step_mission_animations,
};
use crate::mission_markers::{MarkerDelivery, MissionMarkerBindings, MissionMarkerConsumer};
use crate::mission_session::SoundArchive;
use crate::objectives::{LoweredObjectives, ObjectiveSession, SessionLaunchError, SessionTick};
use crate::physics::{BASELINE_FIXED_HZ, PhysicsTickLedger};
use crate::playtest::PlaytestState;
use crate::world_actors::{
    LoweredWorldActors, WorldActorLaunchError, WorldActorSession, WorldActorSessionTick,
    WorldActorTick,
};
use crate::world_facts::{MemberResolver, WorldFactTable, WorldOperands, compose_mission_facts};

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
    /// No production reader declares a block's `BEGIN_DORMANT` lifecycle into
    /// [`BlockLifecycleTable`], so every `Condition::ObjectiveAwake` answers
    /// `false` and no block can complete.
    BlockLifecycles {
        /// What the table is empty of, and whose job filling it is.
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
    /// What the control program answered for this tick.
    pub script: MissionTick,
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
    markers: MissionMarkerConsumer,
    objectives: ObjectiveSession,
    script: MissionSession,
    blocks: BlockLifecycleTable,
    world_facts: WorldFactTable,
    operands: WorldOperands,
    sound_archives: Vec<SoundArchive>,
    refusals: Vec<MissionHostRefusal>,
    generation: SessionGeneration,
    served: SessionId,
    stepped: Option<Tick>,
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
    /// animation join, no world-actor program, the unobserved fact fold and
    /// the undeclared block lifecycles) are appended, so
    /// [`Self::refusals`] is the complete list a run reports.
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
        let mut refusals = seed.refusals.clone();

        let environment = seed.environment.clone();

        let world_actors = match &seed.world_actors {
            Some(lowered) => Some(
                WorldActorSession::launch(lowered.clone(), generation)
                    .map_err(MissionHostLaunchError::WorldActors)?,
            ),
            None => {
                refusals.push(MissionHostRefusal::WorldActors {
                    detail: "this stage carries no lowered world-actor program, so no world-actor \
                             session steps"
                        .to_owned(),
                });
                None
            }
        };

        let mut animation = MissionAnimationPlayer::new(served, BASELINE_FIXED_HZ)
            .map_err(MissionHostLaunchError::Animation)?;
        match &seed.animation {
            Some(binding) => offer_startup_rows(&mut animation, binding),
            None => refusals.push(MissionHostRefusal::AnimationJoin {
                detail: "this stage carries no animation join, so no startup row was offered to \
                         the record player"
                    .to_owned(),
            }),
        }
        let markers = MissionMarkerConsumer::new(served, seed.markers.clone());
        let objectives = ObjectiveSession::launch(seed.objectives.clone(), generation)
            .map_err(MissionHostLaunchError::Objectives)?;
        let script = MissionSession::launch(seed.control.clone(), generation, [])
            .map_err(MissionHostLaunchError::Script)?;

        refusals.push(MissionHostRefusal::WorldObservation {
            detail: "no stage builds a WorldObservation out of the ECS world, so the world fact \
                     table answers from its empty, fail-closed report for every tick"
                .to_owned(),
        });
        refusals.push(MissionHostRefusal::BlockLifecycles {
            detail: "no production reader declares a record's BEGIN_DORMANT lifecycle into the \
                     block lifecycle table, so the table is empty and every Condition::\
                     ObjectiveAwake answers false"
                .to_owned(),
        });

        Ok(Self {
            environment,
            world_actors,
            animation,
            markers,
            objectives,
            script,
            blocks: BlockLifecycleTable::new(),
            world_facts: WorldFactTable::new(seed.resolver.clone()),
            operands: WorldOperands::of(&seed.control),
            sound_archives: seed.sound_archives.clone(),
            refusals,
            generation,
            served,
            stepped: None,
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

        // 4. Script host: the production fact fold, then the program's own
        //    answer for this tick.
        let facts = compose_mission_facts(
            &self.script,
            &self.blocks,
            &self.world_facts,
            &self.operands,
        );
        let script = self
            .script
            .advance(&facts, tick)
            .map_err(MissionHostStepError::Script)?;

        Ok(MissionHostTick {
            tick,
            environment_ticks,
            world_actors,
            animation: composed.records,
            markers: composed.mission.markers,
            objectives: composed.mission.tick,
            script,
        })
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

    /// The block lifecycle table the fact fold answers through.
    #[must_use]
    pub const fn blocks(&self) -> &BlockLifecycleTable {
        &self.blocks
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
        .is_some_and(|host| host.last_tick() == Some(tick))
    {
        return;
    }
    let Some(mut host) = world.remove_resource::<MissionHost>() else {
        return;
    };
    let answer = host.step(world, elapsed);
    let tick = host.last_tick().unwrap_or(tick);
    world.insert_resource(host);
    world.insert_resource(MissionHostReport { tick, answer });
}

/// Installs [`mission_host_tick`] in the fixed-tick schedule, after the
/// physics step of the same tick.
///
/// This is the half the windowed composition wires: it inserts the
/// [`MissionHost`] resource and then calls here, so a composition installs
/// nothing at all when its stage carries no records.
pub fn install_mission_host(app: &mut App) {
    use avian3d::prelude::PhysicsSystems;

    app.add_systems(
        FixedPostUpdate,
        mission_host_tick.after(PhysicsSystems::StepSimulation),
    );
}
