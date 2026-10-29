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
use avian3d::prelude::Position;
use bevy::prelude::*;
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

fn position(app: &App, entity: Entity) -> Vec3 {
    app.world().get::<Position>(entity).expect("a body pose").0
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
    let _ = position(&app, ai);
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
