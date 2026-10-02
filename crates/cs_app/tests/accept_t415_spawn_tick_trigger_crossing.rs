//! Task #415: the gameplay consumption of the swept spawn preflight's
//! spawn-tick trigger crossing.
//!
//! Producer: `cs_app::physics::preflight` (F23-C's swept spawn response, F23-D's
//! second non-blocking cast). Consumer: [`cs_app::objectives`], the F39 trigger
//! boundary. Task test prefix: `accept_t415_`.
//!
//! F23-D measured the hole and closed the **record** half of it: a body spawned
//! inside one tick of travel of a thin trigger crosses the volume inside the
//! very tick it is invisible to collision detection, and the engine reports
//! **no** overlap for it — because F23-C's criterion
//! `accept_f23_c_preflight_never_stops_on_a_sensor` forbids a sensor from
//! stopping or delaying a spawn, so the body keeps its velocity and flies
//! through. The record exists
//! ([`SpawnPreflightEvent::passed`](cs_app::physics::SpawnPreflightEvent::passed));
//! what was left open was the rule — whether gameplay consumes it as a crossing
//! — and that is what these tests pin.
//!
//! Every test drives the production composition: the real
//! [`PhysicsSession`] with the real swept preflight, the real contact reporter,
//! the real adapter's tick ledger and the production
//! [`SpawnTickTriggerPlugin`]. Removing the consumer, its once-per-pair ledger
//! or its delivery system fails them; so does making the delivery touch a body.
//!
//! No original data and no `CS_GAME_DIR`: every mass, extent, position and
//! speed below is newly authored synthetic fixture data, and the geometry is
//! F23-D's (`accept_f23_d_a_spawn_records_a_sensor_it_crossed_before_a_solid_
//! stops_it`), reused so the two stages measure the same crossing. Whether the
//! *original* game reported a spawn-tick crossing, and with what delay, is
//! unknown and is left to the calibration stage.

use bevy::prelude::{Entity, Mut};
use cs_app::objectives::{
    CrossingSource, SpawnTickTriggerPlugin, TriggerCrossing, TriggerCrossings,
};
use cs_app::physics::{
    BASELINE_FIXED_HZ, BodyMode, BodySpec, CONTACT_FACE_TOLERANCE_M, PROBE_RATES_HZ,
    PROBE_SPEEDS_M_S, PhysicsSession, SENSOR_MIN_SPEED_FRACTION, SPAWN_IN_HOLE_TRAVEL_TICKS,
    SessionFrame,
};
use cs_sim::collision::{CollisionLayer, ContactKind, ShapeClass};

/// A 2 cm trigger volume at `x`, the thickness F23-D's contact probe declares
/// for its obstacle. A body travelling more than that per tick crosses it
/// without a sample ever landing inside, which is the whole gap.
fn trigger_spec(x: f32) -> BodySpec {
    BodySpec {
        layer: CollisionLayer::Trigger,
        shape: ShapeClass::Sensor,
        mode: BodyMode::Static,
        mass_kg: 1.0,
        half_extents_m: [0.01, 1.0, 1.0],
        position_m: [x, 0.0, 0.0],
        linear_velocity_m_s: [0.0; 3],
    }
}

/// A 10 cm projectile — the same body F23-D's contact probe fires.
fn projectile_spec(x: f32, speed_m_s: f32) -> BodySpec {
    BodySpec {
        layer: CollisionLayer::Projectile,
        shape: ShapeClass::Solid,
        mode: BodyMode::Dynamic,
        mass_kg: 1.0,
        half_extents_m: [0.05; 3],
        position_m: [x, 0.0, 0.0],
        linear_velocity_m_s: [speed_m_s, 0.0, 0.0],
    }
}

/// A 2 cm solid wall at `x`.
fn wall_spec(x: f32) -> BodySpec {
    BodySpec {
        layer: CollisionLayer::StaticWorld,
        shape: ShapeClass::Solid,
        mode: BodyMode::Static,
        mass_kg: 1.0,
        half_extents_m: [0.01, 2.0, 2.0],
        position_m: [x, 0.0, 0.0],
        linear_velocity_m_s: [0.0; 3],
    }
}

/// A session with the production trigger-crossing consumer installed.
fn consuming_session(hz: u32) -> PhysicsSession {
    PhysicsSession::builder()
        .fixed_hz(hz)
        .configure(|app| {
            app.add_plugins(SpawnTickTriggerPlugin);
        })
        .build()
}

