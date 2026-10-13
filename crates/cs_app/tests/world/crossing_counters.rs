//! Task #643: the ordinary-flight sweep decides each `(body, volume)` pair
//! once per tick — `SweptBodyTracks`' counters count **transitions**, not
//! decision visits.
//!
//! Owner path: `crates/cs_app/src/world/crossings.rs`. Task test prefix:
//! `accept_t643_`.
//!
//! The defect was measured while reviewing #499
//! (`docs/findings/2026-10-04-t499-world-sensor-volume-collision-layer.md`), on
//! the arch world's `trigger.sensor` under the `world_app_with_spawn_preflight()`
//! composition (`bevy 0.19.1` / `avian3d 0.7.0`, `SubstepCount(1)`, 120 Hz,
//! gravity zero): a 192 m/s projectile registered *inside* the volume on its
//! first tick left it on the second, and `exits` read **2** for the one exit
//! transition. `sweep_volume_crossings` built its per-tick decision list as
//! `inside ∪ ends` chained with the swept cast's `touched` keys, and the chain
//! arm did not deduplicate — a cast that *begins* overlapping reports the
//! volume at distance zero, so a volume the body was inside at the segment's
//! start *and* the segment still touched was walked twice, and the
//! `(inside, ends inside) = (true, false)` branch counted the exit twice. The
//! same duplication is latent on the `(false, true)` entry branch — a volume
//! the segment touched *and* ended inside — where the once-per-pair ledger's
//! refusal hid it: `entries` stayed right while `duplicates` counted a visit
//! the decision list should never have produced.
//!
//! The fix deduplicates the decision list itself: each pair is visited once
//! per tick, so `entries`/`exits` count the transitions the doc comments
//! describe. The ledger is untouched — it still refuses a genuinely repeated
//! record; the dedup is what keeps one decision from asking it to.
//!
//! Every test drives production code: the world is built by
//! [`spawn_world`](cs_app::world::spawn_world) from the authored
//! [`arch_world`](cs_app::world::arch_world) record, the projectile is spawned
//! by [`spawn_body`](cs_app::physics::spawn_body) on the `Projectile` layer,
//! and the composition is the fixture's
//! [`spawn_preflight`](cs_app::world::WorldFixtureBuilder::spawn_preflight) —
//! the same one the measurement was taken on, so both crossing producers run
//! against the same pair.
//!
//! No original data and no `CS_GAME_DIR` access: every mass, extent, position
//! and speed below is synthetic fixture content, and nothing here claims what
//! the 2000 PC original did with a trigger volume.

use avian3d::prelude::Position;
use bevy::prelude::Entity;
use cs_app::objectives::{CrossingSource, TriggerCrossings};
use cs_app::physics::{BASELINE_FIXED_HZ, BodyMode, BodySpec, SpawnPreflight, spawn_body};
use cs_app::world::{
    OBJECT_SENSOR, SENSOR_HALF_M, SENSOR_POS_M, SweptBodyTracks, WorldFixture, arch_world,
};
use cs_content::world::WorldObjectId;
use cs_sim::collision::{CollisionLayer, ShapeClass};

/// The rate every cell below is measured at.
const RATE_HZ: u32 = BASELINE_FIXED_HZ;

/// The projectile's box half extent: 10 cm, the body of the #499 measurement.
const PROJECTILE_HALF_M: f32 = 0.05;

/// Ticks of travel between the projectile's leading face and the volume's
/// near face at spawn: F23-B's first-tick hole, as the #499 measurement held
/// it.
const SPAWN_IN_HOLE_TICKS: f32 = 0.4;

/// The measured cell's speed: 1.6 m per tick at 120 Hz, so the spawn tick ends
/// inside the 1.5 m volume and the next tick ends past it — one whole tick
/// inside, then one exit.
const BOUNDARY_SPEED_M_S: f32 = 192.0;

/// The dwell cell's speed: 0.5 m per tick, so the body ends inside the volume
/// on every one of the three ticks — it never leaves.
const DWELL_SPEED_M_S: f32 = 60.0;

/// Ticks flown in total, the spawn tick included.
const TICKS: u64 = 3;

/// The near and far faces of the arch world's `trigger.sensor` along the
/// flight axis `z`, the volume's thin axis.
const ARCH_SENSOR_NEAR_Z_M: f32 = SENSOR_POS_M[2] as f32 - SENSOR_HALF_M[2] as f32;
const ARCH_SENSOR_FAR_Z_M: f32 = SENSOR_POS_M[2] as f32 + SENSOR_HALF_M[2] as f32;

