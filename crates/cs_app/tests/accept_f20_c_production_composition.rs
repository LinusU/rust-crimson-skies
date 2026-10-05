//! Acceptance scenario F20-C (app composition): the production session
//! composition runs the animation path without the caller adding a plugin.
//!
//! Spec: `specs/F20-object-animation-and-authored-destruction-states.md`,
//! stage `### F20-C`. Task test prefix: `accept_f20_c_app_composition_`.

use cs_app::animation::{AnimationPlayback, CommittedSessionTick};
use cs_app::physics::PhysicsSession;
use cs_types::net::SessionId;

fn advances(session: &PhysicsSession) -> u64 {
    session
        .world()
        .expect("an active session owns a world")
        .resource::<AnimationPlayback>()
        .advances()
}

#[test]
fn accept_f20_c_app_composition_production_session_advances_animation() {
    let mut session = PhysicsSession::production_builder().fixed_hz(64).build();
    session
        .world_mut()
        .expect("a fresh session owns a world")
        .insert_resource(AnimationPlayback::new(SessionId::new(1).unwrap()));
    assert_eq!(advances(&session), 0);

    session.step(4).expect("the session steps");

    assert!(
        advances(&session) > 0,
        "committed ticks must advance playback"
    );
    let committed = session
        .world()
        .unwrap()
        .resource::<CommittedSessionTick>()
        .0;
    assert_eq!(committed.0, session.tick());
}

#[test]
fn accept_f20_c_app_composition_plain_builder_does_not_animate() {
    let mut session = PhysicsSession::builder().fixed_hz(64).build();
    session
        .world_mut()
        .unwrap()
        .insert_resource(AnimationPlayback::new(SessionId::new(1).unwrap()));
    session.step(4).expect("the session steps");
    assert_eq!(advances(&session), 0);
}
