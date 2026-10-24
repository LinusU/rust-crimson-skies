//! Acceptance for Rally #1925 `FLAK-C3-TRAILS`: the `OBJECT_MOTION` of the
//! `flak_trailN` objects, measured from the executor (`[[object_motion_fact]]`
//! rows of `data/owner-static/engine-facts.toml`, finding
//! `docs/findings/2026-10-24-flak-c3-trails-object-motion.md`), and the trails
//! drawn from a burst.
//!
//! Test prefix: `accept_flak_c3_trails_`. The synthetic tests drive the
//! production launch law, flight and plugin with authored numbers; the ignored
//! tests read the retail record from `CS_GAME_DIR` and re-read every cited
//! instruction from `CS_ENGINE_IMAGE`.

use std::path::PathBuf;

use bevy::prelude::{App, FixedPostUpdate, Vec3};

use cs_app::mission_session::{
    BURST_IMPACT_ROW, BurstCause, BurstPuff, BurstPuffDefinition, BurstPuffSource,
    BurstPuffSourceResource, FlakBurstFxPlugin, FlakBurstFxReport, FlakEffectDefinition,
    FlakEffectResource, FlakTrail, FlakTrailBody, FlakTrailsDefinition, FlakTrailsResource,
    FlashStep, FlickerStep, MissionBurstPuffPlugin, OrdnanceBurst, PendingFlakBursts, TrailBody,
    TrailMotion, TrailPuffer, read_flak_trails, trail_launch,
};
use cs_app::physics::PhysicsTickLedger;
use cs_types::Tick;

const TABLE: &str = include_str!("../../../data/owner-static/engine-facts.toml");
const IMAGE_BASE: usize = 0x40_0000;

fn motion(min: [f32; 4], max: [f32; 4], gravity: f32, run_time_s: f32) -> TrailMotion {
    TrailMotion {
        node: "flak_trail1".into(),
        gravity,
        range_min: min,
        range_max: max,
        run_time_s,
    }
}

fn near(a: Vec3, b: Vec3) -> bool {
    (a - b).length() < 1e-4
}

#[test]
fn accept_flak_c3_trails_the_launch_is_the_measured_heading_elevation_speed_law() {
    // Heading 0 points to -Z; elevation 60 degrees is e90 = 2/3, and the
    // horizontal share is the linear 1 - e90 = 1/3, not cos(60).
    let m = motion([0.0, 60.0, 18.0, 0.0], [0.0, 60.0, 18.0, 0.0], -3.0, 2.0);
    let launch = trail_launch(&m, [0.5; 4]);
    assert!(
        near(launch.velocity, Vec3::new(0.0, 12.0, -6.0)),
        "{launch:?}"
    );
    assert!(near(launch.acceleration, Vec3::new(0.0, -3.0, 0.0)));
    // Heading 90 degrees points to -X.
    let m = motion([90.0, 0.0, 10.0, 0.0], [90.0, 0.0, 10.0, 0.0], 0.0, 1.0);
    assert!(near(
        trail_launch(&m, [0.0; 4]).velocity,
        Vec3::new(-10.0, 0.0, 0.0)
    ));
    // A negative elevation points down and the share is 1 + e90.
    let m = motion([0.0, -45.0, 20.0, 0.0], [0.0, -45.0, 20.0, 0.0], 0.0, 1.0);
    assert!(near(
        trail_launch(&m, [0.9; 4]).velocity,
        Vec3::new(0.0, -10.0, -10.0)
    ));
    // The fourth word accelerates along the same direction; gravity adds to
    // the second component only.
    let m = motion([0.0, 0.0, 5.0, 2.0], [0.0, 0.0, 5.0, 2.0], -1.0, 1.0);
    let launch = trail_launch(&m, [0.3; 4]);
    assert!(near(launch.velocity, Vec3::new(0.0, 0.0, -5.0)));
    assert!(near(launch.acceleration, Vec3::new(0.0, -1.0, -2.0)));
}

