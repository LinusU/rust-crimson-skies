//! Acceptance for Rally #1885 `FLAK-C3-BURST-FX`: the `flak_effect` a flak
//! round's burst starts (flash light, smoke ball, close-range flicker), from
//! the stored definition and the FLAK-C1 gate law.
//!
//! Test prefix: `accept_flak_c3_`. The synthetic tests drive the production
//! round ([`FlakRound`]) and the production plugin with an authored
//! definition; the ignored test reads the retail record from `CS_GAME_DIR`.

use std::path::PathBuf;

use bevy::prelude::{App, FixedPostUpdate, GlobalTransform, PointLight, Vec3, With};

use cs_app::mission_session::{
    FlakBurstFxPlugin, FlakBurstFxReport, FlakEffectDefinition, FlakEffectResource, FlakFlash,
    FlakFlicker, FlakRecord, FlakRound, FlakStep, FlashStep, FlickerStep, MissionPlayerBody,
    OrdnanceBurst, PendingFlakBursts, PendingOrdnanceBursts, flash_range_at, flicker_color_at,
    flickers, read_flak_effect,
};
use cs_app::physics::PhysicsTickLedger;
use cs_types::Tick;

/// The stored effect's shape with authored numbers.
fn authored() -> FlakEffectDefinition {
    FlakEffectDefinition {
        light_range: [4.0, 10.0],
        light_color: [0.5, 0.5, 0.25],
        flash_steps: vec![
            FlashStep {
                delta_range: [16.0, 40.0],
                run_time_s: 0.3,
            },
            FlashStep {
                delta_range: [-14.0, -34.0],
                run_time_s: 0.1,
            },
        ],
        player_range_m: 100.0,
        flicker_steps: vec![
            FlickerStep {
                from: [1.0, 1.0, 1.0, 0.3],
                to: [0.0, 0.0, 0.0, 0.4],
                run_time_s: 0.2,
            },
            FlickerStep {
                from: [0.0, 0.0, 0.0, 0.4],
                to: [0.0, 0.0, 0.0, 0.4],
                run_time_s: 0.4,
            },
        ],
    }
}

fn record() -> FlakRecord {
    FlakRecord {
        velocity_mps: 850.0,
        range_m: 900.0,
        detonation_distance_m: 20.0,
        impact_proximity_m: 22.0,
        armor_damage: 10.0,
        health_damage: 10.0,
        lock_on: true,
        expires: false,
        detonate_at_range: false,
        cannon: false,
    }
}

/// A real round, flown until it bursts at the end of its range.
fn real_burst(muzzle: [f64; 3]) -> OrdnanceBurst {
    let mut round = FlakRound::fire(&record(), muzzle, [1.0, 0.0, 0.0]).expect("aim");
    for tick in 0..10_000 {
        if let FlakStep::Burst(burst) = round.step(Tick(tick), 1.0 / 120.0, None) {
            return burst;
        }
    }
    panic!("the round never burst");
}

fn stage(effect: Option<FlakEffectDefinition>, player: Option<Vec3>) -> App {
    let mut app = App::new();
    app.add_plugins(FlakBurstFxPlugin);
    app.insert_resource(PhysicsTickLedger {
        timestep_s: 1.0 / 120.0,
        ..Default::default()
    });
    if let Some(effect) = effect {
        app.insert_resource(FlakEffectResource(effect));
    }
    if let Some(at) = player {
        app.world_mut()
            .spawn((MissionPlayerBody, GlobalTransform::from_translation(at)));
    }
    app
}

fn publish(app: &mut App, burst: OrdnanceBurst) {
    app.world_mut()
        .resource_mut::<PendingFlakBursts>()
        .0
        .push(burst);
    app.world_mut().run_schedule(FixedPostUpdate);
}

fn count<T: bevy::prelude::Component>(app: &mut App) -> usize {
    app.world_mut()
        .query_filtered::<(), With<T>>()
        .iter(app.world())
        .count()
}

#[test]
fn accept_flak_c3_the_flash_follows_the_stored_light_steps() {
    let d = authored();
    assert_eq!(flash_range_at(&d, 0.0), Some([4.0, 10.0]));
    let mid = flash_range_at(&d, 0.15).unwrap();
    assert!((mid[0] - 12.0).abs() < 1e-4 && (mid[1] - 30.0).abs() < 1e-4);
    let peak = flash_range_at(&d, 0.3001).unwrap();
    assert!(peak[1] > 49.0);
    assert_eq!(flash_range_at(&d, 0.4), None, "past the last step");
}

