//! Acceptance scenarios F21-F (task #537): the playtest's render-frame loop
//! drives a [`cs_app::camera::CameraSession`] from the authoritative body pose.
//!
//! Test prefix `accept_f21_f_`. These run [`cs_app::playtest::headless_app`], the
//! window's own composition without the window: the production
//! `follow_camera` system feeds the session, and the camera entity is moved to
//! the session frame's view. Wiring is not fidelity: nothing here claims an
//! original camera behavior.

use bevy::input::ButtonState;
use bevy::input::keyboard::{Key, KeyCode, KeyboardInput, NativeKey};
use bevy::prelude::{App, Entity, Transform, With};
use cs_app::camera::{CameraAuthority, ViewRig};
use cs_app::playtest::{PlaytestCamera, PlaytestCameraMarker, headless_app};

fn key(app: &mut App, key_code: KeyCode, state: ButtonState) {
    app.world_mut().write_message(KeyboardInput {
        key_code,
        logical_key: Key::Unidentified(NativeKey::Unidentified),
        state,
        text: None,
        repeat: false,
        window: Entity::PLACEHOLDER,
    });
}

fn tap(app: &mut App, key_code: KeyCode) {
    key(app, key_code, ButtonState::Pressed);
    app.update();
    key(app, key_code, ButtonState::Released);
    app.update();
}

fn camera(app: &App) -> &PlaytestCamera {
    app.world().resource::<PlaytestCamera>()
}

#[test]
fn accept_f21_f_frames_reach_the_camera_entity() {
    let mut app = headless_app();
    for _ in 0..30 {
        app.update();
    }
    let camera = camera(&app);
    assert!(camera.frames >= 10, "frames: {}", camera.frames);
    assert_eq!(camera.last_error, None);
    let frame = camera.session_frame.as_ref().expect("a session frame");
    assert_eq!(
        frame.authority,
        CameraAuthority::Player {
            rig: ViewRig::Chase
        }
    );
    assert_eq!(camera.frame, frame.rig, "the rig frame is the session's");
    let eye = frame.view.pose.position();
    let mut query = app
        .world_mut()
        .query_filtered::<&Transform, With<PlaytestCameraMarker>>();
    let transform = query.single(app.world()).expect("one camera entity");
    assert!((f64::from(transform.translation.x) - eye.x()).abs() < 1e-2);
    assert!((f64::from(transform.translation.y) - eye.y()).abs() < 1e-2);
    assert!((f64::from(transform.translation.z) - eye.z()).abs() < 1e-2);
}

#[test]
fn accept_f21_f_aircraft_swap_rebinds_the_camera_and_resets_the_session() {
    let mut app = headless_app();
    for _ in 0..30 {
        app.update();
    }
    let before = camera(&app)
        .session_frame
        .as_ref()
        .and_then(|frame| frame.view.subject)
        .expect("the camera follows the first body");
    let frames_before = camera(&app).frames;
    tap(&mut app, KeyCode::KeyR);
    for _ in 0..5 {
        app.update();
    }
    let camera = camera(&app);
    assert!(camera.frames > frames_before, "frames keep arriving");
    let frame = camera.session_frame.as_ref().expect("a session frame");
    let after = frame.view.subject.expect("bound to the new body");
    assert_ne!(before, after, "the new pilot's body is a new binding");
    assert_eq!(camera.last_error, None);
    assert_eq!(
        frame.authority,
        CameraAuthority::Player {
            rig: ViewRig::Chase
        },
        "the reset session is back on the chase rig"
    );
}