#[test]
fn accept_flak_c3_trails_each_word_is_a_uniform_draw_between_min_and_max() {
    let m = motion([35.0, 60.0, 18.0, 0.0], [55.0, 70.0, 25.0, 0.0], -3.0, 2.0);
    let low = trail_launch(&m, [0.0; 4]);
    let high = trail_launch(&m, [1.0; 4]);
    let speed = |v: Vec3, e90: f32| {
        // |velocity| = speed * sqrt(T^2 + e90^2), T = 1 - e90.
        let t = 1.0 - e90;
        v.length() / (t * t + e90 * e90).sqrt()
    };
    assert!((speed(low.velocity, 60.0 / 90.0) - 18.0).abs() < 1e-3);
    assert!((speed(high.velocity, 70.0 / 90.0) - 25.0).abs() < 1e-3);
}

#[test]
fn accept_flak_c3_trails_the_flight_lasts_run_time_and_moves_before_it_accelerates() {
    let m = motion([0.0, 0.0, 10.0, 0.0], [0.0, 0.0, 10.0, 0.0], -2.0, 1.0);
    let mut body = TrailBody::launched(Vec3::ZERO, trail_launch(&m, [0.0; 4]), 1.0);
    // The first frame moves by the launch velocity only.
    let (before, done) = body.step(0.3);
    assert_eq!(before, Vec3::ZERO);
    assert!(!done);
    assert!(near(body.position, Vec3::new(0.0, 0.0, -3.0)));
    assert!(near(body.velocity, Vec3::new(0.0, -0.6, -10.0)));
    // 0.3 + 0.3 + 0.3 + 0.3 overshoots RUN_TIME 1.0: the last frame is
    // trimmed to 0.1 s, so the velocity ends exactly v0 + a * RUN_TIME.
    body.step(0.3);
    body.step(0.3);
    let (_, done) = body.step(0.3);
    assert!(done);
    assert!(
        near(body.velocity, Vec3::new(0.0, -2.0, -10.0)),
        "{:?}",
        body.velocity
    );
}

fn effect() -> FlakEffectDefinition {
    FlakEffectDefinition {
        light_range: [4.0, 10.0],
        light_color: [0.5, 0.5, 0.25],
        flash_steps: vec![FlashStep {
            delta_range: [1.0, 1.0],
            run_time_s: 0.1,
        }],
        player_range_m: 100.0,
        flicker_steps: vec![FlickerStep {
            from: [0.0; 4],
            to: [0.0; 4],
            run_time_s: 0.1,
        }],
    }
}

fn trails() -> FlakTrailsDefinition {
    let puffer = TrailPuffer {
        distance_interval_m: 2.5,
        world_velocity: [0.0, 3.0, 0.0],
        min_velocity: [-1.0; 3],
        max_velocity: [1.0; 3],
        friction: 1.0,
        size: [1.5, 2.2],
        lifetime_s: [1.0, 2.5],
    };
    FlakTrailsDefinition {
        trails: vec![
            FlakTrail {
                motion: motion([0.0, 0.0, 10.0, 0.0], [0.0, 0.0, 10.0, 0.0], 0.0, 1.05),
                puffer: puffer.clone(),
            },
            FlakTrail {
                motion: motion([180.0, 0.0, 20.0, 0.0], [180.0, 0.0, 20.0, 0.0], 0.0, 0.525),
                puffer,
            },
        ],
    }
}

fn stage(with_trails: bool) -> App {
    let mut app = App::new();
    app.add_plugins((
        bevy::asset::AssetPlugin::default(),
        FlakBurstFxPlugin,
        MissionBurstPuffPlugin,
    ));
    app.insert_resource(PhysicsTickLedger {
        timestep_s: 1.0 / 120.0,
        ..Default::default()
    });
    app.insert_resource(FlakEffectResource(effect()));
    app.insert_resource(BurstPuffSourceResource(BurstPuffSource {
        definition: BurstPuffDefinition {
            number: 1,
            size: [1.0, 1.0],
            lifetime_s: [1.0, 1.0],
            min_velocity: [0.0; 3],
            max_velocity: [0.0; 3],
            friction: 0.0,
            textures: Vec::new(),
        },
        images: Vec::new(),
        texture_archive: "synthetic".into(),
    }));
    if with_trails {
        app.insert_resource(FlakTrailsResource(trails()));
    }
    app
}

