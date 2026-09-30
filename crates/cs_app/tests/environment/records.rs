//! `accept_f19_a_` tests for the typed environment contract in
//! `cs_content::environment`: the missing-sky diagnostic and the
//! synthetic-only generated sky (F19 behavior 5), a designed fog default
//! that never fills in gameplay visibility (behavior 1), the single
//! authoritative wind field with its separate cosmetic stream (behavior 2),
//! the timeline's validation, the construction refusals and the record
//! fingerprint.
//!
//! Every record is built through production constructors; the assertions
//! read what those constructors produced.

use cs_app::environment::{CLEAR_SKY_TEXTURE_KEY, WIND_SHIFT_TICK, clear_sky_environment};
use cs_content::environment::{
    COSMETIC_WEATHER_DOMAIN, CosmeticWeatherSeed, EnvironmentError, EnvironmentProfile,
    EnvironmentState, EnvironmentTimeline, FogDefinition, GameplayVisibility, LightingDefinition,
    SkyArt, SkyArtError, SkyFallback, TimelineError, WeatherEvent, WindField,
};
use cs_types::content::{ContentId, ContentKind, Origin, Resolved};
use cs_types::evidence::ClaimStatus;
use cs_types::random::{SYNTHETIC_BODY_DOMAIN, SplitMix64};

use crate::common;

/// Lighting with no authored values at all: both fields explicitly unknown.
fn lighting_without_values() -> LightingDefinition {
    LightingDefinition::try_new(
        common::unknown("f19a.test.sun", "this test authors no sun"),
        common::unknown("f19a.test.ambient", "this test authors no ambient"),
    )
    .expect("two unknown values need no range check")
}

/// Lighting with a known sun direction and an explicitly unknown ambient
/// term. The sun's claim id is a parameter so two records can be built
/// that differ in provenance alone.
fn sun_lighting(direction: [f64; 3], sun_claim: &str) -> LightingDefinition {
    LightingDefinition::try_new(
        common::known(common::unit(direction), sun_claim),
        common::unknown("f19a.test.fingerprint.ambient", "no ambient authored"),
    )
    .expect("a unit direction and an unknown ambient term are valid")
}

