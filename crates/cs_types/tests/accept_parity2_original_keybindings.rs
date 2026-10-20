//! Acceptance for Rally #1733 `PARITY2-ORIGINAL-KEYBINDINGS`: the original
//! game's default bindings are the player-facing default map, with the
//! original's modifier-exactness and slot-uniqueness rules.
//!
//! The pinned table below is the owner's static measurement
//! (`OWNER-STATIC-2026-10-05`, table builder `0x4936c0`–`0x493d00`) as ids
//! and packed codes only — static evidence, never `verified_original`. It is
//! written out independently of `ORIGINAL_DEFAULT_BINDINGS` so changing either
//! the recorded table or the production default fails here.

use cs_types::input::{
    Action, BindingSource, BindingTarget, FlightCommand, Key, MouseButton, OriginalKeyCode,
    UiAction,
};
use cs_types::keymap::{
    ControlInput, ControlSlot, ControlsError, ControlsMap, Modifiers, original_edge_action,
};

/// `(command id, key A, key B, joystick button)` for all 64 commands.
const PINNED: [(u16, u16, u16, u8); 64] = [
    (1, 0x0c8, 0, 0),
    (2, 0x0d0, 0, 0),
    (3, 0x0cb, 0, 0),
    (4, 0x0cd, 0, 0),
    (5, 0x033, 0x052, 0),
    (6, 0x034, 0x053, 0),
    (47, 0x426, 0, 0),
    (7, 0x00d, 0, 0),
    (8, 0x00c, 0, 0),
    (9, 0x002, 0, 0),
    (10, 0x003, 0, 0),
    (11, 0x004, 0, 0),
    (12, 0x005, 0, 0),
    (13, 0x006, 0, 0),
    (14, 0x007, 0, 0),
    (15, 0x008, 0, 0),
    (16, 0x009, 0, 0),
    (17, 0x00a, 0, 0),
    (19, 0x039, 0, 1),
    (20, 0x02d, 0, 2),
    (25, 0x03d, 0, 0),
    (26, 0x03e, 0, 7),
    (32, 0x03f, 0, 0),
    (33, 0x040, 0, 8),
    (36, 0x012, 0, 3),
    (37, 0x412, 0, 0),
    (38, 0x212, 0, 0),
    (39, 0x011, 0, 0),
    (40, 0x411, 0, 0),
    (41, 0x211, 0, 0),
    (42, 0x013, 0, 6),
    (43, 0x413, 0, 0),
    (44, 0x213, 0, 0),
    (45, 0x010, 0, 0),
    (46, 0x014, 0, 0),
    (48, 0x41f, 0, 5),
    (49, 0x042, 0, 4),
    (50, 0x043, 0, 0),
    (51, 0x044, 0, 0),
    (52, 0x057, 0, 0),
    (53, 0x058, 0, 0),
    (54, 0x041, 0, 0),
    (55, 0x025, 0, 0),
    (57, 0x024, 0, 0),
    (56, 0x026, 0, 0),
    (58, 0x04f, 0, 0),
    (59, 0x050, 0, 0),
    (60, 0x051, 0, 0),
    (61, 0x04b, 0, 0),
    (62, 0x04c, 0, 0),
    (63, 0x04d, 0, 0),
    (64, 0x047, 0, 0),
    (65, 0x048, 0, 0),
    (66, 0x049, 0, 0),
    (68, 0x04a, 0, 0),
    (67, 0x04e, 0, 0),
    (106, 0x01e, 0, 0),
    (18, 0x031, 0, 0),
    (34, 0x22d, 0, 0),
    (69, 0x001, 0, 0),
    (35, 0x00f, 0, 0),
    (109, 0x029, 0, 0),
    (110, 0x429, 0, 0),
    (111, 0x03b, 0, 0),
];

#[test]
fn accept_parity2_original_keybindings_default_map_is_the_pinned_table() {
    let map = ControlsMap::original_default();
    assert_eq!(map.rows().len(), 64);
    for (row, (id, key_a, key_b, joy)) in map.rows().iter().zip(PINNED) {
        assert_eq!(row.command_id, id);
        assert_eq!(row.key_a.code(), key_a, "command {id} key A");
        assert_eq!(row.key_b.code(), key_b, "command {id} key B");
        assert_eq!(row.joystick_button, joy, "command {id} joystick");
        assert_eq!(row.mouse_button, 0, "command {id}: no default mouse button");
    }
}

