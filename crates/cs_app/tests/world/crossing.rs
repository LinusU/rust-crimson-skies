//! #498: a world trigger volume's **swept** crossing report — the
//! ordinary-flight half of the rule task #415 decided once.
//!
//! Spec: `specs/F18-world-geometry-terrain-water-and-traversable-interiors.md`,
//! stage `### F18-C` (the overlay a crossing feeds), coordinated with F39
//! (`cs_app::objectives` owns the `TriggerCrossing` record, the
//! `TriggerCrossings` stream and its once-per-pair ledger). Task test prefix:
//! `accept_f18_c_`.
//!
//! These tests drive production code only: the worlds are `depot_world` and
//! `harbor_world`, the bodies are the production `spawn_swept_probe`, the
//! producer is `cs_app::world::sweep_volume_crossings` running inside
//! `world_app()`'s composition, and the consumer halves are
//! `cs_app::objectives::TriggerCrossings` and
//! `cs_app::world::apply_overlay_requests`. No test carries its own world
//! builder, trigger or sweep.
//!
//! What is pinned here, on the pinned pair (`bevy 0.19.1` /
//! `avian3d 0.7.0`, `SubstepCount(1)`, 120 Hz fixed, gravity zero):
//!
//! * the crossing is decided from the **body's own motion over the tick** — a
//!   3.33 m tick at 400 m/s completely outruns the depot's 1 m cuboid trigger
//!   volume *and* lands a mesh-derived volume's deep-inside gap, so the sampled
//!   `CollisionStart` stream stays silent on both — and the crossing is still
//!   reported **exactly once**, with the overlay behind it applied once;
//! * the report is **read-only**: every tick that spans the volume is a full
//!   one and the body's velocity is untouched;
//! * a body that turns away **inside** a volume produces nothing beyond the
//!   one entry the pair was already owed — the inside bit is what suppresses a
//!   second crossing, and a body first observed inside a volume reports none;
//! * a body that crosses and **returns** is the same `(actor, volume)` pair:
//!   the second crossing is refused by the one ledger every crossing enters
//!   through, and the overlay still applied once.
//!
//! Observable failure if the pass is removed or weakened: the crossings list
//! stays empty while the body visibly covers the volume's whole thickness in
//! one tick, the door stays shut, a turn-away inside produces a crossing, or a
//! return trip produces a second.

use avian3d::prelude::{LinearVelocity, Position};
use bevy::prelude::{App, Entity, Vec3};
use cs_app::objectives::{CrossingSource, TriggerCrossings};
use cs_app::world::{
    AppliedOverlay, DEPOT_DOOR_OPEN_OFFSET_M, DEPOT_OBJECT_DOOR, DEPOT_OBJECT_TRIGGER,
    DEPOT_TRIGGER_HALF_M, DEPOT_TRIGGER_POS_M, HARBOR_OBJECT_ABSENT, HARBOR_OBJECT_BANNER,
    HARBOR_OBJECT_GROUND, HARBOR_OBJECT_HANGAR, HARBOR_OBJECT_SENSOR, HARBOR_OBJECT_WATER,
    HARBOR_SENSOR_HALF_M, HARBOR_SENSOR_POS_M, MESH_SETTLE_UPDATES, OverlayOutcome, ProbeSpec,
    SpawnedWorld, SweptBodyTracks, WorldContacts, depot_meshes, depot_mission, depot_world,
    fixture_provenance, harbor_meshes, harbor_world, load_world, object_set, overlay_log,
    spawn_swept_probe, world_app, world_instance,
};
use cs_content::world::{MissionOverlay, OverlayEffect, WorldInstance, WorldObjectId};

/// The speed the tunnelling measurements are taken at, in m/s: 3.33 m per tick
/// at the workspace's 120 Hz fixed rate — more than the depot trigger volume's
/// whole 1 m thickness plus the probe's own box, so **no discrete sample lands
/// inside it**. This is the hole the pass exists to close.
const SWEPT_SPEED_M_S: f64 = 400.0;

