//! Acceptance for Rally #1825 `PARITY3-CAMERA-MODES.03`: snap look `K`,
//! smooth look `J`, track target `L` and the numpad look directions drive the
//! mission's look state through the production minting path, and the camera
//! turns by it (finding
//! `docs/findings/2026-10-21-parity3-camera-modes-01-measured.md`, "Look").

use std::f32::consts::{FRAC_PI_2, FRAC_PI_4};
use std::time::Duration;

use avian3d::prelude::{Position, Rotation};
use bevy::app::{App, PostUpdate, Update};
use bevy::ecs::world::World;
use bevy::prelude::{ButtonInput, Entity, KeyCode, Quat, Time, Transform, Vec3};
use bevy::time::Real;
use cs_app::mission_session::{
    CycleMemory, HeldPoint, LookMode, LookTarget, MissionControls, MissionLook, MissionPlayerBody,
    MissionViewMode, MissionZoom, PITCH_SMOOTH_RATE, YAW_SMOOTH_RATE, apply_look, look_direction,
    mint_original_chord_keys, step_look,
};
use cs_app::playtest::PlaytestCameraMarker;
use cs_app::scene::SceneGeneration;
use cs_app::targeting::{
    TargetableBinding, TargetableState, TargetingSession, apply_selection_edges, lower_rules,
    lower_selection_actions, sync_targetable_roster,
};
use cs_content::target_rules::{
    declared_synthetic_selection_actions, declared_synthetic_target_rules,
};
use cs_sim::damage::ActorId;
use cs_sim::targeting::{SelectionFrame, TargetClass};
use cs_types::Tick;
use cs_types::input::{Action, FlightCommand};
use cs_types::keymap::ControlsMap;
use cs_types::net::SessionId;
use cs_types::space::WorldPosition;

const DT: f32 = 1.0 / 60.0;
const BODY_AT: Vec3 = Vec3::new(100.0, 50.0, -20.0);

fn mini_app() -> (App, Entity) {
    let mut app = App::new();
    app.insert_resource(ButtonInput::<KeyCode>::default());
    app.insert_resource(MissionControls(ControlsMap::original_default()));
    app.init_resource::<MissionViewMode>();
    app.init_resource::<CycleMemory>();
    app.init_resource::<MissionZoom>();
    app.init_resource::<HeldPoint>();
    app.init_resource::<MissionLook>();
    app.init_resource::<Time<Real>>();
    app.world_mut()
        .resource_mut::<Time<Real>>()
        .advance_by(Duration::from_secs_f32(DT));
    app.add_systems(Update, step_look);
    app.add_systems(PostUpdate, apply_look);
    app.world_mut().spawn((
        MissionPlayerBody,
        Position(BODY_AT),
        Rotation(Quat::from_rotation_y(0.7)),
    ));
    let camera = app
        .world_mut()
        .spawn((PlaytestCameraMarker, Transform::from_xyz(1.0, 2.0, 3.0)))
        .id();
    (app, camera)
}

fn press(app: &mut App, key: KeyCode) {
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(key);
    mint_original_chord_keys(app.world_mut());
}

fn release(app: &mut App, key: KeyCode) {
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .release(key);
    mint_original_chord_keys(app.world_mut());
}

fn tap(app: &mut App, key: KeyCode) {
    press(app, key);
    release(app, key);
}

fn run(app: &mut App, frames: u32) {
    for _ in 0..frames {
        app.update();
    }
}

fn look(app: &App) -> MissionLook {
    *app.world().resource::<MissionLook>()
}

