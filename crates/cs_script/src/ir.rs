//! The typed, versioned mission IR and its pre-launch validation (F37-A).
//!
//! `specs/F37-mission-ir-and-deterministic-runtime-core.md` and
//! `docs/contracts/SCRIPT-MISSION.md`. This IR is a **new design**: it is not a
//! claim that original mission programs use this structure, and it contains no
//! original opcode numbers. Program *data* lives here; mutable execution state
//! and the evaluator are in [`crate::runtime`], and host effects are applied by
//! the simulation (`cs_sim::mission`).
//!
//! A [`MissionProgram`] only becomes launchable through
//! [`MissionProgram::validate`], which checks symbol uniqueness, references,
//! types and bounds up front and returns a [`ValidatedProgram`]. An
//! instruction or native call the adapter could not decode is carried as an
//! explicit `Unknown` node and makes validation fail with a precise
//! [`ValidationError::UnsupportedInstruction`] trace; it is never a NOP.

use std::collections::BTreeSet;
use std::fmt;

use cs_types::content::{ContentId, ContentKind};

/// The IR version this crate understands.
pub const IR_VERSION: u32 = 1;
/// Most actions one objective may carry; bounds work per tick (contract:
/// "control flow is explicit and bounded"). The same bound covers the action
/// list of one deferred work item ([`Action::Schedule`]).
pub const MAX_ACTIONS_PER_OBJECTIVE: usize = 64;
/// Deepest condition nesting accepted.
pub const MAX_CONDITION_DEPTH: usize = 16;
/// Deepest `Schedule` action-list nesting accepted (contract: "recursion /
/// stack limits").
pub const MAX_ACTION_NESTING: usize = 16;
/// Deepest [`Value::List`] nesting accepted, counting the nested lists
/// themselves: a list may sit inside at most this many enclosing lists. A
/// design bound, not a measured original limit — the deepest measured
/// argument list nests three (`ANIM_STATE`'s descriptor).
pub const MAX_VALUE_DEPTH: usize = 8;
/// Most items one list-shaped value may carry: a [`Value::List`]'s children
/// or a directive's top-level arguments. A design bound, not a measured
/// original limit — the longest measured list is `WAKE_OBJECTIVE`'s fifteen
/// indices.
pub const MAX_VALUE_ITEMS: usize = 64;

/// Stable identity of a mission-scoped actor. Never an entity index.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ActorId(pub u32);

/// Stable identity of a variable or objective inside one program.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SymbolId(pub u32);

/// A byte range in the program's source resource, kept so diagnostics can
/// point at the origin of a node.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SourceSpan {
    pub start: u32,
    pub end: u32,
}

/// The type of a [`Value`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValueType {
    Bool,
    Int,
    Float,
    Str,
    Content,
    Actor,
    Vector,
    OptActor,
    /// An ordered list of values (`Value::List`).
    List,
}

/// A typed IR value. There is no implicit coercion between variants.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Bool(bool),
    /// Checked 32-bit integer.
    Int(i32),
    /// Must be finite; validation rejects NaN and infinity.
    Float(f64),
    Str(String),
    Content(ContentId),
    Actor(ActorId),
    Vector([f64; 3]),
    /// A typed optional actor reference.
    OptActor(Option<ActorId>),
    /// One original argument-list node, carried as structure field for field:
    /// each child is the value at that position of the list, nested lists stay
    /// nested. Measured directive sites spell nested lists
    /// (`COMPLETED_STOPPOINT`'s `[[text,int,int]]`, `ANIM_STATE`'s
    /// `[text,[text,[text],text,[text]]]`); flattening one into positional
    /// arguments would be a format change presented as a binding, so the IR
    /// carries the node itself.
    List(Vec<Value>),
}

/// Why a value lies outside the IR's bounds (contract: "cap memory/time").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ValueDefect {
    /// A float or vector component is NaN or infinite.
    NonFinite,
    /// A [`Value::List`] nested deeper than [`MAX_VALUE_DEPTH`].
    TooDeep,
    /// A [`Value::List`] carrying more than [`MAX_VALUE_ITEMS`] items.
    TooManyItems { count: usize },
}

impl Value {
    /// The value's type.
    pub fn value_type(&self) -> ValueType {
        match self {
            Self::Bool(_) => ValueType::Bool,
            Self::Int(_) => ValueType::Int,
            Self::Float(_) => ValueType::Float,
            Self::Str(_) => ValueType::Str,
            Self::Content(_) => ValueType::Content,
            Self::Actor(_) => ValueType::Actor,
            Self::Vector(_) => ValueType::Vector,
            Self::OptActor(_) => ValueType::OptActor,
            Self::List(_) => ValueType::List,
        }
    }

