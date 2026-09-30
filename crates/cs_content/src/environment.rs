//! Environment data and time domains (F19-A).
//!
//! Spec: `specs/F19-sky-atmosphere-weather-and-visibility.md`, stage
//! `### F19-A`. Shared contract: `docs/contracts/IDENTITY-CONTENT.md`.
//!
//! This module is the **typed input/output contract** of the environment
//! feature — nothing here renders, opens a file or touches Bevy (`cs_content`
//! must never depend on Bevy or Avian). Stage F19-A declares what an
//! environment importer produces and what the runtime consumes; F19-B
//! implements the sky/fog/light and the discovered weather effects against
//! these records, and F19-C wires the authoritative wind and visibility
//! policies into their real producer and consumer.
//!
//! # The records
//!
//! * [`EnvironmentId`] wraps a validated subordinate key — the identity of
//!   one authored environment, addressed inside a world or scenario. It is
//!   deliberately **not** a catalog [`ContentId`]: the workspace's
//!   [`ContentKind`] vocabulary has no environment kind, and inventing one
//!   would claim a namespace `IDENTITY-CONTENT` does not reserve.
//! * [`EnvironmentDefinition`] is one authored environment. It keeps the
//!   categories the deliverable separates — sky art ([`SkyArt`]), sky
//!   orientation ([`SkyOrientation`]), fog ([`FogDefinition`]), lighting
//!   ([`LightingDefinition`]), cloud layers ([`CloudLayer`]), precipitation,
//!   wind and gameplay visibility — as *separate fields*, so a renderer
//!   default in one category can never silently become a value in another.
//! * [`EnvironmentState`] is the gameplay-relevant mutable part: the
//!   authoritative [`WindField`], the [`PrecipitationDefinition`] and the
//!   [`GameplayVisibility`]. The definition exposes each of them at the top
//!   level through this one record ([`EnvironmentDefinition::wind`]),
//!   because a timeline event replaces a whole state: there is exactly one
//!   wind field, never a "definition wind" and a "timeline wind".
//! * [`EnvironmentTimeline`] is the authored sequence of [`WeatherEvent`]s,
//!   each at a whole simulation tick. Integer ticks are the canonical time
//!   of the `IDENTITY-CONTENT` numeric contract; which *domain* those ticks
//!   belong to (and what pause does to them) is decided in `cs_sim`'s
//!   `visibility` module, because `cs_content` must not depend on `cs_sim`.
//! * [`EnvironmentDefinition::record_fingerprint`] is the canonical digest
//!   of what a definition says, in field order, for pinning later.
//!
//! # Known is known, unknown is unknown
//!
//! Every tunable arrives as [`Resolved`]: a record whose evidence never
//! named a value carries [`Resolved::Unknown`] with its claim id and a
//! reason, never a guessed number. [`FogDefinition::designed_default`] is
//! the one explicit way to state a renderer default, and it tags the value
//! `designed` — a default is a design decision, not a measurement.
//!
//! In particular **gameplay visibility is never derived from fog**
//! ([`EnvironmentDefinition::gameplay_visibility`]). Atmospheric visibility
//! that affects AI is its own authored value; a screen-space fog density is
//! a rendering choice, and the two are not interchangeable (F19
//! non-negotiable behavior 1).
//!
//! # Designed vocabulary, not original data
//!
//! The id grammar, [`EnvironmentProfile`], [`SkyArt`], [`SkyOrientation`],
//! [`FogDefinition`], [`LightingDefinition`], [`CloudLayer`],
//! [`PrecipitationKind`], [`WindField`], [`GameplayVisibility`],
//! [`EnvironmentState`] and [`EnvironmentTimeline`] are **newly authored
//! engine contract**. Which environment records the 2000 PC original
//! stores, how it addresses them, what its sun/fog/wind tuning is, which
//! precipitation kinds it can express and what its units are are **unknown**
//! until an evidence stage measures them; nothing in this module claims to
//! reproduce the original. The designed-vs-measured split and the unknowns
//! this stage met are recorded in
//! `docs/findings/2026-09-30-f19-a-environment-data-and-time-domains.md`.

use std::fmt;

use cs_assets::install::sha256;
use cs_types::content::{
    ContentId, ContentKind, Known, Origin, Provenance, Resolved, ResolvedError,
};
use cs_types::evidence::{ClaimId, ClaimIdError, ContentHash};
use cs_types::space::UnitVec3;

/// The longest environment key this module accepts, in bytes.
pub const MAX_ENVIRONMENT_KEY_LEN: usize = 128;

/// How far a sky heading may depart from perpendicular to the horizon and
/// still be accepted, in units of vector length.
///
/// It matches `cs_types::space::UNIT_LENGTH_TOLERANCE`, so a heading that
/// passes [`UnitVec3`]'s unit check is judged against the same numeric
/// scale.
pub const SKY_HEADING_PERPENDICULAR_TOLERANCE: f64 = 1e-6;

/// The root-seed domain of the **cosmetic** weather stream (decorative
/// particles, non-gameplay ambience).
///
/// F19 non-negotiable behavior 2: decorative particles draw from their own
/// domain-separated stream, never from the authoritative wind field and
/// never from a mission's AI stream. A new consumer claims its own domain
/// constant instead of reusing this one — `cs_types::random`'s
/// `SplitMix64::for_domain` guarantees that two different domains never
/// shift each other's values.
pub const COSMETIC_WEATHER_DOMAIN: u64 = 0x434F_534D_4557_4541; // "COSMWEA"

// ---------------------------------------------------------------- identity ---

/// Why an environment key was rejected.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EnvironmentKeyError {
    /// The key was empty or only separators.
    Empty,
    /// The key exceeded [`MAX_ENVIRONMENT_KEY_LEN`] bytes.
    TooLong {
        /// Its length in bytes.
        len: usize,
    },
    /// The key contained a character outside `[a-z0-9._-]` after ASCII
    /// lowercasing.
    BadCharacter {
        /// The offending character.
        ch: char,
    },
    /// The key had no ASCII alphanumeric character.
    NoAlphanumeric,
}

