//! Shared fixtures for the F19-A acceptance tests.
//!
//! Every value is authored here: synthetic identities, directions, winds and
//! claim ids. Nothing is read from `CS_GAME_DIR`, and nothing claims to be
//! an original environment.

use std::time::Duration;

use cs_app::environment::{STORM_RATE_HZ, clear_sky_environment, storm_environment};
use cs_content::environment::{
    EnvironmentDefinition, EnvironmentError, EnvironmentId, EnvironmentProfile, EnvironmentState,
    EnvironmentTimeline, FogDefinition, LightingDefinition, PrecipitationDefinition,
    PrecipitationKind, SkyArt, SkyFallback, SkyOrientation, WindField,
};
use cs_sim::time::TickRate;
use cs_types::content::{ContentId, ContentKind, Known, Origin, Provenance, Resolved};
use cs_types::evidence::ClaimId;
use cs_types::space::{UnitVec3, WorldPosition};

/// One frame of the fixtures' fixed rate: 1/64 s in whole nanoseconds, so
/// one frame commits exactly one tick and the tests' tick arithmetic is
/// exact.
pub const FRAME: Duration = Duration::from_nanos(15_625_000);

/// The clear-sky fixture environment, built by production code.
#[must_use]
pub fn clear() -> EnvironmentDefinition {
    clear_sky_environment().expect("the clear-sky fixture is well formed")
}

/// The storm fixture environment, built by production code.
#[must_use]
pub fn storm() -> EnvironmentDefinition {
    storm_environment().expect("the storm fixture is well formed")
}

/// The fixtures' fixed rate.
#[must_use]
pub fn tick_rate() -> TickRate {
    TickRate::new(STORM_RATE_HZ).expect("the fixture rate is non-zero")
}

/// A finite world position from three components.
#[must_use]
pub fn world(components: [f64; 3]) -> WorldPosition {
    WorldPosition::try_new(components).expect("test coordinates are finite")
}

/// A unit direction from three components.
#[must_use]
pub fn unit(components: [f64; 3]) -> UnitVec3 {
    UnitVec3::try_new(components).expect("test directions are unit length")
}

/// A valid claim id.
#[must_use]
pub fn claim(claim_id: &str) -> ClaimId {
    ClaimId::new(claim_id).expect("test claim ids are valid")
}

/// A value known by test design, with its claim id as provenance.
#[must_use]
pub fn known<T>(value: T, claim_id: &str) -> Resolved<T> {
    Resolved::Known(Known::new(value, Provenance::designed(claim(claim_id))))
}

/// A value the test deliberately knows nothing about.
#[must_use]
pub fn unknown<T>(claim_id: &str, reason: &str) -> Resolved<T> {
    Resolved::unknown(claim(claim_id), reason).expect("test reasons are non-empty")
}

/// The claim id of a known value, or `None` when it is unknown.
#[must_use]
pub fn known_claim<T>(value: &Resolved<T>) -> Option<&str> {
    match value {
        Resolved::Known(known) => Some(known.provenance.claim_id.as_str()),
        Resolved::Unknown { .. } => None,
    }
}

/// The claim id and reason of an unknown value, or `None` when it is known.
#[must_use]
pub fn unknown_claim<T>(value: &Resolved<T>) -> Option<(&str, &str)> {
    match value {
        Resolved::Unknown { claim_id, reason } => Some((claim_id.as_str(), reason.as_str())),
        Resolved::Known(_) => None,
    }
}

/// A minimal environment assembled from production constructors: a fixed
/// identity, a fixed state, and whatever sky, lighting and sky orientation
/// the calling test wants to vary.
///
/// # Errors
///
/// [`EnvironmentError`] when the profile and the sky disagree (a generated
/// sky under the retail profile) — which is exactly what two of the tests
/// come here to provoke.
pub fn definition_with(
    profile: EnvironmentProfile,
    sky: SkyArt,
    lighting: LightingDefinition,
    orientation: Resolved<SkyOrientation>,
) -> Result<EnvironmentDefinition, EnvironmentError> {
    definition_with_origin(
        Origin::SyntheticFixture,
        profile,
        sky,
        lighting,
        orientation,
    )
}

/// [`definition_with`] with an explicit [`Origin`], so a test can hold the
/// content fixed and vary only where the record claims to come from (which
/// the record fingerprint must ignore).
///
/// # Errors
///
/// [`EnvironmentError`] when the profile and the sky disagree (a generated
/// sky under the retail profile).
pub fn definition_with_origin(
    origin: Origin,
    profile: EnvironmentProfile,
    sky: SkyArt,
    lighting: LightingDefinition,
    orientation: Resolved<SkyOrientation>,
) -> Result<EnvironmentDefinition, EnvironmentError> {
    EnvironmentDefinition::try_new(
        EnvironmentId::new("fixture.test").expect("the test environment key is valid"),
        origin,
        profile,
        sky,
        orientation,
        lighting,
        FogDefinition::designed_default(1e-4, [0.7, 0.8, 0.95], "f19a.test.fog")
            .expect("the test fog default is representable"),
        Vec::new(),
        EnvironmentState::new(
            known(
                WindField::try_new([0.0, 0.0, 0.0]).expect("zero wind is finite"),
                "f19a.test.wind",
            ),
            PrecipitationDefinition::new(known(
                PrecipitationKind::Clear,
                "f19a.test.precipitation",
            )),
            unknown("f19a.test.visibility", "the test authors no sight range"),
        ),
        EnvironmentTimeline::default(),
        Provenance::designed(claim("f19a.test.record")),
    )
}

/// [`definition_with`] with the default test orientation: canonical up and a
/// non-axis-aligned heading.
///
/// # Errors
///
/// [`EnvironmentError`] when the profile and the sky disagree.
pub fn definition(
    profile: EnvironmentProfile,
    sky: SkyArt,
    lighting: LightingDefinition,
) -> Result<EnvironmentDefinition, EnvironmentError> {
    definition_with(profile, sky, lighting, default_orientation())
}

/// The default test orientation: canonical up, a 3-4-5 heading that no
/// identity matrix could produce by accident.
#[must_use]
pub fn default_orientation() -> Resolved<SkyOrientation> {
    let orientation = SkyOrientation::try_new(unit([0.0, 1.0, 0.0]), unit([0.6, 0.0, 0.8]))
        .expect("the test heading is perpendicular to the horizon");
    known(orientation, "f19a.test.orientation")
}

/// Sky art with the fixture's authored texture and a diagnostic fallback.
#[must_use]
pub fn authored_sky() -> SkyArt {
    SkyArt::try_new(
        known(
            ContentId::from_source(ContentKind::Image, "fixture.sky.test")
                .expect("the test sky texture key is valid"),
            "f19a.test.sky-texture",
        ),
        SkyFallback::Diagnostic,
    )
    .expect("the test sky texture is an image id")
}