fn burst() -> OrdnanceBurst {
    OrdnanceBurst {
        at: Tick(0),
        position_m: [100.0, 300.0, -50.0],
        cause: BurstCause::RangeEnd,
        splash_radius_m: 22.0,
        armor_damage: 10.0,
        health_damage: 10.0,
        impact_row: BURST_IMPACT_ROW,
    }
}

fn run_ticks(app: &mut App, ticks: u64) {
    for tick in 1..=ticks {
        app.world_mut().resource_mut::<PhysicsTickLedger>().ticks = tick;
        app.world_mut().run_schedule(FixedPostUpdate);
    }
}

#[test]
fn accept_flak_c3_trails_a_burst_throws_each_trail_and_emits_puffs_along_its_path() {
    let mut app = stage(true);
    app.world_mut()
        .resource_mut::<PendingFlakBursts>()
        .0
        .push(burst());
    app.world_mut().run_schedule(FixedPostUpdate);
    let report = *app.world().resource::<FlakBurstFxReport>();
    assert_eq!(report.trails_thrown, 2);
    assert_eq!(report.trails_undrawn, 0);
    let flying = app
        .world_mut()
        .query::<&FlakTrailBody>()
        .iter(app.world())
        .count();
    assert_eq!(flying, 2);

    // Trail 0: 10 m/s for 1.05 s along -Z = 10.5 m, a puff each 2.5 m = 4
    // puffs; trail 1: 20 m/s for 0.525 s along +Z = 10.5 m = 4 puffs.
    run_ticks(&mut app, 130);
    let report = *app.world().resource::<FlakBurstFxReport>();
    assert_eq!(report.trail_puffs_spawned, 8, "{report:?}");
    assert_eq!(report.trail_puffs_undrawn, 0);
    let flying = app
        .world_mut()
        .query::<&FlakTrailBody>()
        .iter(app.world())
        .count();
    assert_eq!(flying, 0, "both flights ended at their RUN_TIME");
    let start = Vec3::new(100.0, 300.0, -50.0);
    let mut away: Vec<(f32, f32)> = app
        .world_mut()
        .query::<&BurstPuff>()
        .iter(app.world())
        .filter(|p| p.size >= 1.5)
        .map(|p| (p.origin.z - start.z, p.origin.x - start.x))
        .collect();
    away.sort_by(|a, b| a.0.total_cmp(&b.0));
    assert_eq!(away.len(), 8);
    // Trail 0 puffs sit 2.5, 5, 7.5 and 10 m down -Z; trail 1's 2.5.. up +Z.
    for (i, expected) in [-10.0, -7.5, -5.0, -2.5, 2.5, 5.0, 7.5, 10.0]
        .iter()
        .enumerate()
    {
        assert!((away[i].0 - expected).abs() < 0.2, "{away:?}");
        assert!(away[i].1.abs() < 1e-3, "no sideways drift: {away:?}");
    }
}

#[test]
fn accept_flak_c3_trails_without_the_definition_no_trail_is_invented() {
    let mut app = stage(false);
    app.world_mut()
        .resource_mut::<PendingFlakBursts>()
        .0
        .push(burst());
    run_ticks(&mut app, 5);
    let report = *app.world().resource::<FlakBurstFxReport>();
    assert_eq!(report.bursts_presented, 1);
    assert_eq!(report.trails_undrawn, 1);
    assert_eq!(report.trails_thrown, 0);
    let flying = app
        .world_mut()
        .query::<&FlakTrailBody>()
        .iter(app.world())
        .count();
    assert_eq!(flying, 0);
}