/// The slower speed the state-machine arms are flown at, in m/s: 0.5 m per
/// tick, so the body spends ticks inside the volume and the inside bit — not
/// the segment alone — is what is exercised.
const DWELL_SPEED_M_S: f64 = 60.0;

/// The probe's half extent along every axis, in meters.
const PROBE_HALF_M: f64 = 0.25;

/// Where a depot flight starts on the tunnel's centreline, in meters.
const DEPOT_START_X_M: f64 = -12.0;

/// The tunnel's centreline height: inside the opening, inside the trigger.
const PROBE_Y_M: f64 = 1.5;

/// The depot trigger's near face, as seen by the probe's *centre*: the
/// volume's face minus the probe's half extent.
fn depot_trigger_near() -> f32 {
    (DEPOT_TRIGGER_POS_M[0] - DEPOT_TRIGGER_HALF_M[0] - PROBE_HALF_M) as f32
}

/// The depot trigger's far face, as seen by the probe's centre.
fn depot_trigger_far() -> f32 {
    (DEPOT_TRIGGER_POS_M[0] + DEPOT_TRIGGER_HALF_M[0] + PROBE_HALF_M) as f32
}

fn object(key: &str) -> WorldObjectId {
    WorldObjectId::new(key).expect("the fixture object key is valid")
}

fn step(app: &mut App, ticks: u64) {
    for _ in 0..ticks {
        app.update();
    }
}

/// The recorded crossings, in decision order.
fn crossings(app: &App) -> Vec<cs_app::objectives::TriggerCrossing> {
    app.world()
        .resource::<TriggerCrossings>()
        .crossings()
        .to_vec()
}

fn applied(app: &App) -> Vec<AppliedOverlay> {
    overlay_log(app.world())
        .expect("the overlay trace is installed by the composition")
        .outcomes()
        .iter()
        .filter_map(OverlayOutcome::applied)
        .cloned()
        .collect()
}

/// The authored objects the world's contact stream named, in first-contact
/// order.
fn contacted_objects(app: &App) -> Vec<String> {
    app.world()
        .resource::<WorldContacts>()
        .contacts()
        .iter()
        .map(|contact| contact.object.as_str().to_owned())
        .collect()
}

/// The depot world loaded with its door overlay, mesh collider settled.
fn loaded_depot(open_door: bool) -> (App, SpawnedWorld) {
    let definition = depot_world().expect("the synthetic depot world is well formed");
    let instance = depot_mission(&definition, open_door, &[]).expect("a valid depot mission");
    let mut app = world_app();
    let report = load_world(&mut app, &definition, &instance, &depot_meshes())
        .expect("the depot world loads");
    step(&mut app, MESH_SETTLE_UPDATES);
    (app, report)
}

/// The harbor world loaded with a mission overlay on its **mesh-derived**
/// trigger volume: crossing `trigger.sensor` displaces the hangar shell.
///
/// The overlay is authored here the way `depot_mission` authors the depot's —
/// an `OverlayEffect::Displace` on a `Sensor` object — so the "a swept
/// crossing reaches the same overlay consumer" claim is exercised on the mesh
/// collider layout too, not only on the cuboid one.
fn loaded_harbor_with_trigger_overlay() -> (App, SpawnedWorld) {
    let definition = harbor_world().expect("the synthetic harbor world is well formed");
    let population = [
        HARBOR_OBJECT_HANGAR,
        HARBOR_OBJECT_SENSOR,
        HARBOR_OBJECT_BANNER,
        HARBOR_OBJECT_WATER,
        HARBOR_OBJECT_GROUND,
        HARBOR_OBJECT_ABSENT,
    ];
    let overlay = MissionOverlay::try_new(
        object(HARBOR_OBJECT_SENSOR),
        OverlayEffect::Displace {
            target: object(HARBOR_OBJECT_HANGAR),
            offset_m: HARBOR_OVERLAY_OFFSET_M,
        },
        fixture_provenance("test.harbor_trigger_overlay"),
    )
    .expect("the harbor trigger overlay's offset is finite");
    let instance: WorldInstance = world_instance(
        &definition,
        Some("synthetic.harbor_world.mission_swept"),
        &population,
        &[],
    )
    .expect("a valid harbor load record")
    .with_mission_layer(vec![overlay], object_set(&[]))
    .expect("one trigger, a finite offset");
    let mut app = world_app();
    let report = load_world(&mut app, &definition, &instance, &harbor_meshes())
        .expect("the harbor world loads");
    step(&mut app, MESH_SETTLE_UPDATES);
    (app, report)
}

