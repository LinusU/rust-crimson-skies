//! The mission host's objective-lifecycle run: which model answers
//! `Condition::ObjectiveAwake`, who advances its timers each host tick and who
//! applies a completion's `WAKE`/`NAP`/`KILL`/`SLEEP` effects.
//!
//! A host runs one of two models, in this order of preference:
//!
//! * **Engine** ([`EngineObjectives`]) — the measured lifecycle model
//!   recovered from the mission's own objective record (M01-B-OBJ-1/2):
//!   `BEGIN_DORMANT`'s four timer slots, `TICK_DEPENDS_ON_OBJ`'s tick gate and
//!   the four completion effects, advanced on its own mission clock. This is
//!   the model a driven run needs, because M01's `Finish` blocks wake only
//!   through completion effects and nap re-wakes — paths the thinner read has
//!   no surface for.
//! * **Declared** ([`BlockLifecycleTable`]) — the record's dormancy read on
//!   its own, from `BEGIN_DORMANT` and `TICK_DEPENDS_ON_OBJ` alone: timed
//!   self-wakes advance on a clock this wrapper keeps, and a completion marks
//!   the block `Done`. `WAKE`, `NAP`, `KILL`, `SLEEP` and `WAKE_WHEN_I_SLEEP`
//!   have no surface here and are not applied — a stage that wants them
//!   carries the engine model; a stage that does not have it reports the
//!   thinner read, which is the honest statement of what it can drive.
//!
//! The per-tick order is the engine's measured one: the timer pass
//! ([`ObjectiveLifecycles::advance`]) runs **before** the control program's
//! completion pass (`MissionHost::step` applies [`ObjectiveLifecycles::complete`]
//! for each `ObjectiveCompleted` event the tick emitted), so a block the timer
//! pass woke this tick can complete on this same tick, exactly as a block the
//! original's pass 1 woke reaches pass 2 awake.
//!
//! # The projection
//!
//! `EngineState` and [`cs_script::runtime::ObjectiveLifecycle`] are the same
//! measured `+0x5c8` vocabulary — `Dormant` 0, `Awake` 1, `Napping` 2 and the
//! state the engine calls `Asleep` is the vocabulary's `Done` 3 — so the fold
//! is a direct projection with one case of its own: a `KILL`ed objective is
//! not present, and "not present" means no row at all, which is the
//! fail-closed read every absent key gives.

use cs_content::objectives_engine::{CueEmission, EngineObjective, EngineObjectives, EngineState};
use cs_script::runtime::{MissionFacts, ObjectiveLifecycle};
use cs_sim::mission::BlockLifecycleTable;

/// Which model the host's `MissionFacts::objectives` map answers from, and
/// whose transitions one host tick applied.
#[derive(Clone, Debug)]
pub enum ObjectiveLifecycles {
    /// The declared dormancy read: a [`BlockLifecycleTable`] plus the mission
    /// clock its timed self-wakes compare against.
    Declared {
        /// The declared table.
        table: BlockLifecycleTable,
        /// The mission clock, in seconds — advanced once per host tick before
        /// the table's timer pass runs.
        clock: f64,
    },
    /// The recovered engine model; its mission clock is its own.
    Engine(EngineObjectives),
}

/// What the timer pass of one host tick did, in engine order.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LifecycleAdvance {
    /// Blocks the timer pass woke (a dormant `wake_at` or a napping
    /// `rewake_after` reaching its slot).
    pub woken: Vec<u32>,
    /// Blocks the timer pass napped (an awake `nap_after` reaching its slot).
    pub napped: Vec<u32>,
    /// Blocks the timer pass put to sleep (an awake `sleep_at` the mission
    /// clock passed).
    pub slept: Vec<u32>,
}

/// What the whole lifecycle step of one host tick did: the timer pass's own
/// report plus the blocks the completion pass completed — each of whose
/// `WAKE`/`NAP`/`KILL`/`SLEEP` effects [`ObjectiveLifecycles::complete`]
/// already applied.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ObjectiveLifecycleTick {
    /// Blocks the timer pass woke this tick.
    pub woken: Vec<u32>,
    /// Blocks the timer pass napped this tick.
    pub napped: Vec<u32>,
    /// Blocks the timer pass put to sleep this tick.
    pub slept: Vec<u32>,
    /// Blocks the completion pass completed this tick, in event order.
    pub completed: Vec<u32>,
}

