//! Acceptance scenario F16-B (AC02): rebase during a projectile flight and a
//! docking approach; the outcome equals an un-rebased run.
//!
//! These tests exercise production code only: [`cs_app::origin`]'s
//! [`WorldOrigin`], [`SpatialAnchor`] and the atomic [`OriginShift`]
//! transaction. The flight and the approach are deliberately simple
//! fixed-tick local (f32) integrations — the same shape a physics step has —
//! and each tick moves an anchor with [`SpatialAnchor::advance_local`], so the
//! rebase really participates in the trajectory instead of being bypassed.
//!
//! What makes them discriminating:
//!
//! * Removing the local re-anchoring from [`OriginShift::apply`] (leaving the
//!   old local cache, or converting it with the old origin) moves the body by
//!   the origin offset, hundreds of metres beyond the tolerance below.
//! * Clearing the swept segment on a rebase (a teleport in disguise) fails
//!   [`accept_f16_b_rebase_preserves_swept_continuity_during_flight`].
//! * A rebase that changed the velocity would fail the explicit speed check
//!   at the rebase tick.

use cs_app::origin::{OriginEpoch, OriginShift, SpatialAnchor, SweptSegment, WorldOrigin};
use cs_types::space::{LocalPosition, WorldPosition};

/// Ticks simulated in each run. 240 ticks at 64 Hz is 3.75 simulated seconds.
const TICKS: u64 = 240;
/// The tick *before* which the rebased run moves the origin.
const REBASE_TICK: u64 = 96;

/// Projectile local displacement per tick, in metres: a fast, slightly
/// descending round. Every component is an exact dyadic fraction, so the
/// fixed-tick f32 integration is exact and the tolerance below only has to
/// absorb the rebase conversion itself.
const PROJECTILE_DELTA: [f32; 3] = [1.875, -0.125, 0.468_75];
/// The docking ship closes on its carrier diagonally, 24 m of travel per
/// second over the run.
const DOCKING_DELTA: [f32; 3] = [-0.125, -0.062_5, 0.062_5];

/// Where the projectile starts its flight.
const PROJECTILE_START: [f64; 3] = [4096.0, 512.0, -2048.0];
/// The carrier: the run ends with the ship exactly on it.
const CARRIER: [f64; 3] = [4000.0, 496.0, -1988.0];
/// The docking ship starts 30 m out and 15 m up from the carrier and closes
/// the gap over exactly [`TICKS`] ticks.
const DOCKING_START: [f64; 3] = [4030.0, 511.0, -2003.0];

/// Where the origin moves at [`REBASE_TICK`], relative to the projectile's
/// own world position. A non-zero offset keeps the converted local endpoints
/// non-trivial, so the conversion is actually exercised. Every component is
/// an exact dyadic fraction so the run stays exact.
const REBASE_OFFSET: [f64; 3] = [512.0, 0.0, 0.25];

/// Declared tolerance of a rebased run's world trajectory against the
/// un-rebased one, in metres.
///
/// A correct rebase changes nothing but the f32 representation of a local
/// coordinate (at most one rounding of a value below 8 km, whose f32 spacing
/// is under 1 mm). A rebase that dropped the frame change would displace a
/// body by the origin offset, which here is more than 100 m.
const REBASE_MATCH_TOLERANCE_M: f64 = 1e-3;

fn world(components: [f64; 3]) -> WorldPosition {
    WorldPosition::try_new(components).expect("test coordinates are finite")
}

/// One body's world position after every tick, plus the rebase evidence when
/// a rebase happened.
struct Run {
    projectile: Vec<WorldPosition>,
    docking: Vec<WorldPosition>,
    /// The origin frame in force after the rebase, and the projectile's swept
    /// segment on its first post-rebase tick.
    rebased: Option<(WorldOrigin, SweptSegment)>,
}

/// Runs the scenario once; `rebased_at` moves the origin at the start of that
/// tick's step. Both runs share identical initial conditions and inputs.
fn simulate(rebased_at: Option<u64>) -> Run {
    let mut origin = WorldOrigin::new(OriginEpoch(0), world([0.0, 0.0, 0.0]));
    let mut projectile = SpatialAnchor::new(&origin, world(PROJECTILE_START)).expect("finite");
    let mut docking = SpatialAnchor::new(&origin, world(DOCKING_START)).expect("finite");

    let mut projectile_track = Vec::with_capacity(TICKS as usize);
    let mut docking_track = Vec::with_capacity(TICKS as usize);
    let mut rebased = None;

    for step in 0..TICKS {
        if rebased_at == Some(step) {
            // The origin jumps near the projectile and every anchor is
            // converted to the new frame before anything simulates in it.
            let [px, py, pz] = projectile.world().to_array();
            let target = world([
                px + REBASE_OFFSET[0],
                py + REBASE_OFFSET[1],
                pz + REBASE_OFFSET[2],
            ]);
            let shift = OriginShift::rebase(origin, target).expect("epoch can rebase");
            let mut anchors = [projectile, docking];
            shift.apply(&mut anchors).expect("both anchors convert");
            origin = shift.to();
            [projectile, docking] = anchors;
        }

        projectile
            .advance_local(&origin, PROJECTILE_DELTA)
            .expect("finite movement");
        docking
            .advance_local(&origin, DOCKING_DELTA)
            .expect("finite movement");
        projectile_track.push(projectile.world());
        docking_track.push(docking.world());

        if rebased_at == Some(step) {
            rebased = Some((
                origin,
                projectile
                    .sweep()
                    .expect("the post-rebase step is continuous"),
            ));
        }
    }

    Run {
        projectile: projectile_track,
        docking: docking_track,
        rebased,
    }
}

