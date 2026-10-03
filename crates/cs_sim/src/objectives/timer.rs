//! Mission timers: a declared start condition, a declared time domain, and one
//! declared action on expiry.
//!
//! Spec: `specs/F39-objectives-triggers-timers-spawn-groups-and-dialogue-cues.md`
//! (F39 owns trigger semantics), stage `### F39-B`, non-negotiable behavior 3:
//!
//! > Timers have a declared start condition and time domain. Reaching a
//! > waypoint can unlock or reset objectives only through a specific program
//! > action.
//!
//! Shared contract: `docs/contracts/SCRIPT-MISSION.md`, "IR requirements" and
//! `docs/contracts/FLIGHT-PHYSICS.md` via `cs_sim::time`'s
//! [`ClockPolicy`](crate::time::ClockPolicy).
//!
//! # The three things this module makes structural
//!
//! 1. **A start condition is data.** [`TimerStart`] is a variant, not a
//!    convention: an unarmed [`MissionTimer`] holds
//!    [`TimerState::NotArmed`], consumes no tick and cannot expire. There is no
//!    way to have a running deadline whose beginning nobody declared.
//! 2. **A time domain is data, and only gameplay domains are accepted.**
//!    [`MissionTimer::new`] refuses a [`ClockPolicy`](crate::time::ClockPolicy)
//!    whose domain is UI wall or unscaled media, because a mission deadline
//!    that a menu frame or a cutscene advances is not a gameplay deadline. The
//!    timer also carries the policy's pause rule, so a caller can read whether
//!    pausing freezes it rather than assuming.
//! 3. **Expiry performs exactly one declared action.** [`TimerAction`] is a
//!    closed set of program actions. There is no "unlock the next objective"
//!    behaviour: crossing a waypoint volume raises a trigger event and nothing
//!    else, so an objective can only be unlocked, reset, succeeded or failed by
//!    an action somebody declared.
//!
//! # Why this is not `cs_sim::time::TickTimer`
//!
//! [`TickTimer`](crate::time::TickTimer) is F16-D's *single* gameplay
//! quantity, fed by one clock's committed ticks and owned by
//! [`GameplayTimeline`](crate::time::GameplayTimeline). A mission program
//! declares one timer per deadline, each with its own start condition and its
//! own domain, and none of them may be re-armed by an unrelated frame. So this
//! is the program's timer *table*: it keeps its own whole-tick countdown and
//! borrows [`TimeDomain`](crate::time::TimeDomain) for the domain vocabulary
//! instead of a second copy of the clock.
//!
//! Like [`TickTimer`](crate::time::TickTimer) it has **no wall-time entry
//! point**: [`MissionTimer::advance`] takes whole committed ticks, never a
//! [`std::time::Duration`], so a paused frame that commits no tick cannot
//! shorten a deadline (F16 AC04's structural form).
//!
//! # What is unknown
//!
//! Which deadlines the original game declares, in which domain, with which
//! start conditions and which actions is **unmeasured**; it is F39-D's
//! calibration with `retail` capability. Nothing here is an original-fidelity
//! claim, and a period is always a caller-supplied input rather than a table
//! this module invented.

use std::fmt;

use cs_script::ir::SymbolId;
use cs_types::Tick;
use cs_types::content::ContentId;

use super::spawn::IdempotencyKey;
use super::state::ObjectiveState;
use super::terminal::TerminalOutcome;
use crate::time::{ClockPolicy, TimeDomain};

/// When a mission timer starts counting.
///
/// The start condition is part of the declaration, never a side effect of
/// construction order: a timer built with [`TimerStart::Never`] stays unarmed
/// for the whole session even if every other kind of event occurs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TimerStart {
    /// Armed only by an explicit [`TimerRequest::Arm`] — a program action,
    /// never a volume crossing.
    OnArm,
    /// Armed automatically at the first evaluated tick at or after this tick.
    AtTick(Tick),
    /// Armed the first tick a named mission signal is raised. A signal raised
    /// *by a timer's own action* is not eligible until the next tick, so two
    /// timers can never chase each other in one tick.
    OnSignal(SymbolId),
    /// Armed the first tick a declared objective reaches a declared state.
    OnObjectiveState {
        objective: SymbolId,
        state: ObjectiveState,
    },
    /// Declared never to run: the timer exists in the table and never arms.
    Never,
}

