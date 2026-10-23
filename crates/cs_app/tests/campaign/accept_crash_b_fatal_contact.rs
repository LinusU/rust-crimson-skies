//! PARITY4-CRASH-B (#1854): a terrain contact that empties the player's hull
//! pool ends the flight and raises a crash event; one that does not keeps
//! flying. The pool is the designed one (the original's zone totals are
//! unknown), the damage is the retail `crash` record's range.

use avian3d::prelude::{AngularVelocity, LinearVelocity, Position, RigidBody};
use bevy::prelude::{App, Entity, Messages, Vec3, With};
use cs_app::mission_session::{MISSION_TARGET_NODE, PlayerVictim, attach_player_victim};
use cs_app::playtest::fatal_contact::{
    FailureCause, FatalContactRule, MISSION_CRASH_PRODUCER, PlayerCrashed,
};
use cs_app::playtest::scene::PlaytestAircraft;
use cs_app::playtest::{PlaytestState, headless_app};
use cs_app::scene::SceneGeneration;
use cs_app::weapons::SessionDamageResolver;
use cs_sim::damage::{DamageChannel, DamageNodeKey, HitEvent};
use cs_types::Tick;
use cs_types::net::{ActorId, EventId, SessionId};

/// The retail `player.zrd` `health_damage_range`.
const RETAIL_RANGE: [f64; 2] = [50.0, 300.0];

fn dive_app(prior_damage: f64) -> (App, Entity, ActorId) {
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
    // Straight down onto the slab, a few metres up.
    let mut entity = app.world_mut().entity_mut(aircraft);
    entity.get_mut::<Position>().unwrap().0 = Vec3::new(0.0, 4.0, 0.0);
    entity.get_mut::<LinearVelocity>().unwrap().0 = Vec3::new(0.0, -100.0, 0.0);
    entity.get_mut::<AngularVelocity>().unwrap().0 = Vec3::ZERO;
    (app, aircraft, actor)
}

fn rule_of(app: &App, aircraft: Entity) -> FatalContactRule {
    *app.world()
        .get::<FatalContactRule>(aircraft)
        .expect("the rule")
}

fn remaining(app: &App, actor: ActorId) -> f64 {
    app.world()
        .resource::<SessionDamageResolver>()
        .resolver()
        .remaining_integrity(&actor, &DamageNodeKey::new(MISSION_TARGET_NODE).unwrap())
        .expect("the hull node")
}

#[test]
fn accept_crash_b_survivable_contact_keeps_flying() {
    // An oblique pass over the slab (severity about 0.12): the floor damage,
    // the designed pool survives it. A direct dive does not (#2005).
    let (mut app, aircraft, actor) = dive_app(0.0);
    let mut entity = app.world_mut().entity_mut(aircraft);
    entity.get_mut::<LinearVelocity>().unwrap().0 = Vec3::new(0.0, -12.0, -100.0);
    entity.get_mut::<avian3d::prelude::Rotation>().unwrap().0 =
        bevy::prelude::Quat::from_rotation_z(0.35);
    let before = remaining(&app, actor);
    for _ in 0..30 {
        app.update();
    }
    assert!(app.world().resource::<PlaytestState>().ground_contacts > 0);
    assert!(
        remaining(&app, actor) < before,
        "the contact did hull damage"
    );
    assert!(
        !app.world()
            .resource::<SessionDamageResolver>()
            .resolver()
            .is_destroyed(&actor)
    );
    assert!(rule_of(&app, aircraft).crash.is_none());
    assert_ne!(
        app.world().get::<RigidBody>(aircraft),
        Some(&RigidBody::Static)
    );
}

#[test]
fn accept_crash_b_fatal_contact_stops_the_flight_and_records_the_cause() {
    // A fresh hull: a head-on contact is fatal on first contact (#2005).
    let (mut app, aircraft, actor) = dive_app(0.0);
    let ticks_before = app
        .world()
        .resource::<cs_app::physics::PhysicsTickLedger>()
        .ticks;
    let mut raised = Vec::new();
    for _ in 0..30 {
        app.update();
        raised.extend(
            app.world()
                .resource::<Messages<PlayerCrashed>>()
                .iter_current_update_messages()
                .copied(),
        );
    }
    let crash = rule_of(&app, aircraft)
        .crash
        .expect("the fatal contact stamped the aircraft");
    assert!(crash.tick > ticks_before);
    assert!(crash.position_m[1] < 10.0, "crashed at the slab");
    let FailureCause::TerrainContact {
        severity, damage, ..
    } = crash.cause;
    assert!(severity > 0.9, "head-on, got {severity}");
    assert!((damage - 2.0 * 300.0 * severity.powi(3)).abs() < 1e-6);
    assert!(
        app.world()
            .resource::<SessionDamageResolver>()
            .resolver()
            .is_destroyed(&actor)
    );
    assert!(app.world().resource::<PlayerVictim>().actor == actor);

    // One event, carrying the same record.
    assert_eq!(raised.len(), 1);
    assert_eq!(raised[0].aircraft, aircraft);
    assert_eq!(raised[0].crash, crash);

    // The simulation is over: the pose no longer moves.
    let frozen = app.world().get::<Position>(aircraft).unwrap().0;
    for _ in 0..30 {
        app.update();
    }
    assert_eq!(app.world().get::<Position>(aircraft).unwrap().0, frozen);
    assert_eq!(
        app.world().get::<LinearVelocity>(aircraft).unwrap().0,
        Vec3::ZERO
    );
    assert_eq!(
        app.world().get::<RigidBody>(aircraft),
        Some(&RigidBody::Static)
    );
    assert_eq!(MISSION_CRASH_PRODUCER, 206);
}

/// The composed M01 player carries the retail `crash` record's damage range and
/// a damage victim, so the rule above is live in the mission, and a spawn that
/// touches nothing does not crash.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_crash_b_m01_player_carries_the_retail_rule() {
    use cs_app::mission_launch::plan_mission_launch;
    use cs_app::mission_session::{MissionPlayerBody, build_headless, stage_for, teardown};

    let root = std::path::PathBuf::from(std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR"));
    let plan = plan_mission_launch(
        &root,
        crate::campaign::common::label("M01"),
        "The Lost Treasure",
    )
    .expect("plan");
    let stage = stage_for(&root, &plan).expect("stage");
    let mut app = build_headless(&stage).expect("M01 composes");
    for _ in 0..5 {
        app.update();
    }
    let player = app
        .world_mut()
        .query_filtered::<Entity, With<MissionPlayerBody>>()
        .single(app.world())
        .expect("the player body");
    let rule = app
        .world()
        .get::<FatalContactRule>(player)
        .expect("the retail rule");
    assert_eq!(rule.health_damage_range, RETAIL_RANGE);
    assert!(rule.crash.is_none());
    assert!(app.world().get_resource::<PlayerVictim>().is_some());
    teardown(&mut app);
}