/// The displacement the harbor test's authored overlay applies to the hangar
/// shell: far enough in `+z` that a collider that stayed behind would be
/// visible in the shell's own `Position`.
const HARBOR_OVERLAY_OFFSET_M: [f64; 3] = [0.0, 0.0, 5.0];

/// The entity the named object's collider hangs on — `None` for a sensor's
/// body, by design, so the *collider* entity is what a crossing names.
fn collider_of(report: &SpawnedWorld, key: &str) -> Entity {
    report
        .collider_for(&object(key))
        .unwrap_or_else(|| panic!("the {key} object has a collider"))
}

/// A per-update position trace of `body` over `ticks` fixed steps.
fn trace_x(app: &mut App, body: Entity, ticks: u64) -> Vec<f32> {
    let mut trace = Vec::new();
    for _ in 0..ticks {
        app.update();
        trace.push(
            app.world()
                .get::<Position>(body)
                .expect("the body still exists")
                .0
                .x,
        );
    }
    trace
}

/// Asserts every step that carries the body's box across the trigger window
/// was a full free one — the report never stopped, delayed or nudged the body
/// it measured.
///
/// `near`/`far` are in **centre space**: the window a body's centre may cross
/// while its box overlaps the volume, which is what both producers actually
/// measure.
fn assert_crossing_steps_are_free(trace: &[f32], start_x: f32, free: f32, near: f32, far: f32) {
    let mut positions = vec![start_x];
    positions.extend_from_slice(trace);
    for index in 0..positions.len() - 1 {
        // Steps entirely clear of the window are uninteresting; the shell's
        // tunnel mouth has its own measured 0.15 m artifact past the door, so
        // the window is the volume's own span and nothing more.
        if positions[index + 1] <= near || positions[index] >= far {
            continue;
        }
        let step = positions[index + 1] - positions[index];
        assert!(
            (step - free).abs() < 0.01,
            "the tick that carried the body across the trigger window \
             (x = {} to {}) covered {step} m of a free {free} m: the report \
             must be a read, never a hold — a short tick here means the sweep \
             is slowing the body it measures",
            positions[index],
            positions[index + 1],
        );
    }
}

