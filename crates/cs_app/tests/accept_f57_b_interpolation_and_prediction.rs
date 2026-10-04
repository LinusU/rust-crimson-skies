//! F57-B acceptance: bounded interpolation and bounded local prediction.
//!
//! Minimum scenario (spec F57 `### F57-B`, sheet AC02): **a server correction
//! during a local boost keeps ammo/fuel authoritative.** The rest are the
//! sheet's other discriminating cases at this stage: AC04 (recycled ids never
//! share interpolation history), the jitter-buffer bounds (teleport, gap, loss,
//! extrapolation limit) and the bounded correction. Everything runs through the
//! production path ledger -> `publish_snapshot` -> bytes -> `RemoteMirror` ->
//! `RemoteInterpolator` / `LocalPredictor`; nothing here is original behavior.

use cs_app::network::physics::{
    BufferRefusal, InterpolationConfig, LocalPredictor, PredictedPose, PredictionConfig,
    ReconcileKind, RemoteInterpolator, RemoteMirror, SampleMode, publish_snapshot,
};
use cs_app::origin::{OriginEpoch, WorldOrigin};
use cs_net::snapshot::Snapshot;
use cs_sim::net_state::{NetActorState, NetLifecycle, NetStateLedger};
use cs_types::Tick;
use cs_types::net::{ActorAllocator, ActorId, SessionId};
use cs_types::space::{Quaternion, WorldPosition};

const SESSION: SessionId = match SessionId::new(57) {
    Some(id) => id,
    None => unreachable!(),
};

fn origin() -> WorldOrigin {
    WorldOrigin::new(
        OriginEpoch(1),
        WorldPosition::try_new([1_000.0, 0.0, 1_000.0]).expect("finite"),
    )
}

fn at(x: f64) -> WorldPosition {
    WorldPosition::try_new([x, 100.0, 0.0]).expect("finite")
}

struct World {
    ledger: NetStateLedger,
    mirror: RemoteMirror,
    interpolator: RemoteInterpolator,
    actor: ActorId,
}

impl World {
    fn new() -> Self {
        let mut ledger = NetStateLedger::new(SESSION);
        let actor = ActorAllocator::new(SESSION).allocate().expect("serial");
        ledger
            .spawn(NetActorState::spawn(
                actor,
                at(1_000.0),
                Quaternion::IDENTITY,
                400,
            ))
            .expect("spawn");
        Self {
            ledger,
            mirror: RemoteMirror::new(SESSION, origin()),
            interpolator: RemoteInterpolator::new(InterpolationConfig::default()),
            actor,
        }
    }

    /// Moves the actor, publishes, round-trips the bytes and feeds the receiver.
    fn step(&mut self, tick: u64, x: f64, mutate: impl FnOnce(&mut NetActorState)) {
        let mut state = *self.ledger.state(self.actor).expect("known");
        state.pose.position = at(x);
        state.linear_velocity_mps = [60.0, 0.0, 0.0];
        mutate(&mut state);
        self.ledger.publish(state).expect("owned generation");
        self.deliver(tick);
    }

    fn deliver(&mut self, tick: u64) {
        let snapshot = publish_snapshot(&self.ledger, self.mirror.origin()).expect("publish");
        let bytes = snapshot.encode(SESSION).expect("encode");
        let decoded = Snapshot::decode(&bytes, SESSION).expect("decode");
        let report = self.mirror.ingest(&decoded, Tick(tick));
        let refusals = self.interpolator.observe(&report, &decoded, &self.mirror);
        assert!(refusals.is_empty(), "{refusals:?}");
    }

    fn x_at(&self, now: u64) -> (f64, SampleMode) {
        let sampled = self
            .interpolator
            .sample(self.actor, Tick(now))
            .expect("representable")
            .expect("buffered");
        (sampled.state.position.x(), sampled.mode)
    }
}

#[test]
fn accept_f57_b_interpolation_blends_between_snapshots_and_never_blends_discrete_state() {
    let mut world = World::new();
    world.step(10, 1_000.0, |s| s.weapons.primary_rounds = 400);
    world.step(20, 1_010.0, |s| s.weapons.primary_rounds = 380);
    // delay 6 ticks: now 21 renders tick 15, halfway.
    let sampled = world
        .interpolator
        .sample(world.actor, Tick(21))
        .unwrap()
        .unwrap();
    assert_eq!(sampled.mode, SampleMode::Interpolated);
    assert!((sampled.state.position.x() - 1_005.0).abs() < 0.05);
    // Rounds are the earlier authoritative record's, not a blend.
    assert_eq!(sampled.state.rounds[0], 400);
}

