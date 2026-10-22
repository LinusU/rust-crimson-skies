//! PARITY4-CRASH-C (#1855): after a fatal terrain contact the mission holds the
//! detached crash camera, keeps the run open until a key is pressed and then
//! settles the failure with the debrief read at the crash tick.
//!
//! The hull pool and the contact that empties it are the designed ones
//! (finding `2026-10-22-crash-b-fatal-terrain-contact.md`); the camera numbers
//! are the `OWNER-STATIC` `camparam.zrd` values of the crash-A finding.

use avian3d::prelude::{AngularVelocity, LinearVelocity, Position, Rotation};
use bevy::app::AppExit;
use bevy::ecs::message::{MessageCursor, Messages};
use bevy::input::ButtonState;
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::prelude::{App, Entity, KeyCode, Quat, Transform, Vec3, With};
use bevy::time::TimeUpdateStrategy;

use cs_app::mission_session::{
    MISSION_TARGET_NODE, MissionHost, MissionPlayerBody, MissionTerminalSource, MissionViewMode,
    PlayerVictim, build_headless, teardown,
};
use cs_app::playtest::PlaytestCameraMarker;
use cs_app::playtest::fatal_contact::{
    CRASH_CAMERA_HEIGHT, CRASH_CAMERA_HORIZONTAL, CRASH_PLACEHOLDER_TARGET, CrashSequence,
    FatalContactRule, crash_camera_point,
};
use cs_app::weapons::SessionDamageResolver;
use cs_sim::damage::{DamageChannel, DamageNodeKey, HitEvent};
use cs_sim::objectives::terminal::TerminalOutcome;
use cs_types::Tick;
use cs_types::net::EventId;

use super::vs_m01_rt_window::{Scratch, synthetic_stage};

pub(super) fn step(app: &mut App) {
    let timestep = app
        .world()
        .resource::<bevy::prelude::Time<bevy::time::Fixed>>()
        .timestep();
    *app.world_mut().resource_mut::<TimeUpdateStrategy>() =
        TimeUpdateStrategy::ManualDuration(timestep);
    app.update();
}

pub(super) fn exits(app: &App, cursor: &mut MessageCursor<AppExit>) -> usize {
    app.world()
        .get_resource::<Messages<AppExit>>()
        .map_or(0, |messages| cursor.read(messages).count())
}

/// Crashes the composed mission's player into a slab laid under it: 700 of the
/// designed 1200 hull are gone, so a head-on contact finishes it.
pub(super) fn crash_the_player(app: &mut App) -> Entity {
    let victim = app.world().resource::<PlayerVictim>().clone();
    let tick = Tick(
        app.world()
            .resource::<cs_app::physics::PhysicsTickLedger>()
            .ticks,
    );
    let session = app
        .world()
        .resource::<SessionDamageResolver>()
        .resolver()
        .session();
    let hit = HitEvent::try_new(
        EventId {
            session,
            tick,
            producer: 9_001,
            sequence: 0,
        },
        None,
        victim.actor,
        DamageNodeKey::new(MISSION_TARGET_NODE).expect("the hull key"),
        DamageChannel::Internal,
        700.0,
    )
    .expect("a finite hit");
    app.world_mut()
        .resource_mut::<SessionDamageResolver>()
        .resolver_mut()
        .resolve(tick, &[hit])
        .expect("the prior hit applies");

    let body = app
        .world_mut()
        .query_filtered::<Entity, With<MissionPlayerBody>>()
        .single(app.world())
        .expect("the player body");
    let spot = Vec3::new(5_000.0, 60.0, 5_000.0);
    cs_app::playtest::scene::spawn_world(app.world_mut()).expect("the slab spawns");
    let mut entity = app.world_mut().entity_mut(body);
    entity.get_mut::<Position>().unwrap().0 = spot;
    // Nose straight down: the flight law keeps the velocity along the nose.
    let nose_down = Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2);
    entity.get_mut::<Rotation>().unwrap().0 = nose_down;
    entity.get_mut::<Transform>().unwrap().rotation = nose_down;
    entity.get_mut::<LinearVelocity>().unwrap().0 = Vec3::new(0.0, -100.0, 0.0);
    entity.get_mut::<AngularVelocity>().unwrap().0 = Vec3::ZERO;
    body
}

