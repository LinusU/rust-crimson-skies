//! Acceptance for Rally #1824 `PARITY3-CAMERA-MODES.02`: the original's view
//! keys (`F9`-`F12` external cameras, `F7` the held-point mode 9) and the zoom
//! keys (numpad `+`/`-`) drive the mission's view state through the production
//! minting path, and the camera is placed from it (finding
//! `docs/findings/2026-10-21-parity3-camera-modes-01-measured.md`; the
//! placements are designed and labelled so in `external_view`).

use avian3d::prelude::{Position, Rotation};
use bevy::app::{App, PostUpdate};
use bevy::prelude::{ButtonInput, Entity, KeyCode, Quat, Transform, Vec3};
use cs_app::mission_session::{
    BACK_DISTANCE_M, CycleMemory, EXTERNAL_DISTANCE_M, HeldPoint, MissionControls,
    MissionPlayerBody, MissionViewMode, MissionZoom, drive_external_camera, external_eye,
    mint_original_chord_keys,
};
use cs_app::playtest::PlaytestCameraMarker;
use cs_types::keymap::ControlsMap;

const BODY_AT: Vec3 = Vec3::new(100.0, 50.0, -20.0);

/// An app with the production minting resources, the external camera system,
/// one body (yawed 0.7 rad) and one camera.
fn mini_app() -> (App, Entity, Entity) {
    let mut app = App::new();
    app.insert_resource(ButtonInput::<KeyCode>::default());
    app.insert_resource(MissionControls(ControlsMap::original_default()));
    app.init_resource::<MissionViewMode>();
    app.init_resource::<CycleMemory>();
    app.init_resource::<MissionZoom>();
    app.init_resource::<HeldPoint>();
    app.add_systems(PostUpdate, drive_external_camera);
    let body = app
        .world_mut()
        .spawn((
            MissionPlayerBody,
            Position(BODY_AT),
            Rotation(Quat::from_rotation_y(0.7)),
        ))
        .id();
    let camera = app
        .world_mut()
        .spawn((PlaytestCameraMarker, Transform::from_xyz(1.0, 2.0, 3.0)))
        .id();
    (app, body, camera)
}

fn tap(app: &mut App, key: KeyCode) {
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(key);
    mint_original_chord_keys(app.world_mut());
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .release(key);
    mint_original_chord_keys(app.world_mut());
}

fn view(app: &App) -> MissionViewMode {
    *app.world().resource::<MissionViewMode>()
}

fn camera_at(app: &App, camera: Entity) -> Vec3 {
    app.world().get::<Transform>(camera).unwrap().translation
}

#[test]
fn accept_parity3_camera_modes_02_the_view_keys_select_the_recorded_modes() {
    let (mut app, _, _) = mini_app();
    for (key, mode, original) in [
        (KeyCode::F9, MissionViewMode::ExternalDown, 1),
        (KeyCode::F10, MissionViewMode::ExternalBack, 2),
        (KeyCode::F11, MissionViewMode::ExternalLeft, 3),
        (KeyCode::F12, MissionViewMode::ExternalRight, 4),
        (KeyCode::F7, MissionViewMode::HeldPoint, 9),
    ] {
        tap(&mut app, key);
        assert_eq!(view(&app), mode, "{key:?}");
        assert_eq!(view(&app).original_mode(), original);
    }
    // `Chase view` is mode 9, not the ordinary chase camera (mode 0).
    assert_ne!(MissionViewMode::HeldPoint, MissionViewMode::Chase);
}

#[test]
fn accept_parity3_camera_modes_02_f8_from_an_external_view_returns_to_the_last_cycle_mode() {
    let (mut app, _, _) = mini_app();
    // Initial memory is mode 6.
    tap(&mut app, KeyCode::F10);
    tap(&mut app, KeyCode::F8);
    assert_eq!(view(&app), MissionViewMode::SecondEye);
    // 6 -> 0 -> 7 remembered, then an external view, then F8 returns to 7.
    tap(&mut app, KeyCode::F8);
    tap(&mut app, KeyCode::F8);
    assert_eq!(view(&app), MissionViewMode::Cockpit);
    tap(&mut app, KeyCode::F11);
    tap(&mut app, KeyCode::F8);
    assert_eq!(view(&app), MissionViewMode::Cockpit);
}