fn game_dir() -> PathBuf {
    PathBuf::from(std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR must name the retail install"))
}

/// **Retail: the five stored trail objects, their motion and their puffers.**
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_flak_c3_trails_retail_call_flak_trails() {
    let d = read_flak_trails(&game_dir()).expect("call_flak_trails reads");
    assert_eq!(d.trails.len(), 5);
    let names: Vec<&str> = d.trails.iter().map(|t| t.motion.node.as_str()).collect();
    assert_eq!(
        names,
        [
            "flak_trail1",
            "flak_trail2",
            "flak_trail3",
            "flak_trail4",
            "flak_trail5"
        ]
    );
    let gravity: Vec<f32> = d.trails.iter().map(|t| t.motion.gravity).collect();
    assert_eq!(gravity, [-3.0, -2.0, -2.0, -1.0, -2.0]);
    let run: Vec<f32> = d.trails.iter().map(|t| t.motion.run_time_s).collect();
    assert_eq!(run, [2.0, 3.0, 2.0, 2.0, 2.5]);
    assert_eq!(d.trails[0].motion.range_min, [35.0, 60.0, 18.0, 0.0]);
    assert_eq!(d.trails[0].motion.range_max, [55.0, 70.0, 25.0, 0.0]);
    assert_eq!(d.trails[2].motion.range_min, [235.0, -35.0, 24.0, 0.0]);
    assert_eq!(d.trails[2].motion.range_max, [255.0, -70.0, 30.0, 0.0]);
    for trail in &d.trails {
        assert_eq!(trail.puffer.distance_interval_m, 2.5);
        assert_eq!(trail.puffer.world_velocity, [0.0, 3.0, 0.0]);
        assert_eq!(trail.puffer.lifetime_s[0], 1.0);
    }
    // Every draw in the stored ranges launches upward for trails 1, 2 and 5.
    for index in [0, 1, 4] {
        for draw in [0.0, 0.5, 1.0] {
            let launch = trail_launch(&d.trails[index].motion, [draw; 4]);
            assert!(launch.velocity.y > 0.0, "trail {index} draw {draw}");
        }
    }
    let down = trail_launch(&d.trails[2].motion, [0.5; 4]);
    assert!(down.velocity.y < 0.0, "trail 3 is thrown downward");
}

// ---- the cited engine rows ----

struct Row {
    name: String,
    va: usize,
    bytes: Vec<u8>,
}

fn field(chunk: &str, name: &str) -> String {
    chunk
        .lines()
        .find_map(|line| {
            let rest = line.strip_prefix(name)?.trim_start().strip_prefix('=')?;
            Some(rest.trim().trim_matches('"').to_string())
        })
        .unwrap_or_else(|| panic!("object_motion_fact lacks {name}"))
}

fn rows() -> Vec<Row> {
    TABLE
        .split("[[object_motion_fact]]")
        .skip(1)
        .map(|chunk| {
            let chunk = chunk.split("\n[").next().unwrap_or(chunk);
            assert_eq!(field(chunk, "claim"), "OWNER-STATIC-2026-10-24");
            assert_eq!(field(chunk, "provenance"), "static");
            assert!(!field(chunk, "fact").is_empty());
            let va = field(chunk, "va");
            let va = usize::from_str_radix(va.trim_start_matches("0x"), 16).expect("hex va");
            let hex = field(chunk, "bytes");
            let bytes = (0..hex.len() / 2)
                .map(|i| u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).expect("hex byte"))
                .collect();
            Row {
                name: field(chunk, "name"),
                va,
                bytes,
            }
        })
        .collect()
}

fn row<'a>(rows: &'a [Row], name: &str) -> &'a Row {
    rows.iter()
        .find(|row| row.name == name)
        .unwrap_or_else(|| panic!("no object_motion_fact named {name}"))
}

