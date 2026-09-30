//! Sky, fog and light effects: what a renderer may draw this frame (F19-B).
//!
//! Spec: `specs/F19-sky-atmosphere-weather-and-visibility.md`, stage
//! `### F19-B`, non-negotiable behaviors 1, 3 and 5. Shared contract:
//! `docs/contracts/IDENTITY-CONTENT.md`.
//!
//! F19-A defined *what an environment says*; this module is the **effect**
//! of that record on one frame of presentation. It turns the authored
//! categories into the three things a renderer must be told, and it refuses
//! to invent any of them:
//!
//! * [`SkyEffect`] — the one question a renderer asks before substituting
//!   anything: is there an authored sky texture, is its absence a diagnostic,
//!   or may this explicitly labeled synthetic/developer run generate one?
//!   F19 non-negotiable behavior 5 makes a missing texture a diagnostic, and
//!   the answer is derived from
//!   [`SkyArt::allows_generated_sky`](cs_content::environment::SkyArt::allows_generated_sky)
//!   so a retail run can never reach a generated sky.
//! * [`FogEffect`] — screen-space fog, with the exponential transmittance a
//!   renderer fades by. It carries **no** gameplay visibility and exposes
//!   none: F19 non-negotiable behavior 1 forbids deriving an AI's sight
//!   range from a screen fog density, so the only thing this record can
//!   produce is a fade, and an unknown density produces a refusal rather
//!   than "no fog".
//! * [`LightEffect`] — the sun direction and ambient term a renderer lights
//!   a scene with. An unknown sun stays unknown: [`LightEffect::rig`] returns
//!   `None` instead of a default direction, because a default sun is a
//!   renderer choice masquerading as an authored one.
//! * [`CloudLayerEffect`] — the authored layers with their coverage still
//!   resolvable, so a layer whose coverage nobody measured is not drawn at
//!   full opacity by accident.
//!
//! [`EnvironmentEffects`] gathers those plus the current
//! [`PrecipitationEffect`](crate::environment::cosmetic::PrecipitationEffect)
//! into the single per-frame record a renderer consumes. It reads the
//! gameplay-relevant state from an
//! [`EnvironmentClock`](crate::environment::EnvironmentClock), so the
//! precipitation it reports is the one the timeline installed, never a
//! definition's stale copy.
//!
//! # What this module does not claim
//!
//! Nothing here renders, and nothing here opens a texture: these are the
//! values a Bevy system will read in F19-C, kept free of ECS types so they
//! stay headless-testable. No original sky, fog or light value is
//! reproduced — see
//! `docs/findings/2026-09-30-f19-b-sky-fog-light-and-weather-effects.md`.

use std::fmt;

use cs_content::environment::{
    CosmeticWeatherSeed, EnvironmentDefinition, EnvironmentProfile, EnvironmentState,
    FogDefinition, LightingDefinition, SkyArt,
};
use cs_types::content::{ContentId, Resolved};
use cs_types::evidence::ClaimId;
use cs_types::space::UnitVec3;

use crate::environment::clock::EnvironmentClock;
use crate::environment::cosmetic::PrecipitationEffect;

/// What a renderer must do about the sky this frame (F19 behavior 5).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SkyEffect {
    /// The authored sky texture exists: draw it.
    Texture {
        /// The authored image id.
        texture: ContentId,
    },
    /// The authored sky texture was not found: report the diagnostic and draw
    /// no substitute. This is what a retail run always gets.
    MissingDiagnostic {
        /// The claim id of the missing texture.
        claim_id: ClaimId,
        /// Why the evidence left it missing.
        reason: String,
    },
    /// Generate a sky. Reachable **only** under an explicitly labeled
    /// synthetic/developer profile: the payload is that label, so a
    /// generated sky can never be mistaken for authored art.
    Generated {
        /// The profile this run is labeled with.
        profile: EnvironmentProfile,
    },
}