#[test]
fn accept_parity3_camera_modes_02_a_key_press_changes_the_resolved_camera_pose() {
    let (mut app, _, camera) = mini_app();
    let rotation = Quat::from_rotation_y(0.7);
    app.update();
    assert_eq!(camera_at(&app, camera), Vec3::new(1.0, 2.0, 3.0));

    let right = rotation * Vec3::X;
    tap(&mut app, KeyCode::F12);
    app.update();
    let placed = *app.world().get::<Transform>(camera).unwrap();
    assert!(
        placed
            .translation
            .distance(BODY_AT + right * EXTERNAL_DISTANCE_M)
            < 1e-3
    );
    // The camera looks at the aircraft.
    let toward = (BODY_AT - placed.translation).normalize();
    assert!(placed.forward().dot(toward) > 0.999);

    tap(&mut app, KeyCode::F11);
    app.update();
    assert!(
        camera_at(&app, camera).distance(BODY_AT - right * EXTERNAL_DISTANCE_M) < 1e-3,
        "left is the opposite side"
    );

    tap(&mut app, KeyCode::F10);
    app.update();
    let back = camera_at(&app, camera) - BODY_AT;
    assert!((back.length() - BACK_DISTANCE_M).abs() < 1e-3);
    assert!(back.dot(rotation * Vec3::NEG_Z) < 0.0, "astern of the nose");

    tap(&mut app, KeyCode::F9);
    app.update();
    assert!(camera_at(&app, camera).y > BODY_AT.y + 1.0, "above");
}

#[test]
fn accept_parity3_camera_modes_02_the_held_point_stays_where_it_was_entered() {
    let (mut app, body, camera) = mini_app();
    app.world_mut()
        .get_mut::<Transform>(camera)
        .unwrap()
        .translation = Vec3::new(5.0, 6.0, 7.0);
    tap(&mut app, KeyCode::F7);
    app.update();
    assert_eq!(camera_at(&app, camera), Vec3::new(5.0, 6.0, 7.0));
    // The aircraft moves; the camera stays and turns to keep it in view.
    app.world_mut().get_mut::<Position>(body).unwrap().0 = Vec3::new(0.0, 0.0, -500.0);
    app.world_mut()
        .get_mut::<Transform>(camera)
        .unwrap()
        .translation = Vec3::splat(99.0);
    app.update();
    assert_eq!(camera_at(&app, camera), Vec3::new(5.0, 6.0, 7.0));
    let toward = (Vec3::new(0.0, 0.0, -500.0) - Vec3::new(5.0, 6.0, 7.0)).normalize();
    let forward = app.world().get::<Transform>(camera).unwrap().forward();
    assert!(forward.dot(toward) > 0.999);
    // Leaving the mode forgets the point.
    tap(&mut app, KeyCode::F10);
    app.update();
    assert_eq!(app.world().resource::<HeldPoint>().0, None);
}