/// **The cuboid arm**: a swept body whose tick outruns the whole trigger
/// volume is still reported — once — and the door still opens.
///
/// At 400 m/s the 3.33 m tick carries the probe's half-metre box from one side
/// of the 1 m `trigger.depot` volume to the other without a single sample
/// landing inside it, so the `CollisionStart` stream the overlay producer
/// reads stays silent (task #401's measurement; the contact log must name
/// everything *else* and not the trigger). The swept pass decides the crossing
/// from the body's own segment instead: the `(probe, volume)` pair enters
/// `TriggerCrossings` exactly once as an `OrdinaryFlightSweep` entry, the same
/// tick's overlay hand-off requests the door, and the door's *collided* half
/// is displaced by the authored offset before the body arrives.
///
/// The second half pins rule 4: every tick that spans the volume is a full
/// free tick, and the body's velocity is exactly what the spawn gave it — the
/// report is a read.
///
/// Observable failure: no crossing in the stream while the body visibly
/// outruns the volume, a short tick inside the crossing window, a velocity
/// that moved, or a door still shut.
#[test]
fn accept_f18_c_a_swept_body_through_a_thin_cuboid_trigger_reports_once_and_opens_the_door() {
    let (mut app, report) = loaded_depot(true);
    let volume = collider_of(&report, DEPOT_OBJECT_TRIGGER);
    let body = spawn_swept_probe(
        &mut app,
        &ProbeSpec {
            position_m: [DEPOT_START_X_M, PROBE_Y_M, 0.0],
            velocity_m_s: [SWEPT_SPEED_M_S, 0.0, 0.0],
            half_extents_m: [PROBE_HALF_M, PROBE_HALF_M, PROBE_HALF_M],
            mass_kg: 250.0,
        },
    )
    .expect("a valid probe");

    let dt = app
        .world()
        .resource::<bevy::time::Time<bevy::time::Fixed>>()
        .timestep()
        .as_secs_f32();
    let free = SWEPT_SPEED_M_S as f32 * dt;
    // Thirty ticks is a hundred metres: the crossing, the opened tunnel, and
    // room after it.
    let trace = trace_x(&mut app, body, 30);

    // The discriminating half first: the discrete stream saw nothing of the
    // trigger. If it had, "the sweep reported the crossing" would be
    // indistinguishable from "the narrow phase happened to".
    let log = contacted_objects(&app);
    assert!(
        !log.iter().any(|name| name == DEPOT_OBJECT_TRIGGER),
        "at 3.33 m per tick no discrete sample lands inside a 1 m volume — \
         the contact stream must *not* name it, which is what makes the \
         crossing below a swept report and not a sampled overlap: {log:?}"
    );

    // Exactly once, with the record's own facts.
    let all = crossings(&app);
    let mine: Vec<_> = all.iter().filter(|c| c.is_by(body)).collect();
    assert_eq!(
        mine.len(),
        1,
        "the swept pass must deliver this pair's entry exactly once: {all:?}"
    );
    let crossing = mine[0];
    assert!(
        crossing.is_of(volume),
        "the crossing names the sensor volume's own collider entity, not \
         another object: {crossing:?}"
    );
    assert!(
        crossing.is_entry(),
        "a pass-through delivers its entry: {crossing:?}"
    );
    assert_eq!(
        crossing.source,
        CrossingSource::OrdinaryFlightSweep,
        "the record says which producer decided it"
    );
    assert!(
        crossing.distance_m > 0.0 && crossing.distance_m <= free + 0.01,
        "distance_m is where along the tick's own {free} m segment the body's \
         box first met the volume: got {}",
        crossing.distance_m
    );
    assert!(
        crossing.tick > 0,
        "the crossing is stamped with a real tick"
    );

    // The overlay consumer got the same report's hand-off, once, before the
    // body reached the panel.
    let outcomes = applied(&app);
    assert_eq!(
        outcomes.len(),
        1,
        "the door overlay applied exactly once: {outcomes:?}"
    );
    let door = report
        .object(&object(DEPOT_OBJECT_DOOR))
        .expect("the door panel is in the report");
    let collider = door
        .collider
        .as_ref()
        .expect("a solid cuboid object has a collider");
    let collided = app
        .world()
        .get::<Position>(collider.entity)
        .expect("the door's collider entity exists")
        .0;
    assert!(
        (collided.z - DEPOT_DOOR_OPEN_OFFSET_M[2] as f32).abs() < 1e-5,
        "and the door's collided half moved by the authored offset: {collided:?}"
    );

    // Read-only: the flight the report measured is the flight the body kept.
    assert_crossing_steps_are_free(
        &trace,
        DEPOT_START_X_M as f32,
        free,
        depot_trigger_near(),
        depot_trigger_far(),
    );
    let velocity = app
        .world()
        .get::<LinearVelocity>(body)
        .expect("the body still exists")
        .0;
    assert!(
        (velocity.x - SWEPT_SPEED_M_S as f32).abs() < 0.01 && velocity.y.abs() < 0.01,
        "a crossing report is a read: the body's velocity is what the spawn \
         gave it, {velocity:?}"
    );
    let end = *trace.last().expect("the body was flown");
    assert!(
        end > 1.0,
        "and the body flew on through the tunnel the report opened, ending at \
         x = {end}"
    );

    // The pass's own counters agree with the stream: one touch, one entry.
    let tracks = app.world().resource::<SweptBodyTracks>();
    assert_eq!(
        tracks.entries(),
        1,
        "the pass delivered the one entry the stream holds: {tracks:?}"
    );
}

