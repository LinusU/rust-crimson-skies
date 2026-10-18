//! Acceptance suite M01-PLAY-AI-WAKE (#1487): the `WAKEUP_ENEMIES` consumer
//! that releases a sleeping scripted aircraft into the designed AI law.
//!
//! Three things this suite proves:
//!
//! 1. **A sleeping record is held until the mission wakes it.** M01's
//!    `bsfury_1` carries the measured `deactivated` field (#681), so the
//!    spawn gives it the kinematic held body #1435 always gave it and no law
//!    ([`cs_app::mission_actors`]); the composed host consumes the record's
//!    own `WAKEUP_ENEMIES` wake and releases it.
//! 2. **The release is the wake the mission spells, and only that.** The
//!    release happens when `OBJECTIVE13`'s `WAKEUP_ENEMIES` site
//!    (`[text bsfury_1, text blackswanzep]`, pinned by
//!    `accept_m01_lc_directive_c.rs`) is emitted by the driven control
//!    program, not on a timer or on the spawn. `bsfury_1` is a scripted
//!    aircraft and is released; `blackswanzep` is a zeppelin and is refused
//!    by name, never silently treated as one.
//! 3. **A released record flies, and a restart re-holds it.** After the
//!    release the body carries the recovered law on its AI seat
//!    (`PlaytestOriginalFlight { is_player: false }`) and the behaviour its
//!    measured fields select ([`MissionAiBehaviour::Pursue`] on the player),
//!    and it actually moves on the fixed tick. A composed restart respawns
//!    the record asleep and held, exactly as a fresh spawn does.
//!
//! Every bound that is not a measured field of the record or of the
//! installation is a **designed** constant of the suite or of
//! `crate::campaign::mission_ai`, never an original measurement: the original's AI
//! control law (`0x48c220`) is unrecovered, and no original executable ran.
//! `retail` is read access to the owner's installation; nothing here is
//! `verified_original`.

use std::path::Path;
use std::time::Duration;

use avian3d::prelude::{Position, RigidBody};
use bevy::prelude::{App, Entity, Time, Vec3};
use bevy::time::TimeUpdateStrategy;

use cs_app::mission_actors::{MissionActorSpawnReport, ScriptedAircraftActor, SpawnedAircraft};
use cs_app::mission_ai::{
    MissionAiAirframe, MissionAiAirframes, MissionAiBehaviour, MissionAiReport, MissionAircraftAi,
    TargetRule, WAKE_ENEMIES_UNKNOWN_NAME_REASON,
};
use cs_app::mission_launch::plan_mission_launch;
use cs_app::mission_session::{
    MissionHost, MissionHostRestartRequest, MissionStage, MissionWorldObservation, build_headless,
    stage_for, teardown,
};
use cs_app::mission_start::{ScriptedAircraft, StoredStartPose, spawn_dispatch};
use cs_app::physics::{BASELINE_FIXED_HZ, PhysicsTickLedger};
use cs_app::playtest::scene::PlaytestOriginalFlight;
use cs_app::world_facts::{MemberObservation, WorldObservation, member_chain};
use cs_script::ir::{
    Action, Condition, DirectiveOperation, IR_VERSION, MissionProgram, Objective, SymbolId, Value,
};
use cs_sim::flight::original::PROVENANCE_LABEL;
use cs_sim::flight::{OriginalAirframe, OriginalFlightModel, OriginalGlobals};
use cs_types::content::Provenance;
use cs_types::evidence::ClaimId;

use crate::campaign::common::label;
use crate::campaign::vs_m01_rt_window::{Scratch, game_dir, synthetic_stage};

/// The elapsed a driven retail step covers: one second of mission clock per
/// host step, the compression `accept_m01_b_wake.rs` documents. Nothing in
/// the lowered program reads the tick-to-second ratio, and this suite reads
/// the *release*, which happens at the OBJECTIVE13 wake and not at a spelled
/// nap delay, so the compression changes only how fast the states arrive.
const DRIVEN_STEP: Duration = Duration::from_secs(1);