/// The same composition with no consumer: the F23-C/F23-D baseline a test
/// compares a delivery against.
fn bare_session(hz: u32) -> PhysicsSession {
    PhysicsSession::builder().fixed_hz(hz).build()
}

/// The crossings the consumer delivered, or a panic naming the gap: a session
/// without the plugin has no resource, which is the state the F23-D review
/// called "the crossing leaves no trace anywhere".
fn crossings(session: &PhysicsSession) -> &TriggerCrossings {
    session
        .world()
        .and_then(|world| world.get_resource::<TriggerCrossings>())
        .expect("SpawnTickTriggerPlugin installs the crossing resource")
}

/// The same resource, mutably, for the acts a consumer performs on its own
/// stream: taking a batch, clearing its counters, resetting its ledger.
fn crossings_mut(session: &mut PhysicsSession) -> Mut<'_, TriggerCrossings> {
    session
        .world_mut()
        .and_then(|world| world.get_resource_mut::<TriggerCrossings>())
        .expect("SpawnTickTriggerPlugin installs the crossing resource")
}

/// One tick's observed state for a body: what the engine classified, what the
/// preflight recorded, and where the body ended up.
///
/// Every entity is replaced by its **role** — its index in the list the trace
/// was given. Two runs of the same geometry live in two worlds, and a world
/// allocates its entity ids from its own count of archetypes, so the same body
/// is `154v0` in one and `155v0` in the other; comparing the raw ids would
/// report a difference that is only the numbering, and comparing the poses
/// alone would hide a changed classification. Roles compare the facts.
#[derive(Debug, PartialEq)]
struct TickTrace {
    /// The classified contacts naming the traced body, in the reporter's own
    /// pair order: both roles, the classification and the tick.
    reports: Vec<((u8, u8), ContactKind, u64)>,
    /// The preflight records naming it: the clamp and the stop, the obstacle's
    /// role and distance, and the crossed volume's role and distance.
    spawns: Vec<PreflightTrace>,
    /// Where the body ended the tick.
    position_m: [f32; 3],
    /// How fast it was going at the end of the tick.
    velocity_m_s: [f32; 3],
}

/// One preflight record, with its entities named by role.
#[derive(Debug, PartialEq)]
struct PreflightTrace {
    clamped: bool,
    stopped: bool,
    hit: Option<u8>,
    distance_m: Option<f32>,
    passed: Option<u8>,
    passed_distance_m: Option<f32>,
}

/// The role an entity has in a trace, or `None` for one the trace was not
/// given.
fn role_of(entity: Entity, roles: &[Entity]) -> Option<u8> {
    roles
        .iter()
        .position(|role| *role == entity)
        .map(|index| index as u8)
}

/// Steps `ticks` ticks and traces `body` through them, naming the other bodies
/// of the scenario through `roles`.
fn trace(
    session: &mut PhysicsSession,
    body: Entity,
    roles: &[Entity],
    ticks: u64,
) -> Vec<TickTrace> {
    let mut history = Vec::new();
    for _ in 0..ticks {
        let frame: SessionFrame = session.step(1).expect("the session is active");
        let reports = frame
            .reports
            .iter()
            .filter(|report| report.bodies.contains(&body))
            .map(|report| {
                (
                    (
                        role_of(report.bodies[0], roles).expect("a reported pair is named"),
                        role_of(report.bodies[1], roles).expect("a reported pair is named"),
                    ),
                    report.kind,
                    report.tick,
                )
            })
            .collect();
        let spawns = frame
            .spawn_events
            .iter()
            .filter(|event| event.body == body)
            .map(|event| PreflightTrace {
                clamped: event.clamped,
                stopped: event.stopped,
                hit: event.hit.and_then(|hit| role_of(hit, roles)),
                distance_m: event.distance_m,
                passed: event.passed.and_then(|passed| role_of(passed, roles)),
                passed_distance_m: event.passed_distance_m,
            })
            .collect();
        let pose = session.pose(body).expect("the body stays in the world");
        history.push(TickTrace {
            reports,
            spawns,
            position_m: pose.position_m,
            velocity_m_s: pose.linear_velocity_m_s,
        });
    }
    history
}