#[test]
fn accept_parity2_original_keybindings_action_map_carries_the_original_keys() {
    let map = ControlsMap::original_default().to_action_map(1.0);
    let target = |key: Key| {
        map.targets_for(BindingSource::Key(key))
            .find(|target| !matches!(target, BindingTarget::Ui(UiAction::NavigateUp)))
    };
    let command = |key: Key| match target(key) {
        Some(BindingTarget::Command(command)) => Some(command),
        _ => None,
    };
    let axis = |key: Key| match target(key) {
        Some(BindingTarget::Axis { command, scale }) => Some((command, scale)),
        _ => None,
    };
    assert_eq!(axis(Key::ArrowUp), Some((FlightCommand::Pitch, -1.0)));
    assert_eq!(axis(Key::ArrowDown), Some((FlightCommand::Pitch, 1.0)));
    assert_eq!(axis(Key::ArrowLeft), Some((FlightCommand::Roll, -1.0)));
    assert_eq!(axis(Key::ArrowRight), Some((FlightCommand::Roll, 1.0)));
    assert_eq!(axis(Key::Comma), Some((FlightCommand::Yaw, -1.0)));
    assert_eq!(axis(Key::Period), Some((FlightCommand::Yaw, 1.0)));
    assert_eq!(axis(Key::Numpad0), Some((FlightCommand::Yaw, -1.0)));
    assert_eq!(axis(Key::NumpadDecimal), Some((FlightCommand::Yaw, 1.0)));
    assert_eq!(command(Key::Equal), Some(FlightCommand::ThrottleStepUp));
    assert_eq!(command(Key::Minus), Some(FlightCommand::ThrottleStepDown));
    assert_eq!(command(Key::Digit1), Some(FlightCommand::ThrottleIdle));
    assert_eq!(command(Key::Digit5), Some(FlightCommand::ThrottleLevel4));
    assert_eq!(command(Key::Digit9), Some(FlightCommand::ThrottleFull));
    assert_eq!(command(Key::Space), Some(FlightCommand::FirePrimary));
    assert_eq!(command(Key::Q), Some(FlightCommand::TargetUnderReticule));
    assert_eq!(command(Key::T), Some(FlightCommand::TargetNothing));
    assert_eq!(
        map.target_for(BindingSource::Key(Key::Escape)),
        Some(BindingTarget::Ui(UiAction::Pause))
    );
    assert_eq!(
        map.target_for(BindingSource::JoystickButton(0)),
        Some(BindingTarget::Command(FlightCommand::FirePrimary))
    );
    // Keys with modifier variants are delivered by the chord minter, not the
    // map: E/W/R/X/L/S, and none of the old developer keys survive.
    for key in [Key::E, Key::W, Key::R, Key::X, Key::L, Key::S] {
        assert_eq!(
            map.targets_for(BindingSource::Key(key)).count(),
            0,
            "{key:?}"
        );
    }
    for key in [Key::A, Key::D, Key::F, Key::C, Key::Z, Key::G, Key::B] {
        assert!(
            !matches!(target(key), Some(BindingTarget::Axis { .. })),
            "{key:?} must not carry a developer flight axis"
        );
    }
    assert!(
        map.targets_for(BindingSource::MouseButton(MouseButton::Left))
            .next()
            .is_none(),
        "the original binds no default mouse button"
    );
}