impl fmt::Display for EnvironmentKeyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => write!(f, "environment key must not be empty"),
            Self::TooLong { len } => {
                write!(
                    f,
                    "environment key is {len} bytes, max is {MAX_ENVIRONMENT_KEY_LEN}"
                )
            }
            Self::BadCharacter { ch } => {
                write!(f, "environment key contains disallowed character {ch:?}")
            }
            Self::NoAlphanumeric => write!(
                f,
                "environment key must contain at least one ASCII alphanumeric character"
            ),
        }
    }
}

impl std::error::Error for EnvironmentKeyError {}

/// Validates an environment key against the shared `[a-z0-9._-]` grammar
/// (ASCII lowercased), so ids are comparable, reportable and stable across
/// parses.
fn validate_environment_key(key: &str) -> Result<String, EnvironmentKeyError> {
    let normalized = key.to_ascii_lowercase();
    if normalized.is_empty() {
        return Err(EnvironmentKeyError::Empty);
    }
    if normalized.len() > MAX_ENVIRONMENT_KEY_LEN {
        return Err(EnvironmentKeyError::TooLong {
            len: normalized.len(),
        });
    }
    if !normalized.chars().any(|ch| ch.is_ascii_alphanumeric()) {
        return Err(EnvironmentKeyError::NoAlphanumeric);
    }
    for ch in normalized.chars() {
        if !ch.is_ascii_alphanumeric() && !matches!(ch, '.' | '_' | '-') {
            return Err(EnvironmentKeyError::BadCharacter { ch });
        }
    }
    Ok(normalized)
}

/// The stable identity of one authored environment: the environment a world
/// variant or a scenario loads.
///
/// It wraps a validated subordinate key rather than a [`ContentId`]:
/// `IDENTITY-CONTENT`'s catalog vocabulary reserves no environment
/// namespace, and claiming one that does not exist would break the contract
/// this stage is written against. An environment is therefore addressed
/// inside its world or scenario, the way `crate::world` addresses a sector.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EnvironmentId(String);

impl EnvironmentId {
    /// Validates and wraps an environment key.
    ///
    /// # Errors
    ///
    /// [`EnvironmentKeyError`] when the key breaks the `[a-z0-9._-]` grammar
    /// or the length limit.
    pub fn new(key: &str) -> Result<Self, EnvironmentKeyError> {
        Ok(Self(validate_environment_key(key)?))
    }

    /// The normalized key.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for EnvironmentId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

// ----------------------------------------------------------------- profile ---

/// Which content rules an environment runs under.
///
/// F19 non-negotiable behavior 5: a *generated* sky may appear only where it
/// is explicitly labeled synthetic/developer. The profile is that label, and
/// it travels with the definition instead of living in renderer state, so a
/// refusal can name the record that asked for it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EnvironmentProfile {
    /// Fidelity rules: only authored content may appear, and a missing sky
    /// texture is a diagnostic instead of a substitute.
    Retail,
    /// Explicitly labeled synthetic/developer content. Generated content is
    /// allowed here and nowhere else.
    SyntheticDeveloper,
}

impl EnvironmentProfile {
    /// The stable label used in diagnostics and fingerprints.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Retail => "retail",
            Self::SyntheticDeveloper => "synthetic-developer",
        }
    }
}

// ------------------------------------------------------------------ sky art ---

/// What the renderer does when the authored sky texture is missing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SkyFallback {
    /// Report a diagnostic and draw no substituted sky. This is the answer a
    /// [`EnvironmentProfile::Retail`] environment gives.
    Diagnostic,
    /// Generate a sky. Allowed **only** under
    /// [`EnvironmentProfile::SyntheticDeveloper`]; the variant carries the
    /// profile it was authored for so a mixed record cannot smuggle a
    /// generated sky into a retail run.
    Generated {
        /// The profile the generated sky was authored for.
        profile: EnvironmentProfile,
    },
}

impl SkyFallback {
    /// Whether this fallback generates a sky instead of reporting one.
    #[must_use]
    pub fn is_generated(&self) -> bool {
        matches!(self, Self::Generated { .. })
    }
}

/// Why a [`SkyArt`] record was rejected.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SkyArtError {
    /// The known texture id is not an image id.
    WrongTextureKind {
        /// The namespace the id actually carried.
        found: ContentKind,
    },
    /// A generated-sky fallback was declared for a profile that may not
    /// generate one.
    NonSyntheticGeneratedSky {
        /// The profile the fallback declared.
        found: EnvironmentProfile,
    },
    /// The record carried a *known* texture and still declared a generated
    /// fallback, which contradicts itself: the fallback only exists because
    /// the texture is missing.
    GeneratedFallbackWithKnownTexture,
}

impl fmt::Display for SkyArtError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WrongTextureKind { found } => write!(
                f,
                "a sky texture id must be an image id, found `{}`",
                found.label()
            ),
            Self::NonSyntheticGeneratedSky { found } => write!(
                f,
                "a generated sky is only allowed under the synthetic/developer profile, found `{}`",
                found.label()
            ),
            Self::GeneratedFallbackWithKnownTexture => {
                write!(f, "a generated-sky fallback requires a missing sky texture")
            }
        }
    }
}

impl std::error::Error for SkyArtError {}

/// Why [`SkyArt::missing`] failed.
#[derive(Debug)]
pub enum MissingSkyError {
    /// The claim id was malformed.
    Claim(ClaimIdError),
    /// The reason an explicit unknown carries was empty.
    Reason(ResolvedError),
}

impl fmt::Display for MissingSkyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Claim(error) => write!(f, "invalid claim id: {error}"),
            Self::Reason(error) => write!(f, "invalid unknown record: {error}"),
        }
    }
}

impl std::error::Error for MissingSkyError {}

impl From<ClaimIdError> for MissingSkyError {
    fn from(error: ClaimIdError) -> Self {
        Self::Claim(error)
    }
}

impl From<ResolvedError> for MissingSkyError {
    fn from(error: ResolvedError) -> Self {
        Self::Reason(error)
    }
}

/// The sky art of one environment: the authored texture, and what happens
/// when it is missing (F19 non-negotiable behavior 5).
///
/// A missing texture is a *diagnostic* by construction — [`SkyArt::missing`]
/// builds [`Resolved::Unknown`] with a reason and a
/// [`SkyFallback::Diagnostic`] — and a generated sky is a record that names
/// the synthetic/developer profile it belongs to, never a silent default.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SkyArt {
    texture: Resolved<ContentId>,
    fallback: SkyFallback,
}

