//! PARITY4-CRASH-D.01 (#1944): every judged terrain contact publishes a
//! message and damages the wing on the contact's side; a fatal one also queues
//! the crash smoke and turns that wing zone `Destroyed` (the hud zone swap's
//! red pair). The zone selection of the original is unknown; the side rule is
//! designed.

use avian3d::prelude::{AngularVelocity, LinearVelocity, Position, Rotation};
use bevy::prelude::{App, Entity, Messages, Quat, Vec3, With};
use cs_app::airframe_damage::AirframeDamageRegion;
use cs_app::mission_session::{
    MISSION_TARGET_NODE, PendingOrdnanceBursts, attach_player_victim, wing_node,
};
use cs_app::playtest::fatal_contact::{FatalContactRule, PlayerCrashed, TerrainContactJudged};
use cs_app::playtest::headless_app;
use cs_app::playtest::scene::PlaytestAircraft;
use cs_app::scene::SceneGeneration;
use cs_app::weapons::SessionDamageResolver;
use cs_sim::damage::{DamageChannel, DamageNodeKey, HitEvent, PartState};
use cs_types::Tick;
use cs_types::net::{ActorId, EventId, SessionId};

const RETAIL_RANGE: [f64; 2] = [50.0, 300.0];

fn dive_app(prior_damage: f64, roll: f32, velocity: Vec3) -> (App, Entity, ActorId) {
    let mut app = headless_app();
    app.update();
    let aircraft = app
        .world_mut()
        .query_filtered::<Entity, With<PlaytestAircraft>>()
        .single(app.world())
        .expect("the playtest aircraft");
    let session = SessionId::new(7).expect("a session id");
    let actor = ActorId { session, serial: 1 };
    let victim = attach_player_victim(app.world_mut(), aircraft, actor, SceneGeneration(1))
        .expect("the player becomes a victim");
    app.world_mut().insert_resource(victim);
    app.world_mut().init_resource::<PendingOrdnanceBursts>();
    app.world_mut()
        .entity_mut(aircraft)
        .insert(FatalContactRule::new(RETAIL_RANGE));
    if prior_damage > 0.0 {
        let hit = HitEvent::try_new(
            EventId {
                session,
                tick: Tick(0),
                producer: 999,
                sequence: 0,
            },
            None,
            actor,
            DamageNodeKey::new(MISSION_TARGET_NODE).unwrap(),
            DamageChannel::Internal,
            prior_damage,
        )
        .unwrap();
        app.world_mut()
            .resource_mut::<SessionDamageResolver>()
            .resolver_mut()
            .resolve(Tick(0), &[hit])
            .expect("the prior hit applies");
    }
    let mut entity = app.world_mut().entity_mut(aircraft);
    entity.get_mut::<Position>().unwrap().0 = Vec3::new(0.0, 4.0, 0.0);
    entity.get_mut::<Rotation>().unwrap().0 = Quat::from_rotation_z(roll);
    entity.get_mut::<LinearVelocity>().unwrap().0 = velocity;
    entity.get_mut::<AngularVelocity>().unwrap().0 = Vec3::ZERO;
    (app, aircraft, actor)
}

fn run(app: &mut App) -> Vec<TerrainContactJudged> {
    let mut judged = Vec::new();
    for _ in 0..30 {
        app.update();
        judged.extend(
            app.world()
                .resource::<Messages<TerrainContactJudged>>()
                .iter_current_update_messages()
                .copied(),
        );
    }
    judged
}

fn state(app: &App, actor: ActorId, region: AirframeDamageRegion) -> PartState {
    app.world()
        .resource::<SessionDamageResolver>()
        .resolver()
        .part_state(&actor, &wing_node(region))
        .expect("the region node")
}

fn other(region: AirframeDamageRegion) -> AirframeDamageRegion {
    match region {
        AirframeDamageRegion::LeftWing => AirframeDamageRegion::RightWing,
        _ => AirframeDamageRegion::LeftWing,
    }
}

#[test]
fn accept_crash_d_01_light_contact_publishes_and_damages_only_the_contacted_wing() {
    let mut wings = Vec::new();
    for roll in [0.35_f32, -0.35] {
        let (mut app, aircraft, actor) = dive_app(0.0, roll, Vec3::new(0.0, -12.0, -100.0));
        let judged = run(&mut app);
        let first = *judged.first().expect("the contact was judged");
        assert_eq!(first.aircraft, aircraft);
        assert!(!first.fatal);
        assert!(first.severity > 0.0 && first.damage >= 2.0 * RETAIL_RANGE[0]);
        assert!(first.point_m.y < 10.0 && first.normal.y > 0.9);
        assert!(
            !app.world()
                .resource::<SessionDamageResolver>()
                .resolver()
                .is_destroyed(&actor)
        );
        assert!(app.world().resource::<Messages<PlayerCrashed>>().is_empty());
        assert!(app.world().resource::<PendingOrdnanceBursts>().0.is_empty());
        assert_ne!(state(&app, actor, first.wing), PartState::Intact);
        assert_eq!(
            state(&app, actor, other(first.wing)),
            PartState::Intact,
            "only the contacted side's wing is damaged"
        );
        wings.push(first.wing);
    }
    assert_ne!(wings[0], wings[1], "opposite rolls touch opposite sides");
}