    /// Finiteness and the list bounds, checked recursively at `depth`
    /// enclosing lists. The recursion is what makes the bounds necessary: an
    /// unbounded `List` would make the check itself unbounded.
    pub(crate) fn check(&self, depth: usize) -> Result<(), ValueDefect> {
        match self {
            Self::Float(f) if !f.is_finite() => Err(ValueDefect::NonFinite),
            Self::Vector(v) if v.iter().any(|c| !c.is_finite()) => Err(ValueDefect::NonFinite),
            Self::List(items) => {
                if depth >= MAX_VALUE_DEPTH {
                    return Err(ValueDefect::TooDeep);
                }
                if items.len() > MAX_VALUE_ITEMS {
                    return Err(ValueDefect::TooManyItems { count: items.len() });
                }
                for item in items {
                    item.check(depth + 1)?;
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }
}

/// Why an actor is no longer in play — or that it still is. The contract
/// keeps these distinct: an actor removed by a cinematic is not necessarily
/// a kill, a capture is not a destruction, and a detach is an event, not a
/// state (F39-E7). `cs_sim`'s actor-fact table is the writer: a state is
/// populated only where an authoritative event produces it, and refused by
/// name where nothing does — a variant's existence never writes it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ActorState {
    Alive,
    Disabled,
    Dead,
    Captured,
    Escaped,
    Detached,
    Despawned,
}

/// Comparison operator for [`Condition::Compare`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompareOp {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

/// A side-effect-free boolean expression.
#[derive(Clone, Debug, PartialEq)]
pub enum Condition {
    Const(bool),
    /// Compares a variable with a literal of the same type.
    Compare {
        variable: SymbolId,
        op: CompareOp,
        value: Value,
    },
    /// True when the actor is in exactly this state.
    ActorIs {
        actor: ActorId,
        state: ActorState,
    },
    Not(Box<Condition>),
    All(Vec<Condition>),
    Any(Vec<Condition>),
    /// An instruction or native call that could not be decoded.
    Unknown {
        instruction: String,
    },
}

/// How a mission ends. Mirrors the contract's terminal vocabulary minus
/// `Running`/`Unsupported`, which are runtime states
/// ([`crate::runtime::TerminalState`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Outcome {
    Succeeded,
    Failed,
    Aborted,
}

/// A measured directive operation — the engine's own vocabulary of the
/// operation one control-record directive key performs, as the M01-LC
/// findings documents established it.
///
/// This mirrors `cs_content::mission_control::DirectiveOperation` variant
/// for variant: `cs_script` may depend on `cs_types` only
/// (`docs/01-ARCHITECTURE.md`), so it cannot name that type, and the mirror
/// is kept honest two ways — [`Self::code`] returns the identical stable
/// identifier the measurement side publishes, and the M01-LC acceptance
/// suite cross-checks every code the measured vocabulary produces against
/// this enum.
///
/// One variant per measured **mechanism**, not per key: keys that share a
/// handler share a variant (`WAKE_OBJECTIVE` and
/// `WAKE_OBJECTIVE_WHEN_I_COMPLETE`; the hundred `INACTIVE<n>` spellings;
/// the four `ADD`/`REMOVE` `_OBJECTIVE`/`_OTHER` `_TARGET` keys). The
/// operation names *what the original is measured to do*; it is not a claim
/// that the engine performs it — the runtime carries it to the host as a
/// documented effect ([`crate::runtime::MissionState::directives`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DirectiveOperation {
    /// `INACTIVE<n>`: each listed name resolves through a chained member
    /// lookup into a handle; the block completes when at least the threshold
    /// of them no longer carry the in-play bit.
    InactiveMembers,
    /// `INACTIVE_COMPLETION_COUNT`: the count of cleared members the
    /// inactive-members evaluator needs.
    InactiveThreshold,
    /// `DANGER_ZONES_COMPLETED`: completes when at least the stored threshold
    /// of the recorded zone flag bytes are nonzero.
    DangerZoneFlags,
    /// `DANGER_ZONES_COMPLETION_COUNT`: that threshold.
    DangerZoneThreshold,
    /// `DEDG`: completes when the named enemy group has at most the spelled
    /// count of members still in play, plus whatever its generator still
    /// owes.
    EnemyGroupDepletion,
    /// `ANIM_STATE`: completes when at least the required count of the listed
    /// animations are in the named state.
    AnimationStates,
    /// `TRAVELERS`: completes when the named subject crosses the radius about
    /// the anchor — or, in the counting mode, when the cumulative group
    /// count reaches the required number.
    Travelers,
    /// `COUNTER`: the block's named-counter triples over the global integer
    /// registry — wake and complete writes plus a per-tick test.
    NamedCounters,
    /// `BEGIN_DORMANT`: the block starts dormant; its first child is the
    /// mission-clock second at which it wakes itself, and the next children
    /// arm the awake, nap and done timers.
    DormantStart,
    /// `TICK_DEPENDS_ON_OBJ`: the dependent runs no timers and evaluates no
    /// conditions while the objective at the stored index is not awake.
    DependencyGate,
    /// `IDENTITY`: the class spelling selects the completion sound channel
    /// and HUD class; the integer is the HUD slot ordinal.
    PresentationIdentity,
    /// `WAKE_OBJECTIVE` / `WAKE_OBJECTIVE_WHEN_I_COMPLETE`: at completion,
    /// wake each listed index in order.
    WakeObjectives,
    /// `SLEEP_OBJECTIVE_WHEN_I_COMPLETE`: at completion, put each listed
    /// index to sleep through the shared transition.
    SleepObjectives,
    /// `KILL_OBJECTIVE_WHEN_I_COMPLETE`: at completion, kill each listed
    /// index — it stops ticking and never counts in the outcome aggregation.
    KillObjectives,
    /// `NAP_OBJECTIVE_WHEN_I_COMPLETE`: at completion, put the target to nap
    /// and re-wake it after the spelled seconds, clearing its completed flag.
    NapObjective,
    /// `ADD`/`REMOVE` `_OBJECTIVE`/`_OTHER` `_TARGET`: at completion, resolve
    /// each name chain to one object and set or clear its target flag.
    SetTargetFlag {
        /// `true` for the objective-target flag, `false` for the
        /// other-target flag.
        objective: bool,
        /// `true` to set the flag, `false` to clear it.
        set: bool,
    },
    /// `COMPLETED_STOPPOINT`: forward the `{int, bool}` pair to the named
    /// stoppoint's two-step advance/select handler.
    AdvanceStopPoint,
    /// `COMPLETED_ZEPCANNONS`: store the byte at the resolved zeppelin's
    /// field.
    ZeppelinCannons,
    /// `SET_AI_NET`: point the named vehicle or zeppelin at the named entry
    /// of the global node list.
    AssignNet,
    /// `SET_AI_TEAM`: write the named actor's team field.
    AssignTeam,
    /// `SET_AI_ATTACK_RADIUS`: write the vehicle's radius triple `r²`, `-r`,
    /// `r`.
    SetAttackRadius,
    /// `START_TAXI`: clear the vehicle's AI hold-off byte.
    ReleaseTaxi,
    /// `SET_HELP_LABEL`: give the resolved object the localized label id and
    /// text.
    SetHelpLabel,
    /// `STOP_QUEUED_SOUNDS`: flag each matching queued-sound entry and
    /// schedule its removal a fixed time later.
    StopQueuedSounds,
    /// `COMPLETED_SOUND_GROUP`: play the sound-group handle through the
    /// completed channel.
    CompletedSoundGroup,
    /// `TIMER_ADJUST` / `ADJUST_TIMER_WHEN_I_COMPLETE`: set or adjust the
    /// mission timer by the spelled seconds at completion.
    AdjustMissionTimer,
    /// `END_TIMER`: stop the mission timer at completion.
    EndMissionTimer,
    /// `WARP_VEHICLE`: teleport the vehicle to a randomly chosen listed
    /// point and add the shared-scalar velocity unless AI-driven.
    WarpVehicle,
    /// `WAKEUP_ENEMIES`: on wake, wake only the named actors that are asleep.
    WakeEnemies,
    /// `WAKEUP_TURRETS`: on wake, set the live byte of every turret entry
    /// whose name matches, `*` consuming exactly one digit.
    WakeTurrets,
    /// `WAKEUP_ZEP_TURRETS`: on wake, activate the named zeppelin-turret
    /// node and all its children.
    WakeZeppelinTurrets,
    /// `WAKEUP_GENERATOR`: on wake, add the spelled count to the named
    /// generator's pending-spawn counter.
    FeedGenerator,
    /// `WAKE_ANIM`: on wake, execute the named animation on the named or
    /// defaulted target.
    WakeAnimation,
    /// `WAKEUP_SOUND_GROUP`: on wake, play the sound-group handle through the
    /// woken channel.
    WakeSoundGroup,
    /// `RESET_TIMER`: on a dormant wake, set and start the mission timer at
    /// the spelled seconds.
    ResetMissionTimer,
    /// `HIDE_OBJ`: on a dormant wake, mark the named objective completed
    /// with no outcome class.
    HideObjective,
    /// `SLEEP_ANIM`: on a nap or done transition, execute the named animation
    /// on the named target.
    TransitionAnimation,
    /// `WAKE_OBJECTIVE_WHEN_I_SLEEP`: wake the listed indices when this
    /// objective auto-naps or auto-dones on its own timers.
    WakeObjectivesOnTransition,
    /// `WON` / `LOST`: the block's outcome class — the mission resolves when
    /// every block of a class completes; the class block itself does not
    /// fire on its own completion.
    OutcomeClass {
        /// `true` for `WON`, `false` for `LOST`.
        won: bool,
    },
}