/// **The mesh arm**: a swept body whose tick lands inside a mesh-derived
/// trigger volume's deep-inside gap is still reported — once — and the overlay
/// behind it still fires.
///
/// The harbor world's `trigger.sensor` is a `FromMesh` sensor volume, and task
/// #401 measured its second failure on top of tunneling: at 400 m/s the body
/// lands deep inside the box mesh where no triangle is within
/// `max_contact_distance`, so parry produces no manifold and the discrete
/// stream reports nothing (30 m/s reports it; 400 m/s does not). The swept
/// pass asks the volume's own geometry — the same `cast_shapes` the body's
/// segment makes — so the crossing is decided where the sampled stream is
/// blind.
///
/// The authored mission overlay displaces the hangar shell when the trigger
/// reports, so "the overlay fires exactly once" is pinned on the mesh collider
/// layout as well as the cuboid one — including the return ticks, where the
/// body is still passing through and each re-decision is refused by the one
/// ledger.
///
/// Observable failure: no crossing while the body covers the volume, a second
/// crossing on the later pass-through ticks, or a hangar that never moved.
#[test]
fn accept_f18_c_a_swept_body_through_a_mesh_trigger_reports_once_and_opens_its_overlay() {
    let (mut app, report) = loaded_harbor_with_trigger_overlay();
    let volume = collider_of(&report, HARBOR_OBJECT_SENSOR);
    let hangar = report
        .object(&object(HARBOR_OBJECT_HANGAR))
        .expect("the hangar is in the report");
    // The probe flies the trigger's own centreline at z = -6, clear of the
    // arch's legs (which span z ∈ [-2, -1] ∪ [1, 2]).
    let body = spawn_swept_probe(
        &mut app,
        &ProbeSpec {
            position_m: [-10.0, HARBOR_SENSOR_POS_M[1], HARBOR_SENSOR_POS_M[2]],
            velocity_m_s: [SWEPT_SPEED_M_S, 0.0, 0.0],
            half_extents_m: [PROBE_HALF_M, PROBE_HALF_M, PROBE_HALF_M],
            mass_kg: 250.0,
        },
    )
    .expect("a valid probe");

    let dt = app
        .world()
        .resource::<bevy::time::Time<bevy::time::Fixed>>()
        .timestep()
        .as_secs_f32();
    let free = SWEPT_SPEED_M_S as f32 * dt;
    // Fifteen ticks is fifty metres: through the whole 8.5 m window and out.
    let trace = trace_x(&mut app, body, 15);

    let log = contacted_objects(&app);
    assert!(
        !log.iter().any(|name| name == HARBOR_OBJECT_SENSOR),
        "the mesh volume's deep-inside gap is measured: the discrete stream \
         must *not* name it at this speed, which is what makes the crossing \
         below the sweep's own report: {log:?}"
    );

    let all = crossings(&app);
    let mine: Vec<_> = all.iter().filter(|c| c.is_by(body)).collect();
    assert_eq!(
        mine.len(),
        1,
        "the mesh volume's crossing is delivered exactly once even though the \
         body spends three ticks inside it and each re-decision is refused: \
         {all:?}"
    );
    let crossing = mine[0];
    assert!(
        crossing.is_of(volume) && crossing.is_entry(),
        "the record names the mesh volume and its entry: {crossing:?}"
    );
    assert_eq!(
        crossing.source,
        CrossingSource::OrdinaryFlightSweep,
        "and says the swept pass decided it"
    );
    assert!(
        crossing.distance_m > 0.0 && crossing.distance_m <= free + 0.01,
        "the entry's distance is inside the tick's own {free} m of travel: \
         got {}",
        crossing.distance_m
    );

    // The same hand-off works on the mesh layout: the authored overlay moved
    // the hangar shell — entity, `Position` and `Transform` together — once.
    let outcomes = applied(&app);
    assert_eq!(
        outcomes.len(),
        1,
        "the trigger's authored overlay applied exactly once: {outcomes:?}"
    );
    let hangar_entity = hangar.visual;
    let moved = app
        .world()
        .get::<Position>(hangar_entity)
        .expect("the hangar's entity exists")
        .0;
    assert!(
        (moved.z - HARBOR_OVERLAY_OFFSET_M[2] as f32).abs() < 1e-5,
        "the hangar's collided pose moved by the authored offset: {moved:?}"
    );

    // And the report was still a read: every step while the box spanned the
    // volume window was the free one, and the body came out the far side.
    let near = (HARBOR_SENSOR_POS_M[0] - HARBOR_SENSOR_HALF_M[0] - PROBE_HALF_M) as f32;
    let far = (HARBOR_SENSOR_POS_M[0] + HARBOR_SENSOR_HALF_M[0] + PROBE_HALF_M) as f32;
    assert_crossing_steps_are_free(&trace, -10.0, free, near, far);
    let end = *trace.last().expect("the body was flown");
    assert!(
        end > far,
        "the body kept going past the volume it was reported in, ending at \
         x = {end} past the window's far edge {far}"
    );
}