/// F23-D's geometry, unchanged: at 120 Hz and 60 m/s the spawn tick is 0.5 m of
/// travel, the trigger sits at `x = -0.10` and the wall at `x = +0.10`, so the
/// projectile meets the trigger 0.24 m into the tick and the wall 0.44 m into
/// it. A body that crosses the trigger and is then stopped by a wall in the
/// same tick crossed the trigger.
struct Geometry {
    trigger: Entity,
    wall: Entity,
    projectile: Entity,
}

impl Geometry {
    /// The roles a trace names them by, in a fixed order so two runs of this
    /// geometry compare value for value.
    fn roles(&self) -> [Entity; 3] {
        [self.trigger, self.wall, self.projectile]
    }

    /// The role of the trigger volume.
    const TRIGGER: u8 = 0;
    /// The role of the solid wall.
    const WALL: u8 = 1;
}

fn f23_d_geometry(session: &mut PhysicsSession) -> Geometry {
    let trigger = session
        .spawn(&trigger_spec(-0.10))
        .expect("the trigger is valid")
        .entity;
    let wall = session
        .spawn(&wall_spec(0.10))
        .expect("the wall is valid")
        .entity;
    let projectile = session
        .spawn(&projectile_spec(-0.40, 60.0))
        .expect("the projectile is valid")
        .entity;
    Geometry {
        trigger,
        wall,
        projectile,
    }
}

/// The role a trace gives a trigger volume in the two-body scenarios below.
const TRIGGER_ROLE: u8 = 0;
/// The role a trace gives the traced body there.
const BODY_ROLE: u8 = 1;

/// The acceptance scenario: a trigger crossed inside the spawn tick, and a
/// wall stopping the same tick, is delivered as one gameplay crossing.
#[test]
fn accept_t415_a_a_spawn_tick_crossing_is_delivered_as_a_gameplay_crossing() {
    let mut session = consuming_session(BASELINE_FIXED_HZ);
    let geometry = f23_d_geometry(&mut session);
    let history = trace(&mut session, geometry.projectile, &geometry.roles(), 6);

    // The reason the record exists, measured in the same run: the engine
    // classified nothing between the projectile and the trigger on any of the
    // six ticks. The one solid report below is the wall, not the volume.
    for (tick, state) in history.iter().enumerate() {
        assert!(
            !state.reports.iter().any(|((first, second), ..)| {
                *first == Geometry::TRIGGER || *second == Geometry::TRIGGER
            }),
            "a volume crossed inside the spawn tick is not classified at all, so \
             this test is not measuring the hole it means to: {tick} {state:?}"
        );
    }
    let wall_reports = history
        .iter()
        .flat_map(|state| state.reports.iter())
        .filter(|((first, second), ..)| *first == Geometry::WALL || *second == Geometry::WALL)
        .count();
    assert_eq!(
        wall_reports, 1,
        "and the wall is still reported exactly once, classified as a solid contact: \
         {history:?}"
    );

    let delivered = crossings(&session);
    let crossing = delivered
        .crossings()
        .first()
        .unwrap_or_else(|| panic!("the crossed trigger is delivered: {history:?}"));
    assert_eq!(
        delivered.delivered(),
        1,
        "exactly one crossing for the pair, whatever the frame length: {history:?}"
    );
    assert_eq!(crossing.actor, geometry.projectile);
    assert_eq!(crossing.volume, geometry.trigger);
    assert!(
        crossing.is_entry(),
        "a body that reaches a volume it was not in enters"
    );
    assert!(
        !crossing.is_exit(),
        "and the crossing is an entry, not a level: {crossing:?}"
    );
    assert_eq!(
        crossing.source,
        CrossingSource::SpawnTickPreflight,
        "the swept preflight's cast decided it: {crossing:?}"
    );
    assert_eq!(
        crossing.tick, 1,
        "on the tick it happened, not a tick later: {crossing:?}"
    );

    // The geometry the crossing claims, cross-checked against the record the
    // producer made: the trigger is met before the wall, both inside the
    // spawn tick's 0.5 m of travel, and both measured from the spawn position.
    let record = history
        .iter()
        .flat_map(|state| state.spawns.iter())
        .find(|event| event.passed == Some(Geometry::TRIGGER))
        .unwrap_or_else(|| panic!("the preflight recorded the crossing: {history:?}"));
    assert_eq!(record.hit, Some(Geometry::WALL), "{record:?}");
    assert_eq!(record.passed_distance_m, Some(crossing.distance_m));
    let wall_distance_m = record.distance_m.expect("a clamp names its distance");
    assert!(
        crossing.distance_m > 0.0 && crossing.distance_m < wall_distance_m,
        "the trigger is met before the wall, from the same spawn position: \
         trigger at {crossing:?}, wall at {wall_distance_m} m"
    );
    assert!(
        crossing.distance_m < 0.5,
        "and both are inside the spawn tick's 0.5 m of travel, or the scenario \
         is not the one this test names: {crossing:?}"
    );

    // The solid response is untouched: the wall still stopped the spawn, and
    // the body rests on its near face instead of rebounding.
    assert!(record.clamped && record.stopped, "{record:?}");
    let pose = session
        .pose(geometry.projectile)
        .expect("the body is still in the world");
    // The wall's near face is at x = 0.09 and the projectile's half extent is
    // 0.05, so resting on the face is x = 0.04; the declared overlap the solver
    // has not yet pushed out is absorbed by the same tolerance F23-D froze.
    let face_rest_x = 0.09 - 0.05 + CONTACT_FACE_TOLERANCE_M as f32;
    assert!(
        pose.position_m[0] < face_rest_x,
        "and it ends at rest against the wall's near face (x < {face_rest_x}): {pose:?}"
    );

    // The consumer reads the preflight log; it does not take it. The session's
    // own per-frame hand-off of the same records is a separate consumer, and a
    // delivery that stole them would take the record away from the diagnostics
    // F23-D relies on.
    let frame_records: usize = history.iter().flat_map(|state| state.spawns.iter()).count();
    assert_eq!(
        frame_records, 1,
        "the record still reaches the session's own frame hand-off: {history:?}"
    );
}