impl DirectiveOperation {
    /// Every measured operation code, as variants — one entry per code
    /// [`Self::code`] publishes, so the parameterized mechanisms appear once
    /// per spelling (`SetTargetFlag` four times, `OutcomeClass` twice).
    pub const ALL: [DirectiveOperation; 43] = [
        Self::InactiveMembers,
        Self::InactiveThreshold,
        Self::DangerZoneFlags,
        Self::DangerZoneThreshold,
        Self::EnemyGroupDepletion,
        Self::AnimationStates,
        Self::Travelers,
        Self::NamedCounters,
        Self::DormantStart,
        Self::DependencyGate,
        Self::PresentationIdentity,
        Self::WakeObjectives,
        Self::SleepObjectives,
        Self::KillObjectives,
        Self::NapObjective,
        Self::SetTargetFlag {
            objective: true,
            set: true,
        },
        Self::SetTargetFlag {
            objective: true,
            set: false,
        },
        Self::SetTargetFlag {
            objective: false,
            set: true,
        },
        Self::SetTargetFlag {
            objective: false,
            set: false,
        },
        Self::AdvanceStopPoint,
        Self::ZeppelinCannons,
        Self::AssignNet,
        Self::AssignTeam,
        Self::SetAttackRadius,
        Self::ReleaseTaxi,
        Self::SetHelpLabel,
        Self::StopQueuedSounds,
        Self::CompletedSoundGroup,
        Self::AdjustMissionTimer,
        Self::EndMissionTimer,
        Self::WarpVehicle,
        Self::WakeEnemies,
        Self::WakeTurrets,
        Self::WakeZeppelinTurrets,
        Self::FeedGenerator,
        Self::WakeAnimation,
        Self::WakeSoundGroup,
        Self::ResetMissionTimer,
        Self::HideObjective,
        Self::TransitionAnimation,
        Self::WakeObjectivesOnTransition,
        Self::OutcomeClass { won: true },
        Self::OutcomeClass { won: false },
    ];

