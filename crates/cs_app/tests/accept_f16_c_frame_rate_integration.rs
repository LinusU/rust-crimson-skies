//! Acceptance scenario F16-C (AC03): equal input over equal wall time at 30,
//! 60 and 144 render FPS produces the same fixed ticks and the same spatial
//! state, including while an origin rebase happens mid-run.
//!
//! These tests exercise production code only: the `cs_sim::time` clock
//! (`SimClock`, the producer of ticks) and [`cs_app::origin`]'s
//! [`SpatialWorld`] plus [`FixedTickDriver`] (the consumer). The driver is the
//! real integration stage the later F13+/F23 fixed-step work consumes; these
//! tests drive it frame by frame the way a render loop would.
//!
//! What makes the scenario discriminating:
//!
//! * If the driver turned a frame's wall delta into a variable dt (or ran one
//!   step per frame) the 30, 60 and 144 FPS runs would disagree: at 30 FPS a
//!   frame carries more wall time than at 144 FPS.
//! * If the rebase policy were evaluated once per render frame instead of
//!   once per fixed tick, the origin would move on a different *tick* in each
//!   run even though the total wall time is the same, so the origin epoch and
//!   position would not agree.
//! * If a refused rebase left the world half-converted, the error test's
//!   before/after snapshot would differ.
//!
//! Every coordinate, velocity and origin threshold here is a newly authored
//! development value, not measured original game data.

use std::time::Duration;

use cs_app::origin::{
    DriverError, FixedTickDriver, OriginError, RebasePolicy, SpatialError, SpatialId,
    SpatialSubsystem, WorldOrigin, local_round_trip_tolerance_m,
};
use cs_sim::time::{ClockPolicy, NANOS_PER_SECOND, TickRate};
use cs_types::Tick;
use cs_types::space::{LocalPosition, SpaceError, WorldPosition};

/// Fixed simulation rate of the fixture.
const TICK_HZ: u32 = 64;
/// Total wall time fed to every run: 2 s at 64 Hz is exactly 128 ticks.
const TOTAL_NANOS: u128 = 2 * NANOS_PER_SECOND;
/// Ticks a 2 s run must reach.
const TICKS: u64 = 128;
/// Local-coordinate limit of the fixture's rebase policy, in metres.
const REBASE_LIMIT_M: f32 = 256.0;

/// World start of each subsystem, in the same order as
/// [`SpatialSubsystem::ALL`]. The records form a loose moving cluster so that
/// moving the origin to one of them keeps the rest inside the limit and the
/// scenario rebases a bounded number of times.
const STARTS: [[f64; 3]; 6] = [
    [-200.0, 0.0, 0.0],    // body
    [-180.0, 16.0, 0.0],   // projectile
    [-210.0, -8.0, 4.0],   // trigger
    [-190.0, 8.0, -4.0],   // ai path
    [-220.0, 0.0, 8.0],    // audio
    [-170.0, -16.0, -8.0], // camera history
];

/// Local displacement one fixed tick applies, in metres. Every component is an
/// exact dyadic fraction, so the f32 integration is reproducible bit for bit.
const DELTAS: [[f32; 3]; 6] = [
    [4.0, 0.0, 0.0],
    [4.25, 0.0, 0.0],
    [3.75, 0.0, 0.0],
    [4.0, 0.0, 0.0],
    [4.0, 0.0, 0.0],
    [4.0, 0.0, 0.0],
];

fn world(components: [f64; 3]) -> WorldPosition {
    WorldPosition::try_new(components).expect("test coordinates are finite")
}

/// The wall-time spans of one render run: `frame_rate` frames of equal
/// duration plus a final remainder frame, so their sum is exactly
/// `total_nanos` at any frame rate. This is the same frame split the F16-A
/// clock test uses.
fn frames_at(frame_rate: u32, total_nanos: u128) -> Vec<Duration> {
    let frames = u128::from(frame_rate);
    let per_frame = total_nanos / frames;
    let mut spans = vec![Duration::from_nanos(per_frame as u64); frames as usize];
    let rest = total_nanos - per_frame * frames;
    spans.push(Duration::from_nanos(rest as u64));
    spans
}

/// One record's final state, read back from the world.
struct RecordSample {
    id: SpatialId,
    subsystem: SpatialSubsystem,
    world: WorldPosition,
    local: LocalPosition,
}

/// Everything one run produced.
struct Run {
    frame_count: usize,
    ticks_per_frame: Vec<u64>,
    tick: Tick,
    origin: WorldOrigin,
    rebase_count: u64,
    first_rebase_tick: Option<Tick>,
    last_rebase_tick: Option<Tick>,
    records: Vec<RecordSample>,
}

