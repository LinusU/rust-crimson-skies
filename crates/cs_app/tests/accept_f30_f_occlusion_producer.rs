//! F30-F: an occlusion producer binds to the crosshair query.
//!
//! The producer ([`produce_crosshair_occlusion`]) asks a sight oracle about
//! the session's eligible actors and hands the answer to the crosshair query
//! as a typed [`cs_app::targeting::OcclusionReport`]. These tests drive the
//! real producer, the real report and the real selection edge; the oracle is
//! the test's own geometry, since the physics oracle needs a physics world.

use std::collections::BTreeSet;

use bevy::ecs::world::World;
use cs_app::scene::SceneGeneration;
use cs_app::targeting::{
    TargetableBinding, TargetableState, TargetingError, TargetingSession, apply_selection_edges,
    lower_rules, lower_selection_actions, produce_crosshair_occlusion, sync_targetable_roster,
};
use cs_content::target_rules::{
    declared_synthetic_selection_actions, declared_synthetic_target_rules,
};
use cs_sim::damage::ActorId;
use cs_sim::targeting::{SelectionAction, SelectionFrame, TargetError};
use cs_types::Tick;
use cs_types::input::{Action, FlightCommand};
use cs_types::net::SessionId;
use cs_types::space::{UnitVec3, WorldPosition};

fn session_id(session: u64) -> SessionId {
    SessionId::new(session).expect("a nonzero session generation")
}

fn actor(session: u64, serial: u64) -> ActorId {
    ActorId {
        session: session_id(session),
        serial,
    }
}

fn pos(position: [f64; 3]) -> WorldPosition {
    WorldPosition::try_new(position).expect("the fixture position is finite")
}

/// A synced world for `session`: the player at the origin, a wingman at
/// x = 50 and a raider at x = 100, both on the +X ray.
fn world_for(session: u64) -> World {
    let generation = SceneGeneration::default().next();
    let declared = declared_synthetic_target_rules();
    let lowered = lower_rules(&declared).expect("the fixture rules lower");
    let bindings = lower_selection_actions(&declared_synthetic_selection_actions())
        .expect("the fixture actions lower");
    let mut world = World::new();
    let mut targeting = TargetingSession::new(
        session_id(session),
        lowered,
        bindings,
        declared.subject().clone(),
        generation,
    );
    targeting
        .bindings_mut()
        .bind(FlightCommand::DropOrdnance, SelectionAction::UnderCrosshair);
    world.insert_resource(targeting);
    let player = cs_sim::targeting::synthetic_player_faction();
    let raiders = cs_sim::targeting::synthetic_raider_faction();
    for (serial, faction, at) in [
        (1, player.clone(), [0.0, 0.0, 0.0]),
        (3, player, [50.0, 0.0, 0.0]),
        (9, raiders, [100.0, 0.0, 0.0]),
    ] {
        world.spawn((
            TargetableBinding {
                actor: actor(session, serial),
                rules: declared.subject().clone(),
                generation,
            },
            TargetableState::aircraft(faction, pos(at)),
        ));
    }
    sync_targetable_roster(&mut world);
    world
}

fn plus_x() -> UnitVec3 {
    UnitVec3::try_new([1.0, 0.0, 0.0]).expect("a unit direction")
}

fn select_under_crosshair(
    world: &mut World,
    ray: &cs_sim::targeting::CrosshairQuery,
    at: u64,
) -> Option<ActorId> {
    apply_selection_edges(
        world,
        actor(7, 1),
        &[Action::Flight(FlightCommand::DropOrdnance)],
        SelectionFrame::at(Tick(at)).with_crosshair(ray),
    )
    .expect("registered")
    .selection
}

/// The oracle's blocked actor is skipped and the one behind it is reached;
/// with nothing blocked the nearer actor wins, so the set is what changed it.
#[test]
fn accept_f30_f_blocked_actor_is_skipped_by_the_crosshair() {
    let mut world = world_for(7);
    let origin = pos([0.0, 0.0, 0.0]);

    let clear = produce_crosshair_occlusion(&world, actor(7, 1), origin, |_, _| false)
        .expect("the observer belongs to the session");
    assert!(clear.blocked().is_empty());
    assert_eq!(clear.tested(), 2, "the observer is never asked about");
    let ray = clear
        .crosshair_query(&world, plus_x(), None)
        .expect("the report is this session's");
    assert_eq!(
        select_under_crosshair(&mut world, &ray, 40),
        Some(actor(7, 3))
    );

    // Only the wingman's segment is obstructed.
    let report = produce_crosshair_occlusion(&world, actor(7, 1), origin, |_, to| to.x() < 75.0)
        .expect("the observer belongs to the session");
    assert_eq!(report.blocked(), &BTreeSet::from([actor(7, 3)]));
    let ray = report
        .crosshair_query(&world, plus_x(), None)
        .expect("the report is this session's");
    assert_eq!(ray.occluded, BTreeSet::from([actor(7, 3)]));
    assert_eq!(
        select_under_crosshair(&mut world, &ray, 41),
        Some(actor(7, 9))
    );
}