    /// The stable identifier a report carries — the identical string
    /// `cs_content::mission_control::DirectiveOperation::code` publishes for
    /// the same operation.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::InactiveMembers => "inactive_members",
            Self::InactiveThreshold => "inactive_threshold",
            Self::DangerZoneFlags => "danger_zone_flags",
            Self::DangerZoneThreshold => "danger_zone_threshold",
            Self::EnemyGroupDepletion => "enemy_group_depletion",
            Self::AnimationStates => "animation_states",
            Self::Travelers => "travelers",
            Self::NamedCounters => "named_counters",
            Self::DormantStart => "dormant_start",
            Self::DependencyGate => "dependency_gate",
            Self::PresentationIdentity => "presentation_identity",
            Self::WakeObjectives => "wake_objectives",
            Self::SleepObjectives => "sleep_objectives",
            Self::KillObjectives => "kill_objectives",
            Self::NapObjective => "nap_objective",
            Self::SetTargetFlag {
                objective: true,
                set: true,
            } => "add_objective_target",
            Self::SetTargetFlag {
                objective: true,
                set: false,
            } => "remove_objective_target",
            Self::SetTargetFlag {
                objective: false,
                set: true,
            } => "add_other_target",
            Self::SetTargetFlag {
                objective: false,
                set: false,
            } => "remove_other_target",
            Self::AdvanceStopPoint => "advance_stop_point",
            Self::ZeppelinCannons => "zeppelin_cannons",
            Self::AssignNet => "assign_net",
            Self::AssignTeam => "assign_team",
            Self::SetAttackRadius => "set_attack_radius",
            Self::ReleaseTaxi => "release_taxi",
            Self::SetHelpLabel => "set_help_label",
            Self::StopQueuedSounds => "stop_queued_sounds",
            Self::CompletedSoundGroup => "completed_sound_group",
            Self::AdjustMissionTimer => "adjust_mission_timer",
            Self::EndMissionTimer => "end_mission_timer",
            Self::WarpVehicle => "warp_vehicle",
            Self::WakeEnemies => "wake_enemies",
            Self::WakeTurrets => "wake_turrets",
            Self::WakeZeppelinTurrets => "wake_zeppelin_turrets",
            Self::FeedGenerator => "feed_generator",
            Self::WakeAnimation => "wake_animation",
            Self::WakeSoundGroup => "wake_sound_group",
            Self::ResetMissionTimer => "reset_mission_timer",
            Self::HideObjective => "hide_objective",
            Self::TransitionAnimation => "transition_animation",
            Self::WakeObjectivesOnTransition => "wake_objectives_on_transition",
            Self::OutcomeClass { won: true } => "outcome_won",
            Self::OutcomeClass { won: false } => "outcome_lost",
        }
    }

    /// The operation a measured code names; `None` for a code this build
    /// does not publish, so a caller holding a newer measurement is refused
    /// rather than silently reading a different operation.
    #[must_use]
    pub fn from_code(code: &str) -> Option<Self> {
        Self::ALL.iter().find(|op| op.code() == code).copied()
    }
}