/// Builds the fixture driver with all six subsystems spawned.
fn driver_with_fixture() -> FixedTickDriver {
    let rate = TickRate::new(TICK_HZ).expect("64 Hz is a valid rate");
    let origin = WorldOrigin::new(cs_app::origin::OriginEpoch(0), world([-240.0, 0.0, 0.0]));
    let policy = RebasePolicy::at_limit(REBASE_LIMIT_M).expect("the limit is positive and finite");
    let mut driver = FixedTickDriver::new(
        ClockPolicy::single_player_simulation(),
        rate,
        origin,
        policy,
    );
    for (index, subsystem) in SpatialSubsystem::ALL.into_iter().enumerate() {
        driver
            .world_mut()
            .spawn(subsystem, world(STARTS[index]), DELTAS[index])
            .expect("the fixture record is finite");
    }
    driver
}

/// Runs the fixture at `frame_rate` render FPS for [`TOTAL_NANOS`] of wall
/// time and returns the final tick count, origin and state.
fn simulate(frame_rate: u32) -> Run {
    let mut driver = driver_with_fixture();
    let spans = frames_at(frame_rate, TOTAL_NANOS);
    let mut ticks_per_frame = Vec::with_capacity(spans.len());
    for span in &spans {
        ticks_per_frame.push(
            driver
                .advance_frame(*span)
                .expect("the fixture never refuses a frame"),
        );
    }
    let records = driver
        .world()
        .records()
        .iter()
        .map(|record| RecordSample {
            id: record.id(),
            subsystem: record.subsystem(),
            world: record.world(),
            local: record.local(),
        })
        .collect();
    Run {
        frame_count: spans.len(),
        ticks_per_frame,
        tick: driver.tick(),
        origin: driver.world().origin(),
        rebase_count: driver.rebase_count(),
        first_rebase_tick: driver.first_rebase_tick(),
        last_rebase_tick: driver.last_rebase_tick(),
        records,
    }
}

/// AC03's minimum scenario: the same 2 s of wall time is split into 30, 60 and
/// 144 render frames; every run must reach the same 128 ticks and end in the
/// same spatial state, including after the origin rebased mid-run.
#[test]
fn accept_f16_c_equal_input_at_30_60_144_fps_agrees_on_ticks_and_state() {
    let runs: Vec<Run> = [30_u32, 60, 144].into_iter().map(simulate).collect();

    // The frame splits really are different, so the agreement below is not
    // vacuous.
    assert_ne!(
        runs[0].frame_count, runs[1].frame_count,
        "30 FPS and 60 FPS must be different frame splits"
    );
    assert_ne!(runs[1].frame_count, runs[2].frame_count);

    for (index, run) in runs.iter().enumerate() {
        assert_eq!(
            run.tick,
            Tick(TICKS),
            "run {index} must reach {TICKS} ticks in 2 s at {TICK_HZ} Hz"
        );
        assert_eq!(
            run.ticks_per_frame.iter().sum::<u64>(),
            TICKS,
            "run {index}: the per-frame ticks must sum to the committed ticks"
        );
        assert!(
            run.rebase_count >= 1,
            "run {index}: the scenario must exercise an origin rebase"
        );
    }

    // Frame-rate independence: every field of the later runs equals the first.
    let reference = &runs[0];
    for (index, run) in runs.iter().enumerate().skip(1) {
        assert_eq!(run.tick, reference.tick, "run {index}: ticks disagree");
        assert_eq!(
            run.origin, reference.origin,
            "run {index}: the origin frame disagrees"
        );
        assert_eq!(
            run.rebase_count, reference.rebase_count,
            "run {index}: the number of rebases disagrees"
        );
        assert_eq!(
            run.first_rebase_tick, reference.first_rebase_tick,
            "run {index}: the first rebase tick disagrees"
        );
        assert_eq!(
            run.last_rebase_tick, reference.last_rebase_tick,
            "run {index}: the last rebase tick disagrees"
        );
        assert_eq!(
            run.records.len(),
            reference.records.len(),
            "run {index}: a subsystem went missing"
        );
        for (actual, wanted) in run.records.iter().zip(&reference.records) {
            assert_eq!(actual.id, wanted.id);
            assert_eq!(actual.subsystem, wanted.subsystem);
            assert_eq!(
                actual.world, wanted.world,
                "run {index}: record {} world position disagrees",
                actual.id.0
            );
            assert_eq!(
                actual.local, wanted.local,
                "run {index}: record {} local pose disagrees",
                actual.id.0
            );
        }
    }

    // Every subsystem is present and its final local address still converts
    // back to its world position within the declared tolerance — the state
    // agreement is a real origin-frame conversion, not a frozen cache.
    let mut seen = Vec::new();
    for sample in &reference.records {
        seen.push(sample.subsystem);
        let back = reference
            .origin
            .world_of(sample.local)
            .expect("a finite local sum");
        let tolerance = local_round_trip_tolerance_m(sample.world, sample.local);
        for (actual, wanted) in back.to_array().into_iter().zip(sample.world.to_array()) {
            assert!(
                (actual - wanted).abs() <= tolerance,
                "record {}: the local cache must address its world position \
                 (got {actual}, expected {wanted}, tolerance {tolerance} m)",
                sample.id.0
            );
        }
    }
    seen.sort_by_key(|subsystem| subsystem.label());
    seen.dedup();
    assert_eq!(
        seen.len(),
        SpatialSubsystem::ALL.len(),
        "every spatial subsystem must be represented"
    );
}