/// The oracle sees each eligible actor's own position and the report's
/// origin, so a physics oracle casts the segment the selection would use.
#[test]
fn accept_f30_f_oracle_is_asked_from_the_origin_to_each_actor() {
    let world = world_for(7);
    let origin = pos([1.0, 2.0, 3.0]);
    let mut asked = Vec::new();
    let report = produce_crosshair_occlusion(&world, actor(7, 1), origin, |from, to| {
        asked.push((from, to));
        false
    })
    .expect("the observer belongs to the session");
    assert_eq!(report.origin(), origin);
    assert_eq!(
        asked,
        vec![
            (origin, pos([50.0, 0.0, 0.0])),
            (origin, pos([100.0, 0.0, 0.0]))
        ]
    );
}

/// A producer for another session generation is refused, and so is a report
/// spent against another session's store.
#[test]
fn accept_f30_f_foreign_session_generation_is_refused() {
    let world = world_for(7);
    let origin = pos([0.0, 0.0, 0.0]);

    let foreign_observer = produce_crosshair_occlusion(&world, actor(8, 1), origin, |_, _| true);
    assert_eq!(
        foreign_observer.map(|_| ()),
        Err(TargetingError::Store(TargetError::ForeignSession {
            expected: 7,
            found: 8
        }))
    );

    let report = produce_crosshair_occlusion(&world, actor(7, 1), origin, |_, _| true)
        .expect("the observer belongs to the session");
    let other = world_for(8);
    assert_eq!(
        report.crosshair_query(&other, plus_x(), None).map(|_| ()),
        Err(TargetingError::Store(TargetError::ForeignSession {
            expected: 8,
            found: 7
        }))
    );
}

/// No session means no producer: refused, never an empty set.
#[test]
fn accept_f30_f_no_session_is_refused() {
    let world = World::new();
    let result = produce_crosshair_occlusion(&world, actor(7, 1), pos([0.0; 3]), |_, _| false);
    assert_eq!(result.map(|_| ()), Err(TargetingError::NoSession));
}

/// The mission's own crosshair runs the producer over the physics world: a
/// static wall on the camera's axis hides the wingman it stands in front of,
/// and the raider off to one side of the wall is still reached.
#[test]
fn accept_f30_f_mission_crosshair_reports_physics_occlusion() {
    use avian3d::prelude::{Collider, RigidBody};
    use bevy::camera::Camera3d;
    use bevy::prelude::{Transform, Vec3};
    use cs_app::mission_session::mission_crosshair;

    let mut app = cs_app::asset_stack::headless_app();
    // The wingman sits on the axis at z = -50, the raider 5.7 degrees off it
    // at z = -100: both inside the designed cone.
    let generation = SceneGeneration::default().next();
    let declared = declared_synthetic_target_rules();
    let lowered = lower_rules(&declared).expect("the fixture rules lower");
    let bindings = lower_selection_actions(&declared_synthetic_selection_actions())
        .expect("the fixture actions lower");
    app.insert_resource(TargetingSession::new(
        session_id(7),
        lowered,
        bindings,
        declared.subject().clone(),
        generation,
    ));
    let player = cs_sim::targeting::synthetic_player_faction();
    let raiders = cs_sim::targeting::synthetic_raider_faction();
    for (serial, faction, at) in [
        (1, player.clone(), [0.0, 0.0, 0.0]),
        (3, player, [0.0, 0.0, -50.0]),
        (9, raiders, [10.0, 0.0, -100.0]),
    ] {
        app.world_mut().spawn((
            TargetableBinding {
                actor: actor(7, serial),
                rules: declared.subject().clone(),
                generation,
            },
            TargetableState::aircraft(faction, pos(at)),
        ));
    }
    sync_targetable_roster(app.world_mut());
    app.world_mut().spawn((
        Camera3d::default(),
        Transform::from_xyz(0.0, 0.0, 0.0),
        bevy::transform::components::GlobalTransform::default(),
    ));
    app.finish();
    app.cleanup();
    for _ in 0..3 {
        app.update();
    }

    let open = mission_crosshair(app.world_mut(), actor(7, 1)).expect("a camera and a session");
    assert!(
        open.occluded.is_empty(),
        "no wall, nothing is reported blocked"
    );

    app.world_mut().spawn((
        RigidBody::Static,
        Collider::cuboid(2.0, 20.0, 1.0),
        Transform::from_translation(Vec3::new(0.0, 0.0, -25.0)),
    ));
    for _ in 0..3 {
        app.update();
    }
    let walled = mission_crosshair(app.world_mut(), actor(7, 1)).expect("a camera and a session");
    assert_eq!(
        walled.occluded,
        BTreeSet::from([actor(7, 3)]),
        "the wall hides only the wingman behind it"
    );
}
