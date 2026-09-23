//! Acceptance scenario F00-A: compile the pinned Bevy/Avian pair and create a
//! dynamic synthetic body (AC01), including its failure cases.
//!
//! These tests exercise production code only: `cs_app::synthetic::
//! SyntheticScene` builds the real Avian world, and `cs_types` validates the
//! typed input. Removing or neutering that implementation makes them fail.

use cs_app::synthetic::SyntheticScene;
use cs_types::{BodyKind, SceneProvenance, SpecError, SyntheticBodySpec, Tick};

/// A dynamic body must integrate under Avian's default downward gravity.
///
/// Observable failure if the implementation spawns anything but a dynamic
/// body (or never runs the physics schedule): the y position never drops.
#[test]
fn accept_f00_a_dynamic_synthetic_body_falls() {
    let spec = SyntheticBodySpec::falling_box(BodyKind::Dynamic);
    let start = spec.position_m;
    let mut scene = SyntheticScene::new(spec).expect("the fixture spec must build a scene");

    assert_eq!(
        scene.provenance(),
        SceneProvenance::Synthetic,
        "the dev scene must be explicitly marked SYNTHETIC"
    );

    scene.step(60);
    let sample = scene.sample();

    assert_eq!(
        sample.tick,
        Tick(60),
        "step(60) must end at exactly 60 simulation ticks"
    );
    assert!(
        sample.position_m[1] < start[1] - 1.0,
        "dynamic body must fall under gravity after 60 ticks: y = {}, start = {}",
        sample.position_m[1],
        start[1]
    );
    assert!(
        sample.linear_velocity_m_s[1] < -1.0,
        "falling body must accumulate downward velocity: {}",
        sample.linear_velocity_m_s[1]
    );
    assert!(
        (sample.position_m[0] - start[0]).abs() < 1e-5
            && (sample.position_m[2] - start[2]).abs() < 1e-5,
        "a body with no lateral forces must not drift sideways: {:?}",
        sample.position_m
    );
}

/// The counterpart failure case: a static spec must not integrate.
///
/// Observable failure if the implementation ignores `BodyKind` and always
/// spawns a dynamic body: this body would fall.
#[test]
fn accept_f00_a_static_synthetic_body_stays_put() {
    let spec = SyntheticBodySpec::falling_box(BodyKind::Static);
    let start = spec.position_m;
    let mut scene = SyntheticScene::new(spec).expect("the fixture spec must build a scene");

    scene.step(60);
    let sample = scene.sample();

    for (axis, (before, after)) in start.iter().zip(sample.position_m).enumerate() {
        assert!(
            (after - before).abs() < 1e-6,
            "static body moved on axis {axis}: {before} -> {after}"
        );
    }
    for component in sample.linear_velocity_m_s {
        assert!(
            component.abs() < 1e-6,
            "static body must not accumulate velocity: {:?}",
            sample.linear_velocity_m_s
        );
    }
}

/// Invalid typed input must be rejected with a structured error that names
/// the offending field, before any world is built.
#[test]
fn accept_f00_a_invalid_body_spec_is_rejected() {
    let mut zero_extent = SyntheticBodySpec::falling_box(BodyKind::Dynamic);
    zero_extent.half_extents_m[0] = 0.0;
    assert!(
        matches!(
            SyntheticScene::new(zero_extent),
            Err(cs_app::synthetic::SyntheticSceneError::InvalidBody(
                SpecError::NonPositiveHalfExtent { axis: "x" }
            ))
        ),
        "a zero half extent must be rejected with the offending axis"
    );

    let mut nan_position = SyntheticBodySpec::falling_box(BodyKind::Dynamic);
    nan_position.position_m[1] = f32::NAN;
    assert!(
        matches!(
            SyntheticScene::new(nan_position),
            Err(cs_app::synthetic::SyntheticSceneError::InvalidBody(
                SpecError::NonFinite {
                    field: "position_m[1]"
                }
            ))
        ),
        "a non-finite position must be rejected with the offending field"
    );
}
