//! #1814: the grouped M01 scenery (mountains, cliffs, land below the world
//! record's grouping nodes) is solid. Retail tests: they compose the real M01.
use std::path::PathBuf;

use bevy::ecs::message::{MessageCursor, Messages};
use bevy::prelude::*;
use cs_app::mission_launch::plan_mission_launch;
use cs_app::mission_session::{
    MissionPlayerBody, MissionWorldParts, WorldSceneryCollider, build_headless, stage_for, teardown,
};
use cs_app::playtest::fatal_contact::TerrainContactJudged;

use crate::campaign::common::label;

/// The `shipwreck` node's world anchor, metres
/// (`docs/findings/2026-10-21-m01-objective-target-points.md`).
const SHIPWRECK: Vec3 = Vec3::new(-8891.4, 5.0, -4685.5);

fn compose() -> bevy::app::App {
    let root = PathBuf::from(std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR"));
    let plan = plan_mission_launch(&root, label("M01"), "The Lost Treasure").expect("plan");
    let stage = stage_for(&root, &plan).expect("stage");
    let mut app = build_headless(&stage).expect("M01 composes");
    for _ in 0..3 {
        app.update();
    }
    app
}

/// Every solid scenery triangle in world space, with the node slot and name.
fn solid_triangles(app: &bevy::app::App) -> Vec<([Vec3; 3], u32, String)> {
    use cs_app::playtest_retail::CONTACT_SWEEP_NODE_FLAGS;
    let scenery = app
        .world()
        .resource::<MissionWorldParts>()
        .scenery()
        .expect("scenery");
    let mut out = Vec::new();
    for p in &scenery.placements {
        if p.node_flags & CONTACT_SWEEP_NODE_FLAGS != CONTACT_SWEEP_NODE_FLAGS {
            continue;
        }
        let Some(c) = scenery.colliders.get(&p.mesh) else {
            continue;
        };
        let v: Vec<Vec3> = c
            .vertices()
            .iter()
            .map(|v| p.global.transform_point(Vec3::from(*v)))
            .collect();
        for t in c.indices() {
            out.push((
                [v[t[0] as usize], v[t[1] as usize], v[t[2] as usize]],
                p.node_slot,
                p.node_name.clone(),
            ));
        }
    }
    out
}

/// Moller-Trumbore, both faces: the distance along `dir` to `tri`.
fn ray_hit(origin: Vec3, dir: Vec3, tri: &[Vec3; 3]) -> Option<f32> {
    let e1 = tri[1] - tri[0];
    let e2 = tri[2] - tri[0];
    let p = dir.cross(e2);
    let det = e1.dot(p);
    if det.abs() < 1e-9 {
        return None;
    }
    let inv = 1.0 / det;
    let t = origin - tri[0];
    let u = t.dot(p) * inv;
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let q = t.cross(e1);
    let v = dir.dot(q) * inv;
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    let d = e2.dot(q) * inv;
    (d > 0.0).then_some(d)
}

/// The first solid triangle a horizontal ray from `origin` along `dir` meets.
fn first_hit(
    tris: &[([Vec3; 3], u32, String)],
    origin: Vec3,
    dir: Vec3,
) -> Option<(f32, u32, String)> {
    tris.iter()
        .filter_map(|(t, slot, name)| ray_hit(origin, dir, t).map(|d| (d, *slot, name.clone())))
        .min_by(|a, b| a.0.total_cmp(&b.0))
}

/// What one [`fly`] saw.
struct Flight {
    /// Ground contacts gained.
    contacts: u64,
    /// Terrain contacts the fatal-contact judge counted.
    judged: usize,
    /// Furthest progress along the heading, metres.
    furthest: f32,
    /// Frames whose step carried the body's centre through a solid triangle.
    crossings: u32,
}

/// Puts the player body at `start` flying along `dir` at `speed`, runs up to
/// `updates` frames and returns (ground contacts gained, furthest progress along
/// `dir` from `start`, metres).
fn fly(
    app: &mut bevy::app::App,
    tris: &[([Vec3; 3], u32, String)],
    start: Vec3,
    dir: Vec3,
    speed: f32,
    updates: usize,
) -> Flight {
    use avian3d::prelude::{AngularVelocity, LinearVelocity, Position, Rotation};
    use cs_app::playtest::PlaytestState;
    let player = app
        .world_mut()
        .query_filtered::<Entity, With<MissionPlayerBody>>()
        .single(app.world())
        .expect("the player body");
    // A fatal contact froze the body and began a crash sequence: release both,
    // as a restart would, so the next flight starts from a live aircraft.
    {
        use cs_app::playtest::fatal_contact::{CrashSequence, FatalContactRule};
        app.world_mut().remove_resource::<CrashSequence>();
        let mut e = app.world_mut().entity_mut(player);
        if let Some(mut rule) = e.get_mut::<FatalContactRule>() {
            rule.crash = None;
        }
        e.insert(avian3d::prelude::RigidBody::Dynamic);
    }
    let rot = Quat::from_rotation_arc(Vec3::NEG_Z, dir);
    {
        let mut e = app.world_mut().entity_mut(player);
        *e.get_mut::<Position>().unwrap() = Position(start);
        *e.get_mut::<Transform>().unwrap() = Transform::from_translation(start).with_rotation(rot);
        *e.get_mut::<Rotation>().unwrap() = Rotation(rot);
        *e.get_mut::<LinearVelocity>().unwrap() = LinearVelocity(dir * speed);
        if let Some(mut w) = e.get_mut::<AngularVelocity>() {
            *w = AngularVelocity(Vec3::ZERO);
        }
    }
    let before = app.world().resource::<PlaytestState>().ground_contacts;
    let mut cursor = MessageCursor::<TerrainContactJudged>::default();
    // Drop the messages the previous flight left.
    cursor.clear(app.world().resource::<Messages<TerrainContactJudged>>());
    let mut judged = 0;
    let mut furthest = 0.0_f32;
    let mut crossings = 0;
    let mut previous = start;
    for _ in 0..updates {
        app.update();
        judged += cursor
            .read(app.world().resource::<Messages<TerrainContactJudged>>())
            .count();
        if let Some(p) = app.world().entity(player).get::<Position>() {
            furthest = furthest.max((p.0 - start).dot(dir));
            let step = p.0 - previous;
            if tris
                .iter()
                .any(|(t, _, _)| ray_hit(previous, step, t).is_some_and(|d| d <= 1.0))
            {
                crossings += 1;
            }
            previous = p.0;
        }
    }
    let after = app.world().resource::<PlaytestState>().ground_contacts;
    Flight {
        contacts: after - before,
        judged,
        furthest,
        crossings,
    }
}

/// The grouped scenery is solid: M01's terrain below the g28xxx grouping nodes
/// (the shipwreck's mountain ring, `g28637`, and the cliffs of `g28911`) stops a
/// body flown into it from outside at speed, and a body flown over it passes.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_scenery_collider_ring_and_cliffs_stop_a_fast_body() {
    let mut app = compose();
    let colliders: Vec<u32> = app
        .world_mut()
        .query::<&WorldSceneryCollider>()
        .iter(app.world())
        .map(|c| c.node_slot)
        .collect();
    assert!(
        colliders.len() > 100,
        "{} scenery colliders",
        colliders.len()
    );
    for slot in [2994, 2965] {
        assert!(colliders.contains(&slot), "node {slot} has no collider");
    }
    let tris = solid_triangles(&app);
    let speed = 113.5;
    let mut ring_cases = 0;
    let mut cliff_cases = 0;
    for (aim, name) in [
        (SHIPWRECK, "g28637"),
        (Vec3::new(-7680.0, 0.0, -4608.0), "g28911"),
    ] {
        for az in (0..12).step_by(3) {
            let a = az as f32 * std::f32::consts::TAU / 12.0;
            let dir_in = Vec3::new(-a.cos(), 0.0, -a.sin());
            for h in [25.0_f32, 110.0] {
                let origin = Vec3::new(aim.x, h, aim.z) - dir_in * 1400.0;
                let Some((d, _, hit_name)) = first_hit(&tris, origin, dir_in) else {
                    continue;
                };
                if hit_name != name || d < 450.0 {
                    continue;
                }
                let start = origin + dir_in * (d - 400.0);
                let flight = fly(&mut app, &tris, start, dir_in, speed, 300);
                eprintln!(
                    "{name} az {az} h {h}: wall at 400 m, contacts {}, judged {}, furthest {:.1}, crossings {}",
                    flight.contacts, flight.judged, flight.furthest, flight.crossings
                );
                assert_eq!(flight.crossings, 0, "{name} az {az} h {h}: tunnelled");
                // Stopped by the wall or, when the wider body met other
                // scenery on the way, by that: either way it was stopped.
                assert!(
                    flight.judged > 0,
                    "{name} az {az} h {h}: the judge counted no terrain contact"
                );
                if name == "g28637" {
                    ring_cases += 1
                } else {
                    cliff_cases += 1
                }
            }
        }
    }
    assert!(ring_cases > 0, "no ray reached the shipwreck ring");
    assert!(cliff_cases > 0, "no ray reached the g28911 cliffs");

    // A touch is not a kill: the same wall met at walking pace is a bounce.
    {
        let a = 3.0 * std::f32::consts::TAU / 12.0;
        let dir_in = Vec3::new(-a.cos(), 0.0, -a.sin());
        let origin = Vec3::new(SHIPWRECK.x, 25.0, SHIPWRECK.z) - dir_in * 1400.0;
        let hit = first_hit(&tris, origin, dir_in).expect("the az 3 ray hits");
        let start = origin + dir_in * (hit.0 - 60.0);
        let flight = fly(&mut app, &tris, start, dir_in, 15.0, 400);
        assert_eq!(flight.crossings, 0, "the slow touch tunnelled");
        let player = app
            .world_mut()
            .query_filtered::<Entity, With<MissionPlayerBody>>()
            .single(app.world())
            .unwrap();
        let rule = app
            .world()
            .entity(player)
            .get::<cs_app::playtest::fatal_contact::FatalContactRule>()
            .expect("the rule");
        assert!(rule.crash.is_none(), "a 15 m/s touch crashed the aircraft");
    }

    // Free air: 400 m up, above every solid triangle, the same line is clear.
    let ceiling = tris
        .iter()
        .flat_map(|t| t.0)
        .map(|v| v.y)
        .fold(0.0, f32::max);
    let dir = Vec3::X;
    let start = Vec3::new(SHIPWRECK.x - 1000.0, ceiling + 150.0, SHIPWRECK.z);
    let flight = fly(&mut app, &tris, start, dir, speed, 200);
    assert_eq!(flight.contacts, 0, "free air reported a contact");
    assert_eq!(flight.crossings, 0);
    assert!(
        flight.furthest > 100.0,
        "free-air flight went {} m",
        flight.furthest
    );

    teardown(&mut app);
    let left = app
        .world_mut()
        .query::<&WorldSceneryCollider>()
        .iter(app.world())
        .count();
    assert_eq!(left, 0, "teardown left scenery colliders");
}