#[test]
fn accept_parity2_original_keybindings_modifier_exactness() {
    let map = ControlsMap::original_default();
    let shift = Modifiers {
        shift: true,
        ..Modifiers::NONE
    };
    let ctrl = Modifiers {
        ctrl: true,
        ..Modifiers::NONE
    };
    let both = Modifiers {
        shift: true,
        ctrl: true,
        alt: false,
    };
    // E: plain next, Shift previous, Ctrl nearest, Ctrl+Shift nothing.
    assert_eq!(map.commands_for_key(0x12, Modifiers::NONE), vec![36]);
    assert_eq!(map.commands_for_key(0x12, shift), vec![37]);
    assert_eq!(map.commands_for_key(0x12, ctrl), vec![38]);
    assert!(map.commands_for_key(0x12, both).is_empty());
    // Q has no modifier combination bound: held modifiers are ignored.
    assert_eq!(map.commands_for_key(0x10, Modifiers::NONE), vec![45]);
    assert_eq!(map.commands_for_key(0x10, shift), vec![45]);
    assert_eq!(map.commands_for_key(0x10, both), vec![45]);
    // X: plain rockets, Ctrl bail out; Shift+X nothing.
    assert_eq!(map.commands_for_key(0x2d, Modifiers::NONE), vec![20]);
    assert_eq!(map.commands_for_key(0x2d, ctrl), vec![34]);
    assert!(map.commands_for_key(0x2d, shift).is_empty());
    // L: plain track-target, Shift level-off; Ctrl+L nothing.
    assert_eq!(map.commands_for_key(0x26, shift), vec![47]);
    assert_eq!(map.commands_for_key(0x26, Modifiers::NONE), vec![56]);
    assert!(map.commands_for_key(0x26, ctrl).is_empty());
    // S: only Shift+S (spyglass) is bound; plain S runs nothing.
    assert_eq!(map.commands_for_key(0x1f, shift), vec![48]);
    assert!(map.commands_for_key(0x1f, Modifiers::NONE).is_empty());
    assert_eq!(
        original_edge_action(47),
        Some(Action::Flight(FlightCommand::LevelOff))
    );
}

#[test]
fn accept_parity2_original_keybindings_slot_uniqueness() {
    let mut map = ControlsMap::original_default();
    // Moving key `E` (0x012) to Fire Guns key A clears it from Next Enemy
    // (key A) and leaves Fire Guns' old Space free.
    let cleared = map
        .assign(
            19,
            ControlSlot::KeyA,
            ControlInput::Key(OriginalKeyCode::new(0x012)),
        )
        .expect("a known command and a key");
    assert_eq!(cleared, vec![36]);
    assert!(map.row(36).expect("row").key_a.is_none());
    assert_eq!(map.row(19).expect("row").key_a.code(), 0x012);
    // The same code in the other key slot is a different slot: nothing else
    // loses it.
    let cleared = map
        .assign(
            20,
            ControlSlot::KeyB,
            ControlInput::Key(OriginalKeyCode::new(0x012)),
        )
        .expect("assigns");
    assert!(cleared.is_empty());
    // Joystick button 1 moves from Fire Guns to Fire Rockets.
    let cleared = map
        .assign(
            20,
            ControlSlot::JoystickButton,
            ControlInput::JoystickButton(1),
        )
        .expect("assigns");
    assert_eq!(cleared, vec![19]);
    assert_eq!(map.row(19).expect("row").joystick_button, 0);
    assert_eq!(map.row(20).expect("row").joystick_button, 1);
    // Mouse button 1 is free by default; a second assignment takes it over.
    map.assign(19, ControlSlot::MouseButton, ControlInput::MouseButton(1))
        .expect("assigns");
    let cleared = map
        .assign(20, ControlSlot::MouseButton, ControlInput::MouseButton(1))
        .expect("assigns");
    assert_eq!(cleared, vec![19]);
    // Refusals change nothing.
    let before = map.clone();
    assert_eq!(
        map.assign(
            999,
            ControlSlot::KeyA,
            ControlInput::Key(OriginalKeyCode::NONE)
        ),
        Err(ControlsError::UnknownCommand { command_id: 999 })
    );
    assert_eq!(
        map.assign(19, ControlSlot::KeyA, ControlInput::JoystickButton(2)),
        Err(ControlsError::WrongSlot)
    );
    assert_eq!(
        map.assign(
            19,
            ControlSlot::JoystickButton,
            ControlInput::JoystickButton(11)
        ),
        Err(ControlsError::ButtonOutOfRange)
    );
    assert_eq!(map, before);
}

#[test]
fn accept_parity2_original_keybindings_help_lists_the_real_bindings() {
    let mut map = ControlsMap::original_default();
    let text = map.help_lines().join("\n");
    assert!(text.contains("Fire guns Space/Joy1"), "{text}");
    assert!(text.contains("Previous enemy Shift+E"), "{text}");
    assert!(text.contains("Bail out Ctrl+X"), "{text}");
    assert!(text.contains("Nose down Up"), "{text}");
    map.assign(
        19,
        ControlSlot::KeyA,
        ControlInput::Key(OriginalKeyCode::new(0x02d)),
    )
    .expect("rebinding works");
    let text = map.help_lines().join("\n");
    assert!(text.contains("Fire guns X/Joy1"), "{text}");
    assert!(text.contains("Fire rockets Joy2"), "{text}");
}