#[test]
fn accept_parity3_camera_modes_03_the_look_mode_keys_set_the_recorded_modes() {
    let (mut app, _) = mini_app();
    assert_eq!(look(&app).mode, LookMode::Snap);
    tap(&mut app, KeyCode::KeyJ);
    assert_eq!(look(&app).mode, LookMode::Smooth);
    tap(&mut app, KeyCode::KeyK);
    assert_eq!(look(&app).mode, LookMode::Snap);
    // Track target toggles with snap.
    tap(&mut app, KeyCode::KeyL);
    assert_eq!(look(&app).mode, LookMode::Track);
    tap(&mut app, KeyCode::KeyL);
    assert_eq!(look(&app).mode, LookMode::Snap);
    // Smooth look zeroes the look state.
    *app.world_mut().resource_mut::<MissionLook>() = MissionLook {
        yaw: 1.0,
        pitch: 0.5,
        yaw_target: 1.0,
        pitch_target: 0.5,
        ..MissionLook::default()
    };
    tap(&mut app, KeyCode::KeyJ);
    let state = look(&app);
    assert_eq!((state.yaw, state.pitch, state.yaw_target), (0.0, 0.0, 0.0));
}

#[test]
fn accept_parity3_camera_modes_03_the_direction_table_is_the_recorded_one() {
    assert_eq!(look_direction(58), Some((1, -1)));
    assert_eq!(look_direction(59), Some((0, -1)));
    assert_eq!(look_direction(61), Some((1, 0)));
    assert_eq!(look_direction(62), Some((0, 0)));
    assert_eq!(look_direction(63), Some((-1, 0)));
    assert_eq!(look_direction(65), Some((0, 1)));
    assert_eq!(look_direction(66), Some((-1, 1)));
    assert_eq!(look_direction(57), None);
    assert_eq!(look_direction(67), None);
}

#[test]
fn accept_parity3_camera_modes_03_snap_look_targets_the_recorded_angles_and_smooths_to_them() {
    let (mut app, _) = mini_app();
    for (key, yaw, pitch) in [
        (KeyCode::Numpad8, 0.0, FRAC_PI_2),
        (KeyCode::Numpad4, FRAC_PI_2, 0.0),
        (KeyCode::Numpad6, -FRAC_PI_2, 0.0),
        (KeyCode::Numpad7, FRAC_PI_4, FRAC_PI_4),
        (KeyCode::Numpad9, -FRAC_PI_4, FRAC_PI_4),
        (KeyCode::Numpad2, std::f32::consts::PI, 0.0),
    ] {
        press(&mut app, key);
        run(&mut app, 1);
        let state = look(&app);
        assert!((state.yaw_target - yaw).abs() < 1e-5, "{key:?} yaw");
        assert!((state.pitch_target - pitch).abs() < 1e-5, "{key:?} pitch");
        run(&mut app, 300);
        let state = look(&app);
        assert!((state.yaw - yaw).abs() < 1e-3, "{key:?} settled yaw");
        assert!((state.pitch - pitch).abs() < 1e-3, "{key:?} settled pitch");
        release(&mut app, key);
    }
    // No key: both targets return to zero.
    run(&mut app, 240);
    let state = look(&app);
    assert_eq!((state.yaw_target, state.pitch_target), (0.0, 0.0));
    assert!(state.yaw.abs() < 1e-3 && state.pitch.abs() < 1e-3);
}

#[test]
fn accept_parity3_camera_modes_03_smoothing_uses_the_measured_rates() {
    let mut state = MissionLook {
        pitch_target: 1.0,
        yaw_target: 1.0,
        ..MissionLook::default()
    };
    // Targets already set and no key: the snap branch zeroes them, so use the
    // smooth branch (no key held leaves the targets alone).
    state.mode = LookMode::Smooth;
    state.step(0.1, LookTarget::default());
    assert!((state.pitch - (1.0 - (-PITCH_SMOOTH_RATE * 0.1_f32).exp())).abs() < 1e-6);
    assert!((state.yaw - (1.0 - (-YAW_SMOOTH_RATE * 0.1_f32).exp())).abs() < 1e-6);
}