/// `OBJECTIVE13`, the block whose measured `WAKEUP_ENEMIES` wake names
/// `bsfury_1` — zero-based index 12.
const WAKE_BLOCK: u32 = 12;

/// **Designed**: the distance a released `bsfury_1` must travel in the fly
/// proof. The record spawns at rest and the law's own 4.47 m/s AI floor plus
/// its throttle move it within a few seconds; the bound is far below what the
/// designed pursuit law reaches (`crate::campaign::mission_ai`'s own retail run closes
/// kilometres in 45 s).
const FLOWN_DISTANCE_M: f32 = 20.0;

// ---------------------------------------------------------------------------
// Shared drive helpers (the shapes accept_m01_b_wake.rs documents)
// ---------------------------------------------------------------------------

/// `workersvoyagezep`'s engine-part chains, in the order the record's
/// `INACTIVE<n>` stages spell them: nine right engines, nine left.
fn workers_voyage_chains() -> Vec<cs_script::ir::MemberName> {
    (1..=9)
        .flat_map(|n| {
            [
                member_chain(["workersvoyagezep", &format!("reng{n}"), "healthy"]),
                member_chain(["workersvoyagezep", &format!("leng{n}"), "healthy"]),
            ]
        })
        .collect()
}

/// The world report a run where `workersvoyagezep` is dead files every step:
/// all eighteen engine parts out of play, groups 1 and 2 living nobody,
/// `wv_drop_copilot` running and `wv_pickup_copilot` + `hooked_to_klondike`
/// executed — the world-side rows the win chain's `INACTIVE`/`DEDG`/
/// `ANIM_STATE` gates read. It is the same report `accept_m01_b_wake.rs` uses
/// to drive the chain through `OBJECTIVE13` (and on to `OBJECTIVE24`).
fn win_observation() -> WorldObservation {
    let mut observation = WorldObservation::new();
    for chain in workers_voyage_chains() {
        observation = observation.member(chain, MemberObservation::no_longer_in_play([0.0; 3]));
    }
    observation
        .group(1, 0)
        .group(2, 0)
        .animation(
            "wv_drop_copilot",
            cs_script::ir::AnimationState::Running.code(),
        )
        .animation(
            "wv_pickup_copilot",
            cs_script::ir::AnimationState::Executed.code(),
        )
        .animation(
            "hooked_to_klondike",
            cs_script::ir::AnimationState::Executed.code(),
        )
}

/// One driven step of the production host, exactly `accept_m01_b_wake.rs`'s
/// seam: the world's report is written through the port, the ledger commits
/// the tick, and the host steps and settles the way `mission_host_tick` does.
fn step_host(app: &mut App, observation: WorldObservation, elapsed: Duration) {
    app.world_mut()
        .insert_resource(MissionWorldObservation { observation });
    app.world_mut().resource_mut::<PhysicsTickLedger>().ticks += 1;
    let mut host = app
        .world_mut()
        .remove_resource::<MissionHost>()
        .expect("the composed app carries the host");
    let produced = host
        .step(app.world_mut(), elapsed)
        .expect("the driven step answers, not a refusal");
    host.settle(app.world_mut(), &produced);
    app.world_mut().insert_resource(host);
}

/// The live entity of a scripted aircraft, by measured record name.
fn entity_named(app: &mut App, name: &str) -> Entity {
    let world = app.world_mut();
    let mut query = world.query::<(Entity, &ScriptedAircraftActor)>();
    query
        .iter(world)
        .find(|(_, actor)| actor.record.name == name)
        .map(|(entity, _)| entity)
        .unwrap_or_else(|| panic!("{name} spawns with the mission's roster"))
}

/// The spawn report's entry for one record.
fn spawned(app: &App, name: &str) -> SpawnedAircraft {
    app.world()
        .resource::<MissionActorSpawnReport>()
        .aircraft
        .iter()
        .find(|spawned| spawned.record == name)
        .cloned()
        .unwrap_or_else(|| panic!("{name} spawns with the mission's roster"))
}

/// The body's authoritative position.
fn position_of(app: &App, entity: Entity) -> Vec3 {
    app.world()
        .get::<Position>(entity)
        .map(|position| position.0)
        .expect("the body still carries a pose")
}