impl TimerStart {
    /// Stable label for diagnostics and evidence records.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::OnArm => "on_arm",
            Self::AtTick(_) => "at_tick",
            Self::OnSignal(_) => "on_signal",
            Self::OnObjectiveState { .. } => "on_objective_state",
            Self::Never => "never",
        }
    }
}

/// What one timer does when it expires.
///
/// A closed set of program actions. The absence of a general "unlock" is the
/// point: F39 non-negotiable behavior 3 makes a specific program action the
/// only way reaching a waypoint unlocks or resets anything, so a waypoint
/// crossing reaches a timer only through the program's own
/// [`TimerRequest::Arm`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TimerAction {
    /// Move a declared objective to a declared state: the unlock, reset,
    /// succeed or fail of F39 non-negotiable behavior 3.
    SetObjectiveState {
        objective: SymbolId,
        state: ObjectiveState,
    },
    /// Raise a named mission signal, eligible to arm other timers from the next
    /// tick.
    Signal(SymbolId),
    /// Ask for a spawn group's instances. The `key` is the authored
    /// idempotency key, so a repeat of the same key is refused by
    /// [`EmissionLedger`](super::spawn::EmissionLedger) instead of spawning a
    /// second wave. `group` is the mission program's spawn-group symbol; the
    /// content behind it is bound by F39-C.
    SpawnGroup {
        key: IdempotencyKey,
        group: SymbolId,
        count: u32,
    },
    /// Play one dialogue cue once per `key`.
    Cue {
        key: IdempotencyKey,
        dialogue: ContentId,
    },
    /// Grant an optional reward. Never terminal: an optional reward is not a
    /// mission ending (F39 non-negotiable behavior 5).
    GrantOptionalReward { reward: ContentId },
    /// Request the mission's terminal outcome.
    Finish(TerminalOutcome),
}

impl TimerAction {
    /// Whether the action asks for a mission ending.
    #[must_use]
    pub const fn is_terminal(&self) -> bool {
        matches!(self, Self::Finish(_))
    }

    /// The outcome this action requests, when it requests one.
    #[must_use]
    pub const fn terminal_outcome(&self) -> Option<TerminalOutcome> {
        match self {
            Self::Finish(outcome) => Some(*outcome),
            _ => None,
        }
    }
}

/// What a program asks of a declared timer on one tick.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimerRequest {
    /// Arm or re-arm the timer now, whatever its declared start condition says.
    /// This is the program action F39 non-negotiable behavior 3 requires: the
    /// only way a crossing, a signal or a state change starts a deadline.
    Arm(SymbolId),
    /// Stop a running timer. It can only run again through its declared start
    /// or a fresh [`TimerRequest::Arm`].
    Cancel(SymbolId),
}

impl TimerRequest {
    /// The timer this request names.
    #[must_use]
    pub const fn timer(self) -> SymbolId {
        match self {
            Self::Arm(timer) | Self::Cancel(timer) => timer,
        }
    }

    /// Stable label for diagnostics.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Arm(_) => "arm",
            Self::Cancel(_) => "cancel",
        }
    }
}

/// A timer declaration's state. Only [`MissionTimer`] changes it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimerState {
    /// Not started. Consumes no tick and cannot expire.
    NotArmed,
    /// Counting down whole committed ticks.
    Armed { since: Tick, remaining: u64 },
    /// Ran out. Its action was performed once; the timer cannot run again
    /// without a declared start or a fresh [`TimerRequest::Arm`].
    Expired { at: Tick },
    /// [`TimerStart::Never`]: the declaration exists and never runs.
    Never,
}

impl TimerState {
    /// Whether the timer is counting down.
    #[must_use]
    pub const fn is_armed(&self) -> bool {
        matches!(self, Self::Armed { .. })
    }
}