/// The documented phase in which an action's effect is resolved. Effects are
/// queued and resolved in this order, never recursively (non-negotiable
/// behavior 2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Phase {
    /// Variable writes, visible to conditions from the next tick.
    State,
    /// The terminal outcome request.
    Terminal,
    /// Reward intents and presentation cues handed to the host.
    Host,
}

/// An ordered action run when an objective's condition first becomes true, or
/// as part of a deferred work item ([`Action::Schedule`]).
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    SetVariable {
        variable: SymbolId,
        value: Value,
    },
    /// Writes the variable with the next draw of the mission's explicit RNG
    /// stream, uniform over the inclusive range `[min, max]` (contract:
    /// "explicit RNG"; determinism comes from the seeded stream, never from
    /// ambient entropy).
    Draw {
        variable: SymbolId,
        min: i32,
        max: i32,
    },
    /// Request the mission's terminal outcome.
    Finish(Outcome),
    /// A reward intent; the host applies it, the runtime emits it once.
    GrantReward {
        reward: ContentId,
    },
    /// Enqueues `actions` as one deferred work item, eligible on the tick
    /// `delay_ticks` after the current one. `delay_ticks == 0` appends to
    /// this tick's work queue, so it still runs this tick — after everything
    /// already queued (non-negotiable behavior 2: no reentrancy).
    Schedule {
        delay_ticks: u64,
        actions: Vec<Action>,
    },
    /// Re-queues the action list containing this action — the objective's
    /// list when reached from an objective, or the scheduled item's list when
    /// reached from a deferred item — eligible `delay_ticks` from now. A
    /// zero-delay `Reschedule` is how a program schedules *itself*; the
    /// per-tick work budget is what bounds it.
    Reschedule {
        delay_ticks: u64,
    },
    /// A measured directive operation handed to the host with the bound
    /// call's own arguments — nested lists stay nested, field for field
    /// ([`Value::List`]). The runtime executes it as a documented host
    /// effect: an emission on the session's directive log
    /// ([`crate::runtime::MissionState::directives`]), exactly once per
    /// execution key. It is never a no-op and never `Unknown`: the operation
    /// names what the original is measured to do, and carrying it to the
    /// host is the action's whole effect.
    Directive {
        /// The measured operation the binding declared.
        operation: DirectiveOperation,
        /// The call's arguments as the site spelled them.
        args: Vec<Value>,
    },
    /// An instruction or native call that could not be decoded.
    Unknown {
        instruction: String,
    },
}

impl Action {
    /// The phase in which this action resolves.
    pub fn phase(&self) -> Phase {
        match self {
            // `Draw` writes a variable and `Schedule`/`Reschedule` mutate the
            // pending queue; both are runtime state, resolved in place.
            Self::SetVariable { .. }
            | Self::Draw { .. }
            | Self::Schedule { .. }
            | Self::Reschedule { .. } => Phase::State,
            Self::Finish(_) => Phase::Terminal,
            Self::GrantReward { .. } | Self::Directive { .. } | Self::Unknown { .. } => Phase::Host,
        }
    }
}

/// A mutable program variable with a declared type.
#[derive(Clone, Debug, PartialEq)]
pub struct Variable {
    pub id: SymbolId,
    pub name: String,
    pub initial: Value,
}

/// One objective: a latch that fires its actions once when `condition` holds.
#[derive(Clone, Debug, PartialEq)]
pub struct Objective {
    pub id: SymbolId,
    /// The stable content id (`ContentKind::Objective`).
    pub content: ContentId,
    pub condition: Condition,
    pub actions: Vec<Action>,
    pub span: Option<SourceSpan>,
}