/// The delivery is a read: a body that crossed a trigger and was then stopped
/// by a wall in the same tick traces bit-identically with and without it.
#[test]
fn accept_t415_a_the_delivery_does_not_move_or_delay_the_body() {
    let mut delivered = consuming_session(BASELINE_FIXED_HZ);
    let with = f23_d_geometry(&mut delivered);
    let delivered_history = trace(&mut delivered, with.projectile, &with.roles(), 6);

    let mut plain = bare_session(BASELINE_FIXED_HZ);
    let without = f23_d_geometry(&mut plain);
    let plain_history = trace(&mut plain, without.projectile, &without.roles(), 6);

    assert_eq!(
        delivered_history, plain_history,
        "the consumer changed a classified report, a spawn record, a position or a \
         velocity"
    );
    // The two worlds allocate their own entities, so the traces are compared
    // through their observed values; the assertion above is on shape and
    // numbers, and this one is on the fact that the crossing exists only in the
    // first world.
    assert_eq!(crossings(&delivered).delivered(), 1);
    assert!(
        plain
            .world()
            .and_then(|world| world.get_resource::<TriggerCrossings>())
            .is_none(),
        "and the bare composition has no consumer at all: without the plugin the \\
         preflight still records the crossing and nothing delivers it, which is \\
         the gap this task closed"
    );

    // A trigger on its own stops nothing: a body that only crosses a volume
    // leaves at its fired speed, which is F23-C's criterion unchanged.
    let mut flying = consuming_session(BASELINE_FIXED_HZ);
    let trigger = flying
        .spawn(&trigger_spec(0.0))
        .expect("the trigger is valid")
        .entity;
    let body = flying
        .spawn(&projectile_spec(-0.4 * 0.5 - 0.05, 60.0))
        .expect("the projectile is valid")
        .entity;
    let history = trace(&mut flying, body, &[trigger, body], 4);
    let spawn = &history[0].spawns[0];
    assert!(!spawn.clamped, "a sensor never stops a spawn: {spawn:?}");
    let pose = flying.pose(body).expect("the body is still in the world");
    assert!(
        pose.position_m[0] > 0.06,
        "the body left the volume instead of resting in it: {pose:?}"
    );
    assert!(
        f64::from(pose.linear_velocity_m_s[0]) >= SENSOR_MIN_SPEED_FRACTION * 60.0,
        "and left at its fired speed ({:?}), a trigger having taken nothing from it",
        pose.linear_velocity_m_s
    );
}