impl SkyArt {
    /// Builds sky art from its parts.
    ///
    /// # Errors
    ///
    /// [`SkyArtError::WrongTextureKind`] when a known texture id is not an
    /// [`ContentKind::Image`], [`SkyArtError::NonSyntheticGeneratedSky`]
    /// when a generated fallback names a profile that may not generate, and
    /// [`SkyArtError::GeneratedFallbackWithKnownTexture`] when a known
    /// texture arrives with a generated fallback.
    pub fn try_new(
        texture: Resolved<ContentId>,
        fallback: SkyFallback,
    ) -> Result<Self, SkyArtError> {
        if let Resolved::Known(known) = &texture {
            if known.value.kind() != ContentKind::Image {
                return Err(SkyArtError::WrongTextureKind {
                    found: known.value.kind(),
                });
            }
            if fallback.is_generated() {
                return Err(SkyArtError::GeneratedFallbackWithKnownTexture);
            }
        }
        if let SkyFallback::Generated { profile } = fallback
            && profile != EnvironmentProfile::SyntheticDeveloper
        {
            return Err(SkyArtError::NonSyntheticGeneratedSky { found: profile });
        }
        Ok(Self { texture, fallback })
    }

    /// Sky art whose texture could not be found: an explicit unknown plus
    /// the diagnostic fallback, never a substituted image.
    ///
    /// # Errors
    ///
    /// [`MissingSkyError::Claim`] for a malformed claim id or
    /// [`MissingSkyError::Reason`] for an empty reason.
    pub fn missing(claim_id: &str, reason: &str) -> Result<Self, MissingSkyError> {
        Ok(Self {
            texture: Resolved::unknown(ClaimId::new(claim_id)?, reason)?,
            fallback: SkyFallback::Diagnostic,
        })
    }

    /// Sky art for a run that is explicitly labeled synthetic/developer: no
    /// texture, and a generated fallback stamped with that profile.
    #[must_use]
    pub fn generated(profile: EnvironmentProfile) -> Self {
        Self {
            texture: Resolved::Unknown {
                claim_id: ClaimId::new("f19a.sky.generated")
                    .expect("the constant claim id is valid"),
                reason: "no authored sky texture; the run is labeled synthetic/developer"
                    .to_owned(),
            },
            fallback: SkyFallback::Generated { profile },
        }
    }

    /// The authored texture, or the explicit unknown its evidence left.
    #[must_use]
    pub fn texture(&self) -> &Resolved<ContentId> {
        &self.texture
    }

    /// What a renderer must do when the texture is missing.
    #[must_use]
    pub fn fallback(&self) -> &SkyFallback {
        &self.fallback
    }

    /// Whether a generated sky may appear in a run of `profile`.
    ///
    /// This is the single question a renderer asks before substituting
    /// anything: it answers `false` for every retail-profile run, whatever
    /// the record's own fallback says.
    #[must_use]
    pub fn allows_generated_sky(&self, profile: EnvironmentProfile) -> bool {
        matches!(
            (&self.fallback, profile),
            (
                SkyFallback::Generated { .. },
                EnvironmentProfile::SyntheticDeveloper
            )
        )
    }
}

// --------------------------------------------------------- sky orientation ---

/// Why a [`SkyOrientation`] was rejected.
#[derive(Clone, Debug, PartialEq)]
pub enum SkyOrientationError {
    /// The heading was not perpendicular to the horizon normal.
    HeadingNotPerpendicular {
        /// The measured absolute dot product of the two vectors.
        dot: f64,
        /// The tolerance it exceeded.
        tolerance: f64,
    },
}

impl fmt::Display for SkyOrientationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::HeadingNotPerpendicular { dot, tolerance } => write!(
                f,
                "the sky heading must be perpendicular to the horizon normal (|dot| = {dot}, tolerance {tolerance})"
            ),
        }
    }
}

impl std::error::Error for SkyOrientationError {}

/// The world orientation a sky is drawn in: which way is up, and where the
/// dome's texture heading points on the horizon.
///
/// F19 non-negotiable behavior 3 has two halves, and this record is the one
/// the *environment* owns: the sky honors world orientation, so orientation
/// is authored data carried through the frame instead of an identity matrix
/// a renderer assumed. The other half — staying centred on camera
/// translation while the world rebases — is `cs_app::environment`'s
/// `SkyFrame`.
///
/// The horizon plane is perpendicular to [`SkyOrientation::up`]; a fixed
/// horizon is a normal that does not move while the world is rebased.
#[derive(Clone, Debug, PartialEq)]
pub struct SkyOrientation {
    up: UnitVec3,
    heading: UnitVec3,
}

impl SkyOrientation {
    /// Validates an orientation: the heading must be perpendicular to the
    /// horizon normal, otherwise the basis it implies is degenerate.
    ///
    /// # Errors
    ///
    /// [`SkyOrientationError::HeadingNotPerpendicular`].
    pub fn try_new(up: UnitVec3, heading: UnitVec3) -> Result<Self, SkyOrientationError> {
        let dot = (up.x() * heading.x() + up.y() * heading.y() + up.z() * heading.z()).abs();
        if dot > SKY_HEADING_PERPENDICULAR_TOLERANCE {
            return Err(SkyOrientationError::HeadingNotPerpendicular {
                dot,
                tolerance: SKY_HEADING_PERPENDICULAR_TOLERANCE,
            });
        }
        Ok(Self { up, heading })
    }

    /// The horizon normal: the direction the sky dome's pole points.
    #[must_use]
    pub fn up(&self) -> UnitVec3 {
        self.up
    }

    /// The heading the dome texture is oriented to, on the horizon plane.
    #[must_use]
    pub fn heading(&self) -> UnitVec3 {
        self.heading
    }

    /// The horizon normal, named the way F19's minimum scenario speaks of
    /// it: a fixed horizon is a normal that does not move.
    #[must_use]
    pub fn horizon_normal(&self) -> UnitVec3 {
        self.up
    }
}

// ------------------------------------------------------------------ lighting ---

