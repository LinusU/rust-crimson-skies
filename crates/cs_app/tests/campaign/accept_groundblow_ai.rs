//! #2224 (`GROUNDBLOW-AI-PROBE`): the ground blow's world-sweep probe is cast
//! for and handed to every flying aircraft, not only the player's seat.
//!
//! Task test prefix: `accept_groundblow_ai_`. The original casts the probe
//! (`0x48bf60`) for every vehicle it runs the force block for; the port's fill
//! used to cast for the player only, so the AI branch of the law's ground blow
//! (`B * ai_groundblow * groundblow_mag`, `0x48c317`) never fired in a composed
//! mission. These tests compose the real M01 (`CS_GAME_DIR`), put an AI aircraft
//! over M01's mapped sea and check that it is probed, while a crashed body gets
//! none. That the AI term then changes its flight is #2107's law, not this fill.
use avian3d::prelude::{LinearVelocity, Position, Rotation};
use bevy::prelude::*;
use cs_app::playtest::OriginalFlightSet;
use cs_app::playtest::PlaytestState;
use cs_app::playtest::fatal_contact::{FailureCause, FatalContactRule, PlayerCrash};
use cs_app::playtest::scene::{PlaytestAircraft, PlaytestOriginalFlight};
use cs_app::playtest::world_sweep::GroundBlowProbes;

use super::accept_groundblow_fill::{
    DIVE_ALTITUDE_M, PROBE_BACKOFF_M, above_the_flat_sea, compose, dive_nose, place, retail_lock,
};

/// The first two AI aircraft records, in entity order.
fn ai_records(app: &mut App) -> Vec<Entity> {
    let mut found: Vec<Entity> = app
        .world_mut()
        .query::<(Entity, &PlaytestOriginalFlight)>()
        .iter(app.world())
        .filter(|(_, flight)| !flight.is_player)
        .map(|(entity, _)| entity)
        .collect();
    found.sort();
    found
}

fn pose(app: &App, entity: Entity) -> (Vec3, Quat, Vec3) {
    let world = app.world();
    (
        world.get::<Position>(entity).expect("a body pose").0,
        world.get::<Rotation>(entity).expect("a body pose").0,
        world.get::<LinearVelocity>(entity).expect("a body pose").0,
    )
}

/// Withholds the fill's answer for the AI record only: the player keeps its own.
#[derive(Resource)]
struct WithholdFor(Entity);