fn distance_m(a: WorldPosition, b: WorldPosition) -> f64 {
    let [ax, ay, az] = a.to_array();
    let [bx, by, bz] = b.to_array();
    ((ax - bx).powi(2) + (ay - by).powi(2) + (az - bz).powi(2)).sqrt()
}

fn assert_close(actual: WorldPosition, wanted: WorldPosition, what: &str) {
    for (axis, (a, w)) in actual
        .to_array()
        .into_iter()
        .zip(wanted.to_array())
        .enumerate()
    {
        assert!(
            (a - w).abs() <= REBASE_MATCH_TOLERANCE_M,
            "{what}: axis {axis} is {a}, expected {w} (tolerance {REBASE_MATCH_TOLERANCE_M} m)"
        );
    }
}

/// AC02's minimum scenario: a rebase while a projectile is flying and a ship
/// is on its docking approach leaves both world trajectories and the docking
/// outcome equal to the un-rebased run, with no velocity jump at the rebase.
#[test]
fn accept_f16_b_rebase_during_projectile_flight_and_docking_approach_matches_unrebased_run() {
    let straight = simulate(None);
    let rebased = simulate(Some(REBASE_TICK));

    assert_eq!(straight.projectile.len(), TICKS as usize);
    assert_eq!(rebased.projectile.len(), TICKS as usize);

    // Every tick agrees, so the rebase introduces neither a persistent offset
    // nor a transient jump.
    for (tick, (straight_position, rebased_position)) in straight
        .projectile
        .iter()
        .zip(&rebased.projectile)
        .enumerate()
    {
        assert_close(
            *rebased_position,
            *straight_position,
            &format!("projectile at tick {tick}"),
        );
    }
    for (tick, (straight_position, rebased_position)) in
        straight.docking.iter().zip(&rebased.docking).enumerate()
    {
        assert_close(
            *rebased_position,
            *straight_position,
            &format!("docking ship at tick {tick}"),
        );
    }

    // The docking outcome agrees: the ship arrives at the carrier on the same
    // tick, at the same place, in both runs.
    let straight_end = *straight.docking.last().expect("a nonempty track");
    let rebased_end = *rebased.docking.last().expect("a nonempty track");
    assert_close(rebased_end, straight_end, "docking end of approach");
    assert!(
        distance_m(straight_end, world(CARRIER)) <= REBASE_MATCH_TOLERANCE_M,
        "the scenario must actually dock: the un-rebased ship ends {} m from the carrier",
        distance_m(straight_end, world(CARRIER))
    );
    assert!(
        distance_m(rebased_end, world(CARRIER)) <= REBASE_MATCH_TOLERANCE_M,
        "the rebase must not miss the dock"
    );

    // No false speed: the world displacement over the rebase tick equals the
    // un-rebased displacement for that same tick, for both bodies.
    let step = REBASE_TICK as usize;
    for (name, straight_track, rebased_track) in [
        ("projectile", &straight.projectile, &rebased.projectile),
        ("docking ship", &straight.docking, &rebased.docking),
    ] {
        let straight_step = [
            straight_track[step].to_array()[0] - straight_track[step - 1].to_array()[0],
            straight_track[step].to_array()[1] - straight_track[step - 1].to_array()[1],
            straight_track[step].to_array()[2] - straight_track[step - 1].to_array()[2],
        ];
        let rebased_step = [
            rebased_track[step].to_array()[0] - rebased_track[step - 1].to_array()[0],
            rebased_track[step].to_array()[1] - rebased_track[step - 1].to_array()[1],
            rebased_track[step].to_array()[2] - rebased_track[step - 1].to_array()[2],
        ];
        for (axis, (a, b)) in rebased_step.into_iter().zip(straight_step).enumerate() {
            assert!(
                (a - b).abs() <= REBASE_MATCH_TOLERANCE_M,
                "{name}: the rebase must not become a velocity impulse (axis {axis}: {a} != {b})"
            );
        }
    }
}

/// The rebase is a rebase, not a teleport: the projectile's swept segment
/// survives, still describes the last real tick of movement, and its local
/// endpoint is addressed in the new frame.
#[test]
fn accept_f16_b_rebase_preserves_swept_continuity_during_flight() {
    let straight = simulate(None);
    let rebased = simulate(Some(REBASE_TICK));
    let (origin_after, segment) = rebased
        .rebased
        .as_ref()
        .expect("the rebased run records its segment");
    assert_eq!(origin_after.epoch(), OriginEpoch(1));

    let step = REBASE_TICK as usize;
    assert_close(
        segment.from_world(),
        straight.projectile[step - 1],
        "the swept segment still starts at the previous tick's world position",
    );
    assert_close(
        segment.from_world(),
        rebased.projectile[step - 1],
        "the swept segment matches the rebased run's own previous tick",
    );

    // Both endpoints are addressed in the new frame, so a swept query can use
    // the local endpoints and still describe the same path.
    let from_local = segment.from_local();
    assert_ne!(from_local, LocalPosition::ZERO);
    assert_close(
        origin_after.world_of(from_local).expect("finite local sum"),
        segment.from_world(),
        "the swept segment's local endpoint converts back to its world endpoint",
    );
}