impl SkyEffect {
    /// Decides the sky effect for a run labeled `run_profile`.
    ///
    /// The decision is [`SkyArt::allows_generated_sky`]'s, asked with the
    /// run's own profile: a known texture is always drawn, a missing one is
    /// a diagnostic unless the *run* is labeled synthetic/developer **and**
    /// the record asked for a generated fallback. Both halves matter — a
    /// synthetic run whose texture was simply never authored still reports a
    /// diagnostic, because the fallback is the record's statement, not the
    /// profile's licence.
    #[must_use]
    pub fn resolve(sky: &SkyArt, run_profile: EnvironmentProfile) -> Self {
        match sky.texture() {
            Resolved::Known(known) => Self::Texture {
                texture: known.value.clone(),
            },
            Resolved::Unknown { claim_id, reason } if sky.allows_generated_sky(run_profile) => {
                Self::Generated {
                    profile: run_profile,
                }
            }
            Resolved::Unknown { claim_id, reason } => Self::MissingDiagnostic {
                claim_id: claim_id.clone(),
                reason: reason.clone(),
            },
        }
    }

    /// Whether this effect generates a sky instead of drawing authored art.
    ///
    /// It is `true` only for a synthetic/developer run, and the label
    /// travels in the payload: a caller that renders cannot lose track of
    /// which of the two it is doing.
    #[must_use]
    pub fn is_generated(&self) -> bool {
        matches!(self, Self::Generated { .. })
    }

    /// The authored texture, or `None` when there is none to draw.
    #[must_use]
    pub fn texture(&self) -> Option<&ContentId> {
        match self {
            Self::Texture { texture } => Some(texture),
            Self::MissingDiagnostic { .. } | Self::Generated { .. } => None,
        }
    }

    /// The claim id and reason of a missing texture, or `None` when the sky
    /// is drawable.
    #[must_use]
    pub fn diagnostic(&self) -> Option<(&str, &str)> {
        match self {
            Self::MissingDiagnostic { claim_id, reason } => {
                Some((claim_id.as_str(), reason.as_str()))
            }
            Self::Texture { .. } | Self::Generated { .. } => None,
        }
    }
}

/// Why a [`FogEffect`] could not produce a fade.
#[derive(Clone, Debug, PartialEq)]
pub enum FogEffectError {
    /// The fog density is an explicit unknown.
    DensityUnknown {
        /// The claim id of the unknown field.
        claim_id: ClaimId,
        /// Why the evidence left it unknown.
        reason: String,
    },
    /// The fog colour is an explicit unknown while the fade needs it.
    ColorUnknown {
        /// The claim id of the unknown field.
        claim_id: ClaimId,
        /// Why the evidence left it unknown.
        reason: String,
    },
    /// A distance was negative or not finite.
    NonFiniteDistance {
        /// The rejected value.
        distance_m: f64,
    },
}

impl fmt::Display for FogEffectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DensityUnknown { claim_id, reason } => {
                write!(
                    f,
                    "the fog density is unknown ({}): {reason}",
                    claim_id.as_str()
                )
            }
            Self::ColorUnknown { claim_id, reason } => {
                write!(
                    f,
                    "the fog colour is unknown ({}): {reason}",
                    claim_id.as_str()
                )
            }
            Self::NonFiniteDistance { distance_m } => {
                write!(f, "the fogged distance {distance_m} must be finite")
            }
        }
    }
}

impl std::error::Error for FogEffectError {}

/// Screen-space fog as a renderer consumes it.
///
/// It is built from one [`FogDefinition`] and keeps every field's resolution
/// state: a known density and an unknown colour is a legal record that can
/// fade but cannot tint, not a failure and not a silent black. **It carries
/// no gameplay visibility** — the fog record has no sight-range accessor and
/// this one does not invent a use for it (F19 non-negotiable behavior 1).
#[derive(Clone, Debug, PartialEq)]
pub struct FogEffect {
    density_per_m: Resolved<f64>,
    color_linear: Resolved<[f64; 3]>,
}

impl FogEffect {
    /// The effect an environment's authored fog produces.
    #[must_use]
    pub fn resolve(fog: &FogDefinition) -> Self {
        Self {
            density_per_m: fog.density_per_m().clone(),
            color_linear: fog.color_linear().clone(),
        }
    }

    /// The fog density in per-meter density, or the explicit unknown.
    #[must_use]
    pub fn density_per_m(&self) -> &Resolved<f64> {
        &self.density_per_m
    }

    /// The fog colour as linear RGB, or the explicit unknown.
    #[must_use]
    pub fn color_linear(&self) -> &Resolved<[f64; 3]> {
        &self.color_linear
    }

