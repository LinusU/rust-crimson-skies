//! Acceptance for Rally #1810 `PARITY3-COCKPIT-VIEW.02`: with the mission view
//! on a pilot-eye mode the camera sits at `body.pose * cockpit_camera
//! translation`, the exterior airframe is hidden, and cycling back returns
//! the chase view (finding
//! `docs/findings/2026-10-21-parity3-cockpit-view-02-eye-camera.md`).
//!
//! The first two tests drive the production system on an authored body (CI
//! runs them); the third composes retail M01 and needs `CS_GAME_DIR`.

use std::path::PathBuf;

use avian3d::prelude::{Position, Rotation};
use bevy::app::{App, Update};
use bevy::prelude::{Entity, Quat, Transform, Vec3, Visibility, With};
use cs_app::mission_launch::plan_mission_launch;
use cs_app::mission_session::{
    MissionCockpitEye, MissionPlayerBody, MissionViewMode, build_headless, drive_cockpit_camera,
    stage_for,
};
use cs_app::playtest::{PlaytestCamera, PlaytestCameraMarker};
use cs_content::campaign_bindings::MissionLabel;

const STORED_EYE: [f32; 3] = [0.0, 0.75, -0.2];

fn near(a: Vec3, b: Vec3) -> bool {
    a.distance(b) < 1e-4
}

/// An app holding only the cockpit camera system, one body and one camera.
fn mini_app(eye: Option<MissionCockpitEye>) -> (App, Entity, Entity) {
    let mut app = App::new();
    app.insert_resource(MissionViewMode::default());
    app.insert_resource(PlaytestCamera::capture_fitted());
    if let Some(eye) = eye {
        app.insert_resource(eye);
    }
    app.add_systems(Update, drive_cockpit_camera);
    let rotation = Quat::from_rotation_y(0.7) * Quat::from_rotation_z(0.3);
    let body = app
        .world_mut()
        .spawn((
            MissionPlayerBody,
            Position(Vec3::new(100.0, 50.0, -20.0)),
            Rotation(rotation),
            Visibility::Inherited,
        ))
        .id();
    let camera = app
        .world_mut()
        .spawn((PlaytestCameraMarker, Transform::from_xyz(1.0, 2.0, 3.0)))
        .id();
    (app, body, camera)
}

#[test]
fn accept_parity3_cockpit_view_02_camera_is_the_body_pose_times_the_eye_node() {
    let nose = Quat::IDENTITY;
    let (mut app, body, camera) = mini_app(Some(MissionCockpitEye::new(STORED_EYE, nose)));
    let position = app.world().get::<Position>(body).unwrap().0;
    let rotation = app.world().get::<Rotation>(body).unwrap().0;

    // Chase: the system leaves the camera and the body alone.
    app.update();
    let untouched = *app.world().get::<Transform>(camera).unwrap();
    assert_eq!(untouched.translation, Vec3::new(1.0, 2.0, 3.0));
    assert_eq!(
        *app.world().get::<Visibility>(body).unwrap(),
        Visibility::Inherited
    );

    // Cockpit: eye = body.pose * stored translation, body forward and roll.
    *app.world_mut().resource_mut::<MissionViewMode>() = MissionViewMode::Cockpit;
    app.update();
    let placed = *app.world().get::<Transform>(camera).unwrap();
    let expected = position + rotation * Vec3::from_array(STORED_EYE);
    assert!(
        near(placed.translation, expected),
        "{} vs {expected}",
        placed.translation
    );
    assert!(placed.rotation.abs_diff_eq(rotation, 1e-6));
    assert_eq!(
        *app.world().get::<Visibility>(body).unwrap(),
        Visibility::Hidden,
        "the exterior airframe is hidden from the cockpit"
    );

    // The second eye view uses the same eye; cycling back restores the body.
    *app.world_mut().resource_mut::<MissionViewMode>() = MissionViewMode::SecondEye;
    app.update();
    assert!(near(
        app.world().get::<Transform>(camera).unwrap().translation,
        expected
    ));
    *app.world_mut().resource_mut::<MissionViewMode>() = MissionViewMode::Chase;
    app.update();
    assert_eq!(
        *app.world().get::<Visibility>(body).unwrap(),
        Visibility::Inherited
    );
}

