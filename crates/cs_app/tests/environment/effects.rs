//! `accept_f19_b_` tests for the sky/fog/light effects a frame may be drawn
//! from: F19 non-negotiable behaviors 1, 3 and 5 at the boundary.
//!
//! The production code under test is `cs_app::environment::effects`:
//! [`SkyEffect`], [`FogEffect`], [`LightEffect`], [`CloudLayerEffect`] and the
//! per-frame [`EnvironmentEffects`]. Every value they report comes from the
//! authored [`EnvironmentDefinition`](cs_content::environment::EnvironmentDefinition)
//! and the state its [`EnvironmentClock`] installed — the tests read what
//! production code produced and never resolve an effect themselves.
//!
//! No original data and no `CS_GAME_DIR`: the environments are the production
//! [`clear_sky_environment`](cs_app::environment::clear_sky_environment) and
//! [`storm_environment`](cs_app::environment::storm_environment) fixtures plus
//! definitions assembled through [`common::definition_with`].

use cs_app::environment::{
    CLEAR_SKY_TEXTURE_KEY, COSMETIC_PARTICLE_COUNT, CloudLayerEffect, EnvironmentClock,
    EnvironmentEffects, FogEffect, FogEffectError, LightEffect, STORM_RATE_HZ, SkyEffect, SkyFrame,
    WIND_SHIFT_TICK, clear_sky_environment,
};
use cs_app::origin::{OriginEpoch, SpatialAnchor, WorldOrigin};
use cs_content::environment::{
    CloudLayer, EnvironmentProfile, FogDefinition, LightingDefinition, PrecipitationKind, SkyArt,
    SkyFallback,
};
use cs_types::content::{ContentKind, Resolved};
use cs_types::space::UnitVec3;

use crate::common;

/// The storm fixture's clock, at the fixtures' fixed rate.
fn storm_clock() -> EnvironmentClock {
    EnvironmentClock::new(&common::storm(), common::tick_rate())
        .expect("the storm fixture's schedule is strictly increasing")
}

/// Advances `clock` to exactly `ticks`, one fixed frame per tick.
fn advance_to(clock: &mut EnvironmentClock, ticks: u64) {
    for _ in 0..ticks {
        clock
            .advance_frame(common::FRAME)
            .expect("the fixture run does not overflow the tick counter");
    }
    assert_eq!(clock.tick().0, ticks);
}