/// Whether a record is still the held kinematic body: no law, no AI, and a
/// kinematic rigid body.
fn is_held(app: &App, entity: Entity) -> bool {
    app.world().get::<PlaytestOriginalFlight>(entity).is_none()
        && app.world().get::<MissionAircraftAi>(entity).is_none()
        && app
            .world()
            .get::<RigidBody>(entity)
            .is_some_and(|body| *body == RigidBody::Kinematic)
}

/// The production stage for retail M01.
fn retail_stage() -> MissionStage {
    let root = game_dir();
    let plan = plan_mission_launch(&root, label("M06"), "The Red Menace")
        .expect("M01's launch closure plans");
    stage_for(&root, &plan).expect("M01's stage reads through the production readers")
}

// ---------------------------------------------------------------------------
// The release path on authored content (runs in CI)
// ---------------------------------------------------------------------------

/// The synthetic record's name; the `_` makes its measured name-prefix
/// dispatch `synthetic`, which is the airframe key the stage carries.
const SYNTHETIC_RECORD: &str = "synthetic_1";

/// The second name the synthetic site spells: nothing in the stage is a
/// scripted aircraft by that name, so it must be refused by name.
const SYNTHETIC_NOT_AN_AIRCRAFT: &str = "synthetic_zeppelin";

/// A synthetic airframe in the measured field shapes, imported through the
/// production `from_values` boundaries — the same tables `vs_m01_rt_window.rs`
/// declares for its synthetic player. It proves the release path, nothing
/// about retail parameters.
fn synthetic_ai_airframe() -> MissionAiAirframe {
    let airframe = OriginalAirframe::from_values(&[
        ("roll_torque", 7.5),
        ("pitch_torque", 3.3),
        ("rudder_torque", 2.0),
        ("level_off_rate", 1.0),
        ("return_rate", 3.0),
        ("ang_momentum_damp", 5.0),
        ("rec_moments_inertia_x", 1.18),
        ("rec_moments_inertia_y", 1.0),
        ("rec_moments_inertia_z", 1.1),
        ("fd_speed", 135.0),
        ("engine_factor", 0.62),
        ("drag_factor", 0.37),
        ("veh_weight", 1900.0),
        ("ref_area", 330.0),
        ("gravity", 20.0),
    ])
    .expect("the declared airframe fields are complete");
    let globals = OriginalGlobals::from_values(&[
        ("nom_gravity", 20.0),
        ("lift_aoa_0_deg", 5.0),
        ("lift_aoa_1_deg", 9.0),
        ("max_aoa_deg", 46.0),
        ("high_g_0", 9.0),
        ("high_g_1", 15.0),
        ("low_g_0", -6.0),
        ("low_g_1", -9.0),
        ("lift_accel_rate", 0.75),
        ("stall_mag", 1.25),
        ("turn_fade_in_mph", 10.0),
        ("turn_fade_out_mph", 50.0),
        ("yaw_low_speed", 0.0625),
        ("yaw_high_speed", 0.17),
        ("yaw_fade_in_mph", 10.0),
        ("yaw_max_mph", 50.0),
        ("yaw_fade_out_mph", 400.0),
    ])
    .expect("the declared globals are complete");
    MissionAiAirframe {
        model: OriginalFlightModel::full(airframe, globals),
        record: "synthetic".to_owned(),
        inheritance_chain: Vec::new(),
        provenance: PROVENANCE_LABEL,
    }
}

/// One synthetic scripted-aircraft record: enabled, asleep, of the other
/// team, its stored target the player. It is authored in the measured
/// `aiv.zrd` shapes the readers and the spawn consume, never a reading of any
/// original mission.
fn synthetic_sleeper() -> ScriptedAircraft {
    let provenance = Provenance::designed(
        ClaimId::new("test.m01-play-ai-wake.record").expect("the claim id is valid"),
    );
    ScriptedAircraft {
        name: SYNTHETIC_RECORD.to_owned(),
        index: 1,
        field_count: 81,
        netids: None,
        stored_pose: Some(StoredStartPose {
            position: [40.0, 20.0, -60.0],
            heading: 0.0,
        }),
        team: Some(2),
        group: None,
        enabled: Some(true),
        primary_target: Some("player".to_owned()),
        title: None,
        starts_asleep: Some(true),
        dispatch: spawn_dispatch(SYNTHETIC_RECORD),
        provenance,
    }
}