/// #2032: a ray straight down at the owner's observed crash point and across the
/// jungle plateau around it meets solid scenery above the sea.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_scenery_collider_jungle_plateau_is_solid_from_above() {
    let mut app = compose();
    let tris = solid_triangles(&app);
    assert!(!tris.is_empty(), "no solid scenery triangles");
    let down = Vec3::NEG_Y;
    let top = 1000.0;

    let (d, slot, name) = first_hit(&tris, Vec3::new(-5055.0, top, -4718.0), down)
        .expect("a ray down at the observed crash point meets scenery");
    let y = top - d;
    eprintln!("(-5055,-4718): y {y:.1} node {slot} {name}");
    assert!(y > 1.0, "the first hit is the sea level, y {y}");

    let (mut hit, mut total) = (0, 0);
    let mut z = -4900.0;
    while z <= -4300.0 {
        let mut x = -7000.0;
        while x <= -3500.0 {
            total += 1;
            if let Some((d, _, _)) = first_hit(&tris, Vec3::new(x, top, z), down) {
                if top - d > 1.0 {
                    hit += 1;
                }
            }
            x += 100.0;
        }
        z += 100.0;
    }
    eprintln!("grid: {hit}/{total} columns meet scenery above the sea");
    assert!(hit > 0, "no column of the plateau grid meets scenery");
    teardown(&mut app);
}