#[test]
fn accept_parity3_camera_modes_02_zoom_keys_move_the_chase_distance_within_the_measured_limits() {
    let (mut app, _, camera) = mini_app();
    // A chase frame 12 m behind the body, as `follow_camera` would write it.
    let chase = BODY_AT + Vec3::new(0.0, 0.0, 12.0);
    let write_chase = |app: &mut App| {
        app.world_mut()
            .get_mut::<Transform>(camera)
            .unwrap()
            .translation = chase;
    };

    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::NumpadSubtract);
    mint_original_chord_keys(app.world_mut());
    assert!(app.world().resource::<MissionZoom>().out_held);
    // Hold for two seconds: the target saturates at 1, the smoothed value
    // approaches it, never exceeding it.
    for _ in 0..120 {
        app.world_mut()
            .resource_mut::<MissionZoom>()
            .step(1.0 / 60.0);
        write_chase(&mut app);
        app.update();
    }
    let zoom = *app.world().resource::<MissionZoom>();
    assert_eq!(zoom.target, 1.0);
    assert!(zoom.smoothed > 0.9 && zoom.smoothed <= 1.0);
    let distance = camera_at(&app, camera).distance(BODY_AT);
    assert!(
        (distance - (12.0 + zoom.smoothed * 10.0)).abs() < 1e-3,
        "{distance}"
    );
    // The camera was rewritten each frame, so the offset does not accumulate.
    write_chase(&mut app);
    app.update();
    assert!((camera_at(&app, camera).distance(BODY_AT) - distance).abs() < 1e-3);

    // Release and zoom in: back to nothing, never below zero.
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .release(KeyCode::NumpadSubtract);
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::NumpadAdd);
    mint_original_chord_keys(app.world_mut());
    assert!(!app.world().resource::<MissionZoom>().out_held);
    for _ in 0..300 {
        app.world_mut()
            .resource_mut::<MissionZoom>()
            .step(1.0 / 60.0);
    }
    let zoom = *app.world().resource::<MissionZoom>();
    assert_eq!(zoom.target, 0.0);
    assert!(zoom.smoothed >= 0.0 && zoom.smoothed < 1e-3);
}

#[test]
fn accept_parity3_camera_modes_02_only_the_chase_view_zooms_and_entering_it_clears_the_zoom() {
    let (mut app, _, camera) = mini_app();
    *app.world_mut().resource_mut::<MissionZoom>() = MissionZoom {
        target: 1.0,
        smoothed: 1.0,
        ..MissionZoom::default()
    };
    tap(&mut app, KeyCode::F12);
    app.update();
    let placed = camera_at(&app, camera);
    assert!(
        placed.distance(
            external_eye(
                MissionViewMode::ExternalRight,
                BODY_AT,
                Quat::from_rotation_y(0.7)
            )
            .unwrap()
        ) < 1e-3,
        "the side view ignores the zoom"
    );
    // F8 from mode 3/4 goes to the remembered cycle mode; reaching mode 0
    // clears the zoom.
    tap(&mut app, KeyCode::F8); // memory is mode 6
    tap(&mut app, KeyCode::F8); // 6 -> 0
    assert_eq!(view(&app), MissionViewMode::Chase);
    let zoom = *app.world().resource::<MissionZoom>();
    assert_eq!((zoom.target, zoom.smoothed), (0.0, 0.0));
}

#[test]
fn accept_parity3_camera_modes_02_a_destroyed_or_swapped_aircraft_is_safe() {
    let (mut app, body, camera) = mini_app();
    tap(&mut app, KeyCode::F12);
    app.update();
    let before = camera_at(&app, camera);

    // Destroyed: no body, the camera is left alone and nothing panics.
    app.world_mut().despawn(body);
    app.update();
    assert_eq!(camera_at(&app, camera), before);

    // Swapped: a new body elsewhere; the next frame follows it.
    app.world_mut().spawn((
        MissionPlayerBody,
        Position(Vec3::new(-300.0, 10.0, 40.0)),
        Rotation(Quat::IDENTITY),
    ));
    app.update();
    let expected = external_eye(
        MissionViewMode::ExternalRight,
        Vec3::new(-300.0, 10.0, 40.0),
        Quat::IDENTITY,
    )
    .unwrap();
    assert!(camera_at(&app, camera).distance(expected) < 1e-3);
}

#[test]
fn accept_parity3_camera_modes_02_the_keep_table_matches_the_recorded_transitions() {
    use MissionViewMode::*;
    assert!(Chase.keeps_state_into(ExternalBack) && ExternalBack.keeps_state_into(Chase));
    assert!(ExternalLeft.keeps_state_into(ExternalRight));
    assert!(ExternalDown.keeps_state_into(ExternalDown));
    assert!(HeldPoint.keeps_state_into(HeldPoint));
    assert!(ExternalDown.keeps_state_into(Cockpit));
    assert!(!Chase.keeps_state_into(HeldPoint));
    assert!(!ExternalLeft.keeps_state_into(Chase));
    assert!(!Chase.keeps_state_into(ExternalDown));
}