/// The lifecycle row one engine objective projects to: the measured `+0x5c8`
/// states are the same four, and a `KILL`ed objective projects no row at all —
/// "not present" is not observed.
fn project(objective: &EngineObjective) -> Option<ObjectiveLifecycle> {
    if !objective.present {
        return None;
    }
    Some(match objective.state {
        EngineState::Dormant => ObjectiveLifecycle::Dormant,
        EngineState::Awake => ObjectiveLifecycle::Awake,
        EngineState::Napping => ObjectiveLifecycle::Napping,
        EngineState::Asleep => ObjectiveLifecycle::Done,
    })
}

impl ObjectiveLifecycles {
    /// The declared dormancy read on its own: the table plus a mission clock
    /// starting at zero.
    #[must_use]
    pub fn declared(table: BlockLifecycleTable) -> Self {
        Self::Declared { table, clock: 0.0 }
    }

    /// The recovered engine model.
    #[must_use]
    pub fn engine(engine: EngineObjectives) -> Self {
        Self::Engine(engine)
    }

    /// The timer pass of one host tick: `elapsed` seconds on the mission
    /// clock, then every armed transition in objective order.
    pub fn advance(&mut self, elapsed: f64) -> LifecycleAdvance {
        match self {
            Self::Declared { table, clock } => {
                *clock += elapsed;
                LifecycleAdvance {
                    woken: table.tick(*clock),
                    napped: Vec::new(),
                    slept: Vec::new(),
                }
            }
            #[allow(clippy::cast_possible_truncation)]
            Self::Engine(engine) => {
                let outcome = engine.advance(elapsed as f32);
                LifecycleAdvance {
                    woken: outcome.woken.iter().map(|index| *index as u32).collect(),
                    napped: outcome.napped.iter().map(|index| *index as u32).collect(),
                    slept: outcome.slept.iter().map(|index| *index as u32).collect(),
                }
            }
        }
    }

    /// The completion pass's own transition: `index` completed, so its
    /// `WAKE`/`NAP`/`KILL`/`SLEEP` effects apply — on the engine model.
    /// The declared read records the completion and leaves its effects
    /// unapplied, which is what "the record's dormancy read alone" means.
    ///
    /// An index the model does not hold is skipped rather than panicked on:
    /// the control program can complete a block the objective record did not
    /// load (the engine loads only the contiguous `OBJECTIVE1..` prefix), and
    /// that completion changes no lifecycle.
    pub fn complete(&mut self, index: u32) {
        match self {
            Self::Declared { table, .. } => {
                table.complete(index);
            }
            Self::Engine(engine) => {
                if engine.get(index as usize).is_some() {
                    engine.complete(index as usize);
                }
            }
        }
    }

    /// One block's projected lifecycle, or `None` when the model holds no
    /// such block — `KILL`ed engine objectives included, which is the
    /// fail-closed read an absent row already was.
    #[must_use]
    pub fn state(&self, index: u32) -> Option<ObjectiveLifecycle> {
        match self {
            Self::Declared { table, .. } => table.state(index),
            Self::Engine(engine) => engine.get(index as usize).and_then(project),
        }
    }

    /// How many objectives the model loads.
    #[must_use]
    pub fn len(&self) -> usize {
        match self {
            Self::Declared { table, .. } => table.len(),
            Self::Engine(engine) => (0usize..)
                .take_while(|index| engine.get(*index).is_some())
                .count(),
        }
    }

    /// Whether the model loads no objective at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The mission clock this model's timers run on, in seconds.
    #[must_use]
    pub fn mission_clock(&self) -> f64 {
        match self {
            Self::Declared { clock, .. } => *clock,
            Self::Engine(engine) => f64::from(engine.mission_clock()),
        }
    }

    /// The sound-group cues the model emitted since the last call, in
    /// emission order; the declared dormancy read emits none.
    ///
    /// This is how one host pass hands each cue to the sound consumer exactly
    /// once: the engine model queues a `WAKEUP_SOUND_GROUP`/`COMPLETED_`
    /// `SOUND_GROUP` emission on a wake or a completion, and this take leaves
    /// the model's log empty for the next pass.
    pub fn take_cues(&mut self) -> Vec<CueEmission> {
        match self {
            Self::Declared { .. } => Vec::new(),
            Self::Engine(engine) => engine.take_cues(),
        }
    }

    /// The `objectives` map the next evaluation reads — every loaded block's
    /// projected lifecycle; absent blocks are absent, never defaulted.
    #[must_use]
    pub fn facts(&self) -> MissionFacts {
        match self {
            Self::Declared { table, .. } => table.facts(),
            Self::Engine(engine) => MissionFacts {
                objectives: (0usize..)
                    .take_while(|index| engine.get(*index).is_some())
                    .filter_map(|index| {
                        project(engine.get(index).expect("take_while bounds the index"))
                            .map(|row| (index as u32, row))
                    })
                    .collect(),
                ..MissionFacts::default()
            },
        }
    }
}