/// The versioned mission program: data only.
///
/// `objectives` contributes two different orders and neither is the other's sort
/// (both are designed policies, pinned by the F37-D corpus):
///
/// - **Execution order** is declaration order. Within one tick the objectives
///   resolve in the order they are declared here, then the deferred work queue
///   drains. When two objectives write the same variable on the same tick, the
///   later declaration's write is the one that lands.
/// - **Observation order** is [`crate::runtime::EventKey`]: session, tick, source
///   symbol, sequence. It does not depend on declaration order at all, so the
///   same objectives declared in any order report the same event sequence.
#[derive(Clone, Debug, PartialEq)]
pub struct MissionProgram {
    pub version: u32,
    /// The mission's stable id (`ContentKind::Mission`).
    pub mission: ContentId,
    pub variables: Vec<Variable>,
    pub objectives: Vec<Objective>,
}

/// Where in the program a diagnostic points: mission id, objective and a
/// short node trace (non-negotiable behavior 4).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProgramLocator {
    pub mission: String,
    pub objective: Option<SymbolId>,
    pub trace: Vec<String>,
}

impl fmt::Display for ProgramLocator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.mission)?;
        if let Some(o) = self.objective {
            write!(f, " objective#{}", o.0)?;
        }
        if !self.trace.is_empty() {
            write!(f, " [{}]", self.trace.join(" > "))?;
        }
        Ok(())
    }
}

/// Why a program cannot launch.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ValidationError {
    UnsupportedVersion {
        found: u32,
    },
    WrongContentKind {
        at: ProgramLocator,
        expected: ContentKind,
    },
    DuplicateSymbol {
        at: ProgramLocator,
        symbol: SymbolId,
    },
    UnknownVariable {
        at: ProgramLocator,
        symbol: SymbolId,
    },
    TypeMismatch {
        at: ProgramLocator,
        expected: ValueType,
        found: ValueType,
    },
    NonFiniteValue {
        at: ProgramLocator,
    },
    InvalidComparison {
        at: ProgramLocator,
        op: CompareOp,
        ty: ValueType,
    },
    ConditionTooDeep {
        at: ProgramLocator,
    },
    /// A `Schedule` action list nested deeper than [`MAX_ACTION_NESTING`].
    ActionsTooDeep {
        at: ProgramLocator,
    },
    /// A `Value::List` nested deeper than [`MAX_VALUE_DEPTH`].
    ValueTooDeep {
        at: ProgramLocator,
    },
    /// A `Value::List` — or a directive's top-level argument list — holding
    /// more than [`MAX_VALUE_ITEMS`] items.
    TooManyValueItems {
        at: ProgramLocator,
        count: usize,
    },
    /// A `Draw` whose `min` exceeds its `max`.
    InvalidRange {
        at: ProgramLocator,
    },
    TooManyActions {
        at: ProgramLocator,
        count: usize,
    },
    /// An undecodable instruction or native call: the mission is Unsupported
    /// and must not progress or reward.
    UnsupportedInstruction {
        at: ProgramLocator,
        instruction: String,
    },
}

impl fmt::Display for ValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedVersion { found } => {
                write!(f, "IR version {found} unsupported (expected {IR_VERSION})")
            }
            Self::WrongContentKind { at, expected } => {
                write!(f, "{at}: id is not a {} id", expected.label())
            }
            Self::DuplicateSymbol { at, symbol } => {
                write!(f, "{at}: duplicate symbol #{}", symbol.0)
            }
            Self::UnknownVariable { at, symbol } => {
                write!(f, "{at}: unknown variable #{}", symbol.0)
            }
            Self::TypeMismatch {
                at,
                expected,
                found,
            } => {
                write!(f, "{at}: expected {expected:?}, found {found:?}")
            }
            Self::NonFiniteValue { at } => write!(f, "{at}: non-finite float"),
            Self::InvalidComparison { at, op, ty } => {
                write!(f, "{at}: {op:?} not defined for {ty:?}")
            }
            Self::ConditionTooDeep { at } => {
                write!(f, "{at}: condition deeper than {MAX_CONDITION_DEPTH}")
            }
            Self::ActionsTooDeep { at } => {
                write!(
                    f,
                    "{at}: scheduled actions deeper than {MAX_ACTION_NESTING}"
                )
            }
            Self::ValueTooDeep { at } => {
                write!(f, "{at}: value list nested deeper than {MAX_VALUE_DEPTH}")
            }
            Self::TooManyValueItems { at, count } => {
                write!(f, "{at}: {count} list items exceeds {MAX_VALUE_ITEMS}")
            }
            Self::InvalidRange { at } => write!(f, "{at}: draw min exceeds max"),
            Self::TooManyActions { at, count } => {
                write!(
                    f,
                    "{at}: {count} actions exceeds {MAX_ACTIONS_PER_OBJECTIVE}"
                )
            }
            Self::UnsupportedInstruction { at, instruction } => {
                write!(f, "{at}: unsupported instruction `{instruction}`")
            }
        }
    }
}

