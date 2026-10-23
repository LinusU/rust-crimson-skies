//! Acceptance PARITY-M01-PLAYER-DAMAGE-TRAIL (#1700): the sprite trail a
//! damaged player airframe leaves. Task test prefix:
//! `accept_parity_m01_player_damage_trail_`.
//!
//! The emitter numbers are the stored `dense_firetrail` (`pufftrails.zrd`);
//! the units, curves and gate are designed and recorded in
//! `docs/findings/2026-10-23-parity-m01-player-damage-trail.md`.

use std::collections::BTreeMap;

use bevy::asset::Assets;
use bevy::mesh::Mesh;
use bevy::prelude::{
    App, FixedPostUpdate, GlobalTransform, Handle, Image, Transform, Vec3, Visibility,
};
use cs_app::airframe_damage::{AirframeDamageRegion, RegionDamagePresentation, WingFlare};
use cs_app::mission_session::{
    DamageTrailArt, DamageTrailDefinition, DamageTrailReport, MissionDamageTrailPlugin,
    TrailEmitter, TrailKind, TrailPuff, colour_at, frame_at, plan_emissions,
};
use cs_app::physics::PhysicsTickLedger;
use cs_app::playtest::propeller::PropellerBlur;
use cs_app::scene::SceneGeneration;
use cs_sim::damage::{ActorId, PartState};
use cs_types::net::SessionId;

fn emitter(interval: f32, textures: &[&str], frames: &[(f32, &str)]) -> TrailEmitter {
    TrailEmitter {
        distance_interval: interval,
        size: [0.4, 0.8],
        lifetime_s: [1.0, 1.0],
        min_velocity: [-0.1; 3],
        max_velocity: [-0.1; 3],
        friction: 1.0,
        textures: textures.iter().map(|t| (*t).to_owned()).collect(),
        frames: frames.iter().map(|(t, n)| (*t, (*n).to_owned())).collect(),
        colors: vec![[0.0, 255.0, 0.0, 0.0, 1.0], [1.0, 0.0, 0.0, 0.0, 0.0]],
    }
}

fn trail_app() -> App {
    let mut app = App::new();
    app.add_plugins(bevy::asset::AssetPlugin::default());
    app.add_plugins(MissionDamageTrailPlugin);
    app.insert_resource(PhysicsTickLedger {
        timestep_s: 1.0 / 120.0,
        ..PhysicsTickLedger::default()
    });
    let definition = DamageTrailDefinition {
        smoke: emitter(1.0, &["smoke101", "smoke102"], &[]),
        fire: emitter(0.25, &[], &[(0.0, "fire_f01"), (0.5, "fire_f02")]),
    };
    let mesh = app
        .world_mut()
        .resource_mut::<Assets<Mesh>>()
        .add(bevy::prelude::Rectangle::new(1.0, 1.0));
    app.insert_resource(DamageTrailArt::from_handles(
        definition,
        BTreeMap::<String, Handle<Image>>::new(),
        mesh,
    ));
    app
}

fn actor() -> ActorId {
    ActorId {
        session: SessionId::new(9).expect("a nonzero session"),
        serial: 1,
    }
}

fn step(app: &mut App, anchor: Entity, x: f32) {
    let mut global = app
        .world_mut()
        .get_mut::<GlobalTransform>(anchor)
        .expect("the anchor");
    *global = GlobalTransform::from(Transform::from_xyz(x, 0.0, 0.0));
    app.world_mut().resource_mut::<PhysicsTickLedger>().ticks += 1;
    app.world_mut().run_schedule(FixedPostUpdate);
}

use bevy::prelude::Entity;

fn spawn_flare(app: &mut App, state: PartState) -> Entity {
    app.world_mut()
        .spawn((
            WingFlare {
                region: AirframeDamageRegion::LeftWing,
            },
            RegionDamagePresentation {
                actor: actor(),
                region: AirframeDamageRegion::LeftWing,
                state,
                generation: SceneGeneration(1),
            },
            Visibility::Hidden,
        ))
        .id()
}

#[test]
fn accept_parity_m01_player_damage_trail_emission_spacing_is_the_stored_distance() {
    let (points, carried) = plan_emissions(Vec3::ZERO, Vec3::new(2.6, 0.0, 0.0), 0.0, 1.0);
    assert_eq!(points.len(), 2);
    assert!((points[0].x - 1.0).abs() < 1e-5 && (points[1].x - 2.0).abs() < 1e-5);
    assert!((carried - 0.6).abs() < 1e-5);
    // The remainder completes the next interval: 0.6 + 0.4 lands one more.
    let (more, rest) = plan_emissions(Vec3::ZERO, Vec3::new(0.5, 0.0, 0.0), carried, 1.0);
    assert_eq!(more.len(), 1);
    assert!((more[0].x - 0.4).abs() < 1e-5 && (rest - 0.1).abs() < 1e-5);
    assert!(plan_emissions(Vec3::ZERO, Vec3::X, 0.0, 0.0).0.is_empty());
}