/// Why a lighting record was rejected.
#[derive(Clone, Debug, PartialEq)]
pub enum LightingError {
    /// A named field was NaN or infinite.
    NonFinite {
        /// The offending field, as the caller spelled it.
        field: &'static str,
    },
    /// An ambient component was negative.
    NegativeAmbient {
        /// The offending component index.
        component: usize,
    },
}

impl fmt::Display for LightingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NonFinite { field } => write!(f, "{field} must be finite"),
            Self::NegativeAmbient { component } => {
                write!(f, "ambient_linear[{component}] must not be negative")
            }
        }
    }
}

impl std::error::Error for LightingError {}

/// The lighting of one environment: the sun's world direction and the
/// ambient term, each independently resolvable.
///
/// They are separate [`Resolved`] values on purpose: evidence may measure a
/// sun direction and still say nothing about ambient light, and a lighting
/// rig that shared one resolution state for both would turn half a
/// measurement into a whole one.
#[derive(Clone, Debug, PartialEq)]
pub struct LightingDefinition {
    sun_direction: Resolved<UnitVec3>,
    ambient_linear: Resolved<[f64; 3]>,
}

impl LightingDefinition {
    /// Validates a lighting record: known ambient components must be finite
    /// and non-negative; an unknown value carries no number to check.
    ///
    /// # Errors
    ///
    /// [`LightingError::NonFinite`] or [`LightingError::NegativeAmbient`].
    pub fn try_new(
        sun_direction: Resolved<UnitVec3>,
        ambient_linear: Resolved<[f64; 3]>,
    ) -> Result<Self, LightingError> {
        if let Resolved::Known(known) = &ambient_linear {
            for (component, value) in known.value.iter().enumerate() {
                if !value.is_finite() {
                    return Err(LightingError::NonFinite {
                        field: "ambient_linear",
                    });
                }
                if *value < 0.0 {
                    return Err(LightingError::NegativeAmbient { component });
                }
            }
        }
        Ok(Self {
            sun_direction,
            ambient_linear,
        })
    }

    /// The sun's world direction, or the explicit unknown its evidence
    /// left. It is a *world* direction: a rebase does not move it.
    #[must_use]
    pub fn sun_direction(&self) -> &Resolved<UnitVec3> {
        &self.sun_direction
    }

    /// The ambient term as linear RGB, or the explicit unknown.
    #[must_use]
    pub fn ambient_linear(&self) -> &Resolved<[f64; 3]> {
        &self.ambient_linear
    }
}

// ----------------------------------------------------------------------- fog ---

/// Why a fog record was rejected.
#[derive(Clone, Debug, PartialEq)]
pub enum FogError {
    /// A named field was NaN or infinite.
    NonFinite {
        /// The offending field, as the caller spelled it.
        field: &'static str,
    },
    /// The density was negative.
    NegativeDensity,
    /// A color component was negative.
    NegativeColor {
        /// The offending component index.
        component: usize,
    },
}

impl fmt::Display for FogError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NonFinite { field } => write!(f, "{field} must be finite"),
            Self::NegativeDensity => write!(f, "density_per_m must not be negative"),
            Self::NegativeColor { component } => {
                write!(f, "color_linear[{component}] must not be negative")
            }
        }
    }
}

impl std::error::Error for FogError {}

/// Why an explicitly unknown fog record failed to build.
#[derive(Debug)]
pub enum FogUnknownError {
    /// The claim id was malformed.
    Claim(ClaimIdError),
    /// The reason was empty.
    EmptyReason,
}

impl fmt::Display for FogUnknownError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Claim(error) => write!(f, "invalid claim id: {error}"),
            Self::EmptyReason => write!(f, "an unknown fog record needs a reason"),
        }
    }
}

impl std::error::Error for FogUnknownError {}

impl From<ClaimIdError> for FogUnknownError {
    fn from(error: ClaimIdError) -> Self {
        Self::Claim(error)
    }
}

/// Why a designed fog default failed to build.
#[derive(Debug)]
pub enum DesignedDefaultError {
    /// The claim id was malformed.
    Claim(ClaimIdError),
    /// The default value failed fog validation.
    Fog(FogError),
}

impl fmt::Display for DesignedDefaultError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Claim(error) => write!(f, "invalid claim id: {error}"),
            Self::Fog(error) => write!(f, "invalid designed fog default: {error}"),
        }
    }
}

impl std::error::Error for DesignedDefaultError {}

impl From<ClaimIdError> for DesignedDefaultError {
    fn from(error: ClaimIdError) -> Self {
        Self::Claim(error)
    }
}

impl From<FogError> for DesignedDefaultError {
    fn from(error: FogError) -> Self {
        Self::Fog(error)
    }
}

/// Screen-space fog: how the renderer fades distance, in per-meter density
/// and linear color.
///
/// Fog is **not** gameplay visibility (F19 non-negotiable behavior 1): a
/// fog value never becomes an AI's sight range, and no constructor in this
/// module converts one into the other. A renderer default is expressed with
/// [`FogDefinition::designed_default`], which tags the value `designed`.
#[derive(Clone, Debug, PartialEq)]
pub struct FogDefinition {
    density_per_m: Resolved<f64>,
    color_linear: Resolved<[f64; 3]>,
}

impl FogDefinition {
    /// Validates a fog record: a known density must be finite and
    /// non-negative, a known color finite and non-negative.
    ///
    /// # Errors
    ///
    /// [`FogError::NonFinite`], [`FogError::NegativeDensity`] or
    /// [`FogError::NegativeColor`].
    pub fn try_new(
        density_per_m: Resolved<f64>,
        color_linear: Resolved<[f64; 3]>,
    ) -> Result<Self, FogError> {
        if let Resolved::Known(known) = &density_per_m {
            if !known.value.is_finite() {
                return Err(FogError::NonFinite {
                    field: "density_per_m",
                });
            }
            if known.value < 0.0 {
                return Err(FogError::NegativeDensity);
            }
        }
        if let Resolved::Known(known) = &color_linear {
            for (component, value) in known.value.iter().enumerate() {
                if !value.is_finite() {
                    return Err(FogError::NonFinite {
                        field: "color_linear",
                    });
                }
                if *value < 0.0 {
                    return Err(FogError::NegativeColor { component });
                }
            }
        }
        Ok(Self {
            density_per_m,
            color_linear,
        })
    }

