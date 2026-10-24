//! Acceptance for Rally #1927: a `wep_27` burst's splash reaches the player
//! through the session's one damage authority. The retail leg test-wakes the
//! first rocket turret of M01 (`aagun99`, which no M01 program wakes), parks
//! the player in its field and reads the hull pool back after the burst. The
//! falloff `1 - d^2/r^2` and the `+0x678` scale are static findings
//! (`docs/findings/2026-10-24-rocket-splash-damage.md`); the hull pool and box
//! are the mission's designed ones, so the size of the drop is not a claim
//! about the original.

use std::path::PathBuf;
use std::time::Duration;

use avian3d::prelude::{LinearVelocity, Position};
use bevy::prelude::{App, Entity, Time, Vec3, With};
use bevy::time::TimeUpdateStrategy;
use cs_app::mission_launch::plan_mission_launch;
use cs_app::mission_session::turret_fire::{MissionTurretFire, TurretWeaponRecord};
use cs_app::mission_session::{
    MISSION_TARGET_NODE, MissionHost, MissionPlayerBody, PlayerVictim, build_headless, stage_for,
};
use cs_app::mission_turrets::TurretMount;
use cs_app::physics::PhysicsTickLedger;
use cs_app::weapons::SessionDamageResolver;
use cs_content::campaign_bindings::MissionLabel;
use cs_sim::damage::DamageNodeKey;
use cs_types::Tick;

fn pin_player(app: &mut App, at: [f64; 3]) {
    let world = app.world_mut();
    let body = world
        .query_filtered::<Entity, With<MissionPlayerBody>>()
        .single(world)
        .expect("one player body");
    world.get_mut::<Position>(body).expect("a pose").0 =
        Vec3::new(at[0] as f32, at[1] as f32, at[2] as f32);
    world.get_mut::<LinearVelocity>(body).expect("a velocity").0 = Vec3::ZERO;
}

fn hull(app: &App) -> f64 {
    let victim = app.world().resource::<PlayerVictim>();
    let key = DamageNodeKey::new(MISSION_TARGET_NODE).unwrap();
    app.world()
        .resource::<SessionDamageResolver>()
        .resolver()
        .remaining_integrity(&victim.actor, &key)
        .expect("the player's hull has a pool")
}

#[test]
#[ignore = "requires CS_GAME_DIR and CS_ENGINE_IMAGE"]
fn accept_rocket_splash_retail_a_burst_lands_its_falloff_on_the_player_hull() {
    let root = PathBuf::from(
        std::env::var_os("CS_GAME_DIR").expect("CS_GAME_DIR must be set for the retail leg"),
    );
    let plan = plan_mission_launch(
        &root,
        MissionLabel::new("M01").expect("a mission label"),
        "The Lost Treasure",
    )
    .expect("M01's launch closure plans");
    let stage = stage_for(&root, &plan).expect("M01's stage reads");
    let bound = &stage.host.turrets;
    let entry = bound
        .entries
        .iter()
        .find(|entry| {
            entry.mount == TurretMount::World
                && entry.position_m.is_some()
                && bound
                    .specs
                    .iter()
                    .find(|spec| spec.index == entry.record)
                    .and_then(|spec| spec.weapon.as_deref())
                    .and_then(|name| bound.weapons.get(name))
                    .is_some_and(|weapon| matches!(weapon, TurretWeaponRecord::Rocket(_)))
        })
        .expect("a world turret with a rocket record is bound")
        .clone();
    let spec = bound
        .specs
        .iter()
        .find(|spec| spec.index == entry.record)
        .expect("its TURRET record is carried");
    let mount = entry.position_m.expect("a world entry has a position");
    let horizontal = (spec.detection_range() * 0.6).clamp(80.0, 400.0);
    let at = [mount[0] + horizontal, mount[1] + horizontal * 0.3, mount[2]];

    let mut app = build_headless(&stage).expect("M01's stage composes headlessly");
    *app.world_mut().resource_mut::<TimeUpdateStrategy>() =
        TimeUpdateStrategy::ManualDuration(Duration::ZERO);
    app.update();
    let timestep = app.world().resource::<Time<bevy::time::Fixed>>().timestep();
    let step = |app: &mut App| {
        pin_player(app, at);
        *app.world_mut().resource_mut::<TimeUpdateStrategy>() =
            TimeUpdateStrategy::ManualDuration(timestep);
        app.update();
    };

    // Asleep it fires nothing and the hull is whole.
    for _ in 0..120 {
        step(&mut app);
    }
    let start = hull(&app);
    assert!(
        app.world()
            .resource::<MissionTurretFire>()
            .report()
            .rocket_splashes
            .is_empty()
    );

    let tick = app.world().resource::<PhysicsTickLedger>().ticks;
    app.world_mut()
        .resource_mut::<MissionHost>()
        .wake_turrets_mut()
        .wake_for_test(&entry.name, Tick(tick));
    let mut splashed = false;
    for _ in 0..3600 {
        step(&mut app);
        if !app
            .world()
            .resource::<MissionTurretFire>()
            .report()
            .rocket_splashes
            .is_empty()
        {
            splashed = true;
            break;
        }
    }
    let report = app.world().resource::<MissionTurretFire>().report().clone();
    assert!(
        splashed,
        "a burst's splash reached the hull within the budget: {report:?}"
    );
    let splash = &report.rocket_splashes[0];
    let burst = report
        .rocket_bursts
        .iter()
        .find(|burst| burst.at == splash.tick)
        .expect("the splash names a burst of this report");
    assert!(splash.fraction > 0.0 && splash.fraction <= 1.0);
    assert!(splash.surface_distance_m < burst.splash_radius_m);
    let expected_fraction = 1.0 - (splash.surface_distance_m / burst.splash_radius_m).powi(2);
    assert!((splash.fraction - expected_fraction).abs() < 1e-9);
    assert!(
        (splash.armor - burst.armor_damage * splash.fraction).abs() < 1e-9,
        "the armor channel is ARMOR_DAMAGE times the falloff: {splash:?}"
    );
    assert!(
        (splash.health - burst.health_damage * splash.fraction).abs() < 1e-9,
        "the internal channel is HEALTH_DAMAGE times the falloff: {splash:?}"
    );
    let end = hull(&app);
    assert!(
        end < start,
        "the burst's splash fell on the hull through the damage authority: {start} -> {end}"
    );
}