/// F19 non-negotiable behavior 5: the renderer draws the authored texture,
/// reports a missing one as a diagnostic, and may generate a sky **only** in
/// a run explicitly labeled synthetic/developer — never because a texture
/// happens to be absent.
#[test]
fn accept_f19_b_a_missing_sky_texture_is_a_diagnostic_and_only_a_labelled_run_generates_one() {
    // The clear-sky fixture authors a texture: the effect is that texture.
    let clear = clear_sky_environment().expect("the fixture is well formed");
    let effects = EnvironmentEffects::resolve(&clear, clear.state(), common::cosmetic_seed());
    let texture = effects
        .sky()
        .texture()
        .expect("the authored sky is drawable")
        .as_str();
    assert_eq!(texture, format!("image/{CLEAR_SKY_TEXTURE_KEY}"));
    assert!(
        !effects.sky().is_generated(),
        "an authored sky is never reported as generated"
    );
    assert_eq!(effects.sky().diagnostic(), None);

    // The storm fixture never found its texture: a diagnostic carrying the
    // claim and the reason, and no substitute.
    let storm = common::storm();
    let storm_effects = EnvironmentEffects::resolve(&storm, storm.state(), common::cosmetic_seed());
    assert_eq!(
        storm_effects.sky().diagnostic(),
        Some((
            "f19a.fixture.storm.sky-texture",
            "the storm fixture authors no sky texture"
        )),
        "a missing texture is reported, never replaced"
    );
    assert_eq!(storm_effects.sky().texture(), None);
    assert!(!storm_effects.sky().is_generated());

    // Labelling the *run* synthetic/developer is not on its own enough: the
    // storm record asked for a diagnostic fallback, so the profile's licence
    // is not the record's statement.
    assert_eq!(
        SkyEffect::resolve(storm.sky(), EnvironmentProfile::SyntheticDeveloper),
        *storm_effects.sky(),
        "a run label cannot override what the record asked for"
    );

    // A record that genuinely asks for a generated sky may generate one
    // under the synthetic/developer label — and the label travels in the
    // payload, so a renderer cannot lose track of what it is doing.
    let generated_sky = SkyArt::generated(EnvironmentProfile::SyntheticDeveloper)
        .expect("the synthetic/developer label may generate a sky");
    let retail = SkyEffect::resolve(&generated_sky, EnvironmentProfile::Retail);
    let synthetic = common::definition(
        EnvironmentProfile::SyntheticDeveloper,
        generated_sky,
        lighting_without_values(),
    )
    .expect("a generated sky is legal under the synthetic/developer profile");
    let synthetic_effects =
        EnvironmentEffects::resolve(&synthetic, synthetic.state(), common::cosmetic_seed());
    assert!(
        synthetic_effects.sky().is_generated(),
        "the record asked for a generated sky in a labelled run"
    );
    assert_eq!(
        synthetic_effects.sky(),
        &SkyEffect::Generated {
            profile: EnvironmentProfile::SyntheticDeveloper
        },
        "the generated sky carries the profile it was generated for"
    );
    assert_eq!(
        synthetic_effects.profile(),
        EnvironmentProfile::SyntheticDeveloper,
        "the profile travels with the frame"
    );

    // The same record asked about under a retail run: still a diagnostic, and
    // never a generated sky.
    assert!(
        !retail.is_generated(),
        "no run of the retail profile may generate a sky"
    );
    assert!(matches!(retail, SkyEffect::MissingDiagnostic { .. }));

    // A record that carries a *known* texture always draws it, whatever the
    // run is labeled: the fallback only exists because a texture is missing.
    let authored = common::authored_sky();
    assert!(matches!(
        SkyEffect::resolve(&authored, EnvironmentProfile::Retail),
        SkyEffect::Texture { .. }
    ));
    assert!(matches!(
        SkyEffect::resolve(&authored, EnvironmentProfile::SyntheticDeveloper),
        SkyEffect::Texture { .. }
    ));
    assert!(
        SkyArt::try_new(
            authored.texture().clone(),
            SkyFallback::Generated {
                profile: EnvironmentProfile::SyntheticDeveloper
            },
        )
        .is_err(),
        "a known texture with a generated fallback contradicts itself"
    );
}

