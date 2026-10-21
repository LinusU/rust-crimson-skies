//! Acceptance for Rally #1809 `PARITY3-COCKPIT-VIEW.01`: the `Cockpit views`
//! key (`F8`, original command 49) cycles the mission's view through the
//! production minting path, and the cycle is the one recorded from the
//! engine image (finding
//! `docs/findings/2026-10-21-parity3-cockpit-view-01-original-view-cycle.md`).
//!
//! Every fixture is newly authored; the ignored test only re-reads cited
//! bytes of the owner's decrypted image.

use bevy::ecs::world::World;
use bevy::prelude::{ButtonInput, KeyCode};

use cs_app::mission_session::{MissionControls, MissionViewMode, mint_original_chord_keys};
use cs_types::keymap::ControlsMap;

fn world() -> World {
    let mut world = World::new();
    world.insert_resource(ButtonInput::<KeyCode>::default());
    world.insert_resource(MissionControls(ControlsMap::original_default()));
    world
}

/// One frame: press `key`, mint, release.
fn tap(world: &mut World, key: KeyCode) {
    world.resource_mut::<ButtonInput<KeyCode>>().press(key);
    mint_original_chord_keys(world);
    world.resource_mut::<ButtonInput<KeyCode>>().release(key);
    mint_original_chord_keys(world);
}

fn view(world: &World) -> MissionViewMode {
    world
        .get_resource::<MissionViewMode>()
        .copied()
        .unwrap_or_default()
}

#[test]
fn accept_parity3_cockpit_view_01_f8_cycles_the_view_through_the_minting_path() {
    let mut world = world();
    assert_eq!(view(&world), MissionViewMode::Chase);
    tap(&mut world, KeyCode::F8);
    assert_eq!(view(&world), MissionViewMode::Cockpit);
    tap(&mut world, KeyCode::F8);
    assert_eq!(view(&world), MissionViewMode::SecondEye);
    tap(&mut world, KeyCode::F8);
    assert_eq!(view(&world), MissionViewMode::Chase);
}

#[test]
fn accept_parity3_cockpit_view_01_other_keys_and_held_keys_do_not_cycle() {
    let mut world = world();
    tap(&mut world, KeyCode::KeyA);
    assert_eq!(view(&world), MissionViewMode::Chase);
    // A held key is one edge, not one press per frame.
    world
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::F8);
    mint_original_chord_keys(&mut world);
    mint_original_chord_keys(&mut world);
    assert_eq!(view(&world), MissionViewMode::Cockpit);
}

#[test]
fn accept_parity3_cockpit_view_01_the_cycle_follows_the_recorded_original_steps() {
    let facts = include_str!("../../../data/owner-static/engine-facts.toml");
    let steps: Vec<(u32, u32)> = facts
        .split("[[view_cycle_step]]")
        .skip(1)
        .map(|chunk| {
            let field = |name: &str| -> u32 {
                chunk
                    .lines()
                    .find_map(|line| line.strip_prefix(name)?.trim().strip_prefix('='))
                    .and_then(|value| value.trim().parse().ok())
                    .unwrap_or_else(|| panic!("view_cycle_step lacks {name}"))
            };
            (field("from"), field("to"))
        })
        .collect();
    assert_eq!(steps.len(), 3);
    for view in [
        MissionViewMode::Chase,
        MissionViewMode::Cockpit,
        MissionViewMode::SecondEye,
    ] {
        assert!(
            steps.contains(&(view.original_mode(), view.next().original_mode())),
            "{view:?} -> {:?} is not a recorded step",
            view.next()
        );
    }
}

#[test]
#[ignore = "requires CS_ENGINE_IMAGE"]
fn accept_parity3_cockpit_view_01_recorded_cycle_matches_the_engine_image() {
    let path = std::env::var("CS_ENGINE_IMAGE").expect("CS_ENGINE_IMAGE must name the image");
    let image = std::fs::read(path).expect("read the engine image");
    let at = |va: usize, bytes: &[u8]| {
        let offset = va - 0x40_0000;
        assert_eq!(
            &image[offset..offset + bytes.len()],
            bytes,
            "bytes at {va:#x}"
        );
    };
    // push 0x489340 / push 0x31: command 49's handler.
    at(0x4896ba, &[0x68, 0x40, 0x93, 0x48, 0x00, 0x6a, 0x31]);
    // The three `to` loads: mov esi,7 / xor esi,esi / mov esi,6.
    at(0x48937c, &[0xbe, 7, 0, 0, 0]);
    at(0x489378, &[0x33, 0xf6]);
    at(0x489371, &[0xbe, 6, 0, 0, 0]);
    // Dispatch on the mode: sub eax,0 / je; sub eax,6 / je; dec eax / je.
    at(0x48935b, &[0x83, 0xe8, 0x00]);
    at(0x489361, &[0x83, 0xe8, 0x06]);
    at(0x489366, &[0x48]);
}