/// F19 non-negotiable behavior 5: a missing sky texture is a diagnostic, a
/// generated sky exists only under the explicitly labeled
/// synthetic/developer profile, and a retail profile refuses it at the door
/// — both at the sky record and at the definition that carries it.
#[test]
fn accept_f19_a_missing_sky_texture_is_a_diagnostic_and_a_generated_sky_needs_the_synthetic_profile()
 {
    // The storm fixture never found a texture: an explicit unknown with its
    // claim and reason, plus the diagnostic fallback — never a substitute.
    let storm = common::storm();
    assert_eq!(
        common::unknown_claim(storm.sky().texture()),
        Some((
            "f19a.fixture.storm.sky-texture",
            "the storm fixture authors no sky texture"
        ))
    );
    assert!(
        matches!(storm.sky().fallback(), SkyFallback::Diagnostic),
        "a missing texture must be reported, not replaced"
    );
    assert!(
        !storm.sky().allows_generated_sky(EnvironmentProfile::Retail),
        "a diagnostic fallback generates nothing in a retail run"
    );
    assert!(
        !storm
            .sky()
            .allows_generated_sky(EnvironmentProfile::SyntheticDeveloper),
        "a diagnostic fallback generates nothing anywhere: it is not a hidden generator"
    );

    // Generated art declares the profile it belongs to, and only the
    // synthetic/developer label may use it.
    let generated = SkyArt::generated(EnvironmentProfile::SyntheticDeveloper);
    assert!(generated.allows_generated_sky(EnvironmentProfile::SyntheticDeveloper));
    assert!(!generated.allows_generated_sky(EnvironmentProfile::Retail));

    // The sky record refuses a generated fallback that names the retail
    // profile outright. The texture must be *missing* for this branch to be
    // reached — a known texture is refused earlier for the contradiction
    // below.
    let missing_texture = SkyArt::missing("f19a.test.missing", "this test authors no texture")
        .expect("the claim id and reason are valid")
        .texture()
        .clone();
    match SkyArt::try_new(
        missing_texture,
        SkyFallback::Generated {
            profile: EnvironmentProfile::Retail,
        },
    ) {
        Err(SkyArtError::NonSyntheticGeneratedSky {
            found: EnvironmentProfile::Retail,
        }) => {}
        other => panic!("a retail generated fallback must be refused, got {other:?}"),
    }

    // A known texture with a generated fallback contradicts itself: the
    // fallback exists only because the texture is missing.
    let known_texture = common::authored_sky().texture().clone();
    match SkyArt::try_new(
        known_texture,
        SkyFallback::Generated {
            profile: EnvironmentProfile::SyntheticDeveloper,
        },
    ) {
        Err(SkyArtError::GeneratedFallbackWithKnownTexture) => {}
        other => panic!("a generated fallback over a known texture must be refused, got {other:?}"),
    }

    // A known texture id must be an image id.
    let wrong_kind = common::known(
        ContentId::from_source(ContentKind::Mission, "m01").expect("the test key is valid"),
        "f19a.test.wrong-kind",
    );
    match SkyArt::try_new(wrong_kind, SkyFallback::Diagnostic) {
        Err(SkyArtError::WrongTextureKind {
            found: ContentKind::Mission,
        }) => {}
        other => panic!("a non-image sky texture must be refused, got {other:?}"),
    }

    // And the definition itself refuses a generated sky in a retail run,
    // whatever the sky record says — the profile travels with the record.
    assert!(matches!(
        common::definition(
            EnvironmentProfile::Retail,
            SkyArt::generated(EnvironmentProfile::SyntheticDeveloper),
            lighting_without_values(),
        ),
        Err(EnvironmentError::GeneratedSkyInRetailProfile)
    ));
    let allowed = common::definition(
        EnvironmentProfile::SyntheticDeveloper,
        SkyArt::generated(EnvironmentProfile::SyntheticDeveloper),
        lighting_without_values(),
    )
    .expect("the synthetic/developer profile may generate a sky");
    assert!(
        allowed
            .sky()
            .allows_generated_sky(EnvironmentProfile::SyntheticDeveloper)
    );
}

/// F19 non-negotiable behavior 1: a renderer default is tagged `designed`,
/// and gameplay visibility keeps its own explicit unknown — a known fog
/// value never becomes a sight range.
#[test]
fn accept_f19_a_designed_fog_default_does_not_fill_in_gameplay_visibility() {
    let environment = common::clear();

    // The fog default is known and *labeled* designed, with its claim id.
    let density = match environment.fog().density_per_m() {
        Resolved::Known(known) => known,
        Resolved::Unknown { .. } => panic!("the fixture authors a designed fog default"),
    };
    assert_eq!(density.provenance.class, ClaimStatus::Designed);
    assert_eq!(
        density.provenance.claim_id.as_str(),
        "f19a.fixture.clear-sky.fog-default"
    );
    assert!((density.value - 1e-4).abs() < f64::EPSILON);

    // A designed default is still refused when it is not representable:
    // "designed" describes its evidence, not a licence for any number.
    assert!(
        FogDefinition::designed_default(-1.0, [0.0, 0.0, 0.0], "f19a.test.bad-fog").is_err(),
        "a negative fog density must be refused"
    );

    // The fog is known while gameplay visibility is explicitly unknown, on
    // the same record: nothing derived one from the other.
    assert!(environment.fog().density_per_m().is_known());
    assert!(!environment.gameplay_visibility().is_known());
    assert_eq!(
        common::unknown_claim(environment.gameplay_visibility()),
        Some((
            "f19a.fixture.clear-sky.visibility",
            "no sight-range value has been authored for this environment"
        ))
    );

    // The definition's accessor and the state's field are the same record,
    // so there is one visibility, not a renderer copy and a gameplay copy.
    assert_eq!(
        environment.gameplay_visibility(),
        environment.state().gameplay_visibility()
    );

    // And the sight range is its own validated quantity: zero or negative
    // is not a representable sight range.
    assert!(GameplayVisibility::try_new(0.0).is_err());
    assert!(GameplayVisibility::try_new(-100.0).is_err());
    assert!(GameplayVisibility::try_new(f64::NAN).is_err());
    assert_eq!(
        GameplayVisibility::try_new(1500.0)
            .expect("a positive finite range is representable")
            .range_m(),
        1500.0
    );
}