#[test]
fn accept_crash_c_the_camera_message_and_key_hand_the_run_over() {
    let scratch = Scratch::new("crash-c-flow");
    let mut app = build_headless(&synthetic_stage(scratch.path())).expect("the stage composes");
    for _ in 0..130 {
        step(&mut app);
    }
    let mut cursor = MessageCursor::<AppExit>::default();
    let _ = exits(&app, &mut cursor);
    *app.world_mut().resource_mut::<MissionViewMode>() = MissionViewMode::Cockpit;
    let body = crash_the_player(&mut app);
    for _ in 0..240 {
        step(&mut app);
        if app.world().contains_resource::<CrashSequence>() {
            break;
        }
    }
    let sequence = *app
        .world()
        .get_resource::<CrashSequence>()
        .expect("the fatal contact started the sequence");
    assert!(
        app.world()
            .get::<FatalContactRule>(body)
            .unwrap()
            .crash
            .is_some()
    );
    assert!(!sequence.released);
    assert_eq!(sequence.target_m, CRASH_PLACEHOLDER_TARGET);

    // The detached camera: a fixed point 45 above and 30 along the heading,
    // looking at the wreck; the cockpit view the player had is gone.
    let wreck = Vec3::from_array(sequence.crash.position_m);
    let rotation = app.world().get::<Rotation>(body).unwrap().0;
    let expected = crash_camera_point(wreck, rotation);
    assert!((sequence.camera_m - expected).length() < 1.0);
    assert!(((sequence.camera_m - wreck).y - CRASH_CAMERA_HEIGHT).abs() < 1e-3);
    let flat = sequence.camera_m - wreck;
    assert!((Vec3::new(flat.x, 0.0, flat.z).length() - CRASH_CAMERA_HORIZONTAL).abs() < 1e-3);
    // A held key from before the crash does not release it; waiting does not
    // settle the run either.
    let mut early_exits = 0;
    for _ in 0..90 {
        step(&mut app);
        early_exits += exits(&app, &mut cursor);
    }
    assert_eq!(early_exits, 0, "the run stays open");
    let host = app.world().resource::<MissionHost>();
    assert!(host.terminal().is_none(), "the crash holds the settle");
    assert_eq!(
        *app.world().resource::<MissionViewMode>(),
        MissionViewMode::Chase
    );
    let camera = *app
        .world_mut()
        .query_filtered::<&Transform, With<PlaytestCameraMarker>>()
        .single(app.world())
        .expect("the camera");
    assert!(
        (camera.translation - sequence.camera_m).length() < 1e-3,
        "the camera is held at the crash point"
    );
    let toward = (app.world().get::<Position>(body).unwrap().0 - camera.translation).normalize();
    assert!(
        camera.forward().dot(toward) > 0.99,
        "and looks at the wreck"
    );

    // Any key releases it; the failure settles with the debrief of the crash
    // tick, not of the key press.
    app.world_mut().write_message(KeyboardInput {
        key_code: KeyCode::Space,
        logical_key: Key::Space,
        state: ButtonState::Pressed,
        text: None,
        repeat: false,
        window: Entity::PLACEHOLDER,
    });
    let mut exit_messages = 0;
    for _ in 0..6 {
        step(&mut app);
        exit_messages += exits(&app, &mut cursor);
    }
    assert_eq!(exit_messages, 1, "exactly one AppExit");
    let terminal = app
        .world()
        .resource::<MissionHost>()
        .terminal()
        .expect("the key handed the run over")
        .clone();
    assert_eq!(terminal.outcome, TerminalOutcome::Failure);
    assert_eq!(terminal.source, MissionTerminalSource::PlayerDestroyed);
    assert_eq!(terminal.exit.code(), 1);
    let debrief = terminal.debrief.expect("the debrief");
    assert_eq!(debrief.run_ticks, sequence.crash.tick);
    assert!(terminal.tick.0 > sequence.crash.tick + 90);
    teardown(&mut app);
    assert!(!app.world().contains_resource::<CrashSequence>());
}

/// The installation's own `Fatal Crash!` (string id 162, name `MSG_CRASH`).
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_crash_c_the_installation_names_the_message() {
    use cs_app::playtest::fatal_contact::FATAL_CRASH_MESSAGE;
    use cs_app::ui::hud::MissionMessages;
    use cs_content::config::MessageLookup;

    let root = std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR must be set");
    let messages = MissionMessages::read(
        std::path::Path::new(&root),
        cs_types::evidence::ContentHash::from_bytes([0; 32]),
    );
    let catalog = messages.catalog().expect("strings.dll reads");
    let MessageLookup::Found { row, .. } = catalog.resolve(FATAL_CRASH_MESSAGE, None) else {
        panic!("MSG_CRASH does not resolve");
    };
    assert_eq!(row.text.as_deref(), Some("Fatal Crash!"));
}
