//! The minimal synthetic environment fixtures (F19-A).
//!
//! Spec: `specs/F19-sky-atmosphere-weather-and-visibility.md`, stage
//! `### F19-A`.
//!
//! Like [`crate::synthetic`], [`crate::world::fixture`] and
//! [`crate::physics::fixture`], this is **production bootstrap code**, not a
//! test-only reimplementation: both environments are built through the same
//! `cs_content::environment` constructors a real importer will call, and the
//! acceptance tests drive these functions rather than authoring their own
//! records.
//!
//! Two environments are enough to exercise the contract:
//!
//! * [`clear_sky_environment`] — a fully authored clear sky with a known
//!   sun, a designed fog default and an **unknown** gameplay visibility:
//!   exactly the pair F19 non-negotiable behavior 1 says must stay
//!   separate.
//! * [`storm_environment`] — a sky texture that was never found (diagnostic,
//!   no substitute) and no authored sun direction at all, plus a timeline
//!   whose wind and precipitation change at [`WIND_SHIFT_TICK`] and whose
//!   gameplay visibility only becomes known at [`VISIBILITY_TICK`].
//!   Before that tick the record's visibility is an explicit unknown, so
//!   "no default was invented" is observable.
//!
//! Both are `Origin::SyntheticFixture` content — newly authored development
//! data that never claims to be an original environment. The
//! [`EnvironmentProfile`] they declare governs *fallbacks* (may a generated
//! sky appear?), not provenance: both run under retail rules, so a missing
//! texture stays a diagnostic.
//!
//! Which environment records the original stores, and what values it
//! authors, is unmeasured — see
//! `docs/findings/2026-09-30-f19-a-environment-data-and-time-domains.md`.

use cs_content::environment::{
    CloudLayer, EnvironmentDefinition, EnvironmentError, EnvironmentId, EnvironmentProfile,
    EnvironmentState, EnvironmentTimeline, FogDefinition, GameplayVisibility, LightingDefinition,
    PrecipitationDefinition, PrecipitationKind, SkyArt, SkyFallback, SkyOrientation, WeatherEvent,
    WindField,
};
use cs_types::content::{ContentId, ContentKind, Known, Origin, Provenance, Resolved};
use cs_types::evidence::ClaimId;
use cs_types::space::UnitVec3;

/// The identity of [`clear_sky_environment`].
pub const CLEAR_ENV_KEY: &str = "fixture.clear-sky";

/// The identity of [`storm_environment`].
pub const STORM_ENV_KEY: &str = "fixture.storm";

/// The authored sky texture of [`clear_sky_environment`].
pub const CLEAR_SKY_TEXTURE_KEY: &str = "fixture.sky.clear-day";

/// The tick (at [`STORM_RATE_HZ`]) the storm's wind and precipitation
/// change on.
pub const WIND_SHIFT_TICK: u64 = 64;

/// The tick (at [`STORM_RATE_HZ`]) the storm's gameplay visibility becomes
/// known. Until then it stays an explicit unknown.
pub const VISIBILITY_TICK: u64 = 192;

/// The fixed rate both fixtures are meant to run at, in ticks per second: a
/// power of two, so one tick is an exact binary fraction of a second and a
/// test's frame arithmetic cannot drift.
pub const STORM_RATE_HZ: u32 = 64;

/// The fixture provenance of one authored value: designed content, never
/// original data.
fn provenance(claim_id: &str) -> Provenance {
    Provenance::designed(ClaimId::new(claim_id).expect("the fixture claim ids are valid"))
}

/// A value known by fixture design.
fn known<T>(value: T, claim_id: &str) -> Resolved<T> {
    Resolved::Known(Known::new(value, provenance(claim_id)))
}

/// A value the fixture deliberately knows nothing about.
fn unknown<T>(claim_id: &str, reason: &str) -> Resolved<T> {
    Resolved::unknown(
        ClaimId::new(claim_id).expect("the fixture claim ids are valid"),
        reason,
    )
    .expect("the fixture reasons are non-empty")
}

/// The clear-sky horizon: canonical up, with a heading that is deliberately
/// *not* an axis, so a frame that quietly replaced the authored orientation
/// with an identity matrix would be caught.
fn clear_sky_orientation() -> Resolved<SkyOrientation> {
    let up = UnitVec3::try_new([0.0, 1.0, 0.0]).expect("the fixture up axis is a unit vector");
    // A 3-4-5 heading on the horizon plane: unit length and exactly
    // perpendicular to up, so no tolerance is doing the work.
    let heading = UnitVec3::try_new([0.6, 0.0, 0.8]).expect("the fixture heading is a unit vector");
    known(
        SkyOrientation::try_new(up, heading)
            .expect("the fixture heading is perpendicular to the horizon"),
        "f19a.fixture.clear-sky.orientation",
    )
}

/// The storm's horizon: canonical up, a different authored heading.
fn storm_orientation() -> Resolved<SkyOrientation> {
    let up = UnitVec3::try_new([0.0, 1.0, 0.0]).expect("the fixture up axis is a unit vector");
    let heading =
        UnitVec3::try_new([-0.8, 0.0, 0.6]).expect("the fixture heading is a unit vector");
    known(
        SkyOrientation::try_new(up, heading)
            .expect("the fixture heading is perpendicular to the horizon"),
        "f19a.fixture.storm.orientation",
    )
}