/// The synthetic control program: one objective that completes on its first
/// evaluated tick and spells the two-name `WAKEUP_ENEMIES` site the measured
/// M01 site spells. Authored here as synthetic content.
fn synthetic_wake_program() -> MissionProgram {
    MissionProgram {
        version: IR_VERSION,
        mission: crate::campaign::common::cid(
            cs_types::content::ContentKind::Mission,
            "synthetic-mission-play-ai-wake",
        ),
        variables: Vec::new(),
        objectives: vec![Objective {
            id: SymbolId(0),
            content: crate::campaign::common::cid(
                cs_types::content::ContentKind::Objective,
                "synthetic-play-ai-wake",
            ),
            condition: Condition::Const(true),
            actions: vec![Action::Directive {
                operation: DirectiveOperation::WakeEnemies,
                args: vec![
                    Value::Str(SYNTHETIC_RECORD.to_owned()),
                    Value::Str(SYNTHETIC_NOT_AN_AIRCRAFT.to_owned()),
                ],
            }],
            span: None,
        }],
    }
}

/// A synthetic stage whose control program spells the two-name wake, whose
/// roster holds one sleeping record and whose airframe table imports its
/// measured name prefix.
fn synthetic_wake_stage(scratch: &Path) -> MissionStage {
    let mut stage = synthetic_stage(scratch);
    stage.aircraft = vec![synthetic_sleeper()];
    let mut models = std::collections::BTreeMap::new();
    models.insert("synthetic".to_owned(), synthetic_ai_airframe());
    stage.ai_airframes = MissionAiAirframes {
        models,
        refusals: Vec::new(),
    };
    stage.host.control = synthetic_wake_program();
    stage
}