/// One tick of travel at `speed_m_s`, in meters.
fn travel_m(speed_m_s: f32) -> f32 {
    speed_m_s / RATE_HZ as f32
}

/// The projectile: a 10 cm swept box on the `Projectile` layer, flying `+z`
/// through the centre of the arch world's sensor volume, spawned
/// [`SPAWN_IN_HOLE_TICKS`] of travel short of the volume's near face — the
/// same body #499's measurement flew.
fn arch_projectile(speed_m_s: f32) -> BodySpec {
    let spawn_z =
        ARCH_SENSOR_NEAR_Z_M - SPAWN_IN_HOLE_TICKS * travel_m(speed_m_s) - PROJECTILE_HALF_M;
    BodySpec {
        layer: CollisionLayer::Projectile,
        shape: ShapeClass::Solid,
        mode: BodyMode::Dynamic,
        mass_kg: 1.0,
        half_extents_m: [PROJECTILE_HALF_M; 3],
        position_m: [SENSOR_POS_M[0] as f32, SENSOR_POS_M[1] as f32, spawn_z],
        linear_velocity_m_s: [0.0, 0.0, speed_m_s],
    }
}

/// A world fixture with the spawn preflight and its gameplay consumer
/// installed — the composition the defect was measured on — and `spec`
/// spawned into it and marked the way `PhysicsSession::spawn` marks a moving
/// body on a swept layer.
fn preflight_fixture(spec: &BodySpec) -> (WorldFixture, Entity) {
    let mut fixture = WorldFixture::builder(arch_world().expect("the arch world is well formed"))
        .spawn_preflight()
        .build()
        .expect("the arch fixture builds");
    let body = spawn_body(fixture.app_mut().world_mut(), spec).expect("the spec is valid");
    fixture
        .app_mut()
        .world_mut()
        .entity_mut(body)
        .insert(SpawnPreflight);
    (fixture, body)
}

/// The one object's collider entity, from a built world's own report.
fn collider_of(fixture: &WorldFixture, key: &str) -> Entity {
    let id = WorldObjectId::new(key).expect("the fixture object key is valid");
    fixture
        .spawned()
        .collider_for(&id)
        .unwrap_or_else(|| panic!("{key} carries a collider in this world"))
}

/// The pass's own counters, read after a tick: `(sweeps, touches, entries,
/// exits)`.
fn counters(fixture: &WorldFixture) -> (u64, u64, u64, u64) {
    let tracks = fixture.world().resource::<SweptBodyTracks>();
    (
        tracks.sweeps(),
        tracks.touches(),
        tracks.entries(),
        tracks.exits(),
    )
}

/// The body's end-of-tick `z`, read from the engine.
fn end_z(fixture: &WorldFixture, body: Entity) -> f32 {
    fixture
        .world()
        .get::<Position>(body)
        .expect("the projectile still exists")
        .0
        .z
}

/// The crossings the pair producers delivered.
fn crossings(fixture: &WorldFixture) -> &TriggerCrossings {
    fixture
        .world()
        .get_resource::<TriggerCrossings>()
        .expect("the composition installs the crossing stream")
}