#[test]
fn accept_parity_m01_player_damage_trail_colour_and_frame_follow_the_stored_keys() {
    let keys = [[0.0, 255.0, 164.0, 90.0, 1.0], [1.0, 0.0, 0.0, 0.0, 0.0]];
    let start = colour_at(&keys, 0.0);
    assert!((start[1] - 164.0 / 255.0).abs() < 1e-6 && start[3] == 1.0);
    let mid = colour_at(&keys, 0.5);
    assert!((mid[0] - 0.5).abs() < 1e-6 && (mid[3] - 0.5).abs() < 1e-6);
    assert_eq!(colour_at(&keys, 2.0), [0.0; 4]);
    assert_eq!(colour_at(&[], 0.3), [1.0; 4]);
    let frames = [
        (0.0, "a".to_owned()),
        (0.05, "b".to_owned()),
        (0.15, "c".to_owned()),
    ];
    assert_eq!(frame_at(&frames, 0.0), Some("a"));
    assert_eq!(frame_at(&frames, 0.1), Some("b"));
    assert_eq!(frame_at(&frames, 9.0), Some("c"));
}

#[test]
fn accept_parity_m01_player_damage_trail_plays_only_while_a_wing_reports_damage() {
    let mut app = trail_app();
    let anchor = app
        .world_mut()
        .spawn((PropellerBlur, GlobalTransform::default()))
        .id();
    let flare = spawn_flare(&mut app, PartState::Intact);
    for i in 0..4u8 {
        step(&mut app, anchor, f32::from(i) * 2.0);
    }
    assert_eq!(
        *app.world().resource::<DamageTrailReport>(),
        DamageTrailReport::default()
    );

    app.world_mut()
        .get_mut::<RegionDamagePresentation>(flare)
        .expect("record")
        .state = PartState::Damaged;
    for i in 4..10u8 {
        step(&mut app, anchor, f32::from(i) * 2.0);
    }
    let report = *app.world().resource::<DamageTrailReport>();
    // Six segments of 2 m: smoke every 1 m, fire every 0.25 m.
    assert_eq!(report.smoke_spawned, 12);
    assert_eq!(report.fire_spawned, 48);
    let world = app.world_mut();
    let kinds: Vec<TrailKind> = world
        .query::<&TrailPuff>()
        .iter(world)
        .map(|p| p.kind)
        .collect();
    assert_eq!(kinds.len(), 60);
    assert!(kinds.contains(&TrailKind::Smoke) && kinds.contains(&TrailKind::Fire));

    // Repaired: no further puffs, and the live ones die at the stored life.
    app.world_mut()
        .get_mut::<RegionDamagePresentation>(flare)
        .expect("record")
        .state = PartState::Intact;
    for i in 10..20u8 {
        step(&mut app, anchor, f32::from(i) * 2.0);
    }
    assert_eq!(
        app.world().resource::<DamageTrailReport>().smoke_spawned,
        12
    );
    app.world_mut().resource_mut::<PhysicsTickLedger>().ticks += 121;
    app.world_mut().run_schedule(FixedPostUpdate);
    let world = app.world_mut();
    assert_eq!(world.query::<&TrailPuff>().iter(world).count(), 0);
}

/// Over real M01 data: the stored emitter numbers, nine decoded textures, and
/// a left-wing hit makes the player's prop1 anchor leave a trail.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_parity_m01_player_damage_trail_retail_dense_firetrail_is_measured_and_burns() {
    use cs_app::mission_launch::plan_mission_launch;
    use cs_app::mission_session::{
        MissionWeapons, build_headless, measure_damage_trail, stage_for,
    };
    use cs_app::weapons::SessionDamageResolver;
    use cs_content::campaign_bindings::MissionLabel;
    use cs_sim::damage::{DamageChannel, DamageNodeKey, HitEvent, HitEventId};
    use cs_types::Tick;

    let root =
        std::path::PathBuf::from(std::env::var_os("CS_GAME_DIR").expect("CS_GAME_DIR must be set"));
    let source = measure_damage_trail(&root, "c3").expect("the trail measures");
    let d = &source.definition;
    assert_eq!(d.smoke.distance_interval, 1.0);
    assert_eq!(d.smoke.size, [0.4, 0.8]);
    assert_eq!(d.smoke.lifetime_s, [3.5, 4.5]);
    assert_eq!(d.smoke.textures, ["smoke101", "smoke102", "smoke103"]);
    assert_eq!(d.smoke.colors.len(), 4);
    assert_eq!(d.fire.distance_interval, 0.25);
    assert_eq!(d.fire.lifetime_s, [0.5, 0.7]);
    assert_eq!(d.fire.frames.len(), 6);
    assert_eq!(d.fire.frames[5].1, "fire_f06");
    assert_eq!(source.images.len(), 9);

    let label = MissionLabel::new("M01").expect("label");
    let plan = plan_mission_launch(&root, label, "The Lost Treasure").expect("M01 plans");
    let stage = stage_for(&root, &plan).expect("M01's stage reads");
    assert!(stage.damage_trail.is_some());
    let mut app = build_headless(&stage).expect("M01 composes");
    app.update();
    app.update();
    let player = app.world().resource::<MissionWeapons>().player();
    assert_eq!(
        app.world().resource::<DamageTrailReport>().passes_burning,
        0
    );

    let event = HitEvent::try_new(
        HitEventId {
            session: player.session,
            tick: Tick(0),
            producer: 999,
            sequence: 0,
        },
        None,
        player,
        DamageNodeKey::new("leftwingdamage").expect("key"),
        DamageChannel::Internal,
        cs_app::mission_session::DESIGNED_REGION_INTEGRITY / 4.0,
    )
    .expect("a finite hit");
    app.world_mut()
        .resource_mut::<SessionDamageResolver>()
        .resolver_mut()
        .resolve(Tick(0), &[event])
        .expect("the hit resolves");
    for _ in 0..30 {
        app.update();
    }
    assert!(app.world().resource::<DamageTrailReport>().passes_burning > 0);
}