/// Nothing is invented for a body that crossed no volume.
#[test]
fn accept_t415_a_body_that_crosses_no_trigger_delivers_nothing() {
    let mut session = consuming_session(BASELINE_FIXED_HZ);
    // A wall far beyond the body's whole flight, and no volume at all.
    let wall = session
        .spawn(&wall_spec(500.0))
        .expect("the wall is valid")
        .entity;
    let body = session
        .spawn(&projectile_spec(-10.0, 60.0))
        .expect("the projectile is valid")
        .entity;
    let history = trace(&mut session, body, &[wall, body], 4);

    let delivered = crossings(&session);
    assert_eq!(
        delivered.delivered(),
        0,
        "a body that crossed no volume produces no crossing: {history:?}"
    );
    assert!(delivered.crossings().is_empty());
    assert_eq!(delivered.duplicates(), 0);
    // And the preflight itself recorded the pass-through as a clear cast: the
    // record exists and names nothing, which is the negative case the consumer
    // reads.
    let record = &history[0].spawns[0];
    assert_eq!(record.passed, None, "{record:?}");
    assert!(!record.clamped, "{record:?}");
}

/// "Exactly once" is a property of the consumer, not of the frame length: a
/// multi-tick render frame re-reads the same preflight record on every tick it
/// runs, and a second body gets its own crossing.
#[test]
fn accept_t415_a_a_multi_tick_frame_delivers_the_crossing_exactly_once() {
    let mut session = consuming_session(BASELINE_FIXED_HZ);
    let trigger = session
        .spawn(&trigger_spec(0.0))
        .expect("the trigger is valid")
        .entity;
    let first = session
        .spawn(&projectile_spec(-0.4 * 0.5 - 0.05, 60.0))
        .expect("the projectile is valid")
        .entity;
    // One frame, three ticks: the record is in the log for all three, because
    // the session drains it at the end of the frame.
    session.step(3).expect("the session is active");
    let delivered = crossings(&session);
    assert_eq!(
        delivered.delivered(),
        1,
        "one crossing out of a three-tick frame: {:?}",
        delivered.crossings()
    );
    assert_eq!(
        delivered.duplicates(),
        2,
        "and the two re-reads are refused rather than delivered again: {:?}",
        delivered.crossings()
    );

    // A later frame adds nothing for the same pair.
    session.step(3).expect("the session is active");
    assert_eq!(crossings(&session).delivered(), 1);

    // A second body crossing the same volume is a second pair, not a duplicate.
    let second = session
        .spawn(&projectile_spec(-0.4 * 0.5 - 0.05, 60.0))
        .expect("the projectile is valid")
        .entity;
    // A third body, spawned nowhere near the volume.
    let elsewhere = session
        .spawn(&projectile_spec(-500.0, 60.0))
        .expect("the projectile is valid")
        .entity;
    session.step(2).expect("the session is active");
    {
        let delivered = crossings(&session);
        assert_eq!(
            delivered.delivered(),
            2,
            "a second actor crossing the same volume enters once each: {:?}",
            delivered.crossings()
        );
        let entries: Vec<&TriggerCrossing> = delivered
            .crossings()
            .iter()
            .filter(|crossing| crossing.is_of(trigger))
            .collect();
        assert_eq!(entries.len(), 2, "{entries:?}");
        assert!(
            entries.iter().all(|crossing| crossing.is_entry()),
            "and both are entries: {entries:?}"
        );
        assert_eq!(
            delivered.crossings_by(first).len(),
            1,
            "each actor's crossings can be read back on its own"
        );
        assert_eq!(delivered.crossings_by(second).len(), 1);
        assert!(
            delivered.crossings_by(elsewhere).is_empty(),
            "and an actor that crossed nothing has none"
        );
    }

    // Taking the batch empties the stream but not the ledger, so a consumer
    // that took its crossings does not see them again, while a *reset* — the
    // world going away — clears the ledger with them.
    let taken = crossings_mut(&mut session).take();
    assert_eq!(taken.len(), 2);
    assert!(crossings(&session).crossings().is_empty());
    session.step(2).expect("the session is active");
    assert!(
        crossings(&session).crossings().is_empty(),
        "a taken crossing does not come back"
    );
}