/// **The wake releases the sleeping record into the designed law, names the
/// name that is not an aircraft, and the released body flies.**
///
/// The spawn-only frame proves the held state, the wake site's emission
/// releases the record, and the fixed loop then moves it: the release is the
/// mission's own directive, not a timer or a spawn default.
#[test]
fn accept_m01_play_ai_wake_synthetic_release_flies_the_record() {
    let scratch = Scratch::new("m01-play-ai-wake");
    let stage = synthetic_wake_stage(scratch.path());
    let mut app = build_headless(&stage).expect("the synthetic stage composes");
    let timestep = app.world().resource::<Time<bevy::time::Fixed>>().timestep();

    // The spawn-only frame: zero fixed ticks run, so Startup spawns the roster
    // and the control program has not evaluated yet.
    *app.world_mut().resource_mut::<TimeUpdateStrategy>() =
        TimeUpdateStrategy::ManualDuration(Duration::ZERO);
    app.update();

    let held = spawned(&app, SYNTHETIC_RECORD);
    assert!(
        held.asleep,
        "the measured `deactivated` field starts the record asleep at the spawn"
    );
    let entity = entity_named(&mut app, SYNTHETIC_RECORD);
    assert!(
        is_held(&app, entity),
        "before the wake the record is the held kinematic body: no law, no AI, kinematic"
    );
    let report = app.world().resource::<MissionAiReport>().clone();
    assert!(
        report
            .held
            .iter()
            .any(|entry| entry.starts_with(SYNTHETIC_RECORD)),
        "the spawn report holds the sleeping record: {:?}",
        report.held
    );
    assert_eq!(
        app.world()
            .resource::<MissionHost>()
            .wake_enemies()
            .applied(),
        0,
        "no wake directive has been consumed before the program evaluates"
    );

    // The fixed loop: the program completes its objective on the first
    // evaluated tick and the host consumes the wake it emitted.
    let mut released = false;
    for _ in 0..120 {
        *app.world_mut().resource_mut::<TimeUpdateStrategy>() =
            TimeUpdateStrategy::ManualDuration(timestep);
        app.update();
        if app.world().get::<PlaytestOriginalFlight>(entity).is_some() {
            released = true;
            break;
        }
    }
    assert!(
        released,
        "the wake releases the sleeping record within the frame budget"
    );

    // The released body is the awake record's body: the recovered law on the
    // AI seat, the behaviour its measured fields select, and a dynamic body.
    let flight = app
        .world()
        .get::<PlaytestOriginalFlight>(entity)
        .expect("the released record carries the law");
    assert!(
        !flight.is_player,
        "the released record flies the law's AI branch, never the player's"
    );
    assert_eq!(
        app.world()
            .get::<MissionAircraftAi>(entity)
            .map(|ai| ai.behaviour.clone()),
        Some(MissionAiBehaviour::Pursue {
            target: TargetRule::Player
        }),
        "the measured team and stored target select the pursuit behaviour once the sleep is consumed"
    );
    assert_eq!(
        app.world().get::<RigidBody>(entity),
        Some(&RigidBody::Dynamic),
        "the release swaps the held kinematic body for the dynamic one forces move"
    );
    assert!(
        !app.world()
            .get::<ScriptedAircraftActor>(entity)
            .expect("the actor binding stays")
            .asleep,
        "the record's own asleep mark is cleared by the release"
    );
    let report = app.world().resource::<MissionAiReport>().clone();
    assert!(
        report.woken.contains(&SYNTHETIC_RECORD.to_owned())
            && report.flown.contains(&SYNTHETIC_RECORD.to_owned()),
        "the report follows the record from held to flown: {report:?}"
    );
    assert!(
        !report
            .held
            .iter()
            .any(|entry| entry.starts_with(SYNTHETIC_RECORD)),
        "the record is no longer held: {:?}",
        report.held
    );

    // The ledger names what it released and refuses what is not an aircraft.
    let host = app.world().resource::<MissionHost>();
    let ledger = host.wake_enemies();
    assert_eq!(ledger.applied(), 1, "one wake site was applied");
    let release = ledger
        .released()
        .iter()
        .find(|release| release.name() == SYNTHETIC_RECORD)
        .unwrap_or_else(|| panic!("the record must be released: {ledger:?}"));
    assert_eq!(
        Some(release.at()),
        host.last_tick(),
        "the release is stamped with the tick the host consumed the directive on"
    );
    let refusal = ledger
        .refused()
        .iter()
        .find(|refusal| refusal.name() == SYNTHETIC_NOT_AN_AIRCRAFT)
        .unwrap_or_else(|| panic!("{SYNTHETIC_NOT_AN_AIRCRAFT} must be refused: {ledger:?}"));
    assert_eq!(refusal.reason(), WAKE_ENEMIES_UNKNOWN_NAME_REASON);

    // The released record actually flies: the fixed loop's decision writes its
    // command and the flight driver moves the dynamic body.
    let start = position_of(&app, entity);
    for _ in 0..(3 * u64::from(BASELINE_FIXED_HZ)) {
        *app.world_mut().resource_mut::<TimeUpdateStrategy>() =
            TimeUpdateStrategy::ManualDuration(timestep);
        app.update();
    }
    let end = position_of(&app, entity);
    assert!(
        end.distance(start) > FLOWN_DISTANCE_M,
        "the released record flies under the law: {start:?} -> {end:?}"
    );

    teardown(&mut app);
}

// ---------------------------------------------------------------------------
// M01's own record, through the installation (retail)
// ---------------------------------------------------------------------------

