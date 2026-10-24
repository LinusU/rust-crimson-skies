//! M01-SEA-CYCLE (#1981): the `wtr00000..15` water cycle plays at the stored
//! rate word, which is **frames per second** (finding
//! `docs/findings/2026-10-24-m01-sea-material-cycle-static.md`).
//!
//! * the synthetic members run the production cycle system on a material and
//!   check the frame it shows after real elapsed time;
//! * the table member checks the committed `[[material_cycle]]` rows;
//! * the image member re-reads the rows and the instruction bytes of the
//!   engine's update (`#[ignore]`, `CS_ENGINE_IMAGE`);
//! * the retail member composes M01 and asserts its water material is a
//!   16-frame cycle at rate 10.0 and the horizon sea's copy follows it
//!   (`#[ignore]`, `CS_GAME_DIR`).

use std::time::Duration;

use bevy::asset::AssetPlugin;
use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use cs_app::engine_facts::engine_facts;
use cs_app::mission_launch::plan_mission_launch;
use cs_app::mission_session::{MissionWorldParts, build_headless, stage_for};
use cs_app::render::material_cycle::{MaterialCyclePlugin, MaterialCycles, cycle_delta};
use cs_content::coordinates::{load_engine_image, original_image_digest};

use crate::campaign::common::label;
use crate::campaign::vs_m01_rt_window::game_dir;

const TABLE_TEXT: &str = include_str!("../../../../data/owner-static/engine-facts.toml");

fn cycle_app(frames: usize, rate: f32) -> (App, Handle<StandardMaterial>, Vec<Handle<Image>>) {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin::default()))
        .init_asset::<Image>()
        .init_asset::<StandardMaterial>()
        .add_plugins(MaterialCyclePlugin)
        .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
            100,
        )));
    let images: Vec<Handle<Image>> = (0..frames)
        .map(|_| {
            app.world_mut()
                .resource_mut::<Assets<Image>>()
                .add(Image::default())
        })
        .collect();
    let material = app
        .world_mut()
        .resource_mut::<Assets<StandardMaterial>>()
        .add(StandardMaterial::default());
    app.world_mut().resource_mut::<MaterialCycles>().add(
        material.clone(),
        images.clone(),
        rate,
        true,
    );
    (app, material, images)
}

fn shown(app: &App, material: &Handle<StandardMaterial>) -> Option<Handle<Image>> {
    app.world()
        .resource::<Assets<StandardMaterial>>()
        .get(material)
        .and_then(|m| m.base_color_texture.clone())
}

/// **Rate 10 plays ten frames a second.** 0.1 s frames: the texture after
/// update `n` is frame `n - 1` (the engine binds the frame, then advances).
#[test]
fn accept_m01_sea_cycle_system_plays_the_rate_as_frames_per_second() {
    let (mut app, material, images) = cycle_app(16, 10.0);
    app.update(); // the first update has a zero delta
    assert_eq!(shown(&app, &material), Some(images[0].clone()));
    for step in 1..=20_usize {
        app.update();
        assert_eq!(
            shown(&app, &material),
            Some(images[(step - 1) % 16].clone()),
            "after {step} updates of 0.1 s at 10 frames/s"
        );
    }
}

/// **A copy of the material follows the same cycle** (the horizon sea).
#[test]
fn accept_m01_sea_cycle_a_followed_copy_shows_the_same_frame() {
    let (mut app, material, images) = cycle_app(4, 10.0);
    let copy = app
        .world_mut()
        .resource_mut::<Assets<StandardMaterial>>()
        .add(StandardMaterial::default());
    assert!(
        app.world_mut()
            .resource_mut::<MaterialCycles>()
            .follow(&material, copy.clone())
    );
    for _ in 0..4 {
        app.update();
    }
    assert_eq!(shown(&app, &copy), shown(&app, &material));
    assert!(shown(&app, &copy).is_some_and(|image| images.contains(&image)));
}

/// **A long frame advances by no more than the engine's clamp.**
#[test]
fn accept_m01_sea_cycle_frame_delta_is_clamped() {
    assert_eq!(cycle_delta(0.016), 0.016);
    assert_eq!(cycle_delta(5.0), 0.125);
}

fn rows() -> Vec<(String, u64, f32)> {
    let mut out = Vec::new();
    let mut current: Option<(String, u64, f32)> = None;
    let mut inside = false;
    for line in TABLE_TEXT.lines().map(str::trim) {
        if line.starts_with('[') {
            if let Some(row) = current.take() {
                out.push(row);
            }
            inside = line == "[[material_cycle]]";
            if inside {
                current = Some((String::new(), 0, 0.0));
            }
        } else if inside && let Some((key, value)) = line.split_once(" = ") {
            let row = current.as_mut().expect("a row");
            match key {
                "name" => row.0 = value.trim_matches('"').to_owned(),
                "va" => row.1 = u64::from_str_radix(value.trim_start_matches("0x"), 16).unwrap(),
                "value" => row.2 = value.parse().unwrap(),
                "provenance" => assert_eq!(value, "\"static\""),
                _ => {}
            }
        }
    }
    out.extend(current);
    out
}