/// The decision is swept, so it does not depend on the tick rate: every probed
/// rate and speed delivers exactly one crossing, on the spawn tick, with the
/// volume met inside that tick's travel — including the one cell where the
/// engine's discrete narrow phase happens to report the same overlap a tick
/// later.
#[test]
fn accept_t415_a_every_probed_rate_and_speed_delivers_exactly_one_crossing() {
    let mut rows: Vec<String> = Vec::new();
    for fixed_hz in PROBE_RATES_HZ {
        for speed_m_s in PROBE_SPEEDS_M_S {
            let mut session = consuming_session(fixed_hz);
            let trigger = session
                .spawn(&trigger_spec(0.0))
                .expect("the trigger is valid")
                .entity;
            let travel_m = speed_m_s / fixed_hz as f32;
            // F23-B's first-tick hole: the spawn sits inside one tick of travel
            // of the volume, where the body is not yet in the broad phase.
            let start_x_m = -(SPAWN_IN_HOLE_TRAVEL_TICKS * travel_m + 0.05);
            let body = session
                .spawn(&projectile_spec(start_x_m, speed_m_s))
                .expect("the projectile is valid")
                .entity;
            let history = trace(&mut session, body, &[trigger, body], 4);

            // The trace names the volume as role 0 and the body as role 1. The
            // reporter's own pair order is Bevy's `Entity` ordering (by
            // generation, then index), so the pair is not `(volume, body)` by
            // construction and the filter asks only which roles are in it.
            let engine_report_ticks: Vec<u64> = history
                .iter()
                .flat_map(|state| state.reports.iter())
                .filter(|((first, second), ..)| {
                    [*first, *second].contains(&TRIGGER_ROLE)
                        && [*first, *second].contains(&BODY_ROLE)
                })
                .map(|(_, _, tick)| *tick)
                .collect();
            let delivered = crossings(&session);
            let crossing = delivered.crossings().first().copied().unwrap_or_else(|| {
                panic!("{fixed_hz} Hz at {speed_m_s} m/s delivered no crossing: {history:?}")
            });
            assert_eq!(
                delivered.delivered(),
                1,
                "{fixed_hz} Hz at {speed_m_s} m/s: {history:?}"
            );
            assert_eq!(
                delivered.duplicates(),
                0,
                "{fixed_hz} Hz at {speed_m_s} m/s"
            );
            assert_eq!(crossing.tick, 1, "{fixed_hz} Hz at {speed_m_s} m/s");
            assert_eq!(crossing.actor, body);
            assert_eq!(crossing.volume, trigger);
            assert!(
                crossing.distance_m > 0.0 && crossing.distance_m < travel_m,
                "{fixed_hz} Hz at {speed_m_s} m/s: the volume is met inside the \
                 spawn tick's {travel_m} m of travel, at {}",
                crossing.distance_m
            );
            // The crossing is never *later* than the engine's own stream, which
            // is the ordering the decision rests on: where the discrete narrow
            // phase happens to find a sample at all it reports the pair on a
            // later tick, and where it finds none the crossing is the only
            // report there will ever be. The count itself is deliberately not
            // pinned — a future engine that closed the hole would add reports,
            // and the invariant worth keeping is that this crossing is not one
            // of them.
            assert!(
                engine_report_ticks.iter().all(|tick| *tick > crossing.tick),
                "{fixed_hz} Hz at {speed_m_s} m/s: the engine's own stream named \
                 the pair on ticks {engine_report_ticks:?} against a crossing on \
                 tick {}: {history:?}",
                crossing.tick
            );
            let pose = session.pose(body).expect("the body is still in the world");
            assert!(
                f64::from(pose.linear_velocity_m_s[0])
                    >= SENSOR_MIN_SPEED_FRACTION * f64::from(speed_m_s),
                "{fixed_hz} Hz at {speed_m_s} m/s: a volume took speed from the \
                 body that crossed it: {pose:?}"
            );
            rows.push(format!(
                "{fixed_hz} Hz / {speed_m_s} m/s: travel {:.2} m, crossing at \
                 {:.3} m, engine reports {engine_report_ticks:?}",
                travel_m, crossing.distance_m
            ));
        }
    }
    // The matrix is the measurement the decision record quotes; a case that
    // silently stopped being probed would otherwise shorten this silently.
    assert_eq!(
        rows.len(),
        PROBE_RATES_HZ.len() * PROBE_SPEEDS_M_S.len(),
        "every probed rate and speed was measured: {rows:?}"
    );
    println!("{}", rows.join("\n"));
}

