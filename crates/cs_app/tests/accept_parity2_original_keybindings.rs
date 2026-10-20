//! Acceptance for Rally #1733 `PARITY2-ORIGINAL-KEYBINDINGS`: the player-
//! facing mission flies on the original game's default controls.
//!
//! The table is static evidence (`OWNER-STATIC-2026-10-05`), never
//! `verified_original`; the pinned codes live in `cs_types`' own acceptance
//! test. This file drives the production seams the mission uses: the
//! `ControlsMap`-derived action map through a real `InputSession`, the raw-key
//! chord minter, and the help/pause screen text.
//!
//! Every fixture is newly authored — no original game content is exercised.

use bevy::ecs::world::World;
use bevy::prelude::{ButtonInput, KeyCode};

use cs_app::input::{DeviceEvent, FrameInput, InputSession, SessionMode};
use cs_app::mission_session::{
    MissionCockpitActions, MissionControls, controls_screen_text, mint_original_chord_keys,
    original_mission_action_map,
};
use cs_sim::control::LocalSeatId;
use cs_types::Tick;
use cs_types::input::{Action, DeviceClass, DeviceId, FlightCommand, Key};
use cs_types::keymap::{ControlInput, ControlSlot, ControlsMap};

fn keyboard() -> DeviceId {
    DeviceId::stable(DeviceClass::Keyboard, "keyboard.fixture/0").expect("a valid identity")
}

fn session(controls: &ControlsMap) -> InputSession {
    let mut session = InputSession::new(
        original_mission_action_map(controls),
        LocalSeatId(0),
        SessionMode::SinglePlayer,
        Tick(0),
    );
    session
        .collector_mut()
        .connect_device(keyboard())
        .expect("the keyboard connects");
    session
}

fn press(session: &mut InputSession, keys: &[Key]) -> cs_app::input::FrameOutcome {
    session
        .pump_frame(
            FrameInput::Devices(&[DeviceEvent::KeyboardFrame {
                device: keyboard(),
                keys: keys.to_vec(),
            }]),
            1,
        )
        .expect("the frame applies")
}

#[test]
fn accept_parity2_original_keybindings_digits_set_the_throttle_through_the_session() {
    let controls = ControlsMap::original_default();
    for (key, expected) in [
        (Key::Digit1, 0.0),
        (Key::Digit5, 0.5),
        (Key::Digit9, 1.0),
        (Key::Digit2, 0.125),
    ] {
        let mut session = session(&controls);
        press(&mut session, &[key]);
        assert!(
            (session.throttle().position() - expected).abs() < 1e-6,
            "{key:?} must set the throttle to {expected}, got {}",
            session.throttle().position()
        );
    }
    // `=` steps up, `-` steps down.
    let mut stepped = session(&controls);
    press(&mut stepped, &[Key::Equal]);
    let up = stepped.throttle().position();
    assert!(up > 0.0, "= steps the throttle up");
    press(&mut stepped, &[]);
    press(&mut stepped, &[Key::Minus]);
    assert!(stepped.throttle().position() < up, "- steps it down");
}

#[test]
fn accept_parity2_original_keybindings_arrows_and_rudder_keys_deflect_the_axes() {
    let controls = ControlsMap::original_default();
    let mut session = session(&controls);
    press(&mut session, &[Key::ArrowDown, Key::ArrowLeft, Key::Period]);
    let axis = |session: &InputSession, command| session.controls().axis(command).unwrap_or(0.0);
    assert!(
        axis(&session, FlightCommand::Pitch) > 0.0,
        "Down is nose up"
    );
    assert!(axis(&session, FlightCommand::Roll) < 0.0, "Left rolls left");
    assert!(
        axis(&session, FlightCommand::Yaw) > 0.0,
        "Period is rudder right"
    );
    // The developer keys carry nothing in the original layout.
    let mut developer = self::session(&controls);
    press(&mut developer, &[Key::W, Key::A, Key::D, Key::F]);
    for command in [
        FlightCommand::Pitch,
        FlightCommand::Roll,
        FlightCommand::Yaw,
    ] {
        assert_eq!(axis(&developer, command), 0.0, "{command:?}");
    }
}

fn mint(controls: ControlsMap, press: KeyCode, held: &[KeyCode]) -> Vec<Action> {
    let mut world = World::new();
    let mut keys = ButtonInput::<KeyCode>::default();
    for key in held {
        keys.press(*key);
    }
    for key in held {
        keys.clear_just_pressed(*key);
    }
    keys.press(press);
    world.insert_resource(keys);
    world.insert_resource(MissionControls(controls));
    mint_original_chord_keys(&mut world);
    world
        .get_resource::<MissionCockpitActions>()
        .map(|queued| queued.actions.clone())
        .unwrap_or_default()
}

