//! PARITY2-DMG-FX-B acceptance (#1806): the dark smoke puffs of an ordnance
//! burst, from the measured `large_black_smokeball` definition.
//!
//! Task test prefix: `accept_parity2_dmg_fx_b_`. The synthetic tests drive the
//! production composition and the production rocket path; the retail test
//! (`#[ignore = "requires CS_GAME_DIR"]`) reads the stored emitter and its
//! textures.

use std::path::PathBuf;

use bevy::prelude::{App, Vec3};

use cs_app::mission_session::{
    BurstPuff, BurstPuffDefinition, BurstPuffReport, BurstPuffSource, BurstPuffSourceResource,
    MissionCockpitActions, PendingOrdnanceBursts, PuffBurst, measure_burst_puffs, plan_burst,
    puff_pose, read_burst_puff_definition, teardown,
};
use cs_app::physics::PhysicsTickLedger;
use cs_types::Tick;
use cs_types::input::{Action, FlightCommand};

use crate::campaign::m01_play_weapons::armed_stage;

/// A definition with the stored emitter's shape and newly authored numbers.
fn authored_definition() -> BurstPuffDefinition {
    BurstPuffDefinition {
        number: 5,
        size: [4.0, 7.0],
        lifetime_s: [4.0, 5.0],
        min_velocity: [-65.0, -65.0, -65.0],
        max_velocity: [65.0, 65.0, 65.0],
        friction: 8.0,
        textures: vec!["a".into(), "b".into(), "c".into()],
    }
}

fn authored_source() -> BurstPuffSource {
    BurstPuffSource {
        definition: authored_definition(),
        images: Vec::new(),
        texture_archive: "synthetic".into(),
    }
}

fn puffs(app: &mut App) -> Vec<BurstPuff> {
    let mut query = app.world_mut().query::<&BurstPuff>();
    query.iter(app.world()).copied().collect()
}

fn fire_rocket(app: &mut App) {
    app.world_mut().insert_resource(MissionCockpitActions {
        actions: vec![Action::Flight(FlightCommand::FireSecondary)],
    });
    app.update();
}

#[test]
fn accept_parity2_dmg_fx_b_a_burst_plans_the_stored_number_inside_the_stored_ranges() {
    let definition = authored_definition();
    let burst = PuffBurst {
        at: Tick(10),
        position: Vec3::new(100.0, 50.0, -20.0),
    };
    let planned = plan_burst(&definition, burst, 120.0);
    assert_eq!(planned.len(), 5);
    for (puff, texture) in &planned {
        assert!((4.0..=7.0).contains(&puff.size));
        assert!(puff.life_ticks >= 480 && puff.life_ticks <= 600);
        assert!(puff.velocity.abs().max_element() <= 65.0);
        assert_eq!(puff.origin, burst.position);
        assert!(*texture < 3);
    }
    assert_eq!(
        planned,
        plan_burst(&definition, burst, 120.0),
        "deterministic"
    );
    let (puff, _) = planned[0];
    let (at_birth, opacity) = puff_pose(&puff, 0, 120.0);
    assert_eq!(at_birth, burst.position);
    assert_eq!(opacity, 1.0);
    let (later, faded) = puff_pose(&puff, puff.life_ticks / 2, 120.0);
    assert!(later.distance(burst.position) > 0.0 && faded < 1.0);
}

/// **A rocket that destroys its target publishes a burst, and the burst draws
/// the stored number of puffs at the rocket's position; they expire with the
/// committed tick ledger.**
#[test]
fn accept_parity2_dmg_fx_b_a_rocket_detonation_draws_the_stored_puffs_and_they_expire() {
    let (_scratch, mut app, _actor) = armed_stage("parity2-dmg-fx-b-rocket");
    app.insert_resource(BurstPuffSourceResource(authored_source()));
    let mut drawn = false;
    for _ in 0..400 {
        fire_rocket(&mut app);
        if app.world().resource::<BurstPuffReport>().bursts_drawn > 0 {
            drawn = true;
            break;
        }
    }
    assert!(
        drawn,
        "a rocket detonation published a burst that was drawn"
    );
    let live = puffs(&mut app);
    assert!(
        live.len() >= 5,
        "at least one burst of 5 puffs: {}",
        live.len()
    );
    let report = *app.world().resource::<BurstPuffReport>();
    assert_eq!(report.bursts_undrawn, 0);
    assert_eq!(report.puffs_spawned % 5, 0);
    // Past every life: all puffs are released.
    let last_birth = live.iter().map(|p| p.born).max().unwrap();
    let longest = live.iter().map(|p| p.life_ticks).max().unwrap();
    app.world_mut().resource_mut::<PhysicsTickLedger>().ticks = last_birth + longest + 1;
    app.world_mut().run_schedule(bevy::prelude::FixedPostUpdate);
    assert!(puffs(&mut app).is_empty(), "every puff expired");
    teardown(&mut app);
}

/// **With no definition installed the burst is counted as undrawn, never
/// invented.**
#[test]
fn accept_parity2_dmg_fx_b_a_burst_without_a_definition_draws_nothing_and_says_so() {
    let (_scratch, mut app, _actor) = armed_stage("parity2-dmg-fx-b-undrawn");
    app.world_mut()
        .resource_mut::<PendingOrdnanceBursts>()
        .0
        .push(PuffBurst {
            at: Tick(1),
            position: Vec3::ZERO,
        });
    app.update();
    let report = *app.world().resource::<BurstPuffReport>();
    assert_eq!(report.bursts_undrawn, 1);
    assert!(puffs(&mut app).is_empty());
    teardown(&mut app);
}

fn game_dir() -> PathBuf {
    PathBuf::from(std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR must name the retail install"))
}

/// **Retail: the stored emitter and its three smoke textures.**
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_parity2_dmg_fx_b_retail_emitter_and_textures() {
    let definition = read_burst_puff_definition(&game_dir()).expect("the emitter reads");
    assert_eq!(definition.number, 5);
    assert_eq!(definition.size, [4.0, 7.0]);
    assert_eq!(definition.lifetime_s, [4.0, 5.0]);
    assert_eq!(definition.friction, 8.0);
    assert_eq!(
        definition.textures,
        ["thickblksmoke01", "thickblksmoke02", "thickblksmoke03"]
    );
    let source = measure_burst_puffs(&game_dir(), "c3").expect("the textures decode");
    assert_eq!(source.images.len(), 3);
    assert!(
        source
            .texture_archive
            .to_ascii_lowercase()
            .contains("zbd/c3/")
    );
}
