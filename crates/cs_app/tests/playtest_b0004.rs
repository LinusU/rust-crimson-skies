//! Task #752 (PLAYTEST-B0004-VISIBILITY) acceptance tests. Prefix:
//! `accept_playtest_b0004_`.
//!
//! Bevy warns (B0004) when an entity with `Visibility` has a parent without it
//! (and likewise for `Transform`). The retail path parents every drawn part to
//! the flight body with `Visibility::Inherited`, so the body must carry it.

use bevy::input::ButtonState;
use bevy::input::keyboard::{Key, KeyCode, KeyboardInput, NativeKey};
use bevy::prelude::{App, ChildOf, Entity, Transform, Visibility, With};
use bevy::window::WindowFocused;
use cs_app::playtest::headless_app;
use cs_app::playtest::scene::PlaytestAircraft;

fn tap_reset(app: &mut App) {
    for state in [ButtonState::Pressed, ButtonState::Released] {
        app.world_mut().write_message(KeyboardInput {
            key_code: KeyCode::KeyR,
            logical_key: Key::Unidentified(NativeKey::Unidentified),
            state,
            text: None,
            repeat: false,
            window: Entity::PLACEHOLDER,
        });
        app.update();
    }
}

/// Attaches a part to every current aircraft exactly as `AircraftPartAsset::spawn`
/// does (`Transform`, `Visibility::Inherited`, `ChildOf`), with a piece below it.
fn attach_parts(app: &mut App) {
    let world = app.world_mut();
    let bodies: Vec<Entity> = world
        .query_filtered::<Entity, With<PlaytestAircraft>>()
        .iter(world)
        .collect();
    assert_eq!(bodies.len(), 1, "exactly one flight body");
    let part = world
        .spawn((
            Transform::IDENTITY,
            Visibility::Inherited,
            ChildOf(bodies[0]),
        ))
        .id();
    world.spawn((Transform::IDENTITY, Visibility::Inherited, ChildOf(part)));
}

fn assert_consistent_hierarchy(app: &mut App) {
    let world = app.world_mut();
    let children: Vec<(Entity, Entity)> = world
        .query::<(Entity, &ChildOf)>()
        .iter(world)
        .map(|(entity, parent)| (entity, parent.parent()))
        .collect();
    assert!(!children.is_empty(), "the test must exercise a hierarchy");
    for (child, parent) in children {
        if world.get::<Visibility>(child).is_some() {
            assert!(
                world.get::<Visibility>(parent).is_some(),
                "{child:?} has Visibility but parent {parent:?} does not"
            );
        }
        if world.get::<Transform>(child).is_some() {
            assert!(
                world.get::<Transform>(parent).is_some(),
                "{child:?} has Transform but parent {parent:?} does not"
            );
        }
    }
}

#[test]
fn accept_playtest_b0004_flight_body_parts_have_consistent_parent_components() {
    let mut app = headless_app();
    app.update();
    attach_parts(&mut app);
    for _ in 0..5 {
        app.update();
    }
    assert_consistent_hierarchy(&mut app);

    // `R` respawns the body; the fresh body must be as consistent as the first.
    app.world_mut().write_message(WindowFocused {
        window: Entity::PLACEHOLDER,
        focused: true,
    });
    tap_reset(&mut app);
    app.update();
    attach_parts(&mut app);
    app.update();
    assert_consistent_hierarchy(&mut app);
}