/// **The measured cell: one exit transition is counted once.**
///
/// The 192 m/s projectile ends its spawn tick inside `trigger.sensor` (the
/// preflight's non-blocking cast is what records the crossing there) and ends
/// the next tick past the far face. On that second tick the volume is both in
/// the body's stored `inside` set *and* in the cast's `touched` — a swept cast
/// that begins overlapping reports the volume at distance zero — so the
/// undeduplicated decision list walked the `(true, false)` branch twice and
/// `exits` counted the single exit as two.
///
/// `touches` is pinned as well as `exits`: the double count exists only while
/// the cast still reports a volume the segment starts inside, and a test that
/// did not check the touch would go green the day an engine stops reporting
/// it, with the defect still in place.
///
/// Observable failure without the deduplication: `exits` reads 2 after the
/// exit tick, not 1.
#[test]
fn accept_t643_an_exit_transition_is_counted_once() {
    let (mut fixture, body) = preflight_fixture(&arch_projectile(BOUNDARY_SPEED_M_S));
    let volume = collider_of(&fixture, OBJECT_SENSOR);

    // Tick 1: first sight. The pass registers the body at the pose the tick
    // integrated it to — inside the volume — and casts nothing: there is no
    // previous sample to form a segment with.
    fixture.step(1);
    let z = end_z(&fixture, body);
    assert!(
        z > ARCH_SENSOR_NEAR_Z_M && z < ARCH_SENSOR_FAR_Z_M,
        "the measured cell must end the spawn tick inside the volume's \
         [{ARCH_SENSOR_NEAR_Z_M}, {ARCH_SENSOR_FAR_Z_M}], got z = {z}"
    );
    assert_eq!(
        counters(&fixture),
        (0, 0, 0, 0),
        "first sight registers the body and sweeps nothing"
    );

    // Tick 2: the one exit transition. The segment starts inside the volume
    // and ends past its far face, the cast still reports it — one touch — and
    // the pair is decided once.
    fixture.step(1);
    assert!(
        end_z(&fixture, body) > ARCH_SENSOR_FAR_Z_M,
        "and the next tick must end past the far face — the exit this test counts"
    );
    let (sweeps, touches, entries, exits) = counters(&fixture);
    assert_eq!(sweeps, 1, "one segment cast ran, on the exit tick");
    assert_eq!(
        touches, 1,
        "the cast begins inside the volume and still reports it at distance \
         zero — this touch is what walked the pair twice before the dedup"
    );
    assert_eq!(
        exits, 1,
        "the body made ONE exit transition, so the counter that documents \
         'exit transitions observed' must read 1 — 2 is the deduplication \
         defect this task removes"
    );
    assert_eq!(
        entries, 0,
        "a body registered inside leaves: an exit is bookkeeping, never an \
         entry the sweep records"
    );

    // Tick 3: the segment is past the volume entirely; nothing about the pair
    // may be counted again.
    fixture.step(1);
    assert_eq!(
        counters(&fixture).3,
        1,
        "a decided transition is not re-counted on a later tick"
    );

    // And the pair's one delivered crossing is the spawn tick's: the sweep
    // contributes no record of its own here, so the stream holds exactly the
    // preflight's entry and nothing else.
    let all = crossings(&fixture).crossings();
    assert_eq!(
        all.len(),
        1,
        "one crossing was delivered for the pair, by the spawn tick: {all:?}"
    );
    assert_eq!(
        all[0].source,
        CrossingSource::SpawnTickPreflight,
        "the ordinary-flight sweep delivered nothing for a body it first met \
         inside — its whole observable part is the exit it counted once: {:?}",
        all[0]
    );
    assert!(
        all[0].is_of(volume),
        "the delivered crossing names the sensor volume's collider entity"
    );
}

/// **A body that never leaves counts no exit.**
///
/// The 60 m/s projectile ends all three ticks inside the volume: registered
/// inside on first sight, inside → inside on both later ticks, so the
/// `(true, true)` branch runs and nothing is ever an exit. `exits` must read
/// 0 throughout — a counter that counted visits instead of transitions could
/// not say so.
#[test]
fn accept_t643_a_body_that_never_leaves_counts_no_exit() {
    let (mut fixture, body) = preflight_fixture(&arch_projectile(DWELL_SPEED_M_S));

    for tick in 1..=TICKS {
        fixture.step(1);
        let z = end_z(&fixture, body);
        assert!(
            z > ARCH_SENSOR_NEAR_Z_M && z < ARCH_SENSOR_FAR_Z_M,
            "the dwell cell must keep the body inside the volume: tick {tick} \
             ended at z = {z}, outside [{ARCH_SENSOR_NEAR_Z_M}, \
             {ARCH_SENSOR_FAR_Z_M}]"
        );
    }
    assert_eq!(
        counters(&fixture).3,
        0,
        "the body never left the volume: no exit transition exists to count"
    );
}