#[test]
fn accept_parity3_camera_modes_03_smooth_look_moves_the_targets_and_keeps_them() {
    let (mut app, _) = mini_app();
    tap(&mut app, KeyCode::KeyJ);
    press(&mut app, KeyCode::Numpad4);
    run(&mut app, 30);
    let held = look(&app);
    // Two radians per second for half a second.
    assert!((held.yaw_target - 1.0).abs() < 1e-3, "{}", held.yaw_target);
    assert_eq!(held.pitch_target, 0.0);
    release(&mut app, KeyCode::Numpad4);
    run(&mut app, 30);
    assert!(
        (look(&app).yaw_target - held.yaw_target).abs() < 1e-6,
        "the smooth target stays when the key is released"
    );
    // Pitch clamps at pi/2.
    press(&mut app, KeyCode::Numpad8);
    run(&mut app, 240);
    assert!((look(&app).pitch_target - FRAC_PI_2).abs() < 1e-5);
    // Yaw wraps instead of growing without bound.
    release(&mut app, KeyCode::Numpad8);
    press(&mut app, KeyCode::Numpad4);
    run(&mut app, 600);
    assert!(look(&app).yaw_target.abs() <= std::f32::consts::PI + 1e-4);
}

#[test]
fn accept_parity3_camera_modes_03_the_look_turns_the_pilot_eye_camera() {
    let (mut app, camera) = mini_app();
    tap(&mut app, KeyCode::F8); // memory 6 -> mode 0... then 7
    tap(&mut app, KeyCode::F8);
    assert!(app.world().resource::<MissionViewMode>().at_pilot_eye());
    let body = Quat::from_rotation_y(0.7);
    app.world_mut()
        .get_mut::<Transform>(camera)
        .unwrap()
        .rotation = body;
    press(&mut app, KeyCode::Numpad4);
    run(&mut app, 120);
    let turned = app.world().get::<Transform>(camera).unwrap().rotation;
    let forward = turned * Vec3::NEG_Z;
    let expected = body * (Quat::from_rotation_y(FRAC_PI_2) * Vec3::NEG_Z);
    assert!(
        forward.dot(expected) > 0.999,
        "looks to the left of the nose"
    );
}

#[test]
fn accept_parity3_camera_modes_03_the_look_orbits_the_chase_camera_without_accumulating() {
    let (mut app, camera) = mini_app();
    assert_eq!(
        *app.world().resource::<MissionViewMode>(),
        MissionViewMode::Chase
    );
    let rotation = Quat::from_rotation_y(0.7);
    let chase = BODY_AT + rotation * Vec3::new(0.0, 0.0, 12.0);
    let write = |app: &mut App| {
        app.world_mut()
            .get_mut::<Transform>(camera)
            .unwrap()
            .translation = chase;
    };
    press(&mut app, KeyCode::Numpad4);
    for _ in 0..120 {
        write(&mut app);
        app.update();
    }
    let placed = *app.world().get::<Transform>(camera).unwrap();
    // Looking left from behind: the camera sits to the aircraft's right.
    let right = rotation * Vec3::X;
    assert!((placed.translation.distance(BODY_AT) - 12.0).abs() < 1e-3);
    assert!((placed.translation - BODY_AT).normalize().dot(right) > 0.99);
    assert!(
        placed
            .forward()
            .dot((BODY_AT - placed.translation).normalize())
            > 0.999
    );
    // Rewriting the frame and running again lands in the same place.
    write(&mut app);
    app.update();
    let again = app.world().get::<Transform>(camera).unwrap().translation;
    assert!(again.distance(placed.translation) < 1e-2);
}

#[test]
fn accept_parity3_camera_modes_03_entering_the_chase_or_an_eye_view_clears_the_look() {
    let (mut app, _) = mini_app();
    press(&mut app, KeyCode::Numpad8);
    run(&mut app, 120);
    release(&mut app, KeyCode::Numpad8);
    *app.world_mut().resource_mut::<MissionLook>() = MissionLook {
        yaw: 0.8,
        pitch: 0.4,
        yaw_target: 0.8,
        pitch_target: 0.4,
        ..MissionLook::default()
    };
    tap(&mut app, KeyCode::F8); // chase -> eye
    let state = look(&app);
    assert_eq!((state.yaw, state.pitch), (0.0, 0.0));
    *app.world_mut().resource_mut::<MissionLook>() = MissionLook {
        yaw: 0.8,
        ..MissionLook::default()
    };
    tap(&mut app, KeyCode::F10); // an external view keeps it
    assert_eq!(look(&app).yaw, 0.8);
    tap(&mut app, KeyCode::F8);
    assert_eq!(look(&app).yaw, 0.0);
}