    /// A renderer default: a known density and color tagged `designed`, so
    /// a default can never be mistaken for a measurement of the original.
    ///
    /// # Errors
    ///
    /// [`DesignedDefaultError::Claim`] for a malformed claim id, or
    /// [`DesignedDefaultError::Fog`] when the default value itself is not a
    /// representable fog record.
    pub fn designed_default(
        density_per_m: f64,
        color_linear: [f64; 3],
        claim_id: &str,
    ) -> Result<Self, DesignedDefaultError> {
        let provenance = Provenance::designed(ClaimId::new(claim_id)?);
        let density = Resolved::Known(Known::new(density_per_m, provenance.clone()));
        let color = Resolved::Known(Known::new(color_linear, provenance));
        Ok(Self::try_new(density, color)?)
    }

    /// Fog with no measured value at all: an explicit unknown, never zero
    /// density standing in for "no evidence".
    ///
    /// # Errors
    ///
    /// [`FogUnknownError::Claim`] for a malformed claim id or
    /// [`FogUnknownError::EmptyReason`] for an empty reason.
    pub fn unknown(claim_id: &str, reason: &str) -> Result<Self, FogUnknownError> {
        if reason.trim().is_empty() {
            return Err(FogUnknownError::EmptyReason);
        }
        let claim = ClaimId::new(claim_id)?;
        Ok(Self {
            density_per_m: Resolved::Unknown {
                claim_id: claim.clone(),
                reason: reason.to_owned(),
            },
            color_linear: Resolved::Unknown {
                claim_id: claim,
                reason: reason.to_owned(),
            },
        })
    }

    /// The fog density, or the explicit unknown its evidence left.
    #[must_use]
    pub fn density_per_m(&self) -> &Resolved<f64> {
        &self.density_per_m
    }

    /// The fog color, or the explicit unknown its evidence left.
    #[must_use]
    pub fn color_linear(&self) -> &Resolved<[f64; 3]> {
        &self.color_linear
    }
}

// ------------------------------------------------------------- cloud layers ---

/// Why a cloud layer was rejected.
#[derive(Clone, Debug, PartialEq)]
pub enum CloudLayerError {
    /// The altitude was NaN or infinite.
    NonFiniteAltitude,
    /// The altitude was negative: this stage records no sub-ground layer.
    NegativeAltitude,
    /// A known coverage was outside `[0, 1]`.
    CoverageOutOfRange {
        /// The rejected coverage.
        coverage: f64,
    },
}

impl fmt::Display for CloudLayerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NonFiniteAltitude => write!(f, "altitude_m must be finite"),
            Self::NegativeAltitude => write!(f, "altitude_m must not be negative"),
            Self::CoverageOutOfRange { coverage } => {
                write!(f, "coverage must be in [0, 1], found {coverage}")
            }
        }
    }
}

impl std::error::Error for CloudLayerError {}

/// One authored cloud layer: an altitude above the ground plane and a
/// coverage fraction.
///
/// A layer is decorative geometry until F19-B says otherwise; nothing in
/// this stage lets a cloud layer change gameplay visibility.
#[derive(Clone, Debug, PartialEq)]
pub struct CloudLayer {
    altitude_m: f64,
    coverage: Resolved<f64>,
}

impl CloudLayer {
    /// Validates a cloud layer.
    ///
    /// # Errors
    ///
    /// [`CloudLayerError::NonFiniteAltitude`],
    /// [`CloudLayerError::NegativeAltitude`] or
    /// [`CloudLayerError::CoverageOutOfRange`].
    pub fn try_new(altitude_m: f64, coverage: Resolved<f64>) -> Result<Self, CloudLayerError> {
        if !altitude_m.is_finite() {
            return Err(CloudLayerError::NonFiniteAltitude);
        }
        if altitude_m < 0.0 {
            return Err(CloudLayerError::NegativeAltitude);
        }
        if let Resolved::Known(known) = &coverage
            && !(0.0..=1.0).contains(&known.value)
        {
            return Err(CloudLayerError::CoverageOutOfRange {
                coverage: known.value,
            });
        }
        Ok(Self {
            altitude_m,
            coverage,
        })
    }

    /// The layer's altitude above the ground plane, in canonical meters.
    #[must_use]
    pub fn altitude_m(&self) -> f64 {
        self.altitude_m
    }

    /// The fraction of sky this layer covers, or the explicit unknown.
    #[must_use]
    pub fn coverage(&self) -> &Resolved<f64> {
        &self.coverage
    }
}

// ----------------------------------------------------------- precipitation ---

/// Which precipitation an environment authors.
///
/// This is a **designed vocabulary**: whether the original distinguishes
/// rain from snow, and what else it can express, is unmeasured. The enum
/// says what this engine can represent today; it does not claim the
/// original's set (recorded as an unknown in the F19-A findings).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PrecipitationKind {
    /// No precipitation is authored.
    Clear,
    /// Rain.
    Rain,
    /// Snow.
    Snow,
}

impl PrecipitationKind {
    /// The stable label used in diagnostics and fingerprints.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Clear => "clear",
            Self::Rain => "rain",
            Self::Snow => "snow",
        }
    }
}

/// The authored precipitation of one environment: a kind, or the explicit
/// unknown its evidence left.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrecipitationDefinition {
    kind: Resolved<PrecipitationKind>,
}

impl PrecipitationDefinition {
    /// Wraps a resolved kind.
    #[must_use]
    pub fn new(kind: Resolved<PrecipitationKind>) -> Self {
        Self { kind }
    }

    /// The authored kind, or the explicit unknown.
    #[must_use]
    pub fn kind(&self) -> &Resolved<PrecipitationKind> {
        &self.kind
    }
}

// ---------------------------------------------------------------------- wind ---

/// Why a wind field was rejected.
#[derive(Clone, Debug, PartialEq)]
pub enum WindError {
    /// A component was NaN or infinite.
    NonFinite {
        /// The offending component index.
        component: usize,
    },
}

impl fmt::Display for WindError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NonFinite { component } => {
                write!(f, "wind velocity_m_s[{component}] must be finite")
            }
        }
    }
}

impl std::error::Error for WindError {}