/// F19 non-negotiable behavior 1: fog fades a frame and is never a sight
/// range. The authored default is tagged designed, the transmittance is a
/// function of the authored density, and an unknown field is refused by name
/// rather than read as "no fog".
#[test]
fn accept_f19_b_fog_fades_the_frame_and_never_becomes_a_sight_range() {
    let clear = clear_sky_environment().expect("the fixture is well formed");
    let fog = FogEffect::resolve(clear.fog());
    let density = match fog.density_per_m() {
        Resolved::Known(known) => known.value,
        Resolved::Unknown { .. } => panic!("the clear-sky fixture authors a designed fog density"),
    };
    assert!(
        density > 0.0,
        "the fixture's designed default is a real density, not zero"
    );

    // At the camera nothing is faded; the further away, the more of the
    // surface's own light the fog replaces.
    assert_eq!(fog.transmittance_at(0.0).expect("0 m is finite"), 1.0);
    let near = fog.transmittance_at(100.0).expect("100 m is finite");
    let far = fog.transmittance_at(5000.0).expect("5000 m is finite");
    assert!(
        near < 1.0 && far < near && far > 0.0,
        "transmittance must fall with distance and stay inside (0, 1): near {near}, far {far}"
    );
    // And it is the authored exponential, not an arbitrary falloff: doubling the
    // distance squares the transmittance, which a linear or clamped fade would
    // not do.
    let double = fog.transmittance_at(200.0).expect("200 m is finite");
    assert!(
        (double - near * near).abs() < 1e-12,
        "transmittance must be exponential in distance: double {double}, near^2 {}",
        near * near
    );

    // The fade lands on the fog colour: `surface * T + color * (1 - T)`, so a
    // black surface *is* the fog colour scaled by how much of the fog the
    // camera still sees through. At the camera nothing is fogged.
    let authored_color = match fog.color_linear() {
        Resolved::Known(known) => known.value,
        Resolved::Unknown { .. } => panic!("the fixture authors a fog colour"),
    };
    for distance_m in [100.0, 5000.0] {
        let transmittance = fog.transmittance_at(distance_m).expect("finite");
        let fogged = fog
            .fade_toward([0.0, 0.0, 0.0], distance_m)
            .expect("both fields are known");
        for component in 0..3 {
            assert!(
                (fogged[component] - authored_color[component] * (1.0 - transmittance)).abs()
                    < 1e-12,
                "the fade must be the authored transmittance of the authored colour"
            );
        }
    }
    // Far enough out the fog *is* the colour: at 100 km this density leaves
    // under 0.001% of the surface showing through.
    let horizon = fog
        .fade_toward([0.0, 0.0, 0.0], 100_000.0)
        .expect("both fields are known");
    for component in 0..3 {
        assert!(
            (horizon[component] - authored_color[component]).abs() < 0.01,
            "a distant surface must converge on the authored fog colour: {horizon:?} vs {authored_color:?}"
        );
    }
    assert_eq!(
        fog.fade_toward([0.5, 0.5, 0.5], 0.0)
            .expect("both fields are known"),
        [0.5, 0.5, 0.5],
        "at the camera the fog must leave the surface alone"
    );

    // The storm authors a denser fog: at the same distance it fades more.
    let storm_fog = FogEffect::resolve(common::storm().fog());
    assert!(
        storm_fog.transmittance_at(1000.0).expect("finite")
            < fog.transmittance_at(1000.0).expect("finite"),
        "the denser authored fog must fade a 1000 m surface further"
    );

    // Gameplay visibility is untouched by any of it: the fog record has no
    // sight range and the state's own authored visibility is what an actor
    // is detected with. A denser fog never shortens it.
    assert!(!clear.gameplay_visibility().is_known());
    let storm = common::storm();
    let downpour = &storm.timeline().events()[1].state();
    let visibility_range_m = match downpour.gameplay_visibility() {
        Resolved::Known(known) => known.value.range_m(),
        Resolved::Unknown { .. } => panic!("the storm authors a sight range at its last event"),
    };
    assert_eq!(visibility_range_m, 400.0);
    assert_ne!(
        fog.transmittance_at(visibility_range_m).expect("finite"),
        1.0,
        "the screen is already faded at the authored sight range, yet the sight range is its own value"
    );

    // An unknown fog field is refused, naming its claim: "no evidence of fog"
    // is not "no fog", and an unfogged world must not be drawn by default.
    let unknown = FogDefinition::unknown("f19b.test.unknown-fog", "this test authors no fog")
        .expect("the claim id and reason are valid");
    let unknown_fog = FogEffect::resolve(&unknown);
    match unknown_fog.transmittance_at(100.0) {
        Err(FogEffectError::DensityUnknown { claim_id, reason }) => {
            assert_eq!(claim_id.as_str(), "f19b.test.unknown-fog");
            assert_eq!(reason, "this test authors no fog");
        }
        other => panic!("an unknown fog density must be refused, got {other:?}"),
    }
    assert!(matches!(
        unknown_fog.fade_toward([1.0, 1.0, 1.0], 100.0),
        Err(FogEffectError::DensityUnknown { .. })
    ));

    // A known density with an unknown colour can still fade, but cannot tint:
    // half a fog is reported as half, never completed with a default colour.
    let half = FogEffect::resolve(
        &FogDefinition::try_new(
            common::known(2e-4, "f19b.test.half-fog-density"),
            common::unknown(
                "f19b.test.half-fog-color",
                "this test authors no fog colour",
            ),
        )
        .expect("a known density needs no range check"),
    );
    assert!(half.transmittance_at(100.0).is_ok());
    match half.fade_toward([1.0, 1.0, 1.0], 100.0) {
        Err(FogEffectError::ColorUnknown { claim_id, .. }) => {
            assert_eq!(claim_id.as_str(), "f19b.test.half-fog-color");
        }
        other => panic!("a tint without an authored colour must be refused, got {other:?}"),
    }

    // A non-finite or negative distance is refused rather than faded through.
    assert!(matches!(
        fog.transmittance_at(f64::NAN),
        Err(FogEffectError::NonFiniteDistance { .. })
    ));
    assert!(matches!(
        fog.transmittance_at(-1.0),
        Err(FogEffectError::NonFiniteDistance { distance_m: -1.0 })
    ));
}