// --- track target ---------------------------------------------------------

const SESSION: u64 = 7;

fn actor(serial: u64) -> ActorId {
    ActorId {
        session: SessionId::new(SESSION).unwrap(),
        serial,
    }
}

fn targeting_world(app: &mut App) {
    let generation = SceneGeneration::default().next();
    let declared = declared_synthetic_target_rules();
    let lowered = lower_rules(&declared).unwrap();
    let bindings = lower_selection_actions(&declared_synthetic_selection_actions()).unwrap();
    let world: &mut World = app.world_mut();
    world.insert_resource(TargetingSession::new(
        SessionId::new(SESSION).unwrap(),
        lowered,
        bindings,
        declared.subject().clone(),
        generation,
    ));
    let player = cs_sim::targeting::synthetic_player_faction();
    let raiders = cs_sim::targeting::synthetic_raider_faction();
    // The player at the origin and one raider 100 m along -Z.
    for (serial, faction, at) in [
        (1, player, [0.0, 0.0, 0.0]),
        (5, raiders, [0.0, 0.0, -100.0]),
    ] {
        let mut state = TargetableState::aircraft(faction, WorldPosition::try_new(at).unwrap());
        state.class = TargetClass::Aircraft;
        world.spawn((
            TargetableBinding {
                actor: actor(serial),
                rules: declared.subject().clone(),
                generation,
            },
            state,
        ));
    }
    sync_targetable_roster(world);
}

#[test]
fn accept_parity3_camera_modes_03_track_target_follows_the_selected_target_and_changes_nothing() {
    let (mut app, _) = mini_app();
    // The body at the origin turned a quarter turn to the left: the raider
    // 100 m along -Z is then to its right.
    let mut bodies = app.world_mut().query::<(&mut Position, &mut Rotation)>();
    for (mut position, mut rotation) in bodies.iter_mut(app.world_mut()) {
        position.0 = Vec3::ZERO;
        rotation.0 = Quat::from_rotation_y(FRAC_PI_2);
    }
    targeting_world(&mut app);
    tap(&mut app, KeyCode::KeyL);
    run(&mut app, 30);
    // Nothing selected: both targets zero.
    assert_eq!(look(&app).mode, LookMode::Track);
    assert_eq!((look(&app).yaw_target, look(&app).pitch_target), (0.0, 0.0));

    apply_selection_edges(
        app.world_mut(),
        actor(1),
        &[Action::Flight(FlightCommand::TargetNext)],
        SelectionFrame::at(Tick(20)),
    )
    .unwrap();
    let before = app.world().resource::<TargetingSession>().clone();
    assert_eq!(before.selection().current(), Some(actor(5)));
    run(&mut app, 240);
    let state = look(&app);
    assert!(
        (state.yaw_target + FRAC_PI_2).abs() < 1e-4,
        "right of the nose is negative yaw: {}",
        state.yaw_target
    );
    assert!(state.pitch_target.abs() < 1e-4);
    assert!(
        (state.yaw + FRAC_PI_2).abs() < 1e-3,
        "and the camera reads it"
    );

    // The middle key recentres in track mode.
    press(&mut app, KeyCode::Numpad5);
    run(&mut app, 2);
    assert_eq!(look(&app).yaw_target, 0.0);
    release(&mut app, KeyCode::Numpad5);

    // Look never selects, drops or retargets.
    let after = app.world().resource::<TargetingSession>();
    assert_eq!(after.selection().current(), before.selection().current());
    assert_eq!(
        after.store().registered().len(),
        before.store().registered().len()
    );
}