/// **The entry half of the same deduplication**: a body that enters the volume
/// inside one tick has the volume in `touched` (the cast crossed its face)
/// *and* in `ends` (it ends inside), the other way the undeduplicated list
/// walked one pair twice.
///
/// The ledger masked the defect on this branch — the second visit's record
/// was refused, so `entries` already read right — and what the refusal cost
/// is pinned here: `duplicates` must stay 0, because a decision list that
/// visits each pair once per tick never hands the ledger a record to refuse.
/// The fix is the deduplication, never a weakened once-per-pair ledger: the
/// delivered stream, the `entries` counter and the crossing's own fields are
/// all asserted to be exactly the single entry the pair earned.
///
/// The projectile is spawned farther back than the measurement's — the spawn
/// tick's preflight cast does not reach the volume — so the body is
/// *registered* outside it, and the ordinary-flight sweep is the producer
/// that decides the entry on the tick the body lands inside. The same flight
/// then covers the exit half of the fix too: the tick after the entry is the
/// measured `inside` ∩ `touched` case again.
///
/// Observable failure without the deduplication: `duplicates` reads 1 on the
/// entry tick and `exits` reads 2 on the exit tick, while the delivered
/// stream looks unchanged — the defect lives exactly in the counters it
/// corrupts.
#[test]
fn accept_t643_an_entry_decision_is_made_once_when_the_segment_ends_inside() {
    // Far enough back that the spawn tick ends *short* of the volume and the
    // second tick ends inside it: z = -7.50 ends tick 1 at -5.90 (outside)
    // and tick 2 at -4.30 (inside).
    let mut spec = arch_projectile(BOUNDARY_SPEED_M_S);
    spec.position_m[2] = -7.50;
    let (mut fixture, body) = preflight_fixture(&spec);
    let volume = collider_of(&fixture, OBJECT_SENSOR);

    // Tick 1: registered outside — the preflight's cast does not reach the
    // volume either, so the pair is still unrecorded when the sweep decides.
    fixture.step(1);
    assert!(
        end_z(&fixture, body) < ARCH_SENSOR_NEAR_Z_M - PROJECTILE_HALF_M,
        "the spawn tick must end clear of the volume's near face, or the \
         entry would be the preflight's rather than the sweep's"
    );
    assert_eq!(
        counters(&fixture),
        (0, 0, 0, 0),
        "first sight registers the body and sweeps nothing"
    );
    assert!(
        crossings(&fixture).crossings().is_empty(),
        "and the pair is not in the stream yet: the entry below is the \
         sweep's own decision, not a re-read of the spawn tick's"
    );

    // Tick 2: the entry — the segment crosses the near face (one touch) and
    // ends inside, which puts the volume in `touched` *and* `ends` at once.
    fixture.step(1);
    let z = end_z(&fixture, body);
    assert!(
        z > ARCH_SENSOR_NEAR_Z_M && z < ARCH_SENSOR_FAR_Z_M,
        "tick 2 must end inside the volume, got z = {z}"
    );
    let (sweeps, touches, entries, exits) = counters(&fixture);
    assert_eq!(
        (sweeps, touches),
        (1, 1),
        "one cast, crossing the near face"
    );
    assert_eq!(
        entries, 1,
        "the entry the pair earned, counted once: {entries}"
    );
    assert_eq!(exits, 0, "no exit yet — the body ends inside");
    let stream = crossings(&fixture);
    let all = stream.crossings();
    assert_eq!(
        all.len(),
        1,
        "exactly one crossing delivered for the pair: {all:?}"
    );
    assert_eq!(
        (all[0].actor, all[0].volume),
        (body, volume),
        "the record names this body and the sensor volume it entered"
    );
    assert_eq!(
        all[0].source,
        CrossingSource::OrdinaryFlightSweep,
        "and the ordinary-flight sweep is the producer that decided it"
    );
    assert!(
        all[0].is_entry(),
        "an outside → inside decision is an entry"
    );
    assert_eq!(all[0].tick, 2, "stamped with the tick it was decided on");
    assert_eq!(
        stream.duplicates(),
        0,
        "the pair is decided once: the ledger is never asked to refuse a \
         second visit the undeduplicated list would have produced — a \
         `duplicates` of 1 here is the defect, hidden by the ledger"
    );

    // Tick 3: the same flight's exit — the volume is inside at the segment's
    // start and still touched along it, the measured double-count case.
    fixture.step(1);
    assert!(
        end_z(&fixture, body) > ARCH_SENSOR_FAR_Z_M,
        "tick 3 must end past the far face — the exit is decided once more"
    );
    let (sweeps, touches, entries, exits) = counters(&fixture);
    assert_eq!(
        (sweeps, touches),
        (2, 2),
        "the exit tick's cast runs and reports the volume it starts inside"
    );
    assert_eq!(
        exits, 1,
        "the one exit transition, counted once — 2 is the defect this task \
         removes"
    );
    assert_eq!(entries, 1, "and the entry count never moved");
    assert_eq!(
        crossings(&fixture).crossings().len(),
        1,
        "the stream still holds the one entry the pair earned — an exit is \
         bookkeeping, never a second record"
    );
}