/// A body that spawns already inside a volume enters once, at the spawn
/// position, and does not enter again while it dwells: the volume's own
/// discrete start and end are the engine's business, and the crossing stream
/// holds one entry for the pair.
#[test]
fn accept_t415_a_a_body_that_spawns_inside_a_volume_enters_once_and_dwells() {
    // 1 m/s at 60 Hz is 1.7 cm of travel per tick, so the body stays inside the
    // 2 cm volume for several ticks after the first one.
    let mut session = consuming_session(60);
    let trigger = session
        .spawn(&trigger_spec(0.0))
        .expect("the trigger is valid")
        .entity;
    let body = session
        .spawn(&projectile_spec(0.0, 1.0))
        .expect("the projectile is valid")
        .entity;
    let history = trace(&mut session, body, &[trigger, body], 6);

    let delivered = crossings(&session);
    assert_eq!(
        delivered.delivered(),
        1,
        "a body that appears inside a volume enters once, and dwelling is not \
         a second entry: {history:?}"
    );
    let crossing = delivered.crossings()[0];
    assert!(crossing.is_entry());
    assert_eq!(crossing.actor, body);
    assert_eq!(crossing.volume, trigger);
    assert_eq!(
        crossing.distance_m, 0.0,
        "at the spawn position: the body met the volume before it moved: {crossing:?}"
    );
    assert_eq!(delivered.duplicates(), 0);

    // The body kept its velocity throughout: a trigger is not an obstacle, and
    // a body that is inside one when it spawns is not stopped by it either.
    for (tick, state) in history.iter().enumerate() {
        assert!(
            (f64::from(state.velocity_m_s[0]) - 1.0).abs() < 1e-3,
            "tick {tick} changed the body's speed: {:?}",
            state.velocity_m_s
        );
    }
    let record = &history[0].spawns[0];
    assert!(!record.clamped, "a volume never clamps a spawn: {record:?}");
    assert!(!record.stopped, "{record:?}");
}

/// The other half of the finding's spawn-tick claim, pinned: a body that
/// *spawns already inside* a volume is the case the engine is **not** blind to,
/// because the body is a sample inside the volume from the start. All four
/// probed combinations are measured, and each one requires the engine's own
/// classified stream to report the pair on the spawn tick.
///
/// That is what separates the two spawn-tick cases from each other. A body that
/// flies through a volume leaves it inside the tick it is invisible to the
/// broad phase, so no sample ever lands inside and the engine reports nothing
/// (the 12-cell matrix above); a body that appears inside one has a sample
/// there, so the engine's discrete narrow phase finds it on the first tick it
/// runs. Without this assertion the record could be misread as a general
/// spawn-tick overlap detector, and the reason F23-D needed a second cast at
/// all would be lost.
#[test]
fn accept_t415_a_the_spawn_inside_case_is_where_the_engine_is_not_blind() {
    let mut rows: Vec<String> = Vec::new();
    for (fixed_hz, speed_m_s) in [(60u32, 60.0f32), (60, 1.0), (120, 0.5), (240, 60.0)] {
        let mut session = consuming_session(fixed_hz);
        let trigger = session
            .spawn(&trigger_spec(0.0))
            .expect("the trigger is valid")
            .entity;
        // The body's centre is the volume's centre, so its 10 cm extent already
        // encloses the 2 cm volume: it is inside before the first tick runs.
        let body = session
            .spawn(&projectile_spec(0.0, speed_m_s))
            .expect("the projectile is valid")
            .entity;
        let history = trace(&mut session, body, &[trigger, body], 3);

        let record = history
            .iter()
            .flat_map(|state| state.spawns.iter())
            .find(|event| event.passed == Some(TRIGGER_ROLE))
            .unwrap_or_else(|| {
                panic!("{fixed_hz} Hz at {speed_m_s} m/s recorded no crossing: {history:?}")
            });
        assert_eq!(
            record.passed_distance_m,
            Some(0.0),
            "{fixed_hz} Hz at {speed_m_s} m/s: the body met the volume before it \
             moved, so the distance is the spawn position itself: {record:?}"
        );
        assert!(
            !record.clamped && !record.stopped,
            "{fixed_hz} Hz at {speed_m_s} m/s: a sensor is not an obstacle, even \
             for a body inside one: {record:?}"
        );

        // The engine's own classified stream, the distinction this test exists
        // for: the body is a sample inside the volume on the spawn tick, so the
        // discrete narrow phase finds the overlap *on that tick* — no hole here.
        // The exact single report is asserted rather than "at least one",
        // because this is the measurement the finding's pass-through / overlap
        // distinction rests on: if the engine ever stopped reporting it, the
        // second cast would be carrying this case too and the record would have
        // to be re-read.
        let engine_kinds: Vec<(ContactKind, u64)> = history
            .iter()
            .flat_map(|state| state.reports.iter())
            .filter(|((first, second), ..)| {
                [*first, *second].contains(&TRIGGER_ROLE) && [*first, *second].contains(&BODY_ROLE)
            })
            .map(|(_, kind, tick)| (*kind, *tick))
            .collect();
        assert_eq!(
            engine_kinds,
            vec![(ContactKind::SensorOverlap, 1)],
            "{fixed_hz} Hz at {speed_m_s} m/s: a body that spawns inside a volume \
             is reported by the engine itself — once, as a sensor overlap, on the \
             spawn tick. This is the case F23-D's second cast is *not* needed \
             for: {history:?}"
        );

        let delivered = crossings(&session);
        assert_eq!(
            delivered.delivered(),
            1,
            "{fixed_hz} Hz at {speed_m_s} m/s: one entry for the pair, whatever \
             the engine also reported: {history:?}"
        );
        let crossing = delivered.crossings()[0];
        assert!(crossing.is_entry(), "{crossing:?}");
        assert_eq!(
            crossing.tick, 1,
            "{fixed_hz} Hz at {speed_m_s} m/s: {crossing:?}"
        );
        assert_eq!(
            crossing.distance_m,
            record.passed_distance_m.unwrap(),
            "{fixed_hz} Hz at {speed_m_s} m/s: the crossing carries the producer's \
             geometry unchanged"
        );
        rows.push(format!(
            "{fixed_hz} Hz at {speed_m_s} m/s: crossing at {} m on tick {}, engine \
             reported {engine_kinds:?}",
            crossing.distance_m, crossing.tick
        ));
    }
    assert_eq!(
        rows.len(),
        4,
        "every probed combination was measured: {rows:?}"
    );
    println!("{}", rows.join("\n"));
}