/// F19 non-negotiable behavior 2: one authoritative wind field, exposed
/// through the state a timeline event replaces, plus a cosmetic stream that
/// is domain-separated from every other consumer.
#[test]
fn accept_f19_a_wind_is_one_authoritative_field_and_the_cosmetic_stream_is_separate() {
    let clear = common::clear();
    let storm = common::storm();

    // The definition's wind *is* the state's wind — one record, not two.
    assert_eq!(clear.wind(), clear.state().wind());
    assert_eq!(storm.wind(), storm.state().wind());
    assert!(clear.wind().is_known());
    assert_eq!(
        common::known_claim(clear.wind()),
        Some("f19a.fixture.clear-sky.wind")
    );

    // A timeline event replaces the state whole: the gust carries a
    // different wind and a different precipitation in the same single
    // record, so a consumer can never read a half-updated air.
    let events = storm.timeline().events();
    assert_eq!(events.len(), 2);
    let gust = &events[0];
    assert_eq!(gust.at_tick(), WIND_SHIFT_TICK);
    assert_ne!(
        gust.state().wind(),
        storm.wind(),
        "the authored gust really changes the wind"
    );
    assert_eq!(
        gust.state().gameplay_visibility(),
        storm.gameplay_visibility(),
        "the gust changes only what it authored; the visibility stays its explicit unknown"
    );

    // The cosmetic stream is deterministic in the root seed and
    // domain-separated from every other consumer.
    let root = 0x1234_5678_9ABC_DEF0;
    let cosmetic = CosmeticWeatherSeed::new(root).stream().next_u64();
    assert_eq!(
        CosmeticWeatherSeed::new(root).stream().next_u64(),
        cosmetic,
        "the cosmetic stream is a pure function of the root seed"
    );
    assert_ne!(
        CosmeticWeatherSeed::new(root ^ 1).stream().next_u64(),
        cosmetic,
        "another root seed draws another cosmetic stream"
    );
    assert_ne!(
        COSMETIC_WEATHER_DOMAIN, SYNTHETIC_BODY_DOMAIN,
        "the cosmetic weather stream owns its own domain constant"
    );
    assert_ne!(
        SplitMix64::for_domain(root, SYNTHETIC_BODY_DOMAIN).next_u64(),
        cosmetic,
        "drawing the cosmetic stream must not share values with another domain"
    );
}

/// The authored schedule is validated once: two events that are not
/// strictly ordered by tick are refused, so a replay cannot depend on the
/// order the list happened to arrive in.
#[test]
fn accept_f19_a_weather_timeline_rejects_unordered_events() {
    let state = EnvironmentState::new(
        common::known(
            WindField::try_new([1.0, 0.0, 0.0]).expect("finite wind"),
            "f19a.test.timeline.wind",
        ),
        cs_content::environment::PrecipitationDefinition::new(common::known(
            cs_content::environment::PrecipitationKind::Clear,
            "f19a.test.timeline.precipitation",
        )),
        common::unknown(
            "f19a.test.timeline.visibility",
            "the test authors no sight range",
        ),
    );
    let at_ten = WeatherEvent::new(10, state.clone());

    assert_eq!(
        EnvironmentTimeline::try_new(vec![at_ten.clone(), WeatherEvent::new(10, state.clone())]),
        Err(TimelineError::UnorderedTicks {
            index: 1,
            previous: 10,
            found: 10
        }),
        "two events on one tick have no defined order"
    );
    assert!(matches!(
        EnvironmentTimeline::try_new(vec![at_ten, WeatherEvent::new(9, state.clone())]),
        Err(TimelineError::UnorderedTicks {
            index: 1,
            previous: 10,
            found: 9
        })
    ));
    assert!(
        EnvironmentTimeline::try_new(vec![
            WeatherEvent::new(1, state.clone()),
            WeatherEvent::new(2, state),
        ])
        .is_ok(),
        "strictly increasing ticks are the valid schedule"
    );
}

