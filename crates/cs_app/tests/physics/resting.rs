//! Task #428: the contact/restitution rule for a body resting against geometry
//! an overlay moves (`accept_t428_`).
//!
//! F18-C recorded an observation it could not attribute: a body pressed against
//! a door panel kept creeping at 0.0124 m/tick (1.49 m/s) with the panel
//! **displaced**, and at exactly the same rate with the panel **despawned** and
//! with the panel **left alone** — so it filed #428 to establish whether that
//! is intended contact behaviour, a restitution default, or a defect, and to
//! make "the door opened" and "the body on it moved" two facts that cannot
//! disagree again.
//!
//! These tests carry that measurement and pin the rule built on it.
//! [`cs_app::physics::resting`] is the production code under test and its two
//! declared constants ([`RESTING_STILL_EPSILON_M_S`],
//! [`RESTING_STILL_TICKS`]) are what the measurements freeze. The full
//! resolution is
//! `docs/findings/2026-10-02-t428-contact-restitution-rule-for-a-resting-body.md`.
//!
//! Two compositions are driven, and the difference between them is the point:
//!
//! * the **raw** one — `asset_stack::headless_app` plus the fixed-rate adapter
//!   only — has no resting rule, so it shows what the contact alone does. This
//!   is where the residual, its independence from restitution/friction/substeps
//!   and its independence from the collider's fate are measured;
//! * the **production** ones — `PhysicsFixture` (through
//!   `PhysicsBodiesPlugin`) and `cs_app::world::world_app` (which adds
//!   `RestingBodiesPlugin` explicitly) — carry the rule, and are where "comes
//!   to rest", "stays put when the door opens" and "is left alone while it is
//!   still moving" are pinned.
//!
//! No original game data and no `CS_GAME_DIR` access: every mass, speed and
//! box here is newly authored fixture data, and nothing here is
//! `verified_original`.

use std::time::Duration;

use avian3d::prelude::{
    Collider, Friction, Gravity, LinearVelocity, Mass, Position, Restitution, RigidBody, Rotation,
    Sensor, SleepThreshold, SubstepCount,
};
use bevy::prelude::{App, Entity, EntityWorldMut, Transform, Vec3, World};
use bevy::time::{Real, Time, TimeUpdateStrategy};
use cs_app::asset_stack::headless_app;
use cs_app::physics::{
    BASELINE_FIXED_HZ, DECLARED_SUBSTEP_COUNT, FixtureBodySpec, PhysicsAdapterPlugin,
    PhysicsFixture, RESTING_STILL_EPSILON_M_S, RestingContact, resting_reports,
};
use cs_app::world::{
    DEPOT_DOOR_HALF_M, DEPOT_DOOR_OPEN_OFFSET_M, DEPOT_OBJECT_DOOR, DEPOT_OBJECT_TRIGGER,
    MESH_SETTLE_UPDATES, OverlayOutcome, ProbeSpec, SpawnedWorld, depot_meshes, depot_mission,
    depot_world, load_world, request_overlay, spawn_discrete_probe, world_app,
};
use cs_content::world::WorldObjectId;

/// The body the measurements use: a 250 kg half-metre box, the same size and
/// mass the depot probe uses, so the numbers here and the depot numbers in the
/// finding are the same kind of number.
const MASS_KG: f32 = 250.0;
const HALF_M: f32 = 0.25;

/// The wall's geometry: a 0.4 m thick slab at the origin, big enough that a
/// body cannot miss it by sliding past an edge.
const WALL_HALF_M: [f32; 3] = [0.2, 3.0, 3.0];

/// The impact speed every contact measurement is taken at, in m/s: 0.25 m of
/// travel per tick at 120 Hz, which the 0.4 m wall cannot be tunnelled through.
const IMPACT_SPEED_M_S: f32 = 30.0;

/// Ticks a contact needs to resolve and its residual to settle. The measured
/// transient is one tick; this is the room to see the residual is *constant*
/// rather than decaying.
const SETTLE_TICKS: u64 = 240;

/// The headless world with the fixed-rate adapter and **no** resting rule, so a
/// test can see what the contact alone leaves behind.
fn raw_app(substeps: u32) -> App {
    let mut app = headless_app();
    app.add_plugins(PhysicsAdapterPlugin::new(BASELINE_FIXED_HZ));
    app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(
        1.0 / BASELINE_FIXED_HZ as f64,
    )));
    app.insert_resource(SubstepCount(substeps));
    app.insert_resource(Gravity::ZERO);
    let startup = app.world().resource::<Time<Real>>().startup();
    app.world_mut()
        .resource_mut::<Time<Real>>()
        .update_with_instant(startup);
    app.finish();
    app.cleanup();
    app
}

/// The wall, as a static body at the origin.
fn spawn_wall(world: &mut World) -> Entity {
    world
        .spawn((
            RigidBody::Static,
            Transform::from_translation(Vec3::ZERO),
            Position(Vec3::ZERO),
            Rotation::default(),
            Collider::cuboid(
                WALL_HALF_M[0] * 2.0,
                WALL_HALF_M[1] * 2.0,
                WALL_HALF_M[2] * 2.0,
            ),
        ))
        .id()
}