impl std::error::Error for ValidationError {}

/// A program that passed [`MissionProgram::validate`]. Only this type can be
/// handed to the runtime.
#[derive(Clone, Debug, PartialEq)]
pub struct ValidatedProgram(MissionProgram);

impl ValidatedProgram {
    /// The validated program data.
    pub fn program(&self) -> &MissionProgram {
        &self.0
    }

    /// Checks one standalone action list against this program's declarations:
    /// the same rules [`MissionProgram::validate`] applies to an objective's
    /// list, at the same bounds.
    ///
    /// A deferred work item in a save record is data from outside the process
    /// (`crate::runtime::MissionState::restore`), and the evaluator's actions
    /// are written on the assumption that validation already refused an
    /// `Unknown` node, an empty `Draw` range and an undeclared write. A record
    /// can carry none of those, so its deferred lists are validated here before
    /// they are allowed back into the queue.
    ///
    /// # Errors
    ///
    /// The first [`ValidationError`] in declaration order, with its locator.
    pub fn validate_actions(&self, actions: &[Action]) -> Result<(), ValidationError> {
        let ctx = Ctx {
            program: &self.0,
            objective: None,
        };
        if actions.len() > MAX_ACTIONS_PER_OBJECTIVE {
            return Err(ValidationError::TooManyActions {
                at: ctx.at(&["deferred actions"]),
                count: actions.len(),
            });
        }
        for (i, action) in actions.iter().enumerate() {
            ctx.action(action, i, 0)?;
        }
        Ok(())
    }
}

struct Ctx<'a> {
    program: &'a MissionProgram,
    objective: Option<SymbolId>,
}

impl Ctx<'_> {
    fn at(&self, trace: &[&str]) -> ProgramLocator {
        ProgramLocator {
            mission: self.program.mission.to_string(),
            objective: self.objective,
            trace: trace.iter().map(|s| (*s).to_owned()).collect(),
        }
    }

    fn variable_type(&self, id: SymbolId) -> Option<ValueType> {
        self.program
            .variables
            .iter()
            .find(|v| v.id == id)
            .map(|v| v.initial.value_type())
    }

    /// The IR's value bounds — finiteness, list depth and item count —
    /// located at `at`.
    fn check_value(&self, at: &ProgramLocator, value: &Value) -> Result<(), ValidationError> {
        match value.check(0) {
            Ok(()) => Ok(()),
            Err(ValueDefect::NonFinite) => Err(ValidationError::NonFiniteValue { at: at.clone() }),
            Err(ValueDefect::TooDeep) => Err(ValidationError::ValueTooDeep { at: at.clone() }),
            Err(ValueDefect::TooManyItems { count }) => Err(ValidationError::TooManyValueItems {
                at: at.clone(),
                count,
            }),
        }
    }

    fn condition(
        &self,
        c: &Condition,
        depth: usize,
        trace: &[&str],
    ) -> Result<(), ValidationError> {
        if depth > MAX_CONDITION_DEPTH {
            return Err(ValidationError::ConditionTooDeep { at: self.at(trace) });
        }
        match c {
            Condition::Const(_) | Condition::ActorIs { .. } => Ok(()),
            Condition::Unknown { instruction } => Err(ValidationError::UnsupportedInstruction {
                at: self.at(trace),
                instruction: instruction.clone(),
            }),
            Condition::Not(inner) => self.condition(inner, depth + 1, trace),
            Condition::All(items) | Condition::Any(items) => items
                .iter()
                .try_for_each(|i| self.condition(i, depth + 1, trace)),
            Condition::Compare {
                variable,
                op,
                value,
            } => {
                let at = || self.at(&[trace, &["compare"]].concat());
                let Some(expected) = self.variable_type(*variable) else {
                    return Err(ValidationError::UnknownVariable {
                        at: at(),
                        symbol: *variable,
                    });
                };
                self.check_value(&at(), value)?;
                if value.value_type() != expected {
                    return Err(ValidationError::TypeMismatch {
                        at: at(),
                        expected,
                        found: value.value_type(),
                    });
                }
                let ordered = matches!(expected, ValueType::Int | ValueType::Float);
                let equality = matches!(op, CompareOp::Eq | CompareOp::Ne);
                if !ordered && !equality {
                    return Err(ValidationError::InvalidComparison {
                        at: at(),
                        op: *op,
                        ty: expected,
                    });
                }
                Ok(())
            }
        }
    }

    fn action(&self, a: &Action, index: usize, depth: usize) -> Result<(), ValidationError> {
        let label = format!("action {index}");
        let at = || self.at(&[label.as_str()]);
        if depth > MAX_ACTION_NESTING {
            return Err(ValidationError::ActionsTooDeep { at: at() });
        }
        match a {
            Action::Finish(_) | Action::GrantReward { .. } | Action::Reschedule { .. } => Ok(()),
            Action::Directive { args, .. } => {
                // A directive's top-level arguments are its argument list;
                // the same bound a `Value::List` carries applies to it.
                if args.len() > MAX_VALUE_ITEMS {
                    return Err(ValidationError::TooManyValueItems {
                        at: at(),
                        count: args.len(),
                    });
                }
                for arg in args {
                    self.check_value(&at(), arg)?;
                }
                Ok(())
            }
            Action::Unknown { instruction } => Err(ValidationError::UnsupportedInstruction {
                at: at(),
                instruction: instruction.clone(),
            }),
            Action::Schedule { actions, .. } => {
                if actions.len() > MAX_ACTIONS_PER_OBJECTIVE {
                    return Err(ValidationError::TooManyActions {
                        at: at(),
                        count: actions.len(),
                    });
                }
                for (i, nested) in actions.iter().enumerate() {
                    self.action(nested, i, depth + 1)?;
                }
                Ok(())
            }
            Action::Draw { variable, min, max } => {
                let Some(expected) = self.variable_type(*variable) else {
                    return Err(ValidationError::UnknownVariable {
                        at: at(),
                        symbol: *variable,
                    });
                };
                if expected != ValueType::Int {
                    return Err(ValidationError::TypeMismatch {
                        at: at(),
                        expected: ValueType::Int,
                        found: expected,
                    });
                }
                if min > max {
                    return Err(ValidationError::InvalidRange { at: at() });
                }
                Ok(())
            }
            Action::SetVariable { variable, value } => {
                let Some(expected) = self.variable_type(*variable) else {
                    return Err(ValidationError::UnknownVariable {
                        at: at(),
                        symbol: *variable,
                    });
                };
                self.check_value(&at(), value)?;
                if value.value_type() != expected {
                    return Err(ValidationError::TypeMismatch {
                        at: at(),
                        expected,
                        found: value.value_type(),
                    });
                }
                Ok(())
            }
        }
    }
}

