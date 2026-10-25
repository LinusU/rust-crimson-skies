//! Do the player's wing guns fire from the original's measured muzzle points?
//! (#2113) Task test prefix: `accept_f27_c_fx_orig_muzzle_mounts_`.
//!
//! The firepoint table rows are `[[player_firepoint_fact]]` of
//! `data/owner-static/engine-facts.toml`, described in
//! `docs/findings/2026-10-25-f27-c-fx-orig-muzzle-mounts.md`. The retail test
//! reads `$CS_GAME_DIR` (never writes it) and fails loudly without it.

use std::path::PathBuf;

use bevy::prelude::{Transform, World};
use cs_app::mission_session::{
    MISSION_GUN_MOUNT_NOSE, MISSION_GUN_MOUNT_WING_LEFT, MISSION_GUN_MOUNT_WING_RIGHT,
    attach_player_rig, open_mission_weapons,
};
use cs_app::scene::SceneGeneration;
use cs_app::weapons::MountPoseBinding;
use cs_sim::time::TickRate;
use cs_types::net::SessionId;

const TABLE: &str = include_str!("../../../../data/owner-static/engine-facts.toml");

/// `(name, va, bytes)` of every `[[player_firepoint_fact]]` row.
fn rows() -> Vec<(String, u64, Vec<u8>)> {
    let field = |chunk: &str, name: &str| {
        chunk
            .lines()
            .find_map(|line| {
                let rest = line.strip_prefix(name)?.trim_start().strip_prefix('=')?;
                Some(rest.trim().trim_matches('"').to_string())
            })
            .unwrap_or_else(|| panic!("a player firepoint row lacks {name}"))
    };
    TABLE
        .split("[[player_firepoint_fact]]")
        .skip(1)
        .map(|chunk| chunk.split("\n[").next().unwrap_or(chunk))
        .map(|chunk| {
            let va = field(chunk, "va");
            let va = u64::from_str_radix(va.trim_start_matches("0x"), 16).expect("hex va");
            assert_eq!(field(chunk, "provenance"), "static");
            assert_eq!(field(chunk, "claim"), "OWNER-STATIC-2026-10-25");
            let hex = field(chunk, "bytes");
            let bytes = (0..hex.len() / 2)
                .map(|i| u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).expect("hex byte"))
                .collect();
            (field(chunk, "name"), va, bytes)
        })
        .collect()
}

#[test]
fn accept_f27_c_fx_orig_muzzle_mounts_rows_pin_the_firepoint_table() {
    let rows = rows();
    let names: Vec<&str> = rows.iter().map(|(name, _, _)| name.as_str()).collect();
    assert_eq!(
        names,
        [
            "firepoint.table_build",
            "firepoint.fire_index",
            "firepoint.default_group",
            "firepoint.default_side",
            "firepoint.side_flip",
            "firepoint.alternate",
        ]
    );
    let get = |name: &str| &rows.iter().find(|(n, _, _)| n == name).expect("a row").2;
    let build = get("firepoint.table_build");
    // The loop: outer counter starts at 0, `add eax, 7` forms N = i - j + 7,
    // the inner loop runs `cmp ebx, 2`, the outer step is `add eax, 2` against 8.
    assert_eq!(build[..7], [0xc7, 0x45, 0xf0, 0, 0, 0, 0]);
    assert!(
        build.windows(3).any(|w| w == [0x83, 0xc0, 0x07]),
        "add eax,7"
    );
    assert!(
        build.windows(3).any(|w| w == [0x83, 0xfb, 0x02]),
        "cmp ebx,2"
    );
    assert!(
        build.windows(3).any(|w| w == [0x83, 0xc0, 0x02]),
        "add eax,2"
    );
    assert!(
        build.windows(3).any(|w| w == [0x83, 0xf8, 0x08]),
        "cmp eax,8"
    );
    // The chooser: fire group [esi+0x604], side [esi+0x4c4], doubled group.
    let index = get("firepoint.fire_index");
    assert_eq!(index[..6], [0x8b, 0x86, 0x04, 0x06, 0x00, 0x00]);
    assert_eq!(index[6..12], [0x8b, 0x8e, 0xc4, 0x04, 0x00, 0x00]);
    assert_eq!(index[12..14], [0xd1, 0xe0], "shl eax, 1");
    // Group and side both start at 0 (`mov [esi+disp], ebx`, ebx = 0).
    assert_eq!(get("firepoint.default_group")[..2], [0x89, 0x9e]);
    assert_eq!(get("firepoint.default_side")[..2], [0x89, 0x9e]);
}

/// The slot order the `firepoint.table_build` loop produces: `N = i - j + 7`
/// for `j` in 0, 2, 4, 6 and `i` in 0, 1.
fn firepoint_numbers_by_slot() -> Vec<i32> {
    let mut slots = Vec::new();
    let mut j = 0;
    while j < 8 {
        for i in 0..2 {
            slots.push(i - j + 7);
        }
        j += 2;
    }
    slots
}

