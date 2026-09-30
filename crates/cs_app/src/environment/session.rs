//! The environment of one running mission: the producer that advances the
//! authored weather and the consumers that read it (F19-C).
//!
//! Spec: `specs/F19-sky-atmosphere-weather-and-visibility.md`, stage
//! `### F19-C`, non-negotiable behaviors 1, 2 and 4 and acceptance case AC03.
//! Shared contract: `docs/contracts/IDENTITY-CONTENT.md`.
//!
//! F19-A and F19-B supplied the records and their effects as separate parts.
//! [`EnvironmentSession`] is the one object a mission holds that joins them:
//!
//! * **Producer.** One [`EnvironmentClock`] advances on gameplay time. Nothing
//!   else moves the weather, so a pause freezes it and a replay reaches the
//!   same states ([`EnvironmentSession::restart`] rebuilds it from the
//!   authored definition, which is the retry and teardown path).
//! * **Flight and projectile consumer.** [`EnvironmentSession::wind`] and
//!   [`EnvironmentSession::flight_environment`] read the wind out of the
//!   state the clock installed at its last tick; an explicit unknown wind is
//!   an error that reaches the caller, never still air.
//! * **AI sight consumer.** [`EnvironmentSession::sight_range_m`] is the
//!   authored gameplay visibility and nothing else. It is never derived from
//!   the fog the renderer draws, and an unknown range is an error that names
//!   its claim rather than a default sight range.
//! * **Randomness.** [`RunSeeds`] keeps the mission's seed and the cosmetic
//!   weather seed in separate fields, and the mission AI stream derives from
//!   the mission seed alone. Advancing the weather, resolving effects and
//!   drawing particles never touch it (AC03).
//!
//! What is not claimed: no original visibility or wind value, and no AI
//! consumer of the sight range exists yet. The range is handed out as a
//! number; what an AI does with it belongs to F31.

use std::fmt;
use std::time::Duration;

use cs_content::environment::{CosmeticWeatherSeed, EnvironmentDefinition};
use cs_sim::ai::navigation::AI_NAVIGATION_DOMAIN;
use cs_sim::flight::FlightEnvironment;
use cs_sim::time::TickRate;
use cs_sim::visibility::TimelineError;
use cs_types::content::Resolved;
use cs_types::evidence::ClaimId;
use cs_types::random::SplitMix64;

use crate::environment::air::{AuthoritativeWind, WindUnavailable};
use crate::environment::clock::EnvironmentClock;
use crate::environment::effects::EnvironmentEffects;

/// The seeds of one mission run, kept apart by type and by field.
///
/// The mission seed feeds gameplay streams; the weather seed feeds only
/// decoration. There is no constructor that derives one from the other
/// except [`RunSeeds::from_root`], which hands both the *same* root and
/// relies on domain separation, so a caller that wants them independent
/// supplies two values.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RunSeeds {
    mission_seed: u64,
    weather: CosmeticWeatherSeed,
}

impl RunSeeds {
    /// A run whose gameplay and cosmetic weather seeds are chosen separately.
    #[must_use]
    pub const fn new(mission_seed: u64, weather: CosmeticWeatherSeed) -> Self {
        Self {
            mission_seed,
            weather,
        }
    }

    /// A run with one `--seed` root for both; the two streams stay separate
    /// through their domains.
    #[must_use]
    pub const fn from_root(root_seed: u64) -> Self {
        Self::new(root_seed, CosmeticWeatherSeed::new(root_seed))
    }

    /// The seed mission gameplay derives its streams from.
    #[must_use]
    pub const fn mission_seed(&self) -> u64 {
        self.mission_seed
    }

    /// The seed decoration draws from.
    #[must_use]
    pub const fn weather(&self) -> CosmeticWeatherSeed {
        self.weather
    }

    /// A fresh copy of the mission AI stream, at its first draw.
    ///
    /// It depends on [`RunSeeds::mission_seed`] and on
    /// [`AI_NAVIGATION_DOMAIN`] only: the weather seed is not an input, which
    /// is the whole of AC03.
    #[must_use]
    pub fn mission_ai_stream(&self) -> SplitMix64 {
        SplitMix64::for_domain(self.mission_seed, AI_NAVIGATION_DOMAIN)
    }
}

/// Why the session has no gameplay sight range.
#[derive(Clone, Debug, PartialEq)]
pub enum VisibilityUnavailable {
    /// The current state's gameplay visibility is an explicit unknown.
    Unknown {
        /// The claim recording what is unknown.
        claim_id: ClaimId,
        /// Why it is unknown.
        reason: String,
    },
}