/// The origin rebase is part of the fixed-tick schedule, not the render
/// schedule: it lands on exactly the same tick at every frame rate, and in the
/// middle of the run.
#[test]
fn accept_f16_c_rebase_lands_on_the_same_tick_at_every_frame_rate() {
    let runs: Vec<Run> = [30_u32, 60, 144].into_iter().map(simulate).collect();
    let first = runs[0]
        .first_rebase_tick
        .expect("the scenario rebases at least once");
    let last = runs[0]
        .last_rebase_tick
        .expect("the scenario rebases at least once");
    assert!(
        first.0 > 0 && last.0 < TICKS,
        "the rebase must be mid-run, got first={} last={}",
        first.0,
        last.0
    );

    for run in &runs {
        assert_eq!(run.first_rebase_tick, Some(first));
        assert_eq!(run.last_rebase_tick, Some(last));
        assert_eq!(run.rebase_count, runs[0].rebase_count);
        // A rebase opens a new epoch, so the origin really moved.
        assert_eq!(run.origin.epoch().0, runs[0].rebase_count);
    }
}

/// The pause integration: a frozen session clock advances no ticks and no
/// spatial state while paused, and resumes exactly where it left off.
#[test]
fn accept_f16_c_paused_session_advances_no_ticks_and_no_spatial_state() {
    let mut driver = driver_with_fixture();
    assert_eq!(
        driver
            .advance_frame(Duration::from_millis(500))
            .expect("runs"),
        32,
        "500 ms at 64 Hz is 32 ticks"
    );
    let before = (driver.tick(), driver.world().clone());

    driver.set_paused(true);
    assert_eq!(
        driver.advance_frame(Duration::from_secs(5)).expect("runs"),
        0,
        "a paused simulation clock produces no ticks"
    );
    assert_eq!(
        (driver.tick(), driver.world().clone()),
        before,
        "paused wall time must not move the world"
    );

    driver.set_paused(false);
    assert_eq!(
        driver
            .advance_frame(Duration::from_millis(500))
            .expect("runs"),
        32,
        "resume must not bank the paused time"
    );
    assert_eq!(driver.tick(), Tick(64));
}

/// Error propagation and retry: a rebase that cannot be expressed in the new
/// frame is reported, leaves the session untouched, and `retry` starts a fresh
/// generation without the failed state.
#[test]
fn accept_f16_c_refused_rebase_is_propagated_and_retry_starts_fresh() {
    let mut driver = driver_with_fixture();
    driver
        .advance_frame(Duration::from_secs(1))
        .expect("the fixture runs");
    let before = (driver.tick(), driver.world().clone());

    let refused = driver.rebase(world([-1.0e40, 0.0, 0.0]));
    assert_eq!(
        refused,
        Err(DriverError::Spatial(SpatialError::Origin(
            OriginError::Space(SpaceError::NonFinite { field: "local.x" })
        ))),
        "an f32 overflow in the new frame must surface as a driver error"
    );
    assert_eq!(
        (driver.tick(), driver.world().clone()),
        before,
        "a refused rebase must leave the whole session untouched"
    );

    driver.retry(WorldOrigin::new(
        cs_app::origin::OriginEpoch(0),
        world([0.0, 0.0, 0.0]),
    ));
    assert_eq!(driver.tick(), Tick(0), "a retry restarts the clock");
    assert!(
        driver.world().is_empty(),
        "a retry must not inherit a stale record"
    );
    assert_eq!(driver.rebase_count(), 0);
    assert_eq!(driver.world().epoch().0, 0);

    assert_eq!(
        driver
            .advance_frame(Duration::from_secs(1))
            .expect("the fresh session runs"),
        64,
        "the fresh session advances from tick 0"
    );
}