#[test]
fn accept_f57_b_loss_is_bridged_then_extrapolation_is_bounded() {
    let mut world = World::new();
    world.step(10, 1_000.0, |_| {});
    world.step(20, 1_010.0, |_| {});
    // Snapshots for ticks 11..19 were lost: the pair still blends.
    assert_eq!(world.x_at(21).1, SampleMode::Interpolated);
    // Past the newest sample: project along velocity (60 m/s, 3 ticks = 3 m).
    let (x, mode) = world.x_at(29);
    assert_eq!(mode, SampleMode::Extrapolated);
    assert!((x - 1_013.0).abs() < 0.1, "{x}");
    // Far past: the pose stops at the limit instead of drifting.
    let (x, mode) = world.x_at(500);
    assert_eq!(mode, SampleMode::ExtrapolationExhausted);
    assert!((x - 1_016.0).abs() < 0.1, "{x}");
}

#[test]
fn accept_f57_b_a_teleport_is_held_not_interpolated_across() {
    let mut world = World::new();
    world.step(10, 1_000.0, |_| {});
    world.step(20, 9_000.0, |_| {});
    // Render tick 15 is between them: held at the pre-teleport pose.
    let (x, mode) = world.x_at(21);
    assert_eq!(mode, SampleMode::Held);
    assert!((x - 1_000.0).abs() < 0.05);
    // Once the render tick reaches the teleport sample it shows it.
    let (x, _) = world.x_at(26);
    assert!((x - 9_000.0).abs() < 0.05);
}

#[test]
fn accept_f57_b_buffer_is_bounded_and_refuses_old_ticks() {
    let mut world = World::new();
    for i in 0..40_u64 {
        world.step(10 + i, 1_000.0 + i as f64, |_| {});
    }
    assert_eq!(
        world.interpolator.sample_count(world.actor),
        world.interpolator.config().capacity
    );
    let record = *world.mirror.aircraft(world.actor).expect("mirrored");
    assert!(matches!(
        world.interpolator.push(&record),
        Err(BufferRefusal::NotNewer { .. })
    ));
}

#[test]
fn accept_f57_b_recycled_ids_never_share_interpolation_history() {
    let mut world = World::new();
    world.step(10, 1_000.0, |_| {});
    world.step(20, 1_010.0, |_| {});
    let old_generation = world.ledger.generation(world.actor).unwrap();
    // The id is reused for a new aircraft far away.
    world.ledger.forget(world.actor).expect("forget");
    world
        .ledger
        .spawn(NetActorState::spawn(
            world.actor,
            at(5_000.0),
            Quaternion::IDENTITY,
            400,
        ))
        .expect("respawn");
    let new_generation = world.ledger.generation(world.actor).unwrap();
    assert!(new_generation > old_generation);
    let stale = *world.mirror.aircraft(world.actor).expect("old mirrored");
    world.deliver(30);
    assert_eq!(world.interpolator.sample_count(world.actor), 1);
    // Sampling between the old and new times never produces a blend of the two.
    for now in 20..45 {
        let (x, _) = world.x_at(now);
        assert!(
            (x - 5_000.0).abs() < 0.05 || (x - 1_010.0).abs() < 0.05 || (x - 1_000.0).abs() < 0.05,
            "blended x {x} at {now}"
        );
        let sampled = world
            .interpolator
            .sample(world.actor, Tick(now))
            .unwrap()
            .unwrap();
        assert_eq!(sampled.state.generation, new_generation.get());
    }
    // A late packet of the old generation is refused.
    assert!(matches!(
        world.interpolator.push(&stale),
        Err(BufferRefusal::StaleGeneration { .. })
    ));
}