impl fmt::Display for VisibilityUnavailable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unknown { claim_id, reason } => write!(
                f,
                "gameplay visibility is unknown (claim {}): {reason}; it is never \
                 derived from fog",
                claim_id.as_str()
            ),
        }
    }
}

impl std::error::Error for VisibilityUnavailable {}

/// One mission's environment: authored definition, running clock and seeds.
#[derive(Clone, Debug, PartialEq)]
pub struct EnvironmentSession {
    definition: EnvironmentDefinition,
    rate: TickRate,
    seeds: RunSeeds,
    clock: EnvironmentClock,
}

impl EnvironmentSession {
    /// Starts a session at tick zero of the authored state.
    ///
    /// # Errors
    ///
    /// [`TimelineError`] from [`EnvironmentClock::new`].
    pub fn new(
        definition: EnvironmentDefinition,
        rate: TickRate,
        seeds: RunSeeds,
    ) -> Result<Self, TimelineError> {
        let clock = EnvironmentClock::new(&definition, rate)?;
        Ok(Self {
            definition,
            rate,
            seeds,
            clock,
        })
    }

    /// The authored definition.
    #[must_use]
    pub const fn definition(&self) -> &EnvironmentDefinition {
        &self.definition
    }

    /// The run's seeds.
    #[must_use]
    pub const fn seeds(&self) -> &RunSeeds {
        &self.seeds
    }

    /// The clock, for reading the tick and the applied events.
    #[must_use]
    pub const fn clock(&self) -> &EnvironmentClock {
        &self.clock
    }

    /// Pauses or resumes the weather.
    pub fn set_paused(&mut self, paused: bool) {
        self.clock.set_paused(paused);
    }

    /// Advances one frame and returns the ticks it committed.
    ///
    /// # Errors
    ///
    /// [`TimelineError`] from [`EnvironmentClock::advance_frame`]; the
    /// session is unchanged when it fails.
    pub fn advance_frame(&mut self, elapsed: Duration) -> Result<u64, TimelineError> {
        self.clock.advance_frame(elapsed)
    }

    /// Tears the running clock down and rebuilds it from the authored
    /// definition at tick zero, keeping the seeds. The retry path after a
    /// failed mission attempt: the same events fire at the same ticks again.
    ///
    /// # Errors
    ///
    /// [`TimelineError`] from [`EnvironmentClock::new`]; the session is
    /// unchanged when it fails.
    pub fn restart(&mut self) -> Result<(), TimelineError> {
        self.clock = EnvironmentClock::new(&self.definition, self.rate)?;
        Ok(())
    }

    /// The authoritative wind of the state the clock has reached.
    ///
    /// # Errors
    ///
    /// [`WindUnavailable`] when the wind is an unknown or out of range.
    pub fn wind(&self) -> Result<AuthoritativeWind, WindUnavailable> {
        AuthoritativeWind::from_state(self.clock.state())
    }

    /// `base` with the current authoritative wind installed, for the flight
    /// model.
    ///
    /// # Errors
    ///
    /// [`WindUnavailable`]; `base` is never returned unchanged in its place.
    pub fn flight_environment(
        &self,
        base: &FlightEnvironment,
    ) -> Result<FlightEnvironment, WindUnavailable> {
        Ok(self.wind()?.flight_environment(base))
    }

    /// The authored gameplay sight range in meters.
    ///
    /// # Errors
    ///
    /// [`VisibilityUnavailable::Unknown`] when the range is unknown.
    pub fn sight_range_m(&self) -> Result<f64, VisibilityUnavailable> {
        match self.clock.state().gameplay_visibility() {
            Resolved::Known(known) => Ok(known.value.range_m()),
            Resolved::Unknown { claim_id, reason } => Err(VisibilityUnavailable::Unknown {
                claim_id: claim_id.clone(),
                reason: reason.clone(),
            }),
        }
    }

    /// The presentation effects of the current state, decorated from the
    /// run's weather seed.
    #[must_use]
    pub fn effects(&self) -> EnvironmentEffects {
        EnvironmentEffects::from_clock(&self.definition, &self.clock, self.seeds.weather())
    }

    /// A fresh copy of the mission AI stream; see
    /// [`RunSeeds::mission_ai_stream`].
    #[must_use]
    pub fn mission_ai_stream(&self) -> SplitMix64 {
        self.seeds.mission_ai_stream()
    }
}