    /// The fraction of a surface's own light that survives `distance_m` of
    /// fog under an exponential transmittance: `exp(-density * distance)`.
    ///
    /// At zero distance it is `1.0` — nothing is faded at the camera — and it
    /// falls towards `0.0` as distance grows, which is the whole observable
    /// effect of the density.
    ///
    /// # Errors
    ///
    /// [`FogEffectError::DensityUnknown`] when the density was never
    /// measured: "no evidence of fog" is not "no fog", so the renderer is
    /// told to report instead of drawing an unfogged world. A renderer
    /// default belongs in [`FogDefinition::designed_default`], tagged
    /// `designed`, not here.
    pub fn transmittance_at(&self, distance_m: f64) -> Result<f64, FogEffectError> {
        if !distance_m.is_finite() || distance_m < 0.0 {
            return Err(FogEffectError::NonFiniteDistance { distance_m });
        }
        let density = match &self.density_per_m {
            Resolved::Known(known) => known.value,
            Resolved::Unknown { claim_id, reason } => {
                return Err(FogEffectError::DensityUnknown {
                    claim_id: claim_id.clone(),
                    reason: reason.clone(),
                });
            }
        };
        Ok((-density * distance_m).exp())
    }

    /// The linear RGB a surface of `surface_linear` fades to over
    /// `distance_m`: `surface * T + fog_color * (1 - T)`.
    ///
    /// # Errors
    ///
    /// [`FogEffectError::DensityUnknown`],
    /// [`FogEffectError::ColorUnknown`] or
    /// [`FogEffectError::NonFiniteDistance`], as
    /// [`FogEffect::transmittance_at`] documents.
    pub fn fade_toward(
        &self,
        surface_linear: [f64; 3],
        distance_m: f64,
    ) -> Result<[f64; 3], FogEffectError> {
        let transmittance = self.transmittance_at(distance_m)?;
        let color = match &self.color_linear {
            Resolved::Known(known) => known.value,
            Resolved::Unknown { claim_id, reason } => {
                return Err(FogEffectError::ColorUnknown {
                    claim_id: claim_id.clone(),
                    reason: reason.clone(),
                });
            }
        };
        let mut faded = [0.0; 3];
        for (component, value) in faded.iter_mut().enumerate() {
            *value = surface_linear[component] * transmittance
                + color[component] * (1.0 - transmittance);
        }
        Ok(faded)
    }
}

/// A sun direction and ambient term a renderer can light a scene with.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SunLightRig {
    /// The sun's world direction. A rebase does not rotate it
    /// (F19 non-negotiable behavior 3): it is authored in world space.
    pub direction: UnitVec3,
    /// The ambient term as linear RGB.
    pub ambient_linear: [f64; 3],
}

/// The lighting of one environment as a renderer consumes it.
#[derive(Clone, Debug, PartialEq)]
pub struct LightEffect {
    sun_direction: Resolved<UnitVec3>,
    ambient_linear: Resolved<[f64; 3]>,
}

impl LightEffect {
    /// The effect an environment's authored lighting produces.
    #[must_use]
    pub fn resolve(lighting: &LightingDefinition) -> Self {
        Self {
            sun_direction: lighting.sun_direction().clone(),
            ambient_linear: lighting.ambient_linear().clone(),
        }
    }

    /// The sun's world direction, or the explicit unknown its evidence left.
    #[must_use]
    pub fn sun_direction(&self) -> &Resolved<UnitVec3> {
        &self.sun_direction
    }

    /// The ambient term as linear RGB, or the explicit unknown.
    #[must_use]
    pub fn ambient_linear(&self) -> &Resolved<[f64; 3]> {
        &self.ambient_linear
    }

    /// A light rig, or `None` when either half is unknown.
    ///
    /// There is no default sun and no default ambient: an environment whose
    /// sun nobody measured must be reported as unlit-by-author, not lit by a
    /// convenient renderer choice. A known sun with an unknown ambient is
    /// also `None` — half a rig would be a guess.
    #[must_use]
    pub fn rig(&self) -> Option<SunLightRig> {
        match (&self.sun_direction, &self.ambient_linear) {
            (Resolved::Known(sun), Resolved::Known(ambient)) => Some(SunLightRig {
                direction: sun.value,
                ambient_linear: ambient.value,
            }),
            (Resolved::Unknown { .. }, _) | (_, Resolved::Unknown { .. }) => None,
        }
    }
}