/// **The turn-away arm**: the inside bit is what a crossing is decided from,
/// and a body that turns around inside a volume produces nothing beyond the
/// one entry the pair was already owed.
///
/// Two arms, because "no fire on a turn-away" has two ways to go wrong:
///
/// * a body the pass first observes **already inside** the volume was inside
///   by construction — it reports nothing, ever, even as it leaves;
/// * a body that entered at a speed that lands it inside, dwelled a tick, and
///   reversed out the way it came is the same `(actor, volume)` pair on every
///   one of those ticks: the stream still holds exactly its entry, and no
///   second record — no `Exit`, no re-entry — appears.
///
/// Observable failure: a crossing recorded for the spawned-inside body, or a
/// second record (an exit, or a re-entry) for the one that turned around.
#[test]
fn accept_f18_c_a_body_that_turns_away_inside_a_trigger_reports_only_the_entry_it_earned() {
    // Arm one: first observed inside. A body that materializes in a volume is
    // the spawn tick's case (task #415), never an ordinary-flight entry.
    let (mut app, _report) = loaded_depot(true);
    let body = spawn_swept_probe(
        &mut app,
        &ProbeSpec {
            position_m: [DEPOT_TRIGGER_POS_M[0], PROBE_Y_M, 0.0],
            velocity_m_s: [-DWELL_SPEED_M_S, 0.0, 0.0],
            half_extents_m: [PROBE_HALF_M, PROBE_HALF_M, PROBE_HALF_M],
            mass_kg: 250.0,
        },
    )
    .expect("a valid probe");
    step(&mut app, 12);
    assert!(
        crossings(&app).iter().all(|c| !c.is_by(body)),
        "a body first observed inside the volume reports nothing, not even as \
         it leaves: {:?}",
        crossings(&app)
    );

    // Arm two: enter, dwell, reverse. The volume window for the probe's
    // centre is x ∈ (-4.75, -3.25); at 0.5 m per tick the body crosses the
    // threshold on the tick that lands it inside, and turning around inside
    // walks it back out the near face.
    let (mut app, _report) = loaded_depot(true);
    let body = spawn_swept_probe(
        &mut app,
        &ProbeSpec {
            position_m: [-8.0, PROBE_Y_M, 0.0],
            velocity_m_s: [DWELL_SPEED_M_S, 0.0, 0.0],
            half_extents_m: [PROBE_HALF_M, PROBE_HALF_M, PROBE_HALF_M],
            mass_kg: 250.0,
        },
    )
    .expect("a valid probe");
    step(&mut app, 9);
    assert!(
        app.world()
            .get::<Position>(body)
            .expect("the body still exists")
            .0
            .x
            > depot_trigger_near(),
        "the fixture must fly the body inside the window before the turn"
    );
    app.world_mut()
        .get_mut::<LinearVelocity>(body)
        .expect("the body still exists")
        .0 = Vec3::new(-DWELL_SPEED_M_S as f32, 0.0, 0.0);
    step(&mut app, 12);
    assert!(
        app.world()
            .get::<Position>(body)
            .expect("the body still exists")
            .0
            .x
            < depot_trigger_near(),
        "and the turn must walk it back out the near face"
    );

    let all = crossings(&app);
    let mine: Vec<_> = all.iter().filter(|c| c.is_by(body)).collect();
    assert_eq!(
        mine.len(),
        1,
        "the pair holds exactly its entry — a body that turned away inside \
         earns no second record: {all:?}"
    );
    assert!(
        mine[0].is_entry(),
        "and it is the entry, never an exit or a re-entry: {:?}",
        mine[0]
    );
    let tracks = app.world().resource::<SweptBodyTracks>();
    assert!(
        tracks.exits() >= 1,
        "the inside bit watched the body leave — the absence of a second \
         crossing is a decided exit, not a pass that never ran: {tracks:?}"
    );
}