/// Invalid values are refused at construction rather than normalized into
/// something representable: non-finite wind, a non-unit heading and a
/// negative ambient term never become records.
#[test]
fn accept_f19_a_invalid_environment_values_are_refused_at_construction() {
    assert!(matches!(
        WindField::try_new([f64::NAN, 0.0, 0.0]),
        Err(cs_content::environment::WindError::NonFinite { component: 0 })
    ));
    assert!(
        LightingDefinition::try_new(
            common::unknown("f19a.test.bad.sun", "unknown sun"),
            common::known([-1.0, 0.0, 0.0], "f19a.test.bad.ambient"),
        )
        .is_err()
    );
    assert!(
        FogDefinition::try_new(
            common::known(-0.5, "f19a.test.bad.density"),
            common::known([0.0, 0.0, 0.0], "f19a.test.bad.color"),
        )
        .is_err()
    );

    // A heading that is not perpendicular to the horizon is refused, so the
    // orientation a frame carries always implies a real basis.
    let up = common::unit([0.0, 1.0, 0.0]);
    let tilted = common::unit([0.6, 0.6, 0.529_150_3]);
    assert!(
        cs_content::environment::SkyOrientation::try_new(up, tilted).is_err(),
        "a heading pointing into the sky is not a horizon heading"
    );
}

/// The record fingerprint says what a definition *says*: equal content
/// hashes equal, a changed value changes it, and provenance is left out.
#[test]
fn accept_f19_a_environment_record_fingerprint_tracks_the_said_content() {
    let first = common::clear();
    let second = common::clear();
    assert_eq!(
        first.record_fingerprint(),
        second.record_fingerprint(),
        "the same authored content must fingerprint identically"
    );
    assert_ne!(
        first.record_fingerprint(),
        common::storm().record_fingerprint(),
        "different content must fingerprint differently"
    );

    // One field changes the digest: a different sun direction.
    let east = common::definition(
        EnvironmentProfile::Retail,
        common::authored_sky(),
        sun_lighting([1.0, 0.0, 0.0], "f19a.test.fingerprint.sun"),
    )
    .expect("the definition is valid");
    let zenith = common::definition(
        EnvironmentProfile::Retail,
        common::authored_sky(),
        sun_lighting([0.0, 1.0, 0.0], "f19a.test.fingerprint.sun"),
    )
    .expect("the definition is valid");
    assert_ne!(
        east.record_fingerprint(),
        zenith.record_fingerprint(),
        "a changed sun direction must change the digest"
    );

    // Provenance says where a record came from, not what it says: the same
    // values under a *different claim id* hash the same, because a known
    // value is hashed as its value alone. (An explicit unknown is hashed
    // with its claim and reason, because "unknown under this claim, for
    // this reason" is itself part of what the record says.)
    let other_claim = common::definition(
        EnvironmentProfile::Retail,
        common::authored_sky(),
        sun_lighting([1.0, 0.0, 0.0], "f19a.test.fingerprint.other-claim"),
    )
    .expect("the definition is valid");
    assert_eq!(
        east.record_fingerprint(),
        other_claim.record_fingerprint(),
        "the claim id of a known value is not part of what the record says"
    );
    assert_eq!(first.origin(), &Origin::SyntheticFixture);
}

/// The clear-sky fixture is exactly the record the other tests assume: an
/// authored texture under a retail profile with a diagnostic fallback, an
/// empty schedule and no generated sky anywhere.
#[test]
fn accept_f19_a_clear_sky_fixture_is_a_retail_diagnostic_record() {
    let environment = clear_sky_environment().expect("the fixture is well formed");
    assert_eq!(environment.id().as_str(), "fixture.clear-sky");
    assert_eq!(environment.profile(), EnvironmentProfile::Retail);
    match environment.sky().texture() {
        Resolved::Known(known) => assert_eq!(
            known.value.as_str(),
            format!("image/{CLEAR_SKY_TEXTURE_KEY}"),
            "the fixture authors its declared sky texture"
        ),
        Resolved::Unknown { .. } => panic!("the clear-sky fixture authors a sky texture"),
    }
    assert!(common::known_claim(environment.sky().texture()).is_some());
    assert!(matches!(
        environment.sky().fallback(),
        SkyFallback::Diagnostic
    ));
    assert!(
        environment.timeline().is_empty(),
        "a clear sky authors no weather change"
    );
}