// ------------------------------------------------- the cited static rows ---

const TABLE: &str = include_str!("../../../data/owner-static/engine-facts.toml");

/// `(name, va, bytes)` of every `splash.*` row of `[[rocket_splash_fact]]`.
fn splash_rows() -> Vec<(String, u64, Vec<u8>)> {
    let field = |chunk: &str, name: &str| {
        chunk
            .lines()
            .find_map(|line| {
                let rest = line.strip_prefix(name)?.trim_start().strip_prefix('=')?;
                Some(rest.trim().trim_matches('"').to_string())
            })
            .unwrap_or_else(|| panic!("a splash row lacks {name}"))
    };
    TABLE
        .split("[[rocket_splash_fact]]")
        .skip(1)
        .map(|chunk| chunk.split("\n[").next().unwrap_or(chunk))
        .filter(|chunk| field(chunk, "name").starts_with("splash."))
        .map(|chunk| {
            let va = field(chunk, "va");
            let va = u64::from_str_radix(va.trim_start_matches("0x"), 16).expect("hex va");
            let hex = field(chunk, "bytes");
            let bytes = (0..hex.len() / 2)
                .map(|i| u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).expect("hex byte"))
                .collect();
            (field(chunk, "name"), va, bytes)
        })
        .collect()
}

#[test]
fn accept_rocket_splash_the_committed_rows_cite_the_falloff_scale_and_pools() {
    let rows = splash_rows();
    let names: Vec<&str> = rows.iter().map(|(name, _, _)| name.as_str()).collect();
    assert_eq!(
        names,
        [
            "splash.falloff",
            "splash.armor_scale",
            "splash.health_scale",
            "splash.victim_armor_pool",
            "splash.victim_health_pool",
            "splash.armor_damage_field",
            "splash.health_damage_field",
        ]
    );
    let get = |name: &str| rows.iter().find(|(n, _, _)| n == name).expect("a row");
    // fld [ebp]; fdiv [ebx+0x40]; fsubr [0x6032dc].
    assert_eq!(&get("splash.falloff").2[3..6], [0xd8, 0x73, 0x40]);
    // fmul [esi+0x678] on both channels.
    for name in ["splash.armor_scale", "splash.health_scale"] {
        assert_eq!(get(name).2, [0xd8, 0x8e, 0x78, 0x06, 0x00, 0x00]);
    }
    // The pools are +0x670 (armor) and +0x674 (health).
    assert_eq!(get("splash.victim_armor_pool").2[2..4], [0x70, 0x06]);
    assert_eq!(get("splash.victim_health_pool").2[2..4], [0x74, 0x06]);
}

#[test]
#[ignore = "requires CS_ENGINE_IMAGE"]
fn accept_rocket_splash_the_rows_match_the_engine_image() {
    let path = std::env::var_os("CS_ENGINE_IMAGE").expect("CS_ENGINE_IMAGE must be set");
    let image = std::fs::read(path).expect("the engine image reads");
    let facts = cs_app::engine_facts::engine_facts().expect("the committed table parses");
    for (name, va, bytes) in splash_rows() {
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
