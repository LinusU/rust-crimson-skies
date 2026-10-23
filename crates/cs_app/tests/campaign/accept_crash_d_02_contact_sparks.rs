//! PARITY4-CRASH-D.02 (#1945): the sparks a terrain contact throws, from the
//! measured `yellow_sparks_follow` emitters (`pufftrails.zrd`).
//!
//! The retail test (`#[ignore = "requires CS_GAME_DIR"]`) reads the stored
//! emitters and their switch-off times and runs the production plugin with
//! them. The synthetic tests run the production plugin with newly authored
//! numbers of the same shape.

use std::path::PathBuf;

use bevy::asset::AssetPlugin;
use bevy::prelude::{App, FixedPostUpdate, MinimalPlugins, Vec3};
use cs_app::airframe_damage::AirframeDamageRegion;
use cs_app::mission_session::{
    ContactSpark, ContactSparkDefinition, ContactSparkReport, ContactSparkSource,
    ContactSparkSourceResource, MissionContactSparkPlugin, SparkEmitter, measure_contact_sparks,
    plan_contact_sparks, read_contact_spark_definition, spark_pose,
};
use cs_app::physics::PhysicsTickLedger;
use cs_app::playtest::fatal_contact::TerrainContactJudged;

fn authored_definition() -> ContactSparkDefinition {
    let emitter = |name: &str, number, off_after_s| SparkEmitter {
        name: name.into(),
        number,
        time_interval_s: 0.03,
        size: [0.01, 0.08],
        lifetime_s: [0.4, 0.7],
        min_velocity: [-5.0; 3],
        max_velocity: [5.0; 3],
        acceleration: [0.0, -9.8, 0.0],
        friction: 6.0,
        textures: vec!["a".into(), "b".into()],
        off_after_s,
    };
    ContactSparkDefinition {
        emitters: vec![emitter("long", 16, 0.25), emitter("short", 6, 0.15)],
    }
}

const HZ: f32 = 120.0;

fn app_with(definition: ContactSparkDefinition) -> App {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin::default()));
    app.insert_resource(ContactSparkSourceResource(ContactSparkSource {
        definition,
        images: Vec::new(),
        texture_archive: "synthetic".into(),
    }));
    app.add_plugins(MissionContactSparkPlugin);
    app.insert_resource(PhysicsTickLedger {
        ticks: 10,
        timestep_s: 1.0 / HZ,
        ..Default::default()
    });
    app.finish();
    app
}

fn publish(app: &mut App, tick: u64, point: Vec3, fatal: bool) {
    let aircraft = app.world_mut().spawn_empty().id();
    app.world_mut().write_message(TerrainContactJudged {
        tick,
        aircraft,
        point_m: point,
        normal: Vec3::Y,
        severity: 0.5,
        damage: 100.0,
        hull_damage: 100.0,
        wing: AirframeDamageRegion::LeftWing,
        fatal,
    });
}

fn sparks(app: &mut App) -> Vec<ContactSpark> {
    let mut query = app.world_mut().query::<&ContactSpark>();
    query.iter(app.world()).cloned().collect()
}

fn step_to(app: &mut App, ticks: u64) {
    app.world_mut().resource_mut::<PhysicsTickLedger>().ticks = ticks;
    app.world_mut().run_schedule(FixedPostUpdate);
}

fn check_contact_throws_then_releases(definition: ContactSparkDefinition) {
    let expected: usize = definition
        .emitters
        .iter()
        .map(|e| {
            let emissions = (0..)
                .take_while(|i| *i as f32 * e.time_interval_s < e.off_after_s - 1e-4)
                .count();
            emissions * e.number as usize
        })
        .sum();
    for fatal in [false, true] {
        let mut app = app_with(definition.clone());
        let point = Vec3::new(30.0, 2.0, -8.0);
        publish(&mut app, 10, point, fatal);
        step_to(&mut app, 10);
        let live = sparks(&mut app);
        assert_eq!(live.len(), expected, "fatal={fatal}");
        assert_eq!(
            app.world().resource::<ContactSparkReport>().sparks_spawned as usize,
            expected
        );
        for spark in &live {
            let emitter = &definition.emitters[spark.emitter];
            assert!(
                (emitter.size[0]..=emitter.size[1]).contains(&spark.size),
                "size {}",
                spark.size
            );
            let lo = (emitter.lifetime_s[0] * HZ).floor() as u64;
            let hi = (emitter.lifetime_s[1] * HZ).ceil() as u64;
            assert!(
                (lo..=hi).contains(&spark.life_ticks),
                "life {}",
                spark.life_ticks
            );
            assert_eq!(spark.origin, point);
            assert!(spark.velocity.abs().max_element() <= 5.0);
        }
        // The first emission is born with the contact; later ones wait.
        assert!(live.iter().any(|s| s.born == 10));
        assert!(live.iter().any(|s| s.born > 10));
        // Gone after the stored life of the last-born, longest-lived spark.
        let end = live.iter().map(|s| s.born + s.life_ticks).max().unwrap();
        step_to(&mut app, end - 1);
        assert!(
            !sparks(&mut app).is_empty(),
            "still alive just before the end"
        );
        step_to(&mut app, end);
        assert!(sparks(&mut app).is_empty(), "every spark expired");
    }
}