/// The body, arriving down `+x` at `speed`.
fn spawn_striker(world: &mut World, start_x_m: f32, speed_m_s: f32) -> Entity {
    world
        .spawn((
            RigidBody::Dynamic,
            Transform::from_translation(Vec3::new(start_x_m, 0.0, 0.0)),
            Position(Vec3::new(start_x_m, 0.0, 0.0)),
            Rotation::default(),
            Mass(MASS_KG),
            LinearVelocity(Vec3::new(speed_m_s, 0.0, 0.0)),
            Collider::cuboid(HALF_M * 2.0, HALF_M * 2.0, HALF_M * 2.0),
        ))
        .id()
}

fn velocity(world: &World, body: Entity) -> Vec3 {
    world
        .get::<LinearVelocity>(body)
        .expect("the body keeps its velocity component")
        .0
}

fn position(world: &World, body: Entity) -> Vec3 {
    world
        .get::<Position>(body)
        .expect("the body keeps its position component")
        .0
}

fn is_resting(world: &World, body: Entity) -> bool {
    world.get::<RestingContact>(body).is_some()
}

/// Runs a raw world with a striker into the wall and returns the body's
/// velocity once the contact has resolved, together with the position it ended
/// at.
///
/// `material` is the seam a test uses to declare Avian's restitution and
/// friction on the wall, which is exactly what the first measurement is about.
fn residual_after_impact(
    substeps: u32,
    material: impl FnOnce(&mut EntityWorldMut<'_>),
) -> (Vec3, Vec3) {
    let mut app = raw_app(substeps);
    let wall = spawn_wall(app.world_mut());
    material(&mut app.world_mut().entity_mut(wall));
    let body = spawn_striker(app.world_mut(), -3.0, IMPACT_SPEED_M_S);
    for _ in 0..SETTLE_TICKS {
        app.update();
    }
    (velocity(app.world(), body), position(app.world(), body))
}

/// The production one-body fixture, with the resting rule installed by
/// `PhysicsBodiesPlugin`.
fn production_fixture() -> PhysicsFixture {
    production_fixture_at(IMPACT_SPEED_M_S, -3.0)
}

/// The same fixture at another impact speed and start, for the cases that need
/// a fast body: the count in [`RESTING_STILL_TICKS`] decides *where* a body is
/// left, and where it is left depends on how fast it arrived.
fn production_fixture_at(speed_m_s: f32, start_x_m: f32) -> PhysicsFixture {
    PhysicsFixture::builder(FixtureBodySpec {
        mass_kg: MASS_KG,
        half_extents_m: [HALF_M; 3],
        position_m: [start_x_m, 0.0, 0.0],
        linear_velocity_m_s: [speed_m_s, 0.0, 0.0],
    })
    .configure(|app| {
        app.insert_resource(SubstepCount(DECLARED_SUBSTEP_COUNT));
    })
    .build()
    .expect("the fixture spec is valid")
}

/// The speed at which a wrong tick count stops being a resting pose: 100 m/s is
/// 0.83 m per tick against the wall's 0.4 m thickness, so a body that is still
/// moving when the count completes has travelled a metre and more past the wall.
const FAST_IMPACT_SPEED_M_S: f32 = 100.0;

// ---------------------------------------------------------------------------
// 1. The measurement the whole resolution rests on.
// ---------------------------------------------------------------------------

/// **The resolution's measurement: the residual a contact leaves is not
/// restitution, not friction and not the substep budget, and it never decays.**
///
/// Four arms over the raw composition, which has no resting rule:
///
/// | arm | what it changes | measured residual |
/// | --- | --- | --- |
/// | default | nothing | 0.981 m/s |
/// | `Restitution::new(0.0)` declared | restitution | 0.981 m/s, bit-identical |
/// | `Friction::new(0.0)` declared | friction | 1.037 m/s — *larger*, so friction damps part of it |
/// | 1 / 4 / 8 substeps | solver budget | 0.933 / 1.219 / 0.820 m/s |
///
/// Every arm leaves a residual **above Avian's own sleep threshold**
/// (`SleepThreshold::linear` is 0.15 m/s), so the engine will never settle the
/// body by itself, and every arm leaves it with no approach velocity — the
/// restitution-0 rule is honoured. That is the whole answer to "intended
/// contact behaviour, a restitution default, or a defect": the contact does
/// the right thing with the approach velocity, and leaves behind a constant
/// leak that nothing retires.
///
/// Observable failure: a rebound (positive `x` velocity at restitution 0), a
/// residual at or below the engine's own sleep threshold, a residual that
/// changes over the run, or an arm where raising friction *removes* the
/// residual.
#[test]
fn accept_t428_the_contact_leaves_a_constant_residual_that_no_material_or_substep_choice_explains()
{
    let sleep_threshold = SleepThreshold::default().linear;

    let default_residual = residual_after_impact(DECLARED_SUBSTEP_COUNT, |_| {});
    assert!(
        default_residual.0.x <= 0.0,
        "restitution 0 must not rebound: the body must not be moving away from \
         the wall it struck, got {:?}",
        default_residual.0
    );
    assert!(
        default_residual.0.length() < IMPACT_SPEED_M_S * 0.1,
        "the approach velocity is gone: 30 m/s in, {:?} left",
        default_residual.0.length()
    );
    assert!(
        default_residual.0.length() > sleep_threshold,
        "and what is left is above the engine's own sleep threshold \
         ({sleep_threshold} m/s), so the engine will never settle this body by \
         itself: {:?} = {} m/s",
        default_residual.0,
        default_residual.0.length()
    );

    // Restitution: declaring the value the default already has changes nothing,
    // bit for bit. This is the arm that rules restitution out.
    let declared = residual_after_impact(DECLARED_SUBSTEP_COUNT, |wall| {
        wall.insert(Restitution::new(0.0));
    });
    assert_eq!(
        declared, default_residual,
        "an explicitly declared `Restitution::new(0.0)` reproduces the default \
         arm bit for bit, so the residual is not a restitution default in \
         effect: {declared:?} vs {default_residual:?}"
    );

    // Friction: removing it makes the residual *larger*, so friction is not
    // causing it either. Not bit-equal, and the test says why rather than
    // asserting equality it does not have.
    let frictionless = residual_after_impact(DECLARED_SUBSTEP_COUNT, |wall| {
        wall.insert(Friction::new(0.0));
    });
    assert!(
        frictionless.0.length() > sleep_threshold,
        "a frictionless wall leaves a residual too — {frictionless:?}"
    );
    assert_ne!(
        frictionless, default_residual,
        "and it differs from the default arm: friction damps part of the leak, \
         so it is not the cause of it"
    );

    // The solver budget: no substep count removes it.
    for substeps in [1, 4, 8] {
        let residual = residual_after_impact(substeps, |_| {});
        assert!(
            residual.0.length() > sleep_threshold,
            "{substeps} substeps still leaves {residual:?}, above the sleep \
             threshold {sleep_threshold} m/s"
        );
    }
}

/// **The residual never decays, which is what makes the rule necessary.**
///
/// The same raw world, stepped far past the settling time, with the velocity
/// sampled every tick after the contact: the measured per-tick change is `0.0`
/// apart from one `6e-8` of float noise, over hundreds of ticks. A leak that
/// decayed would need no rule; a leak that is constant is carried forever by a
/// world with zero gravity and no drag.
///
/// Observable failure: any per-tick velocity change above
/// `RESTING_STILL_EPSILON_M_S` — the constant the rule is built on — after the
/// contact has resolved.
#[test]
fn accept_t428_the_residual_never_decays_so_a_body_in_a_world_with_no_drag_drifts_forever() {
    let mut app = raw_app(DECLARED_SUBSTEP_COUNT);
    spawn_wall(app.world_mut());
    let body = spawn_striker(app.world_mut(), -3.0, IMPACT_SPEED_M_S);
    // Step to just past the resolving tick; the measured transient is one tick.
    let mut previous = None;
    let mut worst_change: f32 = 0.0;
    let mut resolved_at = None;
    for tick in 0..SETTLE_TICKS {
        app.update();
        let current = velocity(app.world(), body);
        if current.x <= 0.0 && current.length() > 0.0 {
            resolved_at = Some(tick);
            previous = Some(current);
            continue;
        }
        if let Some(before) = previous {
            let change = (current - before).length();
            worst_change = worst_change.max(change);
            assert!(
                change <= RESTING_STILL_EPSILON_M_S,
                "tick {tick}: the residual changed by {change} m/s, so it is \
                 decaying rather than constant — the resting rule's premise \
                 would be wrong: {current:?}"
            );
        }
        previous = Some(current);
    }
    let resolved_at = resolved_at.expect("the body reaches the wall and stops");
    assert!(
        worst_change <= RESTING_STILL_EPSILON_M_S,
        "the largest per-tick change over {SETTLE_TICKS} ticks was \
         {worst_change} m/s, above the declared {RESTING_STILL_EPSILON_M_S}"
    );
    // And the drift it produces is real and one-directional, not a float wobble.
    let mut app2 = raw_app(DECLARED_SUBSTEP_COUNT);
    spawn_wall(app2.world_mut());
    let body2 = spawn_striker(app2.world_mut(), -3.0, IMPACT_SPEED_M_S);
    for _ in 0..resolved_at + 1 {
        app2.update();
    }
    let start = position(app2.world(), body2);
    for _ in 0..SETTLE_TICKS {
        app2.update();
    }
    let drifted = (position(app2.world(), body2) - start).length();
    assert!(
        drifted > 0.1,
        "a constant residual with no gravity and no drag carries the body \
         {drifted} m over {SETTLE_TICKS} ticks; a run that does not drift is \
         not the world the rule was measured on"
    );
}

/// **F18-C's attribution, re-measured and pinned: the collider's fate is
/// irrelevant to the leak.**
///
/// The three arms F18-C ran — displace the collider, despawn it, leave it
/// alone — after the body has already been pressed into it and the contact has
/// resolved. All three measure the *same* drift, which is why that stage could
/// not attribute the creep to the overlay that moved the panel.
///
/// The arm is taken on the raw composition, so this is the observation F18-C
/// made, unchanged. [`accept_t428_a_door_opening_does_not_move_a_body_that_already_came_to_rest`]
/// is the same question with the rule installed, and it is the one that decides
/// what happens next.
///
/// Observable failure: any arm whose drift differs from the others, or any arm
/// in which the body resumes its arrival velocity — the "not released"
/// reading, which the measurement rejects.
#[test]
fn accept_t428_the_residual_is_the_same_whether_the_collider_moves_is_despawned_or_is_left_alone() {
    #[derive(Clone, Copy, Debug, PartialEq)]
    enum Arm {
        Untouch,
        Move,
        Despawn,
    }

    let mut drifts = Vec::new();
    for arm in [Arm::Untouch, Arm::Move, Arm::Despawn] {
        let mut app = raw_app(DECLARED_SUBSTEP_COUNT);
        let wall = spawn_wall(app.world_mut());
        let body = spawn_striker(app.world_mut(), -3.0, IMPACT_SPEED_M_S);
        // Press the body into the wall and let the contact resolve, which is the
        // state F18-C's stage measured from.
        for _ in 0..SETTLE_TICKS / 2 {
            app.update();
        }
        assert!(
            velocity(app.world(), body).length() > 0.0,
            "{arm:?}: the contact has resolved and left its residual"
        );
        let before = position(app.world(), body);
        match arm {
            Arm::Untouch => {}
            Arm::Move => {
                app.world_mut().entity_mut(wall).insert((
                    Position(Vec3::new(0.0, 0.0, 4.0)),
                    Transform::from_translation(Vec3::new(0.0, 0.0, 4.0)),
                ));
            }
            Arm::Despawn => {
                app.world_mut().entity_mut(wall).despawn();
            }
        }
        for _ in 0..SETTLE_TICKS {
            app.update();
        }
        let after = position(app.world(), body);
        let drift = after - before;
        assert!(
            velocity(app.world(), body).x < IMPACT_SPEED_M_S * 0.1,
            "{arm:?}: the body must not resume its arrival velocity, got {:?}",
            velocity(app.world(), body)
        );
        drifts.push((arm, drift));
    }
    let reference = drifts[0].1;
    for (arm, drift) in &drifts {
        assert!(
            (drift - reference).length() < 1e-6,
            "{arm:?} drifted by {drift:?} while the untouched control drifted \
             by {reference:?}: the leak depends on what happened to the \
             collider, which is what F18-C could not attribute"
        );
    }
}

// ---------------------------------------------------------------------------
// 2. The rule, on the production compositions.
// ---------------------------------------------------------------------------

/// **The rule: a body struck against world geometry comes to rest and holds
/// its pose.**
///
/// The production fixture, so `RestingBodiesPlugin` is installed by
/// `PhysicsBodiesPlugin` rather than by the test. The same body, in the same
/// world, without the rule is the previous test's `drifted > 0.1 m`; here it
/// does not move at all.
///
/// The marker is deliberately *not* asserted at the end, and the reason is
/// measured rather than convenient: the residual carries the body about 1.3 cm
/// away from the surface it struck, which is further than Avian's contact
/// tolerance, so the pair stops being a contact and the body is at rest in open
/// space. The rule does what it says in both halves — it retires the drift, and
/// it stops claiming a body nothing is touching — so the durable assertions are
/// the ones a reader can rely on: the speed is exactly zero, one residual was
/// retired, the pose is bit-identical over 240 further ticks, and the body never
/// resumes anything.
///
/// Observable failure: a non-zero speed at the end, a drifted pose, a pose that
/// moves by any amount at all after the retirement tick, or a second retirement
/// that would mean the rule re-took a body it had already settled.
#[test]
fn accept_t428_a_body_struck_against_world_geometry_comes_to_rest_and_holds_its_pose() {
    let mut fixture = production_fixture();
    spawn_wall(fixture.world_mut());
    assert!(
        resting_reports(fixture.world()).is_some(),
        "the resting rule is installed by the production bodies plugin"
    );
    // Step to the tick the rule retires the residual, then hold the pose from
    // there: that is the claim being pinned, not "it ends up somewhere".
    let mut retired_at = None;
    for _ in 0..SETTLE_TICKS {
        fixture.step(1);
        let reports = resting_reports(fixture.world()).expect("the rule is installed");
        if retired_at.is_none() && reports.retired == 1 {
            retired_at = Some(fixture.sample().position_m);
        }
        assert!(
            reports.retired <= 1,
            "the rule must retire this body's residual once, not repeatedly: \
             {reports:?}"
        );
    }
    let pose = retired_at.expect("the body reaches the wall and the rule retires its residual");
    let reports = resting_reports(fixture.world()).expect("the rule is installed");

    // The resting pose is *beside* the wall, not merely at rest. This is the
    // assertion that pins `RESTING_STILL_TICKS`: the tick count decides where
    // the body is left, and at four ticks the wall's near face is still ahead
    // of the body. A count of six or more lets a body struck at 100 m/s travel
    // 1.76 m *past* the wall before the count completes — a count that retires
    // a moving body rather than a resting one.
    let wall_face_x = -WALL_HALF_M[0];
    let leading_face = pose[0] + HALF_M;
    assert!(
        leading_face <= wall_face_x,
        "the body rests beside the wall, not beyond it: its leading face is at \
         {leading_face} and the wall's near face is at {wall_face_x}"
    );
    assert!(
        leading_face > wall_face_x - 0.1,
        "and beside it rather than far away: leading face at {leading_face}, \
         wall face at {wall_face_x}"
    );

    assert_eq!(
        fixture.sample().linear_velocity_m_s,
        [0.0; 3],
        "the residual is retired to exactly zero, not to a smaller number"
    );
    assert_eq!(reports.retired, 1, "one residual retired: {reports:?}");
    assert_eq!(
        reports.resting, 0,
        "and nothing is held as resting now: {reports:?}"
    );

    // The control is the previous test: the same world without the rule drifts
    // more than 0.1 m over this span. Here the pose must not move at all.
    fixture.step(SETTLE_TICKS);
    assert_eq!(
        fixture.sample().position_m,
        pose,
        "the pose is bit-identical {SETTLE_TICKS} ticks after the retirement: \
         the rule writes velocity only, so a stopped body stays exactly where \
         the contact left it — where the same body without the rule had drifted \
         more than 0.1 m"
    );
    assert_eq!(
        fixture.sample().linear_velocity_m_s,
        [0.0; 3],
        "and it is still at rest: nothing gives a body at rest in a world with \
         no gravity and no drag a velocity to keep"
    );

    // The fast arm, which is what the count is actually pinned by: a body at
    // 100 m/s covers 0.83 m per tick, so a count long enough that the body is
    // still moving when it completes retires a body that has passed a metre
    // *through* the wall. The rule must stop it while it is still beside the
    // surface, at every speed it can be struck at.
    let mut fast = production_fixture_at(FAST_IMPACT_SPEED_M_S, -3.0);
    spawn_wall(fast.world_mut());
    let mut fast_pose = None;
    for _ in 0..SETTLE_TICKS {
        fast.step(1);
        if fast_pose.is_none() && resting_reports(fast.world()).is_some_and(|r| r.retired == 1) {
            fast_pose = Some(fast.sample().position_m);
        }
    }
    let fast_pose = fast_pose.expect("a body struck at 100 m/s is retired too");
    let wall_face_x = -WALL_HALF_M[0];
    assert!(
        fast_pose[0] + HALF_M <= wall_face_x,
        "a body struck at {FAST_IMPACT_SPEED_M_S} m/s rests beside the wall as \
         well: leading face at {} against the wall face at {wall_face_x}. A \
         longer count would have retired it while it was still travelling \
         through the wall",
        fast_pose[0] + HALF_M
    );
    assert!(
        fast_pose[0] + HALF_M > wall_face_x - 0.2,
        "and beside it rather than far away: leading face at {}, wall face at \
         {wall_face_x}",
        fast_pose[0] + HALF_M
    );
    assert_eq!(
        fast.sample().linear_velocity_m_s,
        [0.0; 3],
        "the fast body is at rest too: {:?}",
        fast.sample().linear_velocity_m_s
    );
}

/// **The rule does not fight a body something is still acting on, and it lets
/// go of a body it no longer holds.**
///
/// Two halves, both about the rule's scope:
///
/// * A body **held by the wall** — spawned already against it, so that it is
///   marked and *still touching*, which is the state the other fixture in this
///   file is not — moves when gameplay pushes it along the face. Measured:
///   1.37 m over sixty ticks under 3 kN of tangential force, with the mark
///   withdrawn. The alternative to this assertion is a rule that freezes a body
///   the game is pushing: measured, an unconditional hold left the body's speed
///   at exactly zero for sixty ticks and moved its pose by 0.12 mm.
/// * A body whose geometry **moves away** is unmarked on the next tick and left
///   entirely alone. This is the case the whole task exists for: the body was
///   never held by the panel, so the panel leaving must not be an event the
///   body reacts to.
///
/// Observable failure: a body that stops dead under a sustained force, or a
/// marker that outlives the contact that earned it.
#[test]
fn accept_t428_the_resting_rule_holds_only_what_geometry_holds_and_lets_go_when_the_geometry_leaves()
 {
    use cs_app::physics::ForceRequest;

    // Half 1: a force reaches a body the wall is *holding*. The premise is
    // asserted, because a 30 m/s striker comes to rest clear of the wall and is
    // therefore not a body this rule holds at all — measuring "the rule is not a
    // brake" on that one would assert nothing at all.
    let mut pushed = production_fixture_at(0.0, -0.44);
    spawn_wall(pushed.world_mut());
    for _ in 0..SETTLE_TICKS {
        pushed.step(1);
    }
    let body = pushed.body();
    assert!(
        is_resting(pushed.world(), body),
        "premise: the body is at rest *and* still touching the wall, which is \
         the state the rule holds: {:?}",
        resting_reports(pushed.world())
    );
    let before = pushed.sample().position_m;
    // Tangential: the wall is not what stops a body pushed along its face.
    for _ in 0..60 {
        pushed.submit(ForceRequest::new(body, [0.0, 0.0, 3_000.0], [0.0; 3]).expect("finite"));
        pushed.step(1);
    }
    let after = pushed.sample().position_m;
    assert!(
        after[2] - before[2] > 0.1,
        "a sustained 3 kN force along the wall must move a body the wall is \
         holding — the rule is not a brake: {before:?} -> {after:?}"
    );
    assert!(
        !is_resting(pushed.world(), body),
        "and the mark goes once the body is moving again: {:?}",
        resting_reports(pushed.world())
    );

    // Half 2: the geometry leaves, and the body is released and untouched.
    let mut fixture = production_fixture();
    let wall = spawn_wall(fixture.world_mut());
    let mut retired_at = None;
    for _ in 0..SETTLE_TICKS {
        fixture.step(1);
        if retired_at.is_none()
            && resting_reports(fixture.world()).is_some_and(|reports| reports.retired == 1)
        {
            retired_at = Some(fixture.sample().position_m);
        }
    }
    let pose = retired_at.expect("the body comes to rest against the wall");
    let body = fixture.body();
    let resting_before = resting_reports(fixture.world()).expect("installed").resting;
    assert!(
        resting_before <= 1,
        "the wall may still be holding the body, and if so exactly one: \
         {resting_before}"
    );

    // The wall goes away: the rule's hold has nothing to rest on.
    fixture.world_mut().entity_mut(wall).despawn();
    fixture.step(1);
    assert!(
        !is_resting(fixture.world(), body),
        "a body nothing is touching is not held as resting by this rule"
    );
    let released = resting_reports(fixture.world())
        .expect("installed")
        .released;
    assert!(
        released >= 1,
        "and the withdrawal is counted: {:?}",
        resting_reports(fixture.world())
    );
    assert_eq!(
        resting_reports(fixture.world()).expect("installed").resting,
        0,
        "nothing is marked at rest once the geometry is gone"
    );

    fixture.step(SETTLE_TICKS);
    assert_eq!(
        fixture.sample().position_m,
        pose,
        "and the released body is left exactly where it was: the geometry going \
         away is not an event the body reacts to"
    );
}

/// **The release mechanism: a marked body that gameplay starts driving is let
/// go, and a marked body nobody is driving is never let go by mistake.**
///
/// These are the two halves of one mechanism and they are the reason the rule
/// carries [`RESTING_RELEASE_TICKS`]. Holding a marked body at zero for as long
/// as it touches geometry is not a stronger claim about rest, it is a body the
/// game cannot move: measured, that hold left a body pressed by 3 kN at exactly
/// zero speed for sixty ticks and moved its pose by 0.12 mm, and swallowed a
/// steady 0.5 m/s velocity write outright.
///
/// The release signal is velocity **change**, not speed, and that choice is
/// forced by a measurement rather than preferred: a resting body in contact is
/// handed a non-zero speed every tick by the solver's own soft-constraint bias
/// (0.0638 m/s on the depot panel), so a speed test would release every resting
/// body and restore the drift. The bias *decays* over nineteen ticks and then
/// stops changing; a driven body changes every tick by `a·dt`. So the dwell has
/// to outlast the decay, and the body must not be released one tick early.
///
/// Observable failure: a driven body still marked after
/// `RESTING_RELEASE_TICKS` ticks, or a body left alone losing its mark and its
/// resting pose to a release the decay should have outlasted.
#[test]
fn accept_t428_a_marked_body_is_released_when_gameplay_drives_it_and_held_while_nothing_does() {
    use cs_app::physics::{ForceRequest, RESTING_RELEASE_TICKS};

    // Half 1: the body is *wedged* — pressed into the wall, so the contact
    // cannot be lost — and driven along the face hard enough to beat the
    // wall's friction. This is the case a held body freezes in: with an
    // unconditional hold it reaches 0.010 m in sixty ticks and stays at zero
    // speed for all of them.
    let mut driven = production_fixture_at(0.0, -0.44);
    spawn_wall(driven.world_mut());
    for _ in 0..SETTLE_TICKS {
        driven.step(1);
    }
    let body = driven.body();
    assert!(is_resting(driven.world(), body), "premise: at rest");
    let pose = driven.sample().position_m;
    let mut released_at = None;
    for tick in 1..=(RESTING_RELEASE_TICKS as u64 + 40) {
        // 3 kN into the wall keeps the contact; 2 kN along it beats the
        // 0.3 friction coefficient's 0.9 kN budget.
        driven.submit(ForceRequest::new(body, [3_000.0, 0.0, 0.0], [0.0; 3]).expect("finite"));
        driven.submit(ForceRequest::new(body, [0.0, 0.0, 2_000.0], [0.0; 3]).expect("finite"));
        driven.step(1);
        if released_at.is_none() && !is_resting(driven.world(), body) {
            released_at = Some(tick);
        }
    }
    let released_at = released_at.expect("a body gameplay drives is released");
    assert!(
        released_at >= RESTING_RELEASE_TICKS as u64,
        "the mark must outlast the solver's decaying bias, measured at 19 \
         consecutive ticks on the depot panel: released after {released_at} ticks \
         with RESTING_RELEASE_TICKS = {RESTING_RELEASE_TICKS}"
    );
    assert!(
        driven.sample().position_m[2] - pose[2] > 0.05,
        "and the body slides once it is free, which an unconditional hold \
         prevents: {pose:?} -> {:?}",
        driven.sample().position_m
    );

    // Half 2: left alone, it keeps its mark and its pose. This is the half a
    // release rule breaks first, and it is why the dwell exists at all.
    let mut quiet = production_fixture_at(0.0, -0.44);
    spawn_wall(quiet.world_mut());
    for _ in 0..SETTLE_TICKS {
        quiet.step(1);
    }
    let body = quiet.body();
    assert!(is_resting(quiet.world(), body), "premise: at rest");
    let pose = quiet.sample().position_m;
    for _ in 0..(SETTLE_TICKS * 3) {
        quiet.step(1);
    }
    assert!(
        is_resting(quiet.world(), body),
        "a body nothing is driving must keep its mark for {} ticks: {:?}",
        SETTLE_TICKS * 3,
        resting_reports(quiet.world())
    );
    assert_eq!(
        quiet.sample().position_m,
        pose,
        "and its pose must not move by so much as a float: a resting body that \
         drifts is the defect this task exists to close"
    );
}

/// **A trigger volume is not geometry: a body crossing one is never retired.**
///
/// The depot's own trigger volume, crossed by the production probe. It is a
/// [`Sensor`] — a reported overlap and never an obstacle — so a body sitting
/// inside one is not resting against anything and the rule must leave it
/// entirely alone: no marker, no retired residual, and its speed still exactly
/// what it was fired at.
///
/// Observable failure: a marker on a body inside a sensor volume, or a
/// `retired` counter above zero for a body that nothing stopped.
#[test]
fn accept_t428_a_body_crossing_a_trigger_volume_is_never_retired_because_a_sensor_is_not_geometry()
{
    let mut fixture = production_fixture();
    let body = fixture.body();
    // A 3 m sensor slab straddling the body, on the same entity as a dynamic
    // body so it is the pair a trigger volume produces.
    let volume: Entity = fixture
        .world_mut()
        .spawn((
            RigidBody::Static,
            Transform::from_translation(Vec3::new(-3.0, 0.0, 0.0)),
            Position(Vec3::new(-3.0, 0.0, 0.0)),
            Collider::cuboid(1.5, 3.0, 3.0),
            Sensor,
        ))
        .id();
    for _ in 0..SETTLE_TICKS {
        fixture.step(1);
    }
    assert!(
        fixture
            .world()
            .get::<Position>(volume)
            .is_some_and(|position| position.0.x == -3.0),
        "premise: the trigger volume is still there"
    );
    let reports = resting_reports(fixture.world()).expect("installed");
    assert!(
        !is_resting(fixture.world(), body),
        "a body inside a sensor volume is crossing a trigger, not resting \
         against it: {reports:?}"
    );
    assert_eq!(
        reports.retired, 0,
        "and nothing was retired for it: {reports:?}"
    );
    assert_eq!(
        fixture.sample().linear_velocity_m_s[0],
        IMPACT_SPEED_M_S,
        "the crossing took nothing from it: a sensor is a reported overlap and \
         never an obstacle, so the rule must not touch its velocity: {:?}",
        fixture.sample().linear_velocity_m_s
    );
}

/// **The rule's bookkeeping is a claim about bodies that exist, and about
/// dynamic ones only.**
///
/// Three boundary cases the rule's own queries decide, each of which a reviewer
/// can check by deleting one clause:
///
/// * A **kinematic** body against a wall is not the rule's business. Its
///   velocity is gameplay's own — a scripted actor is moved by its trajectory
///   (spec non-negotiable behavior 1) — and the measured run keeps its 0.5 m/s
///   for the whole flight along the wall face, untouched and unmarked. A rule
///   that read every touching body would stop a scripted mover dead.
/// * A **despawned** body stops being counted. The `resting` counter is a
///   claim about bodies that exist, so a body that despawns while resting must
///   not be reported as at rest forever; measured, it drops to zero on the next
///   tick. A count that only grows is not a count a caller can act on.
/// * A body the engine has already **put to sleep** is left alone. Measured: a
///   body resting against a wall goes to sleep on its own at tick 60, and the
///   rule neither wakes it nor drops its claim. This is the reason the pass
///   never writes a sleeping body's velocity — a velocity write is the one
///   thing that would wake a body the engine had settled, which would trade a
///   settled body for one the rule keeps re-settling every tick.
///
/// Observable failure: a scripted mover stopped by this rule, a `resting` count
/// that does not fall when its body is gone, or a body that cannot stay asleep.
#[test]
fn accept_t428_the_rule_covers_only_live_dynamic_bodies_and_never_disturbs_a_sleeping_one() {
    use avian3d::prelude::{LinearVelocity, RigidBody, Sleeping};

    // A kinematic body gameplay is driving along a wall.
    let mut scripted = production_fixture_at(0.0, -0.44);
    spawn_wall(scripted.world_mut());
    let body = scripted.body();
    scripted
        .world_mut()
        .entity_mut(body)
        .insert(RigidBody::Kinematic);
    scripted.step(2);
    for _ in 0..120 {
        scripted.step(1);
        scripted
            .world_mut()
            .entity_mut(body)
            .insert(LinearVelocity(Vec3::new(0.0, 0.0, 0.5)));
    }
    let reports = resting_reports(scripted.world()).expect("installed");
    assert!(
        !is_resting(scripted.world(), body),
        "a kinematic body's velocity is gameplay's own, so the rule must not \
         mark it: {reports:?}"
    );
    assert_eq!(
        reports.retired, 0,
        "and must not retire anything on its account: {reports:?}"
    );
    assert!(
        scripted.sample().position_m[2] > 0.4,
        "the scripted mover is still travelling along the wall: {:?}",
        scripted.sample().position_m
    );

    // A body resting against a wall that despawns.
    let mut despawned = production_fixture_at(0.0, -0.44);
    spawn_wall(despawned.world_mut());
    let body = despawned.body();
    let mut resting = false;
    for _ in 0..SETTLE_TICKS {
        despawned.step(1);
        resting = resting_reports(despawned.world()).is_some_and(|r| r.resting == 1);
    }
    assert!(resting, "premise: the body was at rest against the wall");
    despawned.world_mut().entity_mut(body).despawn();
    despawned.step(1);
    let after = resting_reports(despawned.world()).expect("installed");
    assert_eq!(
        after.resting, 0,
        "a body that no longer exists is not a body at rest: {after:?}"
    );

    // A body the engine settles by itself.
    let mut settled = production_fixture_at(0.0, -0.44);
    spawn_wall(settled.world_mut());
    let body = settled.body();
    let mut first_asleep = None;
    for tick in 0..SETTLE_TICKS {
        settled.step(1);
        if first_asleep.is_none() && settled.world().get::<Sleeping>(body).is_some() {
            first_asleep = Some(tick);
        }
    }
    let first_asleep = first_asleep.expect("a body resting against a wall falls asleep");
    for _ in 0..SETTLE_TICKS {
        settled.step(1);
    }
    assert!(
        settled.world().get::<Sleeping>(body).is_some(),
        "the rule must not wake a body the engine settled (it first slept on \
         tick {first_asleep}): writing a velocity component is what would wake \
         it"
    );
    assert!(
        is_resting(settled.world(), body),
        "and it keeps its claim: the body is still resting against the wall"
    );
    assert_eq!(
        resting_reports(settled.world()).expect("installed").resting,
        1,
        "exactly one body at rest: {:?}",
        resting_reports(settled.world())
    );
}

// ---------------------------------------------------------------------------
// 3. The F18-C pair itself: the door opens, the body stays.
// ---------------------------------------------------------------------------

/// **The fact F18-C's stage needed pinned: the door opens and the body on it
/// does not move, and both are separately recorded.**
///
/// The depot world, loaded with the door overlay declared, the world
/// composition (which installs the resting rule) and the production probe. The
/// probe starts *between* the trigger volume and the panel, so nothing fires the
/// overlay on the way in: it strikes the closed panel, comes to rest against it,
/// and is recorded as at rest. Only then is the overlay requested — the
/// mission's own branch producer, which is the same hand-off the contact stream
/// uses.
///
/// Then, for 240 ticks after the door has opened: the overlay log says
/// `Applied`, the panel's *collided* half has moved by the authored offset, and
/// the body's pose is bit-identical. "The door opened" and "the body on it
/// moved" are now three separate records, and they agree.
///
/// Observable failure: the body drifting while the door is open (the reading
/// F18-C could not rule out), a refused overlay, a panel whose collided half did
/// not move, or a body that was never at rest to begin with.
#[test]
fn accept_t428_a_door_opening_does_not_move_a_body_that_already_came_to_rest() {
    let definition = depot_world().expect("the synthetic depot world is well formed");
    let instance =
        depot_mission(&definition, true, &[]).expect("a valid depot mission declares the overlay");
    let mut app = world_app();
    let report: SpawnedWorld =
        load_world(&mut app, &definition, &instance, &depot_meshes()).expect("the depot loads");
    for _ in 0..MESH_SETTLE_UPDATES {
        app.update();
    }

    // Between the trigger volume (x ∈ [-4.5, -3.5]) and the panel (x ≥ -0.5),
    // so the flight cannot fire the overlay on its way in.
    let probe = ProbeSpec {
        position_m: [-3.0, 1.5, 0.0],
        velocity_m_s: [IMPACT_SPEED_M_S as f64, 0.0, 0.0],
        half_extents_m: [HALF_M as f64; 3],
        mass_kg: MASS_KG as f64,
    };
    let body = spawn_discrete_probe(&mut app, &probe).expect("a valid probe spec");
    for _ in 0..SETTLE_TICKS {
        app.update();
    }
    let reports = resting_reports(app.world()).expect("the world composition installs the rule");
    assert!(
        app.world().get::<RestingContact>(body).is_some(),
        "the body struck the closed panel and came to rest against it: {reports:?}"
    );
    assert_eq!(reports.retired, 1, "one residual retired: {reports:?}");
    let resting_pose = app
        .world()
        .get::<Position>(body)
        .expect("the probe keeps its pose")
        .0;
    let panel_before = app
        .world()
        .get::<Position>(
            report
                .object(&WorldObjectId::new(DEPOT_OBJECT_DOOR).expect("valid key"))
                .and_then(|door| door.collider.as_ref())
                .expect("a solid cuboid object has a collider")
                .entity,
        )
        .expect("the panel keeps its collider pose")
        .0;

    // The mission's own branch: the same hand-off the contact stream uses.
    request_overlay(
        &mut app,
        WorldObjectId::new(DEPOT_OBJECT_TRIGGER).expect("valid key"),
    );
    app.update();

    let applied: Vec<_> = cs_app::world::overlay_log(app.world())
        .expect("the overlay trace is installed by the composition")
        .outcomes()
        .iter()
        .filter_map(OverlayOutcome::applied)
        .collect();
    assert_eq!(applied.len(), 1, "the door opened once: {applied:?}");
    let offset = Vec3::new(
        DEPOT_DOOR_OPEN_OFFSET_M[0] as f32,
        DEPOT_DOOR_OPEN_OFFSET_M[1] as f32,
        DEPOT_DOOR_OPEN_OFFSET_M[2] as f32,
    );
    let panel_after = app
        .world()
        .get::<Position>(
            report
                .object(&WorldObjectId::new(DEPOT_OBJECT_DOOR).expect("valid key"))
                .and_then(|door| door.collider.as_ref())
                .expect("a solid cuboid object has a collider")
                .entity,
        )
        .expect("the panel keeps its collider pose")
        .0;
    assert!(
        (panel_after - (panel_before + offset)).length() < 1e-5,
        "the panel's *collided* half moved by the authored offset: \
         {panel_before:?} -> {panel_after:?}"
    );
    // The panel really was in the way: it was closed, half a metre thick, and
    // the body came to rest against its near face.
    assert!(
        panel_before.x.abs() < DEPOT_DOOR_HALF_M[0] as f32 + 1e-5
            && resting_pose.x < panel_before.x,
        "premise: the body is on the near side of a shut panel \
         (panel {panel_before:?}, body {resting_pose:?})"
    );

    for _ in 0..SETTLE_TICKS {
        app.update();
    }
    let end = app
        .world()
        .get::<Position>(body)
        .expect("the probe keeps its pose")
        .0;
    assert_eq!(
        end, resting_pose,
        "the body did not move while the door opened: the two facts are now \
         measured separately ({end:?} vs {resting_pose:?}) and they agree"
    );
    assert!(
        app.world().get::<RestingContact>(body).is_some(),
        "and it is still recorded as at rest"
    );
    let after = resting_reports(app.world()).expect("installed");
    assert_eq!(after.released, 0, "nothing woke it: {after:?}");
    assert_eq!(
        after.retired, 1,
        "and nothing further was retired: {after:?}"
    );
}