#[test]
fn accept_parity2_original_keybindings_chords_follow_the_exact_modifier_rule() {
    let flight = |command| vec![Action::Flight(command)];
    let table = ControlsMap::original_default;
    assert_eq!(
        mint(table(), KeyCode::KeyX, &[]),
        flight(FlightCommand::FireSecondary)
    );
    assert_eq!(
        mint(table(), KeyCode::KeyX, &[KeyCode::ControlLeft]),
        flight(FlightCommand::Eject)
    );
    assert!(mint(table(), KeyCode::KeyX, &[KeyCode::ShiftLeft]).is_empty());
    assert_eq!(
        mint(table(), KeyCode::KeyL, &[KeyCode::ShiftLeft]),
        flight(FlightCommand::LevelOff)
    );
    assert_eq!(
        mint(table(), KeyCode::KeyE, &[KeyCode::ShiftRight]),
        flight(FlightCommand::TargetPrevEnemy)
    );
    assert!(
        mint(
            table(),
            KeyCode::KeyE,
            &[KeyCode::ShiftLeft, KeyCode::ControlLeft]
        )
        .is_empty()
    );
    // A key with no chord bound is the session's, not the minter's.
    assert!(mint(table(), KeyCode::KeyQ, &[]).is_empty());
    assert!(mint(table(), KeyCode::Space, &[]).is_empty());
}

#[test]
fn accept_parity2_original_keybindings_the_spyglass_key_stays_visible_to_its_raw_reader() {
    // The spyglass reads `Shift+S` from the raw keyboard; the minter must not
    // clear that edge, while it still consumes the keys it mints itself.
    let mut world = World::new();
    let mut keys = ButtonInput::<KeyCode>::default();
    keys.press(KeyCode::ShiftLeft);
    keys.clear_just_pressed(KeyCode::ShiftLeft);
    keys.press(KeyCode::KeyS);
    keys.press(KeyCode::KeyE);
    world.insert_resource(keys);
    world.insert_resource(MissionControls(ControlsMap::original_default()));
    mint_original_chord_keys(&mut world);
    let keys = world.resource::<ButtonInput<KeyCode>>();
    assert!(keys.just_pressed(KeyCode::KeyS));
    assert!(!keys.just_pressed(KeyCode::KeyE));
}

#[test]
fn accept_parity2_original_keybindings_a_rebinding_reaches_the_live_inputs() {
    // Move Fire Rockets to `Z` (no chord anywhere): the session now delivers
    // it from the mapped key, and `X` falls back to bail-out only.
    let mut controls = ControlsMap::original_default();
    controls
        .assign(
            20,
            ControlSlot::KeyA,
            ControlInput::Key(cs_types::input::OriginalKeyCode::new(0x2c)),
        )
        .expect("rebinds");
    let mut session = session(&controls);
    let outcome = press(&mut session, &[Key::Z]);
    assert_eq!(
        outcome.delivered,
        vec![Action::Flight(FlightCommand::FireSecondary)]
    );
    assert_eq!(
        mint(controls.clone(), KeyCode::KeyX, &[]),
        Vec::<Action>::new(),
        "plain X no longer runs anything once rockets moved off it"
    );
    assert_eq!(
        mint(controls, KeyCode::KeyX, &[KeyCode::ControlLeft]),
        vec![Action::Flight(FlightCommand::Eject)]
    );
}

#[test]
fn accept_parity2_original_keybindings_help_and_pause_screens_show_the_real_bindings() {
    let controls = ControlsMap::original_default();
    let help = controls_screen_text(&controls, false, true);
    assert!(help.starts_with("HELP"), "{help}");
    for needle in [
        "Fire guns Space/Joy1",
        "Fire rockets X/Joy2",
        "Throttle full 9",
        "Rudder left ,/Num0",
        "Previous enemy Shift+E",
        "Help F1",
    ] {
        assert!(help.contains(needle), "{needle} missing from {help}");
    }
    let pause = controls_screen_text(&controls, true, false);
    assert!(pause.starts_with("PAUSED"), "{pause}");
    assert!(
        pause.contains("Esc") && pause.contains("Pause Esc"),
        "{pause}"
    );
}

#[test]
fn accept_parity2_original_keybindings_committed_facts_match_the_recorded_table() {
    let facts = cs_app::engine_facts::engine_facts().expect("the committed table parses");
    let rows = facts.control_bindings();
    assert_eq!(rows.len(), 64, "one committed row per command");
    for (fact, recorded) in rows.iter().zip(cs_types::input::ORIGINAL_DEFAULT_BINDINGS) {
        assert_eq!(fact.command, recorded.command_id);
        assert_eq!(
            fact.key_a,
            recorded.key_a.code(),
            "command {}",
            fact.command
        );
        assert_eq!(
            fact.key_b,
            recorded.key_b.code(),
            "command {}",
            fact.command
        );
        assert_eq!(fact.joystick, recorded.joystick_button);
        assert_eq!(fact.mouse, recorded.mouse_button);
        assert_eq!(fact.va, u64::from(recorded.registration_va));
    }
    // The live control map starts from exactly these committed rows.
    let map = ControlsMap::original_default();
    for (fact, row) in rows.iter().zip(map.rows()) {
        assert_eq!(
            (fact.command, fact.key_a, fact.key_b, fact.joystick),
            (
                row.command_id,
                row.key_a.code(),
                row.key_b.code(),
                row.joystick_button
            )
        );
    }
}