#[test]
fn accept_flak_c3_the_flicker_gate_is_the_squared_player_range_compare() {
    let d = authored();
    assert!(flickers(&d, [100.0, 0.0, 0.0], [0.0; 3]), "exactly 100 m");
    assert!(!flickers(&d, [100.01, 0.0, 0.0], [0.0; 3]));
    assert!(flickers(&d, [60.0, 80.0, 0.0], [0.0; 3]), "3-4-5: 100 m");
    let start = flicker_color_at(&d, 0.0).unwrap();
    assert_eq!(start, [1.0, 1.0, 1.0, 0.3]);
    assert_eq!(flicker_color_at(&d, 0.6), None);
}

#[test]
fn accept_flak_c3_a_close_burst_flashes_smokes_and_flickers() {
    let burst = real_burst([0.0, 300.0, 0.0]);
    let mut app = stage(Some(authored()), Some(Vec3::new(850.0, 300.0, 0.0)));
    publish(&mut app, burst);
    let report = *app.world().resource::<FlakBurstFxReport>();
    assert_eq!(report.bursts_presented, 1);
    assert_eq!(report.flashes_spawned, 1);
    assert_eq!(report.flickers_started, 1);
    assert_eq!(report.smoke_balls_requested, 1);
    assert_eq!(report.trails_undrawn, 1, "the trails are not drawn");
    assert_eq!(count::<FlakFlash>(&mut app), 1);
    assert_eq!(count::<FlakFlicker>(&mut app), 1);
    let queued = &app.world().resource::<PendingOrdnanceBursts>().0;
    assert_eq!(queued.len(), 1, "the smoke ball is handed to the puffs");
    assert_eq!(queued[0].at, burst.at);
    assert!((queued[0].position.x - burst.position_m[0] as f32).abs() < 1e-3);
    let range = app
        .world_mut()
        .query::<&PointLight>()
        .single(app.world())
        .unwrap()
        .range;
    assert!((range - 10.0).abs() < 1e-3, "starts at the stored range");

    // Past the last step both are released.
    app.world_mut().resource_mut::<PhysicsTickLedger>().ticks = burst.at.0 + 120;
    app.world_mut().run_schedule(FixedPostUpdate);
    assert_eq!(count::<FlakFlash>(&mut app), 0);
    assert_eq!(count::<FlakFlicker>(&mut app), 0);
}

#[test]
fn accept_flak_c3_a_far_burst_has_no_flicker() {
    let burst = real_burst([0.0, 300.0, 0.0]);
    let mut app = stage(Some(authored()), Some(Vec3::new(0.0, 300.0, 0.0)));
    publish(&mut app, burst);
    let report = *app.world().resource::<FlakBurstFxReport>();
    assert_eq!(report.flashes_spawned, 1);
    assert_eq!(report.flickers_started, 0);
    assert_eq!(report.flickers_skipped, 1);
    assert_eq!(count::<FlakFlicker>(&mut app), 0);
}

#[test]
fn accept_flak_c3_without_the_effect_nothing_is_invented() {
    let burst = real_burst([0.0, 300.0, 0.0]);
    let mut app = stage(None, Some(Vec3::ZERO));
    publish(&mut app, burst);
    let report = *app.world().resource::<FlakBurstFxReport>();
    assert_eq!(report.bursts_undrawn, 1);
    assert_eq!(report.flashes_spawned, 0);
    assert_eq!(count::<FlakFlash>(&mut app), 0);
    assert!(app.world().resource::<PendingOrdnanceBursts>().0.is_empty());
}

fn game_dir() -> PathBuf {
    PathBuf::from(std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR must name the retail install"))
}

/// **Retail: the stored light, its two steps, the gate and the six flicker
/// steps.**
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_flak_c3_retail_flak_effect_parts() {
    let d = read_flak_effect(&game_dir()).expect("flak_effect reads");
    assert_eq!(d.light_range, [4.0, 10.0]);
    assert_eq!(d.light_color, [0.95, 0.76, 0.24]);
    assert_eq!(d.flash_steps.len(), 2);
    assert_eq!(d.flash_steps[0].delta_range, [16.0, 40.0]);
    assert_eq!(d.flash_steps[0].run_time_s, 0.3);
    assert_eq!(d.flash_steps[1].delta_range, [-14.0, -34.0]);
    assert_eq!(d.flash_steps[1].run_time_s, 0.1);
    assert_eq!(d.player_range_m, 100.0);
    assert_eq!(d.flicker_steps.len(), 6);
    assert_eq!(d.flicker_steps[0].from, [1.0, 1.0, 1.0, 0.3]);
    assert_eq!(d.flicker_steps[0].run_time_s, 0.2);
    let total: f32 = d.flicker_steps.iter().map(|s| s.run_time_s).sum();
    assert!((total - 1.5).abs() < 1e-4);
}