/// The authoritative wind field: one world velocity in canonical meters per
/// second.
///
/// F19 non-negotiable behavior 2: this is *the* field flight and projectiles
/// consume (`v_air = v_world - wind_world`, `FLIGHT-PHYSICS`). Decorative
/// particles never read it and never derive their stream from it — they use
/// [`CosmeticWeatherSeed`], a separate domain. There is exactly one wind
/// field per environment state, so a definition and its timeline cannot
/// disagree about the air.
#[derive(Clone, Debug, PartialEq)]
pub struct WindField {
    velocity_m_s: [f64; 3],
}

impl WindField {
    /// Validates a wind velocity.
    ///
    /// # Errors
    ///
    /// [`WindError::NonFinite`].
    pub fn try_new(velocity_m_s: [f64; 3]) -> Result<Self, WindError> {
        for (component, value) in velocity_m_s.iter().enumerate() {
            if !value.is_finite() {
                return Err(WindError::NonFinite { component });
            }
        }
        Ok(Self { velocity_m_s })
    }

    /// The velocity in canonical world meters per second.
    #[must_use]
    pub fn velocity_m_s(&self) -> [f64; 3] {
        self.velocity_m_s
    }
}

/// The domain-separated seed of the **cosmetic** weather stream.
///
/// It is its own type so a decorative consumer cannot be handed a mission's
/// AI seed by accident: the two live in different types and draw from
/// different `SplitMix64` domains. The seed never affects the wind field,
/// and drawing from it never touches a gameplay sequence.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CosmeticWeatherSeed(u64);

impl CosmeticWeatherSeed {
    /// Wraps a run's root seed.
    #[must_use]
    pub const fn new(root_seed: u64) -> Self {
        Self(root_seed)
    }

    /// The root seed this cosmetic stream derives from.
    #[must_use]
    pub const fn root_seed(self) -> u64 {
        self.0
    }

    /// A generator for decorative particles only, domain-separated from
    /// every other consumer by [`COSMETIC_WEATHER_DOMAIN`].
    #[must_use]
    pub fn stream(self) -> cs_types::random::SplitMix64 {
        cs_types::random::SplitMix64::for_domain(self.0, COSMETIC_WEATHER_DOMAIN)
    }
}

// ------------------------------------------------------ gameplay visibility ---

/// Why a gameplay-visibility record was rejected.
#[derive(Clone, Debug, PartialEq)]
pub enum VisibilityError {
    /// The range was NaN or infinite.
    NonFinite,
    /// The range was zero or negative.
    NonPositive,
}

impl fmt::Display for VisibilityError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NonFinite => write!(f, "gameplay visibility range_m must be finite"),
            Self::NonPositive => {
                write!(f, "gameplay visibility range_m must be greater than zero")
            }
        }
    }
}

impl std::error::Error for VisibilityError {}

/// Atmospheric visibility **as gameplay sees it**: how far an actor may be
/// detected, in canonical meters.
///
/// It is authored data with its own resolution state and is never derived
/// from [`FogDefinition`] (F19 non-negotiable behavior 1). What the original
/// uses as a sight-range value, and in which unit, is unmeasured; the struct
/// is the typed home that measurement will fill.
#[derive(Clone, Debug, PartialEq)]
pub struct GameplayVisibility {
    range_m: f64,
}

impl GameplayVisibility {
    /// Validates a sight range.
    ///
    /// # Errors
    ///
    /// [`VisibilityError::NonFinite`] or [`VisibilityError::NonPositive`].
    pub fn try_new(range_m: f64) -> Result<Self, VisibilityError> {
        if !range_m.is_finite() {
            return Err(VisibilityError::NonFinite);
        }
        if range_m <= 0.0 {
            return Err(VisibilityError::NonPositive);
        }
        Ok(Self { range_m })
    }

    /// The sight range in canonical meters.
    #[must_use]
    pub fn range_m(&self) -> f64 {
        self.range_m
    }
}

// ------------------------------------------------------- environment state ---

/// The gameplay-relevant, timeline-mutable part of an environment: the
/// authoritative wind, the authored precipitation and the gameplay
/// visibility.
///
/// A [`WeatherEvent`] carries a whole state, so applying an event is a
/// replacement rather than a merge: replaying the same events yields the
/// same state at the same tick, with no leftover field from the previous
/// state (F19 non-negotiable behavior 4).
#[derive(Clone, Debug, PartialEq)]
pub struct EnvironmentState {
    wind: Resolved<WindField>,
    precipitation: PrecipitationDefinition,
    visibility: Resolved<GameplayVisibility>,
}

impl EnvironmentState {
    /// Assembles a state. Nothing here is defaulted: an unresolved field is
    /// a legal, reportable state of the record, not a construction failure.
    #[must_use]
    pub fn new(
        wind: Resolved<WindField>,
        precipitation: PrecipitationDefinition,
        visibility: Resolved<GameplayVisibility>,
    ) -> Self {
        Self {
            wind,
            precipitation,
            visibility,
        }
    }

    /// The authoritative wind field, or the explicit unknown its evidence
    /// left.
    #[must_use]
    pub fn wind(&self) -> &Resolved<WindField> {
        &self.wind
    }

    /// The authored precipitation, whose kind may be the explicit unknown.
    #[must_use]
    pub fn precipitation(&self) -> &PrecipitationDefinition {
        &self.precipitation
    }

    /// The gameplay visibility, or the explicit unknown. It never comes
    /// from fog.
    #[must_use]
    pub fn gameplay_visibility(&self) -> &Resolved<GameplayVisibility> {
        &self.visibility
    }
}

// ----------------------------------------------------------------- timeline ---

/// Why an environment timeline was rejected.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TimelineError {
    /// Two events were not strictly ordered by tick, so their order — and
    /// therefore the state a replay reaches — would depend on how the list
    /// was supplied.
    UnorderedTicks {
        /// The index of the offending event.
        index: usize,
        /// The tick of the event before it.
        previous: u64,
        /// The tick the offending event carried.
        found: u64,
    },
}

impl fmt::Display for TimelineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnorderedTicks {
                index,
                previous,
                found,
            } => write!(
                f,
                "event {index} is at tick {found}, which does not follow the previous event at tick {previous}"
            ),
        }
    }
}

impl std::error::Error for TimelineError {}