#[test]
fn accept_flak_c3_trails_the_committed_rows_state_the_launch_law() {
    let facts = cs_app::engine_facts::engine_facts().expect("the committed table parses");
    assert_eq!(facts.image(), "crimson.decrypted.exe");
    let rows = rows();
    assert!(rows.len() >= 35, "only {} rows", rows.len());
    let mut names: Vec<&str> = rows.iter().map(|row| row.name.as_str()).collect();
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), rows.len(), "row names are unique");
    // MIN fills words 0..3 at +0x20/+0x28/+0x30/+0x38, MAX at +0x24/+0x2c/+0x34/+0x3c.
    let store = |name: &str| *row(&rows, name).bytes.last().unwrap();
    assert_eq!(
        [
            store("parse.range_min_0"),
            store("parse.range_min_1"),
            store("parse.range_min_2"),
            store("parse.range_min_3"),
        ],
        [0x20, 0x28, 0x30, 0x38]
    );
    assert_eq!(
        [
            store("parse.range_max_0"),
            store("parse.range_max_1"),
            store("parse.range_max_2"),
            store("parse.range_max_3"),
        ],
        [0x24, 0x2c, 0x34, 0x3c]
    );
    // The constants the law uses.
    let stored = f64::from_le_bytes(
        row(&rows, "const.pi_over_180")
            .bytes
            .clone()
            .try_into()
            .unwrap(),
    );
    assert!(
        (stored - std::f64::consts::PI / 180.0).abs() < 1e-13,
        "{stored}"
    );
    assert_eq!(
        row(&rows, "const.one_over_90").bytes,
        (1.0f32 / 90.0).to_le_bytes()
    );
    assert_eq!(row(&rows, "const.one").bytes, 1.0f32.to_le_bytes());
    // RUN_TIME is record+0x148 and sets flag 0x400.
    assert_eq!(
        row(&rows, "parse.run_time_flag").bytes[3..],
        [0x00, 0x04, 0x00, 0x00]
    );
    assert_eq!(
        row(&rows, "parse.run_time_store").bytes[3..],
        [0x89, 0x83, 0x48, 0x01, 0x00, 0x00][..]
    );
    let va = |name: &str| row(&rows, name).va;
    for name in [
        "exec.draw_heading",
        "exec.step_dt",
        "exec.velocity_integrate",
    ] {
        let offset = facts
            .image_offset(va(name) as u64)
            .unwrap_or_else(|error| panic!("{name}: {error}"));
        assert!(offset < 0x30_0000, "{name} is past the image");
    }
}

fn image() -> Vec<u8> {
    let path = std::env::var("CS_ENGINE_IMAGE").expect("CS_ENGINE_IMAGE must name the image");
    std::fs::read(path).expect("read the engine image")
}

fn cstr_at(image: &[u8], va: usize) -> &[u8] {
    let start = va - IMAGE_BASE;
    let len = image[start..].iter().position(|&b| b == 0).unwrap();
    &image[start..start + len]
}

#[test]
#[ignore = "requires CS_ENGINE_IMAGE"]
fn accept_flak_c3_trails_cited_rows_match_the_engine_image() {
    let image = image();
    let rows = rows();
    for row in &rows {
        let offset = row.va - IMAGE_BASE;
        assert_eq!(
            &image[offset..offset + row.bytes.len()],
            row.bytes.as_slice(),
            "{} at {:#x}",
            row.name,
            row.va
        );
    }
    // The strings the key tests push are the keys the rows name.
    let pushed = |name: &str| {
        let bytes = &row(&rows, name).bytes;
        assert_eq!(bytes[0], 0x68, "{name}: push imm32");
        u32::from_le_bytes(bytes[1..5].try_into().unwrap()) as usize
    };
    assert_eq!(cstr_at(&image, pushed("parse.gravity_key")), b"GRAVITY");
    assert_eq!(
        cstr_at(&image, pushed("parse.range_min_key")),
        b"TRANSLATION_RANGE_MIN"
    );
    assert_eq!(
        cstr_at(&image, pushed("parse.range_max_key")),
        b"TRANSLATION_RANGE_MAX"
    );
    assert_eq!(cstr_at(&image, pushed("parse.run_time_key")), b"RUN_TIME");
    // The table fill scales rand() into [0, 1]: the stored scale is 1/32767.
    let scale = f32::from_bits(0x3800_0100);
    assert!((scale - 1.0 / 32767.0).abs() < 1e-9);
    let fill = &row(&rows, "table.fill").bytes;
    assert_eq!(fill[6..], 0x3800_0100u32.to_le_bytes());
}