/// **The cross-and-return arm**: once per `(actor, volume)` pair, enforced by
/// the one ledger every crossing enters through — and the overlay the entry
/// fired still applied once.
///
/// The probe crosses the depot trigger eastbound, leaves the far side, is
/// turned around, and crosses it westbound. The second crossing is decided by
/// the same sweep — the segment really does cover the volume again — and is
/// **refused by `TriggerCrossings::record`**, which is where "exactly once"
/// lives. What the test pins is the observable end state: one record, one
/// application, and the refusal counted so a reviewer can see the return
/// crossing was decided and rejected rather than never attempted.
///
/// Observable failure: two records for the pair, an overlay applied twice, or
/// a refusal count of zero (the return crossing never reached the ledger).
#[test]
fn accept_f18_c_a_body_that_crosses_and_returns_is_still_the_same_pair() {
    let (mut app, _report) = loaded_depot(true);
    let body = spawn_swept_probe(
        &mut app,
        &ProbeSpec {
            position_m: [-8.0, PROBE_Y_M, 0.0],
            velocity_m_s: [DWELL_SPEED_M_S, 0.0, 0.0],
            half_extents_m: [PROBE_HALF_M, PROBE_HALF_M, PROBE_HALF_M],
            mass_kg: 250.0,
        },
    )
    .expect("a valid probe");

    // Eastbound, well past the far face; then turn it around westbound, well
    // past the near face. 0.5 m per tick: the window is three ticks across.
    step(&mut app, 12);
    assert!(
        app.world()
            .get::<Position>(body)
            .expect("the body still exists")
            .0
            .x
            > depot_trigger_far(),
        "the eastbound leg must clear the window before the return"
    );
    app.world_mut()
        .get_mut::<LinearVelocity>(body)
        .expect("the body still exists")
        .0 = Vec3::new(-DWELL_SPEED_M_S as f32, 0.0, 0.0);
    step(&mut app, 14);
    assert!(
        app.world()
            .get::<Position>(body)
            .expect("the body still exists")
            .0
            .x
            < depot_trigger_near(),
        "and the return leg must clear it on the other side"
    );

    let all = crossings(&app);
    let mine: Vec<_> = all.iter().filter(|c| c.is_by(body)).collect();
    assert_eq!(
        mine.len(),
        1,
        "the pair entered the stream exactly once across two real crossings: \
         {all:?}"
    );
    let stream = app.world().resource::<TriggerCrossings>();
    assert!(
        stream.duplicates() >= 1,
        "the return crossing was decided and refused by the once-per-pair \
         ledger — a count of zero would mean it never reached one: \
         {} delivered, {} duplicates",
        stream.delivered(),
        stream.duplicates()
    );
    assert_eq!(
        applied(&app).len(),
        1,
        "and the door the entry opened still opened exactly once"
    );
}