/// F19 non-negotiable behaviors 1 and 3 at the light rig: the authored sun
/// and ambient are used verbatim, an unknown sun stays unknown, and half a rig
/// is refused instead of completed with a default.
#[test]
fn accept_f19_b_light_uses_the_authored_sun_and_never_invents_one() {
    let clear = clear_sky_environment().expect("the fixture is well formed");
    let light = EnvironmentEffects::resolve(&clear, clear.state(), common::cosmetic_seed())
        .light()
        .clone();
    let rig = light
        .rig()
        .expect("the clear-sky fixture authors a full rig");
    assert_eq!(rig.direction, common::unit([0.6, 0.8, 0.0]));
    assert_eq!(rig.ambient_linear, [0.2, 0.2, 0.25]);

    // The sun direction is the authored one, verbatim: it is a *world*
    // direction, so nothing about a rebase or a camera may move it. It is
    // exactly what the sky frame carries for the same environment, captured
    // from a real camera anchor.
    let origin = WorldOrigin::new(OriginEpoch(0), common::world([0.0, 0.0, 0.0]));
    let camera = SpatialAnchor::new(&origin, common::world([1000.0, 500.0, -2000.0]))
        .expect("a finite world position anchors");
    let frame = SkyFrame::capture(&clear, &camera);
    assert_eq!(
        frame.known_sun_direction(),
        Some(rig.direction),
        "the light rig and the sky frame must carry one sun direction"
    );

    // The storm authors no sun at all: no rig, and the unknown is reported
    // rather than replaced by a convenient direction.
    let storm = common::storm();
    let storm_light = EnvironmentEffects::resolve(&storm, storm.state(), common::cosmetic_seed())
        .light()
        .clone();
    assert_eq!(
        common::unknown_claim(storm_light.sun_direction()),
        Some((
            "f19a.fixture.storm.sun",
            "the storm fixture authors no sun direction"
        ))
    );
    assert_eq!(
        storm_light.rig(),
        None,
        "an environment with no authored sun must not be lit by a default"
    );

    // Half a rig is also no rig: a measured sun with an unmeasured ambient
    // term would otherwise be completed with a guess.
    let half = lighting_with(
        common::known(common::unit([0.0, 1.0, 0.0]), "f19b.test.half-sun"),
        common::unknown(
            "f19b.test.half-ambient",
            "this test authors no ambient term",
        ),
    )
    .expect("a known sun with an unknown ambient is a legal record");
    assert!(half.sun_direction().is_known());
    assert_eq!(
        LightEffect::resolve(&half).rig(),
        None,
        "a rig needs both halves; a missing ambient is not filled in"
    );

    // The other half: an authored ambient with no sun is equally incomplete.
    let no_sun = lighting_with(
        common::unknown("f19b.test.no-sun", "this test authors no sun"),
        common::known([0.1, 0.1, 0.1], "f19b.test.ambient-only"),
    )
    .expect("an unknown sun with a known ambient is a legal record");
    assert_eq!(LightEffect::resolve(&no_sun).rig(), None);

    // A negative ambient term is still refused at the record, so no rig can
    // carry one.
    assert!(
        lighting_with(
            common::known(common::unit([0.0, 1.0, 0.0]), "f19b.test.bad-sun"),
            common::known([-1.0, 0.0, 0.0], "f19b.test.bad-ambient"),
        )
        .is_none(),
        "a negative ambient component is not a representable rig"
    );
}

/// Lighting with no authored values at all: both fields explicitly unknown.
fn lighting_without_values() -> LightingDefinition {
    LightingDefinition::try_new(
        common::unknown("f19b.test.sun", "this test authors no sun"),
        common::unknown("f19b.test.ambient", "this test authors no ambient"),
    )
    .expect("two unknown values need no range check")
}

/// A [`LightingDefinition`] from two independently resolved fields, or `None`
/// when the pair is not representable.
fn lighting_with(
    sun: Resolved<UnitVec3>,
    ambient: Resolved<[f64; 3]>,
) -> Option<LightingDefinition> {
    LightingDefinition::try_new(sun, ambient).ok()
}