/// Why a timer operation was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimerError {
    /// A zero-tick period: a deadline of no length has no meaning and no
    /// seconds equivalent.
    ZeroPeriod { timer: SymbolId },
    /// The declared policy's domain is not a gameplay domain. UI wall and
    /// unscaled media keep running while gameplay is paused, so a mission
    /// deadline on them would move during a pause.
    NotGameplayDomain { timer: SymbolId, domain: TimeDomain },
    /// A [`TimerStart::Never`] timer was asked to run.
    NeverStarted { timer: SymbolId },
    /// An automatic start tried to re-arm a timer that already ran out. A
    /// timer runs **once per declared start**: its action has been performed,
    /// and repeating it because the same signal or state occurred again is
    /// exactly the duplicate wave or repeated radio line F39 non-negotiable
    /// behavior 4 forbids. Only an explicit [`TimerRequest::Arm`] may run it
    /// again.
    AlreadyExpired { timer: SymbolId },
    /// The timer was not armed, so there was nothing to cancel.
    NotArmed { timer: SymbolId },
}

impl fmt::Display for TimerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ZeroPeriod { timer } => {
                write!(f, "timer {timer:?} declares a zero-tick period")
            }
            Self::NotGameplayDomain { timer, domain } => write!(
                f,
                "timer {timer:?} declares the {} domain, which is not a gameplay domain",
                domain.label()
            ),
            Self::NeverStarted { timer } => {
                write!(f, "timer {timer:?} is declared never to run")
            }
            Self::AlreadyExpired { timer } => {
                write!(f, "timer {timer:?} already ran out")
            }
            Self::NotArmed { timer } => write!(f, "timer {timer:?} is not armed"),
        }
    }
}

impl std::error::Error for TimerError {}

/// One declared mission deadline.
///
/// A timer is armed by its declared [`TimerStart`] (or by an explicit
/// [`TimerRequest`]), counts whole committed ticks, and performs its single
/// [`TimerAction`] the tick it runs out.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MissionTimer {
    id: SymbolId,
    policy: ClockPolicy,
    start: TimerStart,
    period_ticks: u64,
    action: TimerAction,
    state: TimerState,
}

impl MissionTimer {
    /// Declares one timer.
    ///
    /// `policy` states the timer's time domain *and* its pause rule; the
    /// domain is validated here so a caller cannot build a mission deadline
    /// that runs on UI wall or unscaled media time.
    ///
    /// # Errors
    ///
    /// [`TimerError::ZeroPeriod`] for a zero-tick period and
    /// [`TimerError::NotGameplayDomain`] for a non-gameplay domain.
    pub fn new(
        id: SymbolId,
        policy: ClockPolicy,
        start: TimerStart,
        period_ticks: u64,
        action: TimerAction,
    ) -> Result<Self, TimerError> {
        if period_ticks == 0 {
            return Err(TimerError::ZeroPeriod { timer: id });
        }
        let domain = policy.domain();
        if !matches!(
            domain,
            TimeDomain::Simulation | TimeDomain::AuthoritativeGameplay
        ) {
            return Err(TimerError::NotGameplayDomain { timer: id, domain });
        }
        let state = match start {
            TimerStart::Never => TimerState::Never,
            _ => TimerState::NotArmed,
        };
        Ok(Self {
            id,
            policy,
            start,
            period_ticks,
            action,
            state,
        })
    }

    /// The timer symbol, which is also the `source` of its events.
    #[must_use]
    pub const fn id(&self) -> SymbolId {
        self.id
    }

    /// The declared time domain.
    #[must_use]
    pub const fn domain(&self) -> TimeDomain {
        self.policy.domain()
    }

    /// Whether pausing freezes this timer.
    #[must_use]
    pub const fn freezes_while_paused(&self) -> bool {
        matches!(self.policy.pause(), crate::time::PausePolicy::Freeze)
    }

    /// The declared start condition.
    #[must_use]
    pub const fn start(&self) -> TimerStart {
        self.start
    }

    /// The single action this timer performs on expiry.
    #[must_use]
    pub const fn action(&self) -> &TimerAction {
        &self.action
    }

    /// The authored period, in whole ticks. Seconds are
    /// `period_ticks() * rate.dt_seconds()` at the session's
    /// [`TickRate`](crate::time::TickRate); this type holds no wall clock.
    #[must_use]
    pub const fn period_ticks(&self) -> u64 {
        self.period_ticks
    }

    /// The timer's state.
    #[must_use]
    pub const fn state(&self) -> TimerState {
        self.state
    }

    /// Ticks left, while armed.
    #[must_use]
    pub const fn remaining_ticks(&self) -> Option<u64> {
        match self.state {
            TimerState::Armed { remaining, .. } => Some(remaining),
            _ => None,
        }
    }

