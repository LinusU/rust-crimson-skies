//! F00-B: pin the schedule hooks of the pinned Bevy 0.19 / Avian3d 0.7 pair.
//!
//! `docs/01-ARCHITECTURE.md` requires the exact Avian schedule hooks to be
//! taken from the pinned version's API and tested during bootstrap. These
//! probes observe the real world built by `cs_app::synthetic::SyntheticScene`:
//!
//! * the pre-physics probe sits in `FixedPostUpdate` before
//!   `PhysicsSystems::Prepare` — the schedule `PhysicsPlugins::default()`
//!   was added with;
//! * the post-physics probe sits in `Update`, which Bevy's `Main` schedule
//!   runs after the fixed schedules of the same frame.
//!
//! The test asserts ordering, not the number of fixed runs per frame: that
//! count is driven by Bevy's clock accumulator (its first frame reports a
//! zero delta; see
//! `docs/findings/2026-09-23-pinned-bevy-0.19-avian-0.7-schedule-api.md`) and
//! belongs to the fixed-tick smoke of F00-C / AC03.

use avian3d::prelude::{PhysicsSystems, Position, RigidBody};
use bevy::{
    ecs::schedule::IntoScheduleConfigs,
    prelude::{FixedPostUpdate, Query, ResMut, Resource, Update, With},
};
use cs_app::synthetic::SyntheticScene;
use cs_types::{BodyKind, SyntheticBodySpec};

/// Pose of the dynamic body seen before Avian integrates in this tick.
#[derive(Resource, Default)]
struct PrePhysicsSamples(Vec<f32>);

/// Pose of the dynamic body seen in `Update`, after the fixed schedules ran.
#[derive(Resource, Default)]
struct PostPhysicsSamples(Vec<f32>);

fn body_y(bodies: &Query<&Position, With<RigidBody>>) -> f32 {
    bodies
        .iter()
        .next()
        .expect("the synthetic scene always has one rigid body")
        .0
        .y
}

fn sample_before_physics(
    mut pre: ResMut<PrePhysicsSamples>,
    bodies: Query<&Position, With<RigidBody>>,
) {
    pre.0.push(body_y(&bodies));
}

fn sample_after_physics(
    mut post: ResMut<PostPhysicsSamples>,
    bodies: Query<&Position, With<RigidBody>>,
) {
    post.0.push(body_y(&bodies));
}

/// Observable failure if the pinned hooks are misused (physics moved out of
/// `FixedPostUpdate`, or `Update` scheduled before the fixed loop): the
/// pre-physics pose would already have dropped, or the post-physics pose
/// would not have.
#[test]
fn accept_f00_b_physics_integrates_in_fixed_post_update_before_update() {
    let start = SyntheticBodySpec::falling_box(BodyKind::Dynamic).position_m[1];
    let frames: usize = 4;

    let mut scene = SyntheticScene::builder(SyntheticBodySpec::falling_box(BodyKind::Dynamic))
        .configure(|app| {
            app.init_resource::<PrePhysicsSamples>()
                .init_resource::<PostPhysicsSamples>()
                .add_systems(
                    FixedPostUpdate,
                    sample_before_physics.before(PhysicsSystems::Prepare),
                )
                .add_systems(Update, sample_after_physics);
        })
        .build()
        .expect("the fixture spec must build a scene");

    scene.step(frames as u64);

    let world = scene.world();
    let pre = &world.resource::<PrePhysicsSamples>().0;
    let post = &world.resource::<PostPhysicsSamples>().0;

    assert_eq!(
        post.len(),
        frames,
        "`Update` must run exactly once per frame, got {post:?}"
    );
    assert!(
        !pre.is_empty(),
        "FixedPostUpdate must run so physics can step, got {pre:?}"
    );

    assert_eq!(
        pre[0], start,
        "a probe before PhysicsSystems::Prepare must see the untouched pose"
    );

    let pre_last = *pre.last().expect("checked above");
    let post_last = *post.last().expect("checked above");
    assert!(
        post_last < pre_last,
        "physics must integrate inside FixedPostUpdate before `Update` runs in the \
         same frame: pre {pre:?}, post {post:?}"
    );

    assert!(
        post_last < start,
        "the falling body must be below its start pose once physics has stepped: {post:?}"
    );

    for (frame, poses) in post.windows(2).enumerate() {
        assert!(
            poses[1] < poses[0],
            "the falling body must keep accelerating from frame {frame}: {post:?}"
        );
    }
}