#[test]
fn accept_f57_b_a_destroyed_actor_leaves_no_ghost_and_an_ended_generation_stays_ended() {
    let mut world = World::new();
    world.step(10, 1_000.0, |_| {});
    let before = *world.mirror.aircraft(world.actor).expect("mirrored");
    let generation = world.ledger.generation(world.actor).unwrap();
    world
        .ledger
        .end_lifecycle(world.actor, generation, NetLifecycle::Destroyed)
        .expect("destroy");
    world.deliver(11);
    assert!(
        world
            .interpolator
            .sample(world.actor, Tick(30))
            .unwrap()
            .is_none()
    );
    assert_eq!(world.interpolator.track_count(), 0);
    assert!(world.interpolator.push(&before).is_err());
}

fn pose(x: f64) -> PredictedPose {
    PredictedPose {
        position: at(x),
        orientation: Quaternion::IDENTITY,
    }
}

/// AC02: the local boost is predicted (the body runs ahead), then a server
/// record arrives. Ammunition and boost capacity are the server's, the pose is
/// corrected by a bounded smoothing.
#[test]
fn accept_f57_b_a_server_correction_during_a_local_boost_keeps_ammo_and_fuel_authoritative() {
    let mut world = World::new();
    let mut predictor = LocalPredictor::new(
        world.actor,
        world.ledger.generation(world.actor).unwrap().get(),
        PredictionConfig::default(),
    );
    // The local body boosts: it runs 0.3 m/tick ahead of what the server will say.
    for tick in 1..=10_u64 {
        predictor
            .record_predicted(Tick(tick), pose(1_000.0 + tick as f64 * 2.3), true)
            .expect("increasing ticks");
    }
    assert!(predictor.boost_shown());
    // The server saw tick 10 at +2.0 m/tick, with less fuel and fewer rounds
    // than any local guess: boost capacity drained to 0.25, 37 rounds left.
    world.step(10, 1_000.0 + 20.0, |s| {
        s.flight.boost_capacity = 0.25;
        s.weapons.primary_rounds = 37;
    });
    let record = *world.mirror.aircraft(world.actor).expect("mirrored");
    let result = predictor.reconcile(&record).expect("reconciles");
    assert_eq!(result.kind, ReconcileKind::Smoothed);
    assert!((result.error_m - 3.0).abs() < 0.05, "{}", result.error_m);

    let loadout = predictor.authoritative().expect("server word");
    assert_eq!(loadout.rounds[0], 37);
    assert!((loadout.flight[2] - 0.25).abs() < 1e-4);
    // Still "boosting" on screen, with no way to reach the capacity from it.
    assert!(predictor.boost_shown());

    // The error is removed in bounded steps that sum to the whole error.
    let mut total = 0.0;
    let mut steps = 0;
    while predictor.correcting() {
        let step = predictor.next_correction();
        assert!(step.translation_m[0].abs() <= 3.0 / 5.0 + 0.1);
        total += step.translation_m[0];
        steps += 1;
    }
    assert_eq!(steps, PredictionConfig::default().correction_ticks);
    assert!((total - (record.position.x() - (1_000.0 + 23.0))).abs() < 1e-6);
    // A stale record cannot rewind the authoritative loadout.
    let mut older = record;
    older.tick = Tick(5);
    older.rounds = [400, 400];
    assert!(matches!(
        predictor.reconcile(&older),
        Err(BufferRefusal::NotNewer { .. })
    ));
    assert_eq!(predictor.authoritative().unwrap().rounds[0], 37);
}

#[test]
fn accept_f57_b_a_large_error_snaps_and_an_old_generation_is_refused() {
    let mut world = World::new();
    let generation = world.ledger.generation(world.actor).unwrap().get();
    let mut predictor = LocalPredictor::new(world.actor, generation, PredictionConfig::default());
    predictor
        .record_predicted(Tick(10), pose(1_100.0), false)
        .unwrap();
    world.step(10, 1_000.0, |_| {});
    let record = *world.mirror.aircraft(world.actor).unwrap();
    let result = predictor.reconcile(&record).unwrap();
    assert_eq!(result.kind, ReconcileKind::Snapped);
    let step = predictor.next_correction();
    assert!((step.translation_m[0] + 100.0).abs() < 1e-6);
    assert!(!predictor.correcting());

    let mut wrong_generation = record;
    wrong_generation.generation = generation + 1;
    wrong_generation.tick = Tick(11);
    let mut fresh = LocalPredictor::new(world.actor, generation + 1, PredictionConfig::default());
    assert!(fresh.reconcile(&record).is_err());
    assert!(predictor.reconcile(&wrong_generation).is_err());
}