/// One authored weather change, scheduled at a whole simulation tick.
///
/// Integer ticks are the canonical time of the `IDENTITY-CONTENT` numeric
/// contract: an event's instant is a tick count, never a floating-point
/// second, so a pause, a frame split or a replay cannot move it.
#[derive(Clone, Debug, PartialEq)]
pub struct WeatherEvent {
    at_tick: u64,
    state: EnvironmentState,
}

impl WeatherEvent {
    /// Schedules `state` at tick `at_tick`.
    #[must_use]
    pub fn new(at_tick: u64, state: EnvironmentState) -> Self {
        Self { at_tick, state }
    }

    /// The tick this event fires on.
    #[must_use]
    pub fn at_tick(&self) -> u64 {
        self.at_tick
    }

    /// The state this event installs when its tick is reached.
    #[must_use]
    pub fn state(&self) -> &EnvironmentState {
        &self.state
    }
}

/// The authored weather timeline of one environment: events in strictly
/// increasing tick order, validated once at construction.
///
/// The timeline says *what* changes and *when*, in ticks. Which time domain
/// those ticks belong to, what pausing does to them and how a replay
/// reproduces them is decided in `cs_sim`'s `visibility` module, which runs
/// this list on the authoritative gameplay clock.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct EnvironmentTimeline {
    events: Vec<WeatherEvent>,
}

impl EnvironmentTimeline {
    /// Validates and wraps the authored events.
    ///
    /// # Errors
    ///
    /// [`TimelineError::UnorderedTicks`] when two events are not strictly
    /// increasing by tick.
    pub fn try_new(events: Vec<WeatherEvent>) -> Result<Self, TimelineError> {
        for (index, window) in events.windows(2).enumerate() {
            if window[1].at_tick <= window[0].at_tick {
                return Err(TimelineError::UnorderedTicks {
                    index: index + 1,
                    previous: window[0].at_tick,
                    found: window[1].at_tick,
                });
            }
        }
        Ok(Self { events })
    }

    /// The events in ascending tick order.
    #[must_use]
    pub fn events(&self) -> &[WeatherEvent] {
        &self.events
    }

    /// Whether no change is authored at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }
}

// ------------------------------------------------------ environment record ---

/// Why an [`EnvironmentDefinition`] was rejected.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EnvironmentError {
    /// The record asks for a generated sky while running under the retail
    /// profile (F19 non-negotiable behavior 5).
    GeneratedSkyInRetailProfile,
    /// The timeline's events are not strictly increasing by tick.
    UnorderedTimeline(TimelineError),
}

impl fmt::Display for EnvironmentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::GeneratedSkyInRetailProfile => write!(
                f,
                "a generated sky is only allowed under the synthetic/developer profile"
            ),
            Self::UnorderedTimeline(error) => write!(f, "invalid weather timeline: {error}"),
        }
    }
}

impl std::error::Error for EnvironmentError {}

impl From<TimelineError> for EnvironmentError {
    fn from(error: TimelineError) -> Self {
        Self::UnorderedTimeline(error)
    }
}

/// One authored environment: sky art, sky orientation, fog, lighting, cloud
/// layers, the gameplay-relevant state and its timeline.
///
/// Construction validates the whole record once — a generated sky outside a
/// synthetic run and an unordered timeline are refused at the door — so a
/// consumer walks a record whose profile and schedule already hold.
///
/// Every category is a separate field, and the state's wind, precipitation
/// and visibility are exposed *through* the state
/// ([`EnvironmentDefinition::wind`]), because a timeline event replaces a
/// state: one authoritative wind field, not two copies that can drift.
#[derive(Clone, Debug, PartialEq)]
pub struct EnvironmentDefinition {
    id: EnvironmentId,
    origin: Origin,
    profile: EnvironmentProfile,
    sky: SkyArt,
    sky_orientation: Resolved<SkyOrientation>,
    lighting: LightingDefinition,
    fog: FogDefinition,
    cloud_layers: Vec<CloudLayer>,
    state: EnvironmentState,
    timeline: EnvironmentTimeline,
    provenance: Provenance,
}

impl EnvironmentDefinition {
    /// Builds and validates an environment definition.
    ///
    /// # Errors
    ///
    /// [`EnvironmentError::GeneratedSkyInRetailProfile`] when a retail
    /// profile carries a generated-sky fallback, or
    /// [`EnvironmentError::UnorderedTimeline`] when the schedule is not
    /// strictly increasing by tick.
    // Eleven explicit parts rather than a builder: every one of them is a
    // field of the record, so an importer cannot forget one and get a
    // defaulted category back.
    #[allow(clippy::too_many_arguments)]
    pub fn try_new(
        id: EnvironmentId,
        origin: Origin,
        profile: EnvironmentProfile,
        sky: SkyArt,
        sky_orientation: Resolved<SkyOrientation>,
        lighting: LightingDefinition,
        fog: FogDefinition,
        cloud_layers: Vec<CloudLayer>,
        state: EnvironmentState,
        timeline: EnvironmentTimeline,
        provenance: Provenance,
    ) -> Result<Self, EnvironmentError> {
        if profile == EnvironmentProfile::Retail && sky.fallback().is_generated() {
            return Err(EnvironmentError::GeneratedSkyInRetailProfile);
        }
        Ok(Self {
            id,
            origin,
            profile,
            sky,
            sky_orientation,
            lighting,
            fog,
            cloud_layers,
            state,
            timeline,
            provenance,
        })
    }

    /// The environment's identity.
    #[must_use]
    pub fn id(&self) -> &EnvironmentId {
        &self.id
    }

    /// Where this definition came from: installation bytes, a synthetic
    /// fixture or designed content.
    #[must_use]
    pub fn origin(&self) -> &Origin {
        &self.origin
    }

    /// The content rules this environment runs under.
    #[must_use]
    pub fn profile(&self) -> EnvironmentProfile {
        self.profile
    }

    /// The sky art, including what a renderer must do when the texture is
    /// missing.
    #[must_use]
    pub fn sky(&self) -> &SkyArt {
        &self.sky
    }

    /// The sky's world orientation, or the explicit unknown its evidence
    /// left.
    #[must_use]
    pub fn sky_orientation(&self) -> &Resolved<SkyOrientation> {
        &self.sky_orientation
    }

    /// The lighting rig.
    #[must_use]
    pub fn lighting(&self) -> &LightingDefinition {
        &self.lighting
    }