/// The per-frame record follows the weather **timeline**, not the
/// definition's initial state: the storm draws clear skies' worth of nothing
/// until its authored gust installs rain, and only the effects the record
/// changes change.
#[test]
fn accept_f19_b_frame_effects_follow_the_weather_timeline_and_not_the_definition() {
    let storm = common::storm();
    let mut clock = storm_clock();

    // Before the gust: the authored initial state, clear.
    let before = EnvironmentEffects::from_clock(&storm, &clock, common::cosmetic_seed());
    assert!(
        !before.precipitation().is_decorated(),
        "the storm's initial state authors a clear sky, so nothing is drawn"
    );
    assert_eq!(before.precipitation().field(), None);

    advance_to(&mut clock, WIND_SHIFT_TICK);
    let after = EnvironmentEffects::from_clock(&storm, &clock, common::cosmetic_seed());
    assert!(
        after.precipitation().is_decorated(),
        "the installed gust authors rain, so the frame now decorates"
    );
    let field = after
        .precipitation()
        .field()
        .expect("the installed state authors a particle field");
    assert_eq!(
        field.kind(),
        PrecipitationKind::Rain,
        "the field decorates the kind the installed event authored"
    );
    assert_eq!(field.particles().len(), COSMETIC_PARTICLE_COUNT);

    // The definition's authored presentation is unaffected by the weather
    // state: the sky diagnostic, the fog and the (absent) sun are the same
    // records in both frames, so a gust cannot quietly change the art.
    assert_eq!(after.sky(), before.sky());
    assert_eq!(after.fog(), before.fog());
    assert_eq!(after.light(), before.light());

    // Frame rate does not matter: the effects read the state the clock
    // installed at its last committed tick, so a coarse frame split reaches
    // the same frame content at the same tick.
    let mut coarse = storm_clock();
    for _ in 0..(WIND_SHIFT_TICK / 4) {
        coarse
            .advance_frame(common::FRAME * 4)
            .expect("the fixture run does not overflow the tick counter");
    }
    assert_eq!(coarse.tick().0, WIND_SHIFT_TICK);
    assert_eq!(
        EnvironmentEffects::from_clock(&storm, &coarse, common::cosmetic_seed()),
        after,
        "the same committed tick must produce the same frame content"
    );

    assert_eq!(STORM_RATE_HZ, 64);
}

/// Cloud layers carry their authored coverage, and a layer whose coverage
/// nobody measured is drawn at no coverage rather than at full opacity.
#[test]
fn accept_f19_b_cloud_layers_keep_their_own_coverage_and_never_invent_one() {
    let clear = clear_sky_environment().expect("the fixture is well formed");
    let effects = EnvironmentEffects::resolve(&clear, clear.state(), common::cosmetic_seed());
    let layers = effects.cloud_layers();
    assert_eq!(layers.len(), 1, "the fixture authors one cloud layer");
    assert_eq!(layers[0].altitude_m(), 1500.0);
    assert_eq!(layers[0].coverage(), Some(0.25));

    // The storm authors no layer at all: no layer is invented.
    let storm = common::storm();
    assert!(
        EnvironmentEffects::from_clock(&storm, &storm_clock(), common::cosmetic_seed())
            .cloud_layers()
            .is_empty()
    );

    // A layer whose coverage is unknown reports no coverage. It is still a
    // legal record with an altitude — the unknown is per field.
    let unknown_layer = CloudLayer::try_new(
        900.0,
        common::unknown("f19b.test.coverage", "this test authors no coverage"),
    )
    .expect("an unknown coverage needs no range check");
    let effect =
        CloudLayerEffect::new(unknown_layer.altitude_m(), unknown_layer.coverage().clone());
    assert_eq!(effect.altitude_m(), 900.0);
    assert_eq!(
        effect.coverage(),
        None,
        "an unknown coverage must not become full opacity"
    );

    // And a coverage outside [0, 1] is still refused at the record.
    assert!(CloudLayer::try_new(1000.0, common::known(1.5, "f19b.test.bad-coverage")).is_err());
    assert!(CloudLayer::try_new(-1.0, common::known(0.5, "f19b.test.bad-altitude")).is_err());

    // The texture ids the effects carry are image ids, so a renderer cannot be
    // handed a mission id to load as a sky.
    let clear_effect = EnvironmentEffects::resolve(&clear, clear.state(), common::cosmetic_seed());
    let texture = clear_effect
        .sky()
        .texture()
        .expect("the authored sky is drawable")
        .clone();
    assert_eq!(texture.kind(), ContentKind::Image);
}