/// **Against the owner's installation, M01's own `OBJECTIVE13` wake releases
/// `bsfury_1` into the designed law, it flies, and a restart re-holds it.**
///
/// `bsfury_1` is the one record M01's `deactivated` field sleeps. It spawns
/// held; the measured win chain wakes `OBJECTIVE13`, whose own
/// `WAKEUP_ENEMIES` site names `bsfury_1` and `blackswanzep`; the host
/// consumes it, releases the aircraft and refuses the zeppelin by name. The
/// released record then flies on the fixed tick, and the composed restart
/// respawns it asleep and held.
#[test]
#[ignore = "requires CS_GAME_DIR and CS_ENGINE_IMAGE"]
fn accept_m01_play_ai_wake_retail_the_wake_releases_bsfury_1_and_a_restart_re_holds_it() {
    let stage = retail_stage();
    let mut app = build_headless(&stage).expect("M01's stage composes headlessly");
    let timestep = app.world().resource::<Time<bevy::time::Fixed>>().timestep();

    // The spawn-only frame: zero fixed ticks run, so `bsfury_1` is exactly the
    // body the spawn made and the host has consumed no directive.
    *app.world_mut().resource_mut::<TimeUpdateStrategy>() =
        TimeUpdateStrategy::ManualDuration(Duration::ZERO);
    app.update();

    let spawn = spawned(&app, "bsfury_1");
    assert!(
        spawn.asleep,
        "bsfury_1 is the one record M01's measured deactivated field sleeps"
    );
    let entity = entity_named(&mut app, "bsfury_1");
    assert!(
        is_held(&app, entity),
        "before the wake bsfury_1 is the held kinematic body, exactly as #1435 spawned it"
    );
    assert!(
        app.world()
            .resource::<MissionAiReport>()
            .held
            .iter()
            .any(|entry| entry.starts_with("bsfury_1")),
        "the spawn's own report holds bsfury_1"
    );
    assert_eq!(
        app.world()
            .resource::<MissionHost>()
            .wake_enemies()
            .applied(),
        0,
        "no wake directive is consumed on the spawn tick"
    );

    // Drive M01's own win chain to the block its record spells the wake on.
    // The chain climbs OBJECTIVE6 -> 8 -> 9 -> 10 -> 49 -> 50 -> 11 -> 12 ->
    // OBJECTIVE13; the release lands on the latter's wake, before the chain
    // reaches its terminal.
    let mut released = false;
    for _ in 0..400 {
        step_host(&mut app, win_observation(), DRIVEN_STEP);
        if app.world().get::<PlaytestOriginalFlight>(entity).is_some() {
            released = true;
            break;
        }
        assert!(
            app.world().resource::<MissionHost>().terminal().is_none(),
            "the wake is on the win chain and must land before the terminal does"
        );
    }
    assert!(
        released,
        "M01's own OBJECTIVE13 wake releases bsfury_1 within the budget"
    );
    assert!(
        app.world()
            .resource::<MissionHost>()
            .lifecycles()
            .state(WAKE_BLOCK)
            .is_some(),
        "the block the record spells the wake on is loaded"
    );

    // The released aircraft: the recovered law on the AI seat, the behaviour
    // its measured fields select, and a dynamic body the forces move.
    let flight = app
        .world()
        .get::<PlaytestOriginalFlight>(entity)
        .expect("the released record carries the law");
    assert!(
        !flight.is_player,
        "bsfury_1 flies the law's AI branch, never the player's"
    );
    assert_eq!(
        app.world()
            .get::<MissionAircraftAi>(entity)
            .map(|ai| ai.behaviour.clone()),
        Some(MissionAiBehaviour::Pursue {
            target: TargetRule::Player
        }),
        "bsfury_1's measured team and stored `player` target select the pursuit behaviour"
    );
    assert_eq!(
        app.world().get::<RigidBody>(entity),
        Some(&RigidBody::Dynamic),
        "the release swaps the held kinematic body for the dynamic one forces move"
    );
    let report = app.world().resource::<MissionAiReport>().clone();
    assert!(
        report.woken.contains(&"bsfury_1".to_owned()),
        "the report names the record the wake released: {report:?}"
    );
    assert!(
        report.flown.contains(&"bsfury_1".to_owned())
            && !report
                .held
                .iter()
                .any(|entry| entry.starts_with("bsfury_1")),
        "bsfury_1 moves from held to flown: {report:?}"
    );

    // The ledger consumed exactly the one wake site and named the zeppelin.
    let host = app.world().resource::<MissionHost>();
    let ledger = host.wake_enemies();
    assert_eq!(
        ledger.applied(),
        1,
        "M01 spells exactly one WAKEUP_ENEMIES site, applied once"
    );
    assert!(
        ledger
            .released()
            .iter()
            .any(|release| release.name() == "bsfury_1"),
        "the ledger records the release: {ledger:?}"
    );
    assert_eq!(
        ledger.released().len(),
        1,
        "only bsfury_1 is a scripted aircraft of that site: {ledger:?}"
    );
    let refusal = ledger
        .refused()
        .iter()
        .find(|refusal| refusal.name() == "blackswanzep")
        .unwrap_or_else(|| panic!("blackswanzep must be refused by name: {ledger:?}"));
    assert_eq!(refusal.reason(), WAKE_ENEMIES_UNKNOWN_NAME_REASON);
    assert!(
        host.last_tick().is_some(),
        "the host was stepped when the release landed"
    );

    // It flies: the fixed decision writes the pursuit command and the flight
    // driver moves the dynamic body.
    let start = position_of(&app, entity);
    for _ in 0..(3 * u64::from(BASELINE_FIXED_HZ)) {
        *app.world_mut().resource_mut::<TimeUpdateStrategy>() =
            TimeUpdateStrategy::ManualDuration(timestep);
        app.update();
    }
    let end = position_of(&app, entity);
    assert!(
        end.distance(start) > FLOWN_DISTANCE_M,
        "the released bsfury_1 flies under the law: {start:?} -> {end:?}"
    );

    // A composed restart re-holds it: the roster respawns from the record's
    // own measured sleep, and the fresh host has applied no directive.
    app.world_mut().insert_resource(MissionHostRestartRequest);
    app.update();

    let after_restart = entity_named(&mut app, "bsfury_1");
    assert!(
        is_held(&app, after_restart),
        "the restart respawns bsfury_1 as the held kinematic body"
    );
    let report = app.world().resource::<MissionAiReport>().clone();
    assert!(
        report
            .held
            .iter()
            .any(|entry| entry.starts_with("bsfury_1"))
            && report.woken.is_empty(),
        "the fresh spawn holds bsfury_1 again: {report:?}"
    );
    assert_eq!(
        app.world()
            .resource::<MissionHost>()
            .wake_enemies()
            .applied(),
        0,
        "the restarted host has applied no directive of the fresh session"
    );

    teardown(&mut app);
}

