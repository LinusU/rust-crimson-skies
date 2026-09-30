//! Cosmetic weather: the decorative particle field and its own random stream
//! (F19-B).
//!
//! Spec: `specs/F19-sky-atmosphere-weather-and-visibility.md`, stage
//! `### F19-B`, non-negotiable behavior 2. Shared contract:
//! `docs/contracts/IDENTITY-CONTENT.md`.
//!
//! Rain and snow are decoration. This module is what "decoration draws from
//! its own stream" means in code:
//!
//! * The particles come from
//!   [`CosmeticWeatherSeed`](cs_content::environment::CosmeticWeatherSeed),
//!   whose stream lives in `COSMETIC_WEATHER_DOMAIN`. A mission's AI stream
//!   is a different domain of the same generator, so no number drawn here can
//!   be or become an AI draw, and adding a decorative consumer later cannot
//!   move a value gameplay already observed.
//! * The particles are then **advected by the authoritative wind field** and
//!   by nothing else. Wind is physical: a gust really does carry the rain,
//!   so [`CosmeticField::particles`] (the random part) is independent of it
//!   while [`CosmeticParticle::drifted_offset_m`] (the motion) is not. That
//!   split is what makes "the decoration follows the weather, the decoration's
//!   randomness does not depend on it" an observable property instead of a
//!   comment.
//! * Only effects **actually authored** for the environment are produced: a
//!   clear state has no particles at all, and a state whose precipitation
//!   kind is an explicit unknown draws nothing and reports its claim.
//!
//! # What this module does not claim
//!
//! No original precipitation model is reproduced: there is no per-kind fall
//! speed, no particle size, no camera-relative spawn volume and no splash or
//! impact rule, because nothing has measured any of them. [`PrecipitationKind`]
//! is a designed vocabulary and this module deliberately does **not**
//! differentiate `Rain` from `Snow` beyond which kind is authored — inventing
//! two different drift rates would be a tuning table nobody measured. The
//! field's half extent
//! ([`COSMETIC_FIELD_HALF_EXTENT_M`]) is declared presentation design for the
//! same reason, and F19-D may replace it.

use std::fmt;

use cs_content::environment::{CosmeticWeatherSeed, EnvironmentState, PrecipitationKind};
use cs_types::content::Resolved;
use cs_types::evidence::ClaimId;

use crate::environment::air::AuthoritativeWind;

/// The half width of the camera-relative cube a cosmetic particle field
/// occupies, in meters.
///
/// **Designed, not measured.** It is the volume a renderer wraps a particle
/// system around the camera; the original's own volume, if it spawns
/// particles at all, is unmeasured. Nothing about gameplay depends on it:
/// no collision, sight range or trajectory reads a decorative offset.
pub const COSMETIC_FIELD_HALF_EXTENT_M: f64 = 60.0;

/// How many particles a cosmetic field holds.
///
/// **Designed, not measured.** A count, chosen once so the field is a bounded
/// cost; the original's particle count is unknown.
pub const COSMETIC_PARTICLE_COUNT: usize = 16;

/// Why a [`CosmeticField`] could not be built from a seed.
#[derive(Clone, Debug, PartialEq)]
pub enum CosmeticFieldError {
    /// A drawn offset was not finite, which would put a particle at an
    /// unrepresentable place.
    NonFiniteDraw {
        /// The index of the offending particle.
        index: usize,
    },
}

impl fmt::Display for CosmeticFieldError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NonFiniteDraw { index } => {
                write!(f, "cosmetic particle {index} drew a non-finite offset")
            }
        }
    }
}

impl std::error::Error for CosmeticFieldError {}

/// One decorative particle: a camera-relative offset and nothing else.
///
/// It is deliberately not a physical body. It has no mass, no velocity of its
/// own and no collision, so it can never enter a gameplay query — a rain
/// particle that could be shot down would be a gameplay object wearing a
/// costume.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CosmeticParticle {
    offset_m: [f64; 3],
}

impl CosmeticParticle {
    /// The particle's camera-relative offset in meters, as drawn.
    #[must_use]
    pub const fn offset_m(&self) -> [f64; 3] {
        self.offset_m
    }

    /// Where the particle sits after `elapsed_s` of `wind`.
    ///
    /// The wind is the *authoritative* field, not a decorative one: rain
    /// drifts with the same air the aircraft flies in, which is why a test
    /// can compare a gusted and a calm frame and see exactly the wind's own
    /// displacement.
    ///
    /// `elapsed_s` is only used for the advection: the module models no
    /// fall, so a negative value is refused rather than run backwards.
    ///
    /// # Errors
    ///
    /// [`CosmeticFieldError::NonFiniteDraw`] when `elapsed_s` is negative or
    /// not finite.
    pub fn drifted_offset_m(
        &self,
        wind: &AuthoritativeWind,
        elapsed_s: f64,
    ) -> Result<[f64; 3], CosmeticFieldError> {
        if !elapsed_s.is_finite() || elapsed_s < 0.0 {
            return Err(CosmeticFieldError::NonFiniteDraw { index: 0 });
        }
        Ok([
            self.offset_m[0] + wind.velocity_m_s()[0] * elapsed_s,
            self.offset_m[1] + wind.velocity_m_s()[1] * elapsed_s,
            self.offset_m[2] + wind.velocity_m_s()[2] * elapsed_s,
        ])
    }
}

