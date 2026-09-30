//! The environment clock: the authored weather timeline on its time domain
//! (F19-A).
//!
//! Spec: `specs/F19-sky-atmosphere-weather-and-visibility.md`, stage
//! `### F19-A`, non-negotiable behavior 4.
//!
//! [`EnvironmentClock`] is the one place the authored record meets the
//! authored schedule: it takes an
//! [`EnvironmentDefinition`](cs_content::environment::EnvironmentDefinition),
//! lifts its [`EnvironmentState`](cs_content::environment::EnvironmentState)
//! and its [`EnvironmentTimeline`](cs_content::environment::EnvironmentTimeline)
//! into `cs_sim::visibility`'s generic
//! [`VisibilityTimeline`](cs_sim::visibility::VisibilityTimeline), and
//! advances them on [`ENVIRONMENT_TIME_DOMAIN`](cs_sim::visibility::ENVIRONMENT_TIME_DOMAIN).
//!
//! The wiring is deliberately thin and total:
//!
//! * one [`WeatherEvent`](cs_content::environment::WeatherEvent) becomes one
//!   [`VisibilityEvent`](cs_sim::visibility::VisibilityEvent), in authored
//!   order, with its tick copied verbatim — no retiming, no rounding;
//! * the current state is the record's own
//!   [`EnvironmentState`](cs_content::environment::EnvironmentState), so a
//!   consumer reads the *same* wind field the timeline replaced, never a
//!   cached copy;
//! * pause, replay and the refusal of injected ticks come from the domain,
//!   so no environment code re-implements a clock rule.
//!
//! What this stage does **not** do: no weather is simulated, no wind is
//! consumed and no particle is drawn. F19-B implements the effects and
//! F19-C wires wind and visibility into their real producers and consumers.

use std::time::Duration;

use cs_content::environment::{
    EnvironmentDefinition, EnvironmentId, EnvironmentState, WeatherEvent,
};
use cs_sim::time::{TickRate, TimeDomain};
use cs_sim::visibility::{TimelineError, VisibilityEvent, VisibilityTimeline};
use cs_types::Tick;

/// The environment of one session: its current
/// [`EnvironmentState`](cs_content::environment::EnvironmentState) and the
/// clock that replaces that state at authored ticks.
#[derive(Clone, Debug, PartialEq)]
pub struct EnvironmentClock {
    id: EnvironmentId,
    timeline: VisibilityTimeline<EnvironmentState>,
}

impl EnvironmentClock {
    /// Lifts an authored environment's state and timeline onto its time
    /// domain.
    ///
    /// # Errors
    ///
    /// [`TimelineError::UnorderedTicks`] when the authored events are not
    /// strictly increasing by tick. The definition itself was already
    /// validated by
    /// [`EnvironmentDefinition::try_new`](cs_content::environment::EnvironmentDefinition::try_new),
    /// so this can only fail for a timeline assembled some other way.
    pub fn new(environment: &EnvironmentDefinition, rate: TickRate) -> Result<Self, TimelineError> {
        let events: Vec<VisibilityEvent<EnvironmentState>> = environment
            .timeline()
            .events()
            .iter()
            .map(|event: &WeatherEvent| {
                VisibilityEvent::new(event.at_tick(), event.state().clone())
            })
            .collect();
        Ok(Self {
            id: environment.id().clone(),
            timeline: VisibilityTimeline::try_new(rate, environment.state().clone(), events)?,
        })
    }

    /// The environment this clock runs.
    #[must_use]
    pub fn id(&self) -> &EnvironmentId {
        &self.id
    }

    /// The domain these events advance in: authoritative gameplay time.
    #[must_use]
    pub fn domain(&self) -> TimeDomain {
        self.timeline.domain()
    }

    /// The current state: the authored initial state, replaced by each
    /// event as its tick was reached.
    #[must_use]
    pub fn state(&self) -> &EnvironmentState {
        self.timeline.state()
    }

    /// The last committed tick.
    #[must_use]
    pub fn tick(&self) -> Tick {
        self.timeline.tick()
    }

    /// Whether the local player is paused.
    #[must_use]
    pub fn is_paused(&self) -> bool {
        self.timeline.is_paused()
    }

    /// Pauses or resumes the environment. While paused no event fires, so a
    /// paused run's weather never moves.
    pub fn set_paused(&mut self, paused: bool) {
        self.timeline.set_paused(paused);
    }

    /// Advances one render frame of `elapsed` wall time, installing every
    /// authored event whose tick the clock committed, and returns the whole
    /// ticks that frame gained.
    ///
    /// # Errors
    ///
    /// [`TimelineError::Clock`] if the tick counter would wrap; nothing is
    /// mutated when it does.
    pub fn advance_frame(&mut self, elapsed: Duration) -> Result<u64, TimelineError> {
        self.timeline.advance_frame(elapsed)
    }

    /// Injects whole ticks on local authority — always refused, because the
    /// environment runs on a domain with no local speed-up authority.
    ///
    /// # Errors
    ///
    /// [`cs_sim::time::TimeError::NoSpeedUpAuthority`], always. Nothing is
    /// mutated.
    pub fn advance_fixed_ticks(&mut self, ticks: u64) -> Result<u64, TimelineError> {
        self.timeline.advance_fixed_ticks(ticks)
    }

    /// The indices of the authored events already installed, in
    /// installation order. `EnvironmentDefinition::timeline().events()` at
    /// the same index is the event that fired.
    #[must_use]
    pub fn applied(&self) -> &[usize] {
        self.timeline.applied()
    }
}