impl MissionProgram {
    /// Validates version, ids, references, types and bounds, in declaration
    /// order, returning the first failure with its locator.
    ///
    /// # Errors
    ///
    /// A [`ValidationError`]; in particular an `Unknown` instruction yields
    /// [`ValidationError::UnsupportedInstruction`] rather than being skipped.
    pub fn validate(self) -> Result<ValidatedProgram, ValidationError> {
        let ctx = Ctx {
            program: &self,
            objective: None,
        };
        if self.version != IR_VERSION {
            return Err(ValidationError::UnsupportedVersion {
                found: self.version,
            });
        }
        if self.mission.kind() != ContentKind::Mission {
            return Err(ValidationError::WrongContentKind {
                at: ctx.at(&[]),
                expected: ContentKind::Mission,
            });
        }
        let mut seen = BTreeSet::new();
        for v in &self.variables {
            if !seen.insert(v.id) {
                return Err(ValidationError::DuplicateSymbol {
                    at: ctx.at(&["variables"]),
                    symbol: v.id,
                });
            }
            ctx.check_value(&ctx.at(&["variables"]), &v.initial)?;
        }
        for o in &self.objectives {
            let ctx = Ctx {
                program: &self,
                objective: Some(o.id),
            };
            if !seen.insert(o.id) {
                return Err(ValidationError::DuplicateSymbol {
                    at: ctx.at(&[]),
                    symbol: o.id,
                });
            }
            if o.content.kind() != ContentKind::Objective {
                return Err(ValidationError::WrongContentKind {
                    at: ctx.at(&[]),
                    expected: ContentKind::Objective,
                });
            }
            if o.actions.len() > MAX_ACTIONS_PER_OBJECTIVE {
                return Err(ValidationError::TooManyActions {
                    at: ctx.at(&[]),
                    count: o.actions.len(),
                });
            }
            ctx.condition(&o.condition, 0, &["condition"])?;
            for (i, a) in o.actions.iter().enumerate() {
                ctx.action(a, i, 0)?;
            }
        }
        Ok(ValidatedProgram(self))
    }
}