#[test]
fn accept_m01_sea_cycle_rows_parse_and_agree_with_the_production_clamp() {
    engine_facts().expect("the committed table parses");
    let rows = rows();
    assert_eq!(rows.len(), 3);
    let value = |name: &str| rows.iter().find(|r| r.0 == name).expect("row").2;
    assert_eq!(value("cycle_ms_to_seconds"), 0.001);
    assert_eq!(value("cycle_time_scale"), 1.0);
    assert_eq!(cycle_delta(10.0), value("cycle_delta_max_seconds"));
}

#[test]
#[ignore = "requires CS_ENGINE_IMAGE"]
fn accept_m01_sea_cycle_rows_and_instructions_match_the_image() {
    let loaded = load_engine_image().expect("CS_ENGINE_IMAGE names the measured image");
    assert_eq!(loaded.digest, original_image_digest());
    let facts = engine_facts().expect("the committed table parses");
    let image = loaded.bytes.as_slice();
    let at = |va: u64| -> usize {
        usize::try_from(facts.image_offset(va).expect("va inside the image")).expect("an offset")
    };
    for (name, va, value) in rows() {
        let offset = at(va);
        let bytes: [u8; 4] = image[offset..offset + 4].try_into().expect("four bytes");
        assert_eq!(
            f32::from_le_bytes(bytes),
            value,
            "{name} drifted at {va:#x}"
        );
    }
    let bytes = |va: u64, want: &[u8], what: &str| {
        let offset = at(va);
        assert_eq!(
            &image[offset..offset + want.len()],
            want,
            "{what} at {va:#x}"
        );
    };
    // The frame binding: ftol(position) % count indexes the frame table
    // (cycle +0x8, +0x10, +0x18), stored into the material's texture index.
    bytes(
        0x55b1ec,
        &[
            0xd9, 0x46, 0x08, 0xe8, 0x6c, 0xbc, 0x09, 0x00, 0x99, 0xf7, 0x7e, 0x10, 0x8b, 0x4e,
            0x18, 0x8b, 0x14, 0x91, 0x89, 0x57, 0x10,
        ],
        "frame = table[ftol(position) % count]",
    );
    // position += [0x9ad744] * rate: the rate is per second of frame delta.
    bytes(
        0x55b201,
        &[
            0xd9, 0x05, 0x44, 0xd7, 0x9a, 0x00, 0xd8, 0x4e, 0x0c, 0xd8, 0x46, 0x08,
        ],
        "position += delta * rate",
    );
    // A zero first word stops on the last frame instead of looping.
    bytes(
        0x55b213,
        &[0x83, 0x39, 0x00, 0x75, 0x21, 0x8b, 0x51, 0x10],
        "non-looping clamp is gated on cycle +0",
    );
    // The update runs for a drawn polygon group whose flag byte has bit 0x4.
    bytes(
        0x552ea6,
        &[0xf6, 0xc4, 0x04, 0x74, 0x09, 0x56, 0xe8, 0xef, 0x82],
        "the draw calls the update for cycled materials",
    );
    // The delta: milliseconds times 0.001, times the scale, clamped.
    bytes(
        0x59c0aa,
        &[0xd8, 0x0d, 0xdc, 0x75, 0x60, 0x00],
        "ms * 0.001",
    );
    bytes(
        0x59c11b,
        &[0xd8, 0x0d, 0x4c, 0xd7, 0x9a, 0x00, 0xd9, 0x1d, 0x44, 0xd7],
        "the delta is stored to 0x9ad744",
    );
    bytes(
        0x59c129,
        &[
            0xd9, 0x05, 0x44, 0xd7, 0x9a, 0x00, 0xd8, 0x1d, 0x58, 0xb1, 0x63, 0x00,
        ],
        "the delta is compared with the 0.125 s maximum",
    );
}

/// **M01's water is a 16-frame cycle at the stored rate 10.0**, bound by the
/// production world draw, and the horizon sea's material follows it.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_sea_cycle_retail_water_plays_sixteen_frames() {
    let root = game_dir();
    let plan = plan_mission_launch(&root, label("M01"), "The Lost Treasure")
        .expect("M01's launch closure plans");
    let stage = stage_for(&root, &plan).expect("M01's stage reads");
    let mut app = build_headless(&stage).expect("M01's stage composes headlessly");
    app.update();
    let parts = app.world().resource::<MissionWorldParts>().clone();
    let art = parts.water_art().expect("M01's world binds water art");
    let cycles = app.world().resource::<MaterialCycles>();
    let cycle = cycles
        .cycle_of(&art.material)
        .expect("the water material is cycled");
    assert_eq!(cycle.frames.len(), 16);
    assert_eq!(cycle.rate, 10.0);
    assert!(cycle.looping);
}