/// A clear sky with an authored texture, a known sun, a designed fog
/// default and an unknown gameplay visibility.
///
/// # Errors
///
/// [`EnvironmentError`] only for a record this fixture did not intend to
/// build; the parts above are validated as they are constructed.
pub fn clear_sky_environment() -> Result<EnvironmentDefinition, EnvironmentError> {
    let sky = SkyArt::try_new(
        known(
            ContentId::from_source(ContentKind::Image, CLEAR_SKY_TEXTURE_KEY)
                .expect("the fixture sky texture key is valid"),
            "f19a.fixture.clear-sky.sky-texture",
        ),
        SkyFallback::Diagnostic,
    )
    .expect("the fixture sky texture is an image id with a diagnostic fallback");

    let lighting = LightingDefinition::try_new(
        known(
            UnitVec3::try_new([0.6, 0.8, 0.0]).expect("the fixture sun is a unit vector"),
            "f19a.fixture.clear-sky.sun",
        ),
        known([0.2, 0.2, 0.25], "f19a.fixture.clear-sky.ambient"),
    )
    .expect("the fixture lighting is finite and non-negative");

    // A renderer default, tagged designed: F19 behavior 1 allows a default,
    // as long as it cannot be mistaken for a measurement.
    let fog = FogDefinition::designed_default(
        1e-4,
        [0.7, 0.8, 0.95],
        "f19a.fixture.clear-sky.fog-default",
    )
    .expect("the fixture fog default is representable");

    let state = EnvironmentState::new(
        known(
            WindField::try_new([3.0, 0.0, -1.5]).expect("the fixture wind is finite"),
            "f19a.fixture.clear-sky.wind",
        ),
        PrecipitationDefinition::new(known(
            PrecipitationKind::Clear,
            "f19a.fixture.clear-sky.precipitation",
        )),
        unknown(
            "f19a.fixture.clear-sky.visibility",
            "no sight-range value has been authored for this environment",
        ),
    );

    EnvironmentDefinition::try_new(
        EnvironmentId::new(CLEAR_ENV_KEY).expect("the fixture environment key is valid"),
        Origin::SyntheticFixture,
        EnvironmentProfile::Retail,
        sky,
        clear_sky_orientation(),
        lighting,
        fog,
        vec![
            CloudLayer::try_new(1500.0, known(0.25, "f19a.fixture.clear-sky.cloud-coverage"))
                .expect("the fixture cloud layer is representable"),
        ],
        state,
        EnvironmentTimeline::default(),
        provenance("f19a.fixture.clear-sky.record"),
    )
}

/// A storm whose sky texture was never found and whose weather changes on
/// the timeline: wind and precipitation at [`WIND_SHIFT_TICK`], gameplay
/// visibility — still an explicit unknown until then — at
/// [`VISIBILITY_TICK`].
///
/// # Errors
///
/// [`EnvironmentError`] only for a record this fixture did not intend to
/// build; the parts above are validated as they are constructed.
pub fn storm_environment() -> Result<EnvironmentDefinition, EnvironmentError> {
    let sky = SkyArt::missing(
        "f19a.fixture.storm.sky-texture",
        "the storm fixture authors no sky texture",
    )
    .expect("the fixture sky claim ids are valid and its reason is non-empty");

    let lighting = LightingDefinition::try_new(
        unknown(
            "f19a.fixture.storm.sun",
            "the storm fixture authors no sun direction",
        ),
        unknown(
            "f19a.fixture.storm.ambient",
            "the storm fixture authors no ambient term",
        ),
    )
    .expect("two unknown values need no range check");

    let fog =
        FogDefinition::designed_default(6e-4, [0.55, 0.58, 0.62], "f19a.fixture.storm.fog-default")
            .expect("the fixture fog default is representable");

    let unknown_visibility = || -> Resolved<GameplayVisibility> {
        unknown(
            "f19a.fixture.storm.visibility",
            "the storm's sight range is authored later on the timeline",
        )
    };

    let initial = EnvironmentState::new(
        known(
            WindField::try_new([8.0, 0.0, 0.0]).expect("the fixture wind is finite"),
            "f19a.fixture.storm.wind.initial",
        ),
        PrecipitationDefinition::new(known(
            PrecipitationKind::Clear,
            "f19a.fixture.storm.precipitation.initial",
        )),
        unknown_visibility(),
    );

    let gust = EnvironmentState::new(
        known(
            WindField::try_new([16.0, 0.0, 2.0]).expect("the fixture wind is finite"),
            "f19a.fixture.storm.wind.gust",
        ),
        PrecipitationDefinition::new(known(
            PrecipitationKind::Rain,
            "f19a.fixture.storm.precipitation.gust",
        )),
        unknown_visibility(),
    );

    let downpour = EnvironmentState::new(
        known(
            WindField::try_new([16.0, 0.0, 2.0]).expect("the fixture wind is finite"),
            "f19a.fixture.storm.wind.downpour",
        ),
        PrecipitationDefinition::new(known(
            PrecipitationKind::Rain,
            "f19a.fixture.storm.precipitation.downpour",
        )),
        known(
            GameplayVisibility::try_new(400.0)
                .expect("the fixture sight range is a positive finite number"),
            "f19a.fixture.storm.visibility",
        ),
    );

    let timeline = EnvironmentTimeline::try_new(vec![
        WeatherEvent::new(WIND_SHIFT_TICK, gust),
        WeatherEvent::new(VISIBILITY_TICK, downpour),
    ])
    .expect("the fixture timeline is strictly increasing by tick");

    EnvironmentDefinition::try_new(
        EnvironmentId::new(STORM_ENV_KEY).expect("the fixture environment key is valid"),
        Origin::SyntheticFixture,
        EnvironmentProfile::Retail,
        sky,
        storm_orientation(),
        lighting,
        fog,
        Vec::new(),
        initial,
        timeline,
        provenance("f19a.fixture.storm.record"),
    )
}