/// The crossing a consumer reads names the actor, the volume, the tick and the
/// geometry, and answers "was this my body / my volume / an entry" without the
/// caller matching on fields.
#[test]
fn accept_t415_a_the_delivered_crossing_names_its_pair_tick_and_geometry() {
    let mut session = consuming_session(BASELINE_FIXED_HZ);
    let trigger = session
        .spawn(&trigger_spec(0.0))
        .expect("the trigger is valid")
        .entity;
    let body = session
        .spawn(&projectile_spec(-0.25, 60.0))
        .expect("the projectile is valid")
        .entity;
    let other = session
        .spawn(&projectile_spec(-5.0, 60.0))
        .expect("the projectile is valid")
        .entity;
    session.step(2).expect("the session is active");

    let crossing: TriggerCrossing = crossings(&session).crossings()[0];
    assert!(crossing.is_by(body) && !crossing.is_by(other));
    assert!(crossing.is_of(trigger) && !crossing.is_of(other));
    assert!(crossing.is_entry() && !crossing.is_exit());
    assert_eq!(
        crossing.source.to_string(),
        "spawn_tick_preflight",
        "a trace that cannot name its producer is not one"
    );
    // The projectile met the volume's near face 0.19 m into the tick's 0.5 m
    // of travel: the trigger's half thickness is 0.01 and the projectile's half
    // extent is 0.05, so the sweep touches at 0.25 - 0.01 - 0.05 = 0.19 m.
    assert!(
        (crossing.distance_m - 0.19).abs() < 1e-3,
        "the crossing carries the geometry a consumer orders it by: {crossing:?}"
    );
    assert_eq!(
        crossing.tick,
        1,
        "and the tick it happened on, not the tick the consumer read it on (the \
         session is two ticks in: {:?})",
        session.tick()
    );

    // A cleared stream keeps the ledger, so a consumer that took its batch and
    // then cleared the counters does not see the same crossing twice; a reset
    // takes the ledger with them.
    session
        .world_mut()
        .expect("active")
        .resource_mut::<TriggerCrossings>()
        .clear();
    let mut cleared = crossings_mut(&mut session);
    assert!(cleared.crossings().is_empty());
    assert_eq!(cleared.delivered(), 0);
    assert!(
        !cleared.record(crossing),
        "and the pair is still known, so a re-read is refused"
    );
    assert_eq!(cleared.duplicates(), 1);
    session
        .world_mut()
        .expect("active")
        .resource_mut::<TriggerCrossings>()
        .reset();
    let mut reset = crossings_mut(&mut session);
    assert!(
        reset.record(crossing),
        "a reset world starts with an empty ledger"
    );
    assert_eq!(reset.delivered(), 1);
}