/// One authored cloud layer as a renderer consumes it.
#[derive(Clone, Debug, PartialEq)]
pub struct CloudLayerEffect {
    altitude_m: f64,
    coverage: Resolved<f64>,
}

impl CloudLayerEffect {
    /// The effect one authored layer produces.
    #[must_use]
    pub fn new(altitude_m: f64, coverage: Resolved<f64>) -> Self {
        Self {
            altitude_m,
            coverage,
        }
    }

    /// The layer's altitude above the ground plane, in canonical meters.
    #[must_use]
    pub const fn altitude_m(&self) -> f64 {
        self.altitude_m
    }

    /// The known coverage fraction, or `None` when the evidence left it
    /// unknown. A renderer draws a layer it knows nothing about at no
    /// coverage rather than at full opacity.
    #[must_use]
    pub fn coverage(&self) -> Option<f64> {
        self.coverage.clone().known()
    }
}

/// Every environment effect of one frame, gathered for a renderer.
#[derive(Clone, Debug, PartialEq)]
pub struct EnvironmentEffects {
    profile: EnvironmentProfile,
    sky: SkyEffect,
    fog: FogEffect,
    light: LightEffect,
    cloud_layers: Vec<CloudLayerEffect>,
    precipitation: PrecipitationEffect,
}

impl EnvironmentEffects {
    /// Resolves an environment's presentation effects from its definition,
    /// the gameplay-relevant `state` its timeline has reached and the
    /// cosmetic `seed` decoration draws from.
    ///
    /// `state` is passed separately from `definition` on purpose: sky art,
    /// fog and lighting are authored once, while the precipitation is the
    /// part a [`WeatherEvent`](cs_content::environment::WeatherEvent)
    /// replaces. Reading the definition's initial state here instead would
    /// draw the authored weather forever.
    ///
    /// `seed` is the run's **cosmetic** seed, and it is a required argument:
    /// decorative particles have no default stream, because inventing one
    /// here would put a value into the frame that no authored record asked
    /// for. Its type keeps it apart from a mission's AI seed.
    #[must_use]
    pub fn resolve(
        definition: &EnvironmentDefinition,
        state: &EnvironmentState,
        cosmetic_seed: CosmeticWeatherSeed,
    ) -> Self {
        let profile = definition.profile();
        Self {
            profile,
            sky: SkyEffect::resolve(definition.sky(), profile),
            fog: FogEffect::resolve(definition.fog()),
            light: LightEffect::resolve(definition.lighting()),
            cloud_layers: definition
                .cloud_layers()
                .iter()
                .map(|layer| CloudLayerEffect::new(layer.altitude_m(), layer.coverage().clone()))
                .collect(),
            precipitation: PrecipitationEffect::resolve(state, cosmetic_seed),
        }
    }

    /// Resolves the effects of a running environment clock.
    ///
    /// This is the path a session uses: the precipitation comes from the
    /// state the clock installed at its last committed tick, so a frame
    /// cannot draw weather the timeline has already moved past.
    #[must_use]
    pub fn from_clock(
        definition: &EnvironmentDefinition,
        clock: &EnvironmentClock,
        cosmetic_seed: CosmeticWeatherSeed,
    ) -> Self {
        Self::resolve(definition, clock.state(), cosmetic_seed)
    }

    /// The content rules these effects run under. The profile is the label a
    /// generated sky carries, so it travels with the frame.
    #[must_use]
    pub const fn profile(&self) -> EnvironmentProfile {
        self.profile
    }

    /// What to do about the sky.
    #[must_use]
    pub const fn sky(&self) -> &SkyEffect {
        &self.sky
    }

    /// The screen-space fog effect.
    #[must_use]
    pub const fn fog(&self) -> &FogEffect {
        &self.fog
    }

    /// The lighting effect.
    #[must_use]
    pub const fn light(&self) -> &LightEffect {
        &self.light
    }

    /// The authored cloud layers, in authored order.
    #[must_use]
    pub fn cloud_layers(&self) -> &[CloudLayerEffect] {
        &self.cloud_layers
    }

    /// The precipitation of the current state.
    #[must_use]
    pub const fn precipitation(&self) -> &PrecipitationEffect {
        &self.precipitation
    }
}