fn withhold_ai_probe(mut probes: ResMut<GroundBlowProbes>, target: Res<WithholdFor>) {
    let ai = target.0;
    probes.hits.retain(|(entity, _)| *entity != ai);
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_groundblow_ai_aircraft_near_the_sea_is_probed() {
    let _guard = retail_lock();
    let mut app = compose();
    app.update();
    let ai = *ai_records(&mut app)
        .first()
        .expect("M01 flies an AI aircraft");
    let (_, centre, top) = above_the_flat_sea(&mut app);
    let start = Vec3::new(centre.x, top + DIVE_ALTITUDE_M, centre.z + PROBE_BACKOFF_M);
    place(&mut app, ai, start, dive_nose(), 60.0);

    let before = app.world().resource::<PlaytestState>().groundblow_probes;
    let mut answers = 0;
    for _ in 0..4 {
        app.update();
        let probes = app.world().resource::<GroundBlowProbes>();
        // The player's record and the AI's both cast.
        assert_eq!(probes.cast_this_tick, 2, "player and AI both cast");
        if let Some(hit) = probes.hit(ai) {
            assert!((0.0..=400.0).contains(&hit.distance_m), "{hit:?}");
            answers += 1;
        }
    }
    assert!(
        answers >= 3,
        "the AI over the sea was filled on {answers}/4"
    );
    // The driver handed the law the probe for both seats on each of the ticks.
    let after = app.world().resource::<PlaytestState>().groundblow_probes;
    assert!(after - before >= 8, "{} applications", after - before);
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_groundblow_ai_crashed_body_gets_no_probe() {
    let _guard = retail_lock();
    let mut app = compose();
    app.update();
    let crashed = *ai_records(&mut app)
        .first()
        .expect("M01 flies an AI aircraft");
    let flying = app
        .world_mut()
        .query_filtered::<Entity, With<PlaytestAircraft>>()
        .single(app.world())
        .expect("the player aircraft");
    let (_, centre, top) = above_the_flat_sea(&mut app);
    let start = Vec3::new(centre.x, top + DIVE_ALTITUDE_M, centre.z + PROBE_BACKOFF_M);
    place(&mut app, crashed, start, dive_nose(), 60.0);
    place(&mut app, flying, start + Vec3::X * 20.0, dive_nose(), 60.0);
    let mut rule = FatalContactRule::new([0.0, 0.0]);
    rule.crash = Some(PlayerCrash {
        tick: 0,
        position_m: start.to_array(),
        cause: FailureCause::TerrainContact {
            terrain: Entity::PLACEHOLDER,
            severity: 1.0,
            damage: 0.0,
        },
    });
    app.world_mut().entity_mut(crashed).insert(rule);
    app.update();
    let probes = app.world().resource::<GroundBlowProbes>();
    assert!(probes.hit(crashed).is_none(), "a crashed body was probed");
    assert!(
        probes.hit(flying).is_some(),
        "the flying player was not probed"
    );
    assert_eq!(probes.cast_this_tick, 1, "only the flying record casts");
}

/// Withholding the probe from the AI record changes the AI's trajectory: two
/// M01s in lockstep with the AI on the same dive over the sea, one handed the
/// sweep's answer and one with the AI's answer withheld between fill and driver.
/// The AI branch of the law (`B * ai_groundblow * groundblow_mag`) is the only
/// difference, so any divergence of the AI body is that term firing.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_groundblow_ai_withholding_the_probe_changes_the_ai_trajectory() {
    let _guard = retail_lock();
    let mut filled = compose();
    let mut withheld = compose();
    filled.update();
    withheld.update();
    let a = *ai_records(&mut filled)
        .first()
        .expect("M01 flies an AI aircraft");
    let b = *ai_records(&mut withheld)
        .first()
        .expect("M01 flies an AI aircraft");
    withheld.insert_resource(WithholdFor(b));
    withheld.add_systems(
        FixedUpdate,
        withhold_ai_probe
            .after(OriginalFlightSet::GroundBlowFill)
            .before(OriginalFlightSet::OriginalFlight),
    );
    let (_, centre, top) = above_the_flat_sea(&mut filled);
    let start = Vec3::new(centre.x, top + DIVE_ALTITUDE_M, centre.z + PROBE_BACKOFF_M);
    place(&mut filled, a, start, dive_nose(), 60.0);
    place(&mut withheld, b, start, dive_nose(), 60.0);

    let mut answers = 0;
    let mut diverged = false;
    for _ in 0..3 {
        filled.update();
        withheld.update();
        if filled
            .world()
            .resource::<GroundBlowProbes>()
            .hit(a)
            .is_some()
        {
            answers += 1;
        }
        assert!(
            withheld
                .world()
                .resource::<GroundBlowProbes>()
                .hit(b)
                .is_none(),
            "the withheld run still carried the AI's probe"
        );
        diverged |= pose(&filled, a) != pose(&withheld, b);
    }
    assert!(
        answers >= 1,
        "the sweep answered no tick: the comparison is vacuous"
    );
    assert!(
        diverged,
        "withholding the AI's probe changed nothing: the AI term did not fire"
    );
}

/// Prints what the all-seat probe costs per tick on M01 (one AI record plus
/// the player): the wall time of whole `App::update` calls. It does not isolate
/// the fill system. Measurement only, for `docs/findings/`.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_groundblow_ai_measure_m01_tick_budget() {
    let _guard = retail_lock();
    let mut app = compose();
    for _ in 0..30 {
        app.update();
    }
    let n = 600;
    let mut times = Vec::with_capacity(n);
    for _ in 0..n {
        let t = std::time::Instant::now();
        app.update();
        times.push(t.elapsed().as_micros());
    }
    times.sort_unstable();
    let probes = app.world().resource::<GroundBlowProbes>();
    println!(
        "BUDGET update_us p50={} p95={} p99={} max={} cast_per_tick={} cast_total={}",
        times[n / 2],
        times[n * 95 / 100],
        times[n * 99 / 100],
        times[n - 1],
        probes.cast_this_tick,
        probes.cast_total
    );
}