#[test]
fn accept_f27_c_fx_orig_muzzle_mounts_fire_group_zero_is_firepoint_seven_then_eight() {
    let slots = firepoint_numbers_by_slot();
    assert_eq!(slots, [7, 8, 5, 6, 3, 4, 1, 2]);
    // Slot k = 2 * group + side; the group and the side both start at 0, so
    // the first two shots of a fresh session come from slots 0 and 1.
    let first_pair = [slots[0], slots[1]];
    assert_eq!(first_pair, [7, 8]);
}

#[test]
#[ignore = "requires CS_ENGINE_IMAGE"]
fn accept_f27_c_fx_orig_muzzle_mounts_rows_match_the_engine_image() {
    let path = std::env::var_os("CS_ENGINE_IMAGE").expect("CS_ENGINE_IMAGE must be set");
    let image = std::fs::read(path).expect("the engine image reads");
    let facts = cs_app::engine_facts::engine_facts().expect("the committed table parses");
    for (name, va, bytes) in rows() {
        let offset = facts
            .image_offset(va)
            .unwrap_or_else(|error| panic!("{name}: {error}")) as usize;
        assert_eq!(
            &image[offset..offset + bytes.len()],
            bytes.as_slice(),
            "{name} at {va:#x}"
        );
    }
}

fn mount_offset(world: &mut World, key: &str) -> [f32; 3] {
    let mut query = world.query::<(&MountPoseBinding, &Transform)>();
    let (_, transform) = query
        .iter(world)
        .find(|(binding, _)| binding.mount.as_str() == key)
        .unwrap_or_else(|| panic!("no rig mount {key}"));
    transform.translation.to_array()
}

/// The rig the session hangs on the player: wing guns at the measured points,
/// the nose gun still at its designed placement.
fn rig_offsets() -> ([f32; 3], [f32; 3], [f32; 3]) {
    let mut world = World::new();
    let sessions = open_mission_weapons(
        SessionId::new(7).expect("a nonzero session generation"),
        TickRate::new(60).expect("a nonzero tick rate"),
        SceneGeneration::default().next(),
    )
    .expect("the mission weapon session opens");
    sessions.install(&mut world).expect("the sessions install");
    let player = world.spawn_empty().id();
    attach_player_rig(&mut world, player).expect("the rig attaches");
    (
        mount_offset(&mut world, MISSION_GUN_MOUNT_WING_LEFT),
        mount_offset(&mut world, MISSION_GUN_MOUNT_WING_RIGHT),
        mount_offset(&mut world, MISSION_GUN_MOUNT_NOSE),
    )
}

fn assert_close(found: [f32; 3], wanted: [f32; 3], what: &str) {
    for axis in 0..3 {
        assert!(
            (found[axis] - wanted[axis]).abs() < 1e-5,
            "{what}: {found:?} is not {wanted:?}"
        );
    }
}

#[test]
fn accept_f27_c_fx_orig_muzzle_mounts_wing_guns_hang_at_the_measured_points() {
    let (left, right, nose) = rig_offsets();
    // `firepoint7` and `firepoint8` of `player_pfighter` (retail, planes.zbd).
    assert_close(
        left,
        [-1.165_008, -0.620_576_6, -1.199_482],
        "left wing gun",
    );
    assert_close(
        right,
        [1.204_927_2, -0.584_683_8, -1.199_482],
        "right wing gun",
    );
    // The nose gun has no measured muzzle: it keeps its designed placement.
    assert_close(nose, [0.0, -0.4, -4.0], "nose gun");
}

fn game_dir() -> PathBuf {
    PathBuf::from(
        std::env::var("CS_GAME_DIR").unwrap_or_else(|_| {
            panic!("CS_GAME_DIR is not set: #2113 needs the retail capability")
        }),
    )
}

/// The world position of `player_pfighter`'s `firepoint<n>` marker.
fn retail_firepoint(n: u32) -> [f32; 3] {
    let sources = cs_app::playtest_retail::read_playtest_sources(&game_dir(), "C1C")
        .expect("the retail sources read");
    let adapter = cs_app::playtest_retail::playtest_adapter().expect("the adapter builds");
    let planes = cs_app::playtest_retail::aircraft_graph(sources.aircraft(), &adapter)
        .expect("planes.zbd builds a graph");
    let wanted = format!("firepoint{n}");
    let mut found = Vec::new();
    for node in planes.nodes() {
        if node.name() != wanted {
            continue;
        }
        let mut cur = node.parent().cloned();
        while let Some(id) = cur {
            let parent = planes.node(&id).expect("the parent exists");
            if parent.name() == "player_pfighter" {
                found.push(node.world_transform().translation());
                break;
            }
            cur = parent.parent().cloned();
        }
    }
    assert_eq!(found.len(), 1, "exactly one {wanted} under player_pfighter");
    found[0].map(|v| v as f32)
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f27_c_fx_orig_muzzle_mounts_retail_firepoints_are_the_rig_offsets() {
    let slots = firepoint_numbers_by_slot();
    let (left, right, _) = rig_offsets();
    assert_close(
        retail_firepoint(slots[0] as u32),
        left,
        "firepoint of slot 0",
    );
    assert_close(
        retail_firepoint(slots[1] as u32),
        right,
        "firepoint of slot 1",
    );
}