    /// Whether the declared start condition arms this timer by itself on
    /// `tick`, with no program action.
    ///
    /// Only [`TimerStart::AtTick`] does: `OnSignal` and `OnObjectiveState` are
    /// armed by the runtime when the signal or state is *observed*, and
    /// `OnArm` only by an explicit request, so this predicate is exactly the
    /// set of timers the runtime arms unattended.
    #[must_use]
    pub fn auto_arms_on(&self, tick: Tick) -> bool {
        matches!(self.start, TimerStart::AtTick(at) if tick >= at)
    }

    /// Arms or re-arms the timer at `tick` with its full period.
    ///
    /// This is the explicit program action: it works from any state except a
    /// [`TimerStart::Never`] declaration, so a repeating deadline is re-armed
    /// by the program that declared it rather than by a repeat of some other
    /// event.
    ///
    /// # Errors
    ///
    /// [`TimerError::NeverStarted`] for a [`TimerStart::Never`] declaration.
    pub fn arm(&mut self, tick: Tick) -> Result<(), TimerError> {
        if let TimerState::Never = self.state {
            return Err(TimerError::NeverStarted { timer: self.id });
        }
        self.state = TimerState::Armed {
            since: tick,
            remaining: self.period_ticks,
        };
        Ok(())
    }

    /// Arms the timer because its **declared** start condition was observed at
    /// `tick` (`AtTick`, `OnSignal` or `OnObjectiveState`).
    ///
    /// # Errors
    ///
    /// [`TimerError::NeverStarted`] for a [`TimerStart::Never`] declaration and
    /// [`TimerError::AlreadyExpired`] when the timer already ran out, so a
    /// repeated signal cannot replay a timer's action.
    pub fn auto_arm(&mut self, tick: Tick) -> Result<(), TimerError> {
        match self.state {
            TimerState::Never => return Err(TimerError::NeverStarted { timer: self.id }),
            TimerState::Expired { .. } => {
                return Err(TimerError::AlreadyExpired { timer: self.id });
            }
            _ => {}
        }
        self.state = TimerState::Armed {
            since: tick,
            remaining: self.period_ticks,
        };
        Ok(())
    }

    /// Stops a running timer. A stopped timer keeps its declaration and can run
    /// again only through its declared start or a fresh
    /// [`TimerRequest::Arm`].
    ///
    /// # Errors
    ///
    /// [`TimerError::NotArmed`] when the timer is not running, and
    /// [`TimerError::NeverStarted`] for a [`TimerStart::Never`] declaration.
    pub fn cancel(&mut self) -> Result<(), TimerError> {
        if let TimerState::Never = self.state {
            return Err(TimerError::NeverStarted { timer: self.id });
        }
        if !self.state.is_armed() {
            return Err(TimerError::NotArmed { timer: self.id });
        }
        self.state = TimerState::NotArmed;
        Ok(())
    }

    /// Consumes `committed` whole committed ticks and reports whether the timer
    /// ran out on this call.
    ///
    /// Saturating at zero and latching [`TimerState::Expired`] means a long
    /// frame produces **one** expiry, not one per tick it covered. The tick the
    /// expiry belongs to is stamped by the caller through
    /// [`stamp_expiry`](Self::stamp_expiry), because `advance` takes only a
    /// tick *count*.
    pub fn advance(&mut self, committed: u64) -> bool {
        let TimerState::Armed { since, remaining } = self.state else {
            return false;
        };
        let left = remaining.saturating_sub(committed);
        if left == 0 {
            self.state = TimerState::Expired { at: Tick(0) };
            true
        } else {
            self.state = TimerState::Armed {
                since,
                remaining: left,
            };
            false
        }
    }

    /// Stamps the tick an expiry belongs to.
    ///
    /// Kept separate from [`advance`](Self::advance) because `advance` takes
    /// only a tick *count*: the caller knows which tick the committed ticks
    /// belong to, and the runtime's own event order is what makes that tick
    /// authoritative.
    pub fn stamp_expiry(&mut self, tick: Tick) {
        if matches!(self.state, TimerState::Expired { .. }) {
            self.state = TimerState::Expired { at: tick };
        }
    }

    /// The tick this timer ran out on, once it has.
    #[must_use]
    pub const fn expired_at(&self) -> Option<Tick> {
        match self.state {
            TimerState::Expired { at } => Some(at),
            _ => None,
        }
    }
}