#[test]
fn accept_crash_d_01_head_on_contact_destroys_wing_and_hull_and_queues_smoke() {
    let (mut app, aircraft, actor) = dive_app(0.0, 0.35, Vec3::new(0.0, -100.0, 0.0));
    let judged = run(&mut app);
    let fatal: Vec<_> = judged.iter().filter(|j| j.fatal).collect();
    assert_eq!(fatal.len(), 1);
    let fatal = fatal[0];
    assert!(fatal.severity > 0.9);
    assert_eq!(state(&app, actor, fatal.wing), PartState::Destroyed);
    assert_eq!(state(&app, actor, other(fatal.wing)), PartState::Intact);
    assert!(
        app.world()
            .resource::<SessionDamageResolver>()
            .resolver()
            .is_destroyed(&actor)
    );
    let crash = app
        .world()
        .get::<FatalContactRule>(aircraft)
        .unwrap()
        .crash
        .expect("crashed");
    let bursts = &app.world().resource::<PendingOrdnanceBursts>().0;
    assert_eq!(bursts.len(), 1);
    assert_eq!(bursts[0].position.to_array(), crash.position_m);
    assert_eq!(bursts[0].at, Tick(crash.tick));
}

/// A wing zone the damage resolver reports `Destroyed` is drawn with the red
/// `redhilite` / `red_hatchptrn` pair by the HUD art's zone swap (the existing
/// designed mapping); the other wing keeps its authored material.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_crash_d_01_destroyed_wing_zone_draws_the_red_pair_on_the_dial() {
    use bevy::prelude::{Assets, StandardMaterial};
    use cs_app::mission_session::{
        HudZonePiece, MissionHudArt, PlayerVictim, ZonePieceRole, build_headless, stage_for,
        teardown, update_hud_art,
    };

    let root = std::path::PathBuf::from(std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR"));
    let plan = cs_app::mission_launch::plan_mission_launch(
        &root,
        crate::campaign::common::label("M01"),
        "The Lost Treasure",
    )
    .expect("plan");
    let stage = stage_for(&root, &plan).expect("stage");
    let mut app = build_headless(&stage).expect("M01 composes");
    // The composition schedules the dial's drive only in a window; the
    // headless app runs the same production system.
    app.add_systems(bevy::prelude::Update, update_hud_art);
    for _ in 0..3 {
        app.update();
    }
    let actor = app.world().resource::<PlayerVictim>().actor;
    let hit = HitEvent::try_new(
        EventId {
            session: actor.session,
            tick: Tick(1),
            producer: 999,
            sequence: 0,
        },
        None,
        actor,
        wing_node(AirframeDamageRegion::LeftWing),
        DamageChannel::Internal,
        10_000.0,
    )
    .unwrap();
    app.world_mut()
        .resource_mut::<SessionDamageResolver>()
        .resolver_mut()
        .resolve(Tick(1), &[hit])
        .expect("the wing hit applies");
    assert_eq!(
        state(&app, actor, AirframeDamageRegion::LeftWing),
        PartState::Destroyed
    );
    for _ in 0..3 {
        app.update();
    }
    let world = app.world_mut();
    let pieces: Vec<(HudZonePiece, bevy::prelude::Handle<StandardMaterial>)> = world
        .query::<(
            &HudZonePiece,
            &bevy::prelude::MeshMaterial3d<StandardMaterial>,
        )>()
        .iter(world)
        .map(|(piece, material)| (piece.clone(), material.0.clone()))
        .collect();
    assert!(!pieces.is_empty(), "the dial carries zone pieces");
    let images = world.resource::<MissionHudArt>().images.clone();
    let materials = world.resource::<Assets<StandardMaterial>>();
    let mut red_pieces = 0;
    for (piece, handle) in pieces {
        let texture = materials
            .get(&handle)
            .and_then(|m| m.base_color_texture.clone());
        let red = match piece.role {
            ZonePieceRole::Hilite => "redhilite",
            ZonePieceRole::Hatch => "red_hatchptrn",
        };
        if piece.region == AirframeDamageRegion::LeftWing {
            assert_eq!(texture.as_ref(), images.get(red), "{:?}", piece.role);
            red_pieces += 1;
        } else {
            assert_ne!(texture.as_ref(), images.get(red), "{:?}", piece.region);
        }
    }
    assert!(red_pieces >= 1);
    teardown(&mut app);
}