/// **The wake is M01's own site, and the release does not fire without it.**
///
/// The same stage with the recovered engine lifecycle model removed: the
/// `WAKE_ANIM`/`NAP` effects that carry the chain have no model to apply to,
/// so `OBJECTIVE13` never wakes and `bsfury_1` stays held. This is the
/// fail-closed leg: the release is the mission's own directive and nothing
/// else spawns it flying.
#[test]
#[ignore = "requires CS_GAME_DIR and CS_ENGINE_IMAGE"]
fn accept_m01_play_ai_wake_retail_without_the_lifecycle_model_bsfury_1_stays_held() {
    let mut stage = retail_stage();
    stage.host.engine_objectives = None;
    let mut app = build_headless(&stage).expect("M01's stage composes headlessly");
    // One spawn-only frame: the roster exists, and no fixed tick has stepped
    // the host with a world report the chain could climb.
    *app.world_mut().resource_mut::<TimeUpdateStrategy>() =
        TimeUpdateStrategy::ManualDuration(Duration::ZERO);
    app.update();

    for _ in 0..60 {
        step_host(&mut app, win_observation(), DRIVEN_STEP);
    }

    let entity = entity_named(&mut app, "bsfury_1");
    assert!(
        is_held(&app, entity),
        "without the lifecycle model the chain cannot reach OBJECTIVE13, so bsfury_1 stays held"
    );
    assert_eq!(
        app.world()
            .resource::<MissionHost>()
            .wake_enemies()
            .applied(),
        0,
        "no wake directive was emitted without the chain that spells it"
    );
    assert!(
        app.world().resource::<MissionAiReport>().woken.is_empty(),
        "nothing was released"
    );

    teardown(&mut app);
}