/// A deterministic decorative particle field.
///
/// Equality is equality of the drawn offsets, so two fields built from the
/// same seed compare equal even when they are advected by different winds.
#[derive(Clone, Debug, PartialEq)]
pub struct CosmeticField {
    kind: PrecipitationKind,
    particles: Vec<CosmeticParticle>,
}

impl CosmeticField {
    /// Draws the field from `seed`'s cosmetic stream.
    ///
    /// # Errors
    ///
    /// [`CosmeticFieldError::NonFiniteDraw`] if a draw produced a
    /// non-finite offset, which cannot happen for a
    /// [`CosmeticWeatherSeed`] stream (every draw is an exact integer scaled
    /// into `[0, 1)`) and exists so a future draw cannot smuggle an
    /// unrepresentable particle into a frame.
    pub fn new(
        kind: PrecipitationKind,
        seed: CosmeticWeatherSeed,
    ) -> Result<Self, CosmeticFieldError> {
        let mut stream = seed.stream();
        let mut particles = Vec::with_capacity(COSMETIC_PARTICLE_COUNT);
        for index in 0..COSMETIC_PARTICLE_COUNT {
            let offset_m = [
                signed_unit(&mut stream) * COSMETIC_FIELD_HALF_EXTENT_M,
                signed_unit(&mut stream) * COSMETIC_FIELD_HALF_EXTENT_M,
                signed_unit(&mut stream) * COSMETIC_FIELD_HALF_EXTENT_M,
            ];
            if offset_m.iter().any(|value| !value.is_finite()) {
                return Err(CosmeticFieldError::NonFiniteDraw { index });
            }
            particles.push(CosmeticParticle { offset_m });
        }
        Ok(Self { kind, particles })
    }

    /// Which authored precipitation this field decorates.
    #[must_use]
    pub const fn kind(&self) -> PrecipitationKind {
        self.kind
    }

    /// The particles as drawn, before any wind advection.
    #[must_use]
    pub fn particles(&self) -> &[CosmeticParticle] {
        &self.particles
    }

    /// The field advected by `wind` over `elapsed_s`.
    ///
    /// # Errors
    ///
    /// [`CosmeticFieldError::NonFiniteDraw`] when `elapsed_s` is negative or
    /// not finite.
    pub fn drifted(
        &self,
        wind: &AuthoritativeWind,
        elapsed_s: f64,
    ) -> Result<Vec<CosmeticParticle>, CosmeticFieldError> {
        self.particles
            .iter()
            .map(|particle| {
                particle
                    .drifted_offset_m(wind, elapsed_s)
                    .map(|offset_m| CosmeticParticle { offset_m })
            })
            .collect()
    }
}

/// The precipitation of the current environment state, as a renderer consumes
/// it.
///
/// Three states, and only three: particles were authored, no precipitation
/// was authored, or the kind is an explicit unknown. "Unknown" is not "clear"
/// — an unknown kind draws nothing and says why, so a renderer never turns
/// missing evidence into a clear sky.
#[derive(Clone, Debug, PartialEq)]
pub enum PrecipitationEffect {
    /// No precipitation is authored: no particle field at all.
    Clear,
    /// Precipitation is authored: the cosmetic field that decorates it.
    Field(CosmeticField),
    /// The authored kind is an explicit unknown.
    Unknown {
        /// The claim id of the unknown kind.
        claim_id: ClaimId,
        /// Why the evidence left it unknown.
        reason: String,
    },
}

impl PrecipitationEffect {
    /// Resolves the precipitation of `state` against the cosmetic `seed`.
    ///
    /// The seed is its own parameter, typed
    /// [`CosmeticWeatherSeed`], so a caller cannot hand a mission's AI seed
    /// to decoration and cannot quietly inherit one by omission.
    #[must_use]
    pub fn resolve(state: &EnvironmentState, seed: CosmeticWeatherSeed) -> Self {
        match state.precipitation().kind() {
            Resolved::Known(known) => match known.value {
                PrecipitationKind::Clear => Self::Clear,
                kind => Self::Field(
                    CosmeticField::new(kind, seed)
                        .expect("a cosmetic stream draw is always a finite offset"),
                ),
            },
            Resolved::Unknown { claim_id, reason } => Self::Unknown {
                claim_id: claim_id.clone(),
                reason: reason.clone(),
            },
        }
    }

    /// The particle field, or `None` for a clear or unknown precipitation.
    #[must_use]
    pub fn field(&self) -> Option<&CosmeticField> {
        match self {
            Self::Field(field) => Some(field),
            Self::Clear | Self::Unknown { .. } => None,
        }
    }

    /// Whether this state draws a particle field at all.
    #[must_use]
    pub const fn is_decorated(&self) -> bool {
        matches!(self, Self::Field(_))
    }

    /// The claim id and reason of an unknown kind, or `None` when the kind is
    /// known.
    #[must_use]
    pub fn unknown(&self) -> Option<(&str, &str)> {
        match self {
            Self::Unknown { claim_id, reason } => Some((claim_id.as_str(), reason.as_str())),
            Self::Clear | Self::Field(_) => None,
        }
    }
}

/// One draw mapped to `[-1, 1)`, the centre of the cosmetic field's cube.
fn signed_unit(stream: &mut cs_types::random::SplitMix64) -> f64 {
    stream.unit_f64() * 2.0 - 1.0
}
