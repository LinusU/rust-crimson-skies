//! Gameplay visibility and the environment's time domain (F19-A).
//!
//! Spec: `specs/F19-sky-atmosphere-weather-and-visibility.md`, stage
//! `### F19-A`. Shared contract: `docs/contracts/IDENTITY-CONTENT.md`.
//!
//! Stage F19-A defines the *time domain* half of the environment feature:
//! the authored weather timeline is a list of states at whole simulation
//! ticks (`cs_content::environment::EnvironmentTimeline`), and this module
//! is what gives those ticks a domain, a pause rule and a replay rule.
//!
//! * [`ENVIRONMENT_TIME_DOMAIN`] states the domain in one place: the
//!   environment runs on **authoritative gameplay** time, the same domain
//!   objective timers and weapon cooldowns are judged by, so a weather
//!   change and an objective deadline can never disagree about when the
//!   game is paused or how far a run has advanced.
//! * [`environment_clock_policy`] is that decision as a [`ClockPolicy`]:
//!   pause **freezes** it, and there is no local speed-up authority, so a
//!   replay cannot be jumped forward.
//! * [`VisibilityTimeline`] is the generic runner: it owns a fixed-rate
//!   [`SimClock`] on that policy, holds the authored events in tick order
//!   and installs an event's state only when the clock has actually
//!   committed that tick. It is generic over the state because `cs_sim`
//!   must not depend on `cs_content`; `cs_app::environment` runs
//!   `cs_content::environment::EnvironmentState` on it.
//!
//! # Why this is not a timer in the renderer
//!
//! F19 non-negotiable behavior 4: weather changes that matter to gameplay
//! are deterministic timeline events, and pause and replay respect their
//! time domains. That is only true if the events advance on committed
//! integer ticks rather than on frame deltas:
//!
//! * a paused frame commits zero ticks, so no event fires (the pause rule);
//! * any split of the same wall time into frames commits the same ticks,
//!   because [`SimClock`] accumulates in integers (the frame-rate rule);
//! * [`VisibilityTimeline::advance_fixed_ticks`] refuses outright, so a
//!   replay or a speed-up cannot move an event's instant (the replay rule);
//! * the state is *replaced* by the event that fired, so replaying the same
//!   events reaches the same state at the same tick with no leftover field.
//!
//! Which values are authored, and that gameplay visibility is never derived
//! from screen fog, is `cs_content::environment`'s contract; nothing here
//! invents a default for a state it is handed.

use std::time::Duration;

use cs_types::Tick;

use crate::time::{ClockPolicy, SimClock, TickRate, TimeDomain, TimeError};

/// The time domain every environment change runs in: authoritative gameplay
/// time (`F19` non-negotiable behavior 4).
///
/// It is a single constant rather than a per-record choice: an authored
/// weather timeline has no "which clock am I on" field to get wrong, and a
/// consumer that needs the domain asks here.
pub const ENVIRONMENT_TIME_DOMAIN: TimeDomain = TimeDomain::AuthoritativeGameplay;

/// The clock policy of [`ENVIRONMENT_TIME_DOMAIN`]: pause freezes the
/// environment, and no local authority may inject ticks.
///
/// # Examples
///
/// ```
/// use cs_sim::visibility::environment_clock_policy;
/// use cs_sim::time::{PausePolicy, SpeedUpPolicy, TimeDomain};
///
/// let policy = environment_clock_policy();
/// assert_eq!(policy.domain(), TimeDomain::AuthoritativeGameplay);
/// assert_eq!(policy.pause(), PausePolicy::Freeze);
/// assert_eq!(policy.speed_up(), SpeedUpPolicy::NoLocalAuthority);
/// ```
#[must_use]
pub const fn environment_clock_policy() -> ClockPolicy {
    ClockPolicy::authoritative_gameplay()
}

/// Why a [`VisibilityTimeline`] was rejected or refused an advance.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TimelineError {
    /// Two events were not strictly ordered by tick, so the state a replay
    /// reaches would depend on how the list was supplied.
    UnorderedTicks {
        /// The index of the offending event.
        index: usize,
        /// The tick of the event before it.
        previous: u64,
        /// The tick the offending event carried.
        found: u64,
    },
    /// The underlying clock refused the operation (an injected advance on a
    /// policy that grants no local speed-up authority, a paused freeze, a
    /// wrapping tick counter).
    Clock(TimeError),
}

impl std::fmt::Display for TimelineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnorderedTicks {
                index,
                previous,
                found,
            } => write!(
                f,
                "event {index} is at tick {found}, which does not follow the previous event at tick {previous}"
            ),
            Self::Clock(error) => write!(f, "environment clock refused the advance: {error}"),
        }
    }
}

impl std::error::Error for TimelineError {}

impl From<TimeError> for TimelineError {
    fn from(error: TimeError) -> Self {
        Self::Clock(error)
    }
}

/// One scheduled state change: the state to install at a whole tick.
///
/// The event carries a *whole* state, not a delta, so applying it is a
/// replacement: nothing from the previous state can survive by accident.
#[derive(Clone, Debug, PartialEq)]
pub struct VisibilityEvent<T> {
    at_tick: u64,
    state: T,
}

impl<T> VisibilityEvent<T> {
    /// Schedules `state` at tick `at_tick`.
    #[must_use]
    pub fn new(at_tick: u64, state: T) -> Self {
        Self { at_tick, state }
    }