    /// The sun's world direction, or the explicit unknown. Convenience over
    /// [`EnvironmentDefinition::lighting`]; it is the same record.
    #[must_use]
    pub fn sun_direction(&self) -> &Resolved<UnitVec3> {
        self.lighting.sun_direction()
    }

    /// The screen-space fog record. It is *not* gameplay visibility.
    #[must_use]
    pub fn fog(&self) -> &FogDefinition {
        &self.fog
    }

    /// The authored cloud layers, in supplied order.
    #[must_use]
    pub fn cloud_layers(&self) -> &[CloudLayer] {
        &self.cloud_layers
    }

    /// The gameplay-relevant state: wind, precipitation and gameplay
    /// visibility.
    #[must_use]
    pub fn state(&self) -> &EnvironmentState {
        &self.state
    }

    /// The **one authoritative wind field** flight and projectiles consume.
    #[must_use]
    pub fn wind(&self) -> &Resolved<WindField> {
        self.state.wind()
    }

    /// The authored precipitation of this environment: the record is always
    /// present, its kind may be an explicit unknown.
    #[must_use]
    pub fn precipitation(&self) -> &PrecipitationDefinition {
        self.state.precipitation()
    }

    /// The gameplay visibility actors are detected with — its own authored
    /// value, never inferred from [`EnvironmentDefinition::fog`].
    #[must_use]
    pub fn gameplay_visibility(&self) -> &Resolved<GameplayVisibility> {
        self.state.gameplay_visibility()
    }

    /// The authored weather timeline.
    #[must_use]
    pub fn timeline(&self) -> &EnvironmentTimeline {
        &self.timeline
    }

    /// The provenance of the record itself.
    #[must_use]
    pub fn provenance(&self) -> &Provenance {
        &self.provenance
    }

    /// A canonical SHA-256 fingerprint of what this definition *says*: its
    /// identity, profile, sky art, orientation, lighting, fog, cloud layers,
    /// state and timeline, in field order.
    ///
    /// Like `crate::world::WorldDefinition::record_fingerprint` it
    /// fingerprints the record, not the source bytes, and leaves `origin`
    /// and `provenance` out: those say where the record came from, not what
    /// it says. The result is a [`ContentHash`] (64 lowercase hex
    /// characters), stable across Rust releases.
    #[must_use]
    pub fn record_fingerprint(&self) -> ContentHash {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"cs/content/environment/record/v1\0");
        push_text(&mut bytes, self.id.as_str());
        push_text(&mut bytes, self.origin.label());
        push_text(&mut bytes, self.profile.label());

        push_resolved(&mut bytes, self.sky.texture(), |bytes, id| {
            push_text(bytes, id.as_str())
        });
        match self.sky.fallback() {
            SkyFallback::Diagnostic => bytes.push(0),
            SkyFallback::Generated { profile } => {
                bytes.push(1);
                push_text(&mut bytes, profile.label());
            }
        }

        push_resolved(&mut bytes, self.sky_orientation(), |bytes, orientation| {
            push_unit(bytes, orientation.up());
            push_unit(bytes, orientation.heading());
        });
        push_resolved(&mut bytes, self.lighting.sun_direction(), |bytes, sun| {
            push_unit(bytes, *sun)
        });
        push_resolved(
            &mut bytes,
            self.lighting.ambient_linear(),
            |bytes, ambient| {
                for value in *ambient {
                    push_f64(bytes, value);
                }
            },
        );
        push_resolved(&mut bytes, self.fog.density_per_m(), |bytes, density| {
            push_f64(bytes, *density)
        });
        push_resolved(&mut bytes, self.fog.color_linear(), |bytes, color| {
            for value in *color {
                push_f64(bytes, value);
            }
        });

        bytes.extend_from_slice(&(self.cloud_layers.len() as u32).to_le_bytes());
        for layer in &self.cloud_layers {
            push_f64(&mut bytes, layer.altitude_m());
            push_resolved(&mut bytes, layer.coverage(), |bytes, coverage| {
                push_f64(bytes, *coverage)
            });
        }

        push_state(&mut bytes, self.state());

        bytes.extend_from_slice(&(self.timeline.events().len() as u32).to_le_bytes());
        for event in self.timeline.events() {
            bytes.extend_from_slice(&event.at_tick().to_le_bytes());
            push_state(&mut bytes, event.state());
        }

        sha256(&bytes)
    }
}

/// Encodes one gameplay-relevant state: wind, precipitation and gameplay
/// visibility, each as a [`Resolved`] record.
fn push_state(bytes: &mut Vec<u8>, state: &EnvironmentState) {
    push_resolved(bytes, state.wind(), |bytes, wind| {
        for value in wind.velocity_m_s() {
            push_f64(bytes, value);
        }
    });
    push_resolved(bytes, state.precipitation().kind(), |bytes, kind| {
        push_text(bytes, kind.label());
    });
    push_resolved(bytes, state.gameplay_visibility(), |bytes, visibility| {
        push_f64(bytes, visibility.range_m());
    });
}

/// Appends a length-free, NUL-terminated string: no byte sequence can be
/// mistaken for a terminator inside the next field.
fn push_text(bytes: &mut Vec<u8>, value: &str) {
    bytes.extend_from_slice(value.as_bytes());
    bytes.push(0);
}

fn push_f64(bytes: &mut Vec<u8>, value: f64) {
    bytes.extend_from_slice(&value.to_bits().to_le_bytes());
}

fn push_unit(bytes: &mut Vec<u8>, value: UnitVec3) {
    for component in value.to_array() {
        push_f64(bytes, component);
    }
}

/// Encodes a [`Resolved`] record: a `1` and the value, or a `0`, the claim
/// id and the reason an explicit unknown carries.
fn push_resolved<T>(
    bytes: &mut Vec<u8>,
    value: &Resolved<T>,
    push_value: impl FnOnce(&mut Vec<u8>, &T),
) {
    match value {
        Resolved::Known(known) => {
            bytes.push(1);
            push_value(bytes, &known.value);
        }
        Resolved::Unknown { claim_id, reason } => {
            bytes.push(0);
            push_text(bytes, claim_id.as_str());
            push_text(bytes, reason);
        }
    }
}
