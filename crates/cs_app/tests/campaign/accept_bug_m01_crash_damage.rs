//! BUG-M01-TERRAIN-CRASH-DAMAGE (#2005): a direct terrain impact by an
//! undamaged player is fatal on first contact, a glancing one is survivable.
//!
//! The lethal threshold is a *designed* calibration of the unrecovered zone
//! health (`fatal_contact::DESIGNED_LETHAL_CRASH_DAMAGE`), not a measurement;
//! the severity `s` and the retail `[50, 300]` range are the original's.

use avian3d::prelude::{AngularVelocity, LinearVelocity, Position, Rotation};
use bevy::prelude::{App, Entity, Messages, Quat, Vec3, With};
use cs_app::mission_session::{
    DESIGNED_PLAYER_INTEGRITY, MISSION_TARGET_NODE, attach_player_victim,
};
use cs_app::playtest::fatal_contact::{
    FatalContactRule, PlayerCrashed, TerrainContactJudged, crash_hull_damage,
};
use cs_app::playtest::headless_app;
use cs_app::playtest::scene::PlaytestAircraft;
use cs_app::scene::SceneGeneration;
use cs_app::weapons::SessionDamageResolver;
use cs_sim::contact_response::crash_damage;
use cs_sim::damage::DamageNodeKey;
use cs_types::net::{ActorId, SessionId};

const RETAIL_RANGE: [f64; 2] = [50.0, 300.0];

fn flight(velocity: Vec3, roll: f32) -> (App, Entity, ActorId) {
    let mut app = headless_app();
    app.update();
    let aircraft = app
        .world_mut()
        .query_filtered::<Entity, With<PlaytestAircraft>>()
        .single(app.world())
        .expect("the playtest aircraft");
    let actor = ActorId {
        session: SessionId::new(7).expect("a session id"),
        serial: 1,
    };
    let victim = attach_player_victim(app.world_mut(), aircraft, actor, SceneGeneration(1))
        .expect("the player becomes a victim");
    app.world_mut().insert_resource(victim);
    let mut entity = app.world_mut().entity_mut(aircraft);
    entity.insert(FatalContactRule::new(RETAIL_RANGE));
    entity.get_mut::<Position>().unwrap().0 = Vec3::new(0.0, 4.0, 0.0);
    entity.get_mut::<Rotation>().unwrap().0 = Quat::from_rotation_z(roll);
    entity.get_mut::<LinearVelocity>().unwrap().0 = velocity;
    entity.get_mut::<AngularVelocity>().unwrap().0 = Vec3::ZERO;
    (app, aircraft, actor)
}

fn run(app: &mut App, ticks: usize) -> (Vec<TerrainContactJudged>, usize) {
    let (mut judged, mut crashed) = (Vec::new(), 0);
    for _ in 0..ticks {
        app.update();
        judged.extend(
            app.world()
                .resource::<Messages<TerrainContactJudged>>()
                .iter_current_update_messages()
                .copied(),
        );
        crashed += app
            .world()
            .resource::<Messages<PlayerCrashed>>()
            .iter_current_update_messages()
            .count();
    }
    (judged, crashed)
}

fn hull(app: &App, actor: ActorId) -> f64 {
    app.world()
        .resource::<SessionDamageResolver>()
        .resolver()
        .remaining_integrity(&actor, &DamageNodeKey::new(MISSION_TARGET_NODE).unwrap())
        .expect("the hull node")
}

#[test]
fn accept_bug_m01_crash_damage_head_on_is_fatal_on_first_contact() {
    let (mut app, aircraft, actor) = flight(Vec3::new(0.0, -100.0, 0.0), 0.0);
    assert_eq!(hull(&app, actor), DESIGNED_PLAYER_INTEGRITY, "a fresh hull");
    let (judged, crashed) = run(&mut app, 60);
    // One episode, counted once, fatal, and nothing after the crash.
    assert_eq!(judged.len(), 1, "{judged:?}");
    let first = judged[0];
    assert!(first.fatal && first.severity > 0.9, "{first:?}");
    assert!(first.hull_damage >= DESIGNED_PLAYER_INTEGRITY);
    assert_eq!(crashed, 1, "exactly one PlayerCrashed");
    assert_eq!(hull(&app, actor), 0.0);
    assert!(
        app.world()
            .get::<FatalContactRule>(aircraft)
            .unwrap()
            .crash
            .is_some()
    );
}

#[test]
fn accept_bug_m01_crash_damage_oblique_contact_is_survivable() {
    // About seven degrees to the surface: severity near 0.12.
    let (mut app, _, actor) = flight(Vec3::new(0.0, -12.0, -100.0), 0.35);
    let (judged, crashed) = run(&mut app, 60);
    let first = *judged.first().expect("the contact was judged");
    assert!(first.severity > 0.0 && first.severity < 0.3, "{first:?}");
    assert!(!first.fatal);
    assert_eq!(crashed, 0);
    assert!(hull(&app, actor) > 0.8 * DESIGNED_PLAYER_INTEGRITY);
}

#[test]
fn accept_bug_m01_crash_damage_curve_separates_skims_glances_and_strikes() {
    let raw = |s: f64| crash_damage(RETAIL_RANGE, s, true);
    let hull = |s: f64| crash_hull_damage(raw(s), DESIGNED_PLAYER_INTEGRITY);
    // The sea skim and the island graze of the pinned M01 no-input flight
    // (severities 0..0.41): several of them are survivable.
    for s in [0.0, 0.05, 0.316, 0.41] {
        assert!(5.0 * hull(s) < DESIGNED_PLAYER_INTEGRITY, "s={s}");
    }
    // A strike 45 degrees off the normal is heavy but takes a second hit.
    assert!(hull(0.71) > 0.3 * DESIGNED_PLAYER_INTEGRITY);
    assert!(hull(0.71) < DESIGNED_PLAYER_INTEGRITY);
    assert!(2.0 * hull(0.71) >= DESIGNED_PLAYER_INTEGRITY);
    // Anything near head-on is fatal alone.
    for s in [0.8, 0.9, 1.0] {
        assert!(hull(s) >= DESIGNED_PLAYER_INTEGRITY - 1e-9, "s={s}");
    }
}