#[test]
fn accept_crash_d_02_a_published_contact_throws_the_stored_sparks_and_they_expire() {
    check_contact_throws_then_releases(authored_definition());
}

#[test]
fn accept_crash_d_02_sparks_fall_under_the_stored_acceleration_and_fade() {
    let planned = plan_contact_sparks(&authored_definition(), 10, Vec3::ZERO, HZ);
    let first = planned[0].clone();
    let (at_birth, opacity) = spark_pose(&first, 0, HZ);
    assert_eq!(at_birth, Vec3::ZERO);
    assert_eq!(opacity, 1.0);
    let mut still = first.clone();
    still.velocity = Vec3::ZERO;
    let (later, faded) = spark_pose(&still, still.life_ticks / 2, HZ);
    assert!(later.y < 0.0, "gravity pulls down: {later}");
    assert!(faded < 1.0);
    assert_eq!(
        planned,
        plan_contact_sparks(&authored_definition(), 10, Vec3::ZERO, HZ),
        "deterministic"
    );
}

#[test]
fn accept_crash_d_02_a_contact_without_a_definition_draws_nothing_and_says_so() {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin::default()));
    app.add_plugins(MissionContactSparkPlugin);
    app.insert_resource(PhysicsTickLedger::default());
    app.finish();
    publish(&mut app, 1, Vec3::ZERO, false);
    app.world_mut().run_schedule(FixedPostUpdate);
    let report = *app.world().resource::<ContactSparkReport>();
    assert_eq!(report.contacts_undrawn, 1);
    assert!(sparks(&mut app).is_empty());
}

fn game_dir() -> PathBuf {
    PathBuf::from(std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR must name the retail install"))
}

/// **Retail: the stored emitters, their switch-off times and textures.**
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_crash_d_02_retail_emitters_and_switch_offs() {
    let definition = read_contact_spark_definition(&game_dir()).expect("the emitters read");
    assert_eq!(definition.emitters.len(), 2);
    let trail = &definition.emitters[0];
    assert_eq!(trail.name, "trailpuffer2");
    assert_eq!(trail.number, 16);
    assert_eq!(trail.time_interval_s, 0.03);
    assert_eq!(trail.size, [0.01, 0.08]);
    assert_eq!(trail.lifetime_s, [0.4, 0.7]);
    assert_eq!(trail.min_velocity, [-5.0; 3]);
    assert_eq!(trail.max_velocity, [5.0; 3]);
    assert_eq!(trail.acceleration, [0.0, -9.8, 0.0]);
    assert_eq!(trail.friction, 6.0);
    assert_eq!(trail.textures, ["exp_yel01", "poleflare"]);
    assert_eq!(trail.off_after_s, 0.25);
    let chip = &definition.emitters[1];
    assert_eq!(chip.name, "chippuffer1");
    assert_eq!(chip.number, 6);
    assert_eq!(chip.time_interval_s, 0.03);
    assert_eq!(chip.size, [0.02, 0.06]);
    assert_eq!(chip.lifetime_s, [0.3, 0.7]);
    assert_eq!(chip.min_velocity, [-5.0; 3]);
    assert_eq!(chip.max_velocity, [5.0; 3]);
    assert_eq!(chip.acceleration, [0.0, -9.8, 0.0]);
    assert_eq!(chip.friction, 6.0);
    assert_eq!(chip.textures, ["smoke101", "smoke102", "smoke103"]);
    assert_eq!(chip.off_after_s, 0.15);
    let source = measure_contact_sparks(&game_dir(), "c3").expect("the textures decode");
    assert_eq!(source.images.len(), 5);
    // The production plugin with the retail numbers.
    check_contact_throws_then_releases(source.definition);
}