    /// The tick this event fires on.
    #[must_use]
    pub fn at_tick(&self) -> u64 {
        self.at_tick
    }

    /// The state this event installs when its tick is reached.
    #[must_use]
    pub fn state(&self) -> &T {
        &self.state
    }
}

/// The authored environment events running on the authoritative gameplay
/// clock.
///
/// `T` is the state an event installs; this runner is generic because
/// `cs_sim` cannot depend on `cs_content`. The concrete pairing lives in
/// `cs_app::environment::EnvironmentClock`.
#[derive(Clone, Debug, PartialEq)]
pub struct VisibilityTimeline<T> {
    clock: SimClock,
    events: Vec<VisibilityEvent<T>>,
    /// Index of the next event that has not been installed yet.
    next: usize,
    /// Indices of the events already installed, in installation order.
    applied: Vec<usize>,
    state: T,
}

impl<T: Clone> VisibilityTimeline<T> {
    /// Builds a timeline at `rate` from an initial state and the authored
    /// events.
    ///
    /// Events at tick `0` are installed immediately — tick 0 is where the
    /// run starts, so an event scheduled there is part of the initial
    /// state's surroundings rather than something a frame has to earn.
    ///
    /// # Errors
    ///
    /// [`TimelineError::UnorderedTicks`] when two events are not strictly
    /// increasing by tick. Nothing is constructed when it fails.
    pub fn try_new(
        rate: TickRate,
        initial: T,
        events: Vec<VisibilityEvent<T>>,
    ) -> Result<Self, TimelineError> {
        for (index, window) in events.windows(2).enumerate() {
            if window[1].at_tick <= window[0].at_tick {
                return Err(TimelineError::UnorderedTicks {
                    index: index + 1,
                    previous: window[0].at_tick,
                    found: window[1].at_tick,
                });
            }
        }
        let due_at_start = events.iter().take_while(|event| event.at_tick == 0).count();
        let mut state = initial;
        let mut applied = Vec::with_capacity(due_at_start);
        for (index, event) in events.iter().take(due_at_start).enumerate() {
            state = event.state.clone();
            applied.push(index);
        }
        Ok(Self {
            clock: SimClock::new(environment_clock_policy(), rate),
            events,
            next: due_at_start,
            applied,
            state,
        })
    }

    /// The domain this timeline runs in.
    #[must_use]
    pub const fn domain(&self) -> TimeDomain {
        ENVIRONMENT_TIME_DOMAIN
    }

    /// The fixed-rate clock these events advance on.
    #[must_use]
    pub const fn clock(&self) -> &SimClock {
        &self.clock
    }

    /// The last committed tick.
    #[must_use]
    pub const fn tick(&self) -> Tick {
        self.clock.tick()
    }

    /// Whether the local player is paused.
    #[must_use]
    pub const fn is_paused(&self) -> bool {
        self.clock.is_paused()
    }

    /// Pauses or resumes the timeline. While paused the clock commits no
    /// ticks, so no event fires.
    pub fn set_paused(&mut self, paused: bool) {
        self.clock.set_paused(paused);
    }

    /// The current state: the initial one, replaced by each event as its
    /// tick was reached.
    #[must_use]
    pub const fn state(&self) -> &T {
        &self.state
    }

    /// The authored events, in tick order.
    #[must_use]
    pub fn events(&self) -> &[VisibilityEvent<T>] {
        &self.events
    }

    /// The indices of the events already installed, in installation order.
    #[must_use]
    pub fn applied(&self) -> &[usize] {
        &self.applied
    }

    /// Advances one render frame of `elapsed` wall time, installing every
    /// event whose tick the clock has now committed, and returns the whole
    /// ticks that frame gained.
    ///
    /// # Errors
    ///
    /// [`TimelineError::Clock`] if the tick counter would wrap; nothing is
    /// mutated when it does. A paused frame returns `0` ticks and installs
    /// nothing.
    pub fn advance_frame(&mut self, elapsed: Duration) -> Result<u64, TimelineError> {
        let ticks = self.clock.advance(elapsed)?;
        self.install_due();
        Ok(ticks)
    }

    /// Injects whole ticks on local authority.
    ///
    /// The environment runs on [`ENVIRONMENT_TIME_DOMAIN`], whose policy
    /// grants no local speed-up authority, so this always refuses: a
    /// replay, a speed-up or a scripted jump can never move a weather
    /// event's instant. The refusal precedes the pause check, so a paused
    /// timeline reports [`TimeError::NoSpeedUpAuthority`] and never
    /// [`TimeError::ClockPaused`].
    ///
    /// # Errors
    ///
    /// [`TimeError::NoSpeedUpAuthority`], always. Nothing is mutated.
    pub fn advance_fixed_ticks(&mut self, ticks: u64) -> Result<u64, TimelineError> {
        self.clock.advance_fixed_ticks(ticks)?;
        self.install_due();
        Ok(ticks)
    }

    /// Installs every event whose tick is now due, in tick order.
    fn install_due(&mut self) {
        while self.next < self.events.len() && self.events[self.next].at_tick <= self.clock.tick().0
        {
            self.state = self.events[self.next].state.clone();
            self.applied.push(self.next);
            self.next += 1;
        }
    }
}