#[test]
fn accept_parity3_cockpit_view_02_the_eye_follows_the_body_every_frame_unsmoothed() {
    let (mut app, body, camera) =
        mini_app(Some(MissionCockpitEye::new(STORED_EYE, Quat::IDENTITY)));
    *app.world_mut().resource_mut::<MissionViewMode>() = MissionViewMode::Cockpit;
    app.update();
    // Teleport the body: the very next frame the eye is already there.
    app.world_mut().get_mut::<Position>(body).unwrap().0 = Vec3::new(-900.0, 10.0, 4000.0);
    app.world_mut().get_mut::<Rotation>(body).unwrap().0 = Quat::IDENTITY;
    app.update();
    let placed = app.world().get::<Transform>(camera).unwrap().translation;
    assert!(near(placed, Vec3::new(-900.0, 10.75, 3999.8)), "{placed}");
}

#[test]
fn accept_parity3_cockpit_view_02_without_an_eye_the_chase_view_stays() {
    let (mut app, body, camera) = mini_app(None);
    *app.world_mut().resource_mut::<MissionViewMode>() = MissionViewMode::Cockpit;
    app.update();
    assert_eq!(
        app.world().get::<Transform>(camera).unwrap().translation,
        Vec3::new(1.0, 2.0, 3.0)
    );
    assert_eq!(
        *app.world().get::<Visibility>(body).unwrap(),
        Visibility::Inherited
    );
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_parity3_cockpit_view_02_retail_m01_cockpit_eye_is_the_player_cockpit_camera() {
    let root = PathBuf::from(std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR must be set"));
    let label = MissionLabel::new("M01").expect("label");
    let plan = plan_mission_launch(&root, label, "The Lost Treasure").expect("M01 plans");
    let stage = stage_for(&root, &plan).expect("M01's stage reads");
    let mut app = build_headless(&stage).expect("M01 composes");
    app.update();
    // The headless composition may carry the launch cinematic; the cockpit
    // view is for the flown mission.
    app.world_mut()
        .remove_resource::<cs_app::mission_session::LaunchCinematic>();

    // The eye is the stored translation of player_pfighter's cockpit_camera.
    let eye = *app.world().resource::<MissionCockpitEye>();
    let stored = eye.stored();
    assert!(
        (stored[1] - 0.75).abs() < 1e-6 && (stored[2] + 0.2).abs() < 1e-6,
        "{stored:?}"
    );

    let chase = {
        app.update();
        let world = app.world_mut();
        world
            .query_filtered::<&Transform, With<PlaytestCameraMarker>>()
            .single(world)
            .map(|t| t.translation)
            .expect("one camera")
    };
    *app.world_mut().resource_mut::<MissionViewMode>() = MissionViewMode::Cockpit;
    app.update();
    let world = app.world_mut();
    let (body, position, rotation) = world
        .query_filtered::<(Entity, &Position, &Rotation), With<MissionPlayerBody>>()
        .single(world)
        .map(|(e, p, r)| (e, p.0, r.0))
        .expect("one player body");
    let placed = world
        .query_filtered::<&Transform, With<PlaytestCameraMarker>>()
        .single(world)
        .copied()
        .expect("one camera");
    let expected = position + rotation * Vec3::from_array(stored);
    assert!(
        near(placed.translation, expected),
        "{} vs {expected}",
        placed.translation
    );
    assert!(placed.rotation.abs_diff_eq(rotation, 1e-5));
    assert!(
        placed.translation.distance(chase) > 5.0,
        "not the chase eye"
    );
    assert_eq!(*world.get::<Visibility>(body).unwrap(), Visibility::Hidden);

    // Back to the chase view: the body shows again and the camera is the
    // chase rig's pose (astern and above), not the eye.
    *app.world_mut().resource_mut::<MissionViewMode>() = MissionViewMode::SecondEye;
    app.update();
    *app.world_mut().resource_mut::<MissionViewMode>() = MissionViewMode::Chase;
    app.update();
    app.update();
    let world = app.world_mut();
    assert_eq!(
        *world.get::<Visibility>(body).unwrap(),
        Visibility::Inherited
    );
    let placed = world
        .query_filtered::<&Transform, With<PlaytestCameraMarker>>()
        .single(world)
        .map(|t| t.translation)
        .expect("one camera");
    let frame = world
        .resource::<PlaytestCamera>()
        .frame
        .as_ref()
        .map(|f| f.pose.position())
        .expect("the chase rig resolved a frame");
    let rig = Vec3::new(frame.x() as f32, frame.y() as f32, frame.z() as f32);
    assert!(near(placed, rig), "{placed} vs rig {rig}");
}
