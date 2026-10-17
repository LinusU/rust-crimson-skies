//! Acceptance suite M01-PLAY-AI-RESTART (#1509): a composed restart must
//! respawn the mission's scripted aircraft **flying**, exactly as the first
//! spawn does.
//!
//! Two things this suite proves:
//!
//! 1. **The composed restart keeps the AI airframe table** (CI, synthetic).
//!    `mission_host_restart` takes the [`MissionStage`] resource out of the
//!    world before it calls `MissionHost::restart`, so a spawn that re-read
//!    the resource there would find no airframe table: every non-asleep
//!    scripted aircraft would come back as the kinematic held body, with no
//!    law and no side, for the rest of the run. A synthetic stage's one
//!    scripted record — an imported airframe and a measured team — is driven
//!    through the composed entry (a [`MissionHostRestartRequest`] consumed by
//!    `app.update()`) and comes back flying, with the same flown/held split,
//!    the same designed behaviour and the same hostile roster relation, under
//!    freshly allocated session actors.
//! 2. **Every M01 record that flew before the restart flies after it**
//!    (retail). Against the owner's installation, M01's own roster spawns
//!    through `stage_for`'s imported airframes, the composed restart runs, and
//!    every record that flew before flies after it — the same behaviours, the
//!    freshly allocated session actors — while the one measured `deactivated`
//!    sleeper stays held on both sides of the restart.
//!
//! Every bound that is not a measured field of the record or of the
//! installation is a **designed** constant of this suite or of
//! `crate::campaign::mission_ai`, never an original measurement: the original's AI
//! control law (`0x48c220`) is unrecovered, and no original executable ran.
//! `retail` is read access to the owner's installation; nothing here is
//! `verified_original`.

use std::path::Path;
use std::time::Duration;

use avian3d::prelude::RigidBody;
use bevy::prelude::{App, Entity, Time};
use bevy::time::TimeUpdateStrategy;

use cs_app::mission_actors::{MissionActorSpawnReport, ScriptedAircraftActor, SpawnedAircraft};
use cs_app::mission_ai::{
    MissionAiAirframe, MissionAiAirframes, MissionAiBehaviour, MissionAiReport, MissionAircraftAi,
    TargetRule,
};
use cs_app::mission_launch::plan_mission_launch;
use cs_app::mission_session::{
    MISSION_FACTION_HOSTILE, MissionHostRestartReport, MissionHostRestartRequest, MissionStage,
    build_headless, stage_for, teardown,
};
use cs_app::mission_start::{ScriptedAircraft, StoredStartPose, spawn_dispatch};
use cs_app::playtest::scene::PlaytestOriginalFlight;
use cs_app::targeting::TargetingSession;
use cs_sim::damage::ActorId;
use cs_sim::flight::original::PROVENANCE_LABEL;
use cs_sim::flight::{OriginalAirframe, OriginalFlightModel, OriginalGlobals};
use cs_types::content::{ContentId, ContentKind, Provenance};
use cs_types::evidence::ClaimId;

use crate::campaign::common::label;
use crate::campaign::vs_m01_rt_window::{Scratch, game_dir, synthetic_stage};

/// How many committed fixed ticks each driven phase runs: enough for the
/// weapon/targeting re-open (`ensure_mission_weapons`) and the arming pass to
/// roster a freshly respawned generation, and a small designed budget rather
/// than a measured one.
const DRIVE_TICKS: u64 = 5;

/// The synthetic record's name; the `_` makes its measured name-prefix
/// dispatch `synthetic`, which is the airframe key the stage carries.
const SYNTHETIC_RECORD: &str = "synthetic_1";

/// The synthetic stage's `player` record's measured team. The flyer's own
/// team differs from it, so the designed reading selects the pursuit
/// behaviour and the hostile roster side on both sides of the restart.
const SYNTHETIC_PLAYER_TEAM: u32 = 1;

/// The synthetic flyer's measured team.
const SYNTHETIC_FLYER_TEAM: u32 = 2;

// ---------------------------------------------------------------------------
// Shared helpers (the shapes `m01_play_ai_wake.rs` documents)
// ---------------------------------------------------------------------------

/// The production stage for retail M01.
fn retail_stage() -> MissionStage {
    let root = game_dir();
    let plan = plan_mission_launch(&root, label("M01"), "The Lost Treasure")
        .expect("M01's launch closure plans");
    stage_for(&root, &plan).expect("M01's stage reads through the production readers")
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

/// The AI report's own flown/held split, sorted: the two sides of the
/// comparison a restart must reproduce record for record.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Split {
    flown: Vec<String>,
    held: Vec<String>,
}

fn split(app: &App) -> Split {
    let report = app.world().resource::<MissionAiReport>();
    let mut flown = report.flown.clone();
    let mut held = report.held.clone();
    flown.sort();
    held.sort();
    Split { flown, held }
}

/// One fixed-tick frame per update, exactly as the AI suites drive the
/// composed loop.
fn drive(app: &mut App, ticks: u64) {
    let timestep = app.world().resource::<Time<bevy::time::Fixed>>().timestep();
    for _ in 0..ticks {
        *app.world_mut().resource_mut::<TimeUpdateStrategy>() =
            TimeUpdateStrategy::ManualDuration(timestep);
        app.update();
    }
}

/// The faction a record is rostered under in the targeting store, or `None`
/// while the store holds no record for it — the roster relation the gun
/// step's pair check reads (`MissionAircraftAi::opposed` decides it).
fn roster_faction(app: &App, actor: ActorId) -> Option<ContentId> {
    app.world()
        .get_resource::<TargetingSession>()
        .and_then(|targeting| targeting.store().record(&actor))
        .map(|record| record.faction.clone())
}

// ---------------------------------------------------------------------------
// The composed restart on authored content (runs in CI)
// ---------------------------------------------------------------------------

/// A synthetic airframe in the measured field shapes, imported through the
/// production `from_values` boundaries — the same tables `m01_play_ai_wake.rs`
/// declares. It proves the restart path, nothing about retail parameters.
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

/// One synthetic scripted-aircraft record: enabled, awake, of the other
/// team, its stored target the player. It is authored in the measured
/// `aiv.zrd` shapes the readers and the spawn consume, never a reading of any
/// original mission.
fn synthetic_flyer() -> ScriptedAircraft {
    let provenance = Provenance::designed(
        ClaimId::new("test.m01-play-ai-restart.flyer").expect("the claim id is valid"),
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
        team: Some(SYNTHETIC_FLYER_TEAM),
        group: None,
        enabled: Some(true),
        primary_target: Some("player".to_owned()),
        title: None,
        starts_asleep: Some(false),
        dispatch: spawn_dispatch(SYNTHETIC_RECORD),
        provenance,
    }
}

/// The stage's `player` record: what makes the stage carry a measured player
/// team, so `player_team_of` (and the view that carries it) has a side to
/// read on both sides of the restart. The roster excludes it exactly as
/// `MissionAircraftRoster::from_records` excludes M01's own.
fn synthetic_player_record() -> ScriptedAircraft {
    let provenance = Provenance::designed(
        ClaimId::new("test.m01-play-ai-restart.player").expect("the claim id is valid"),
    );
    ScriptedAircraft {
        name: "player".to_owned(),
        index: 0,
        field_count: 81,
        netids: None,
        stored_pose: Some(StoredStartPose {
            position: [0.0, 20.0, 0.0],
            heading: 0.0,
        }),
        team: Some(SYNTHETIC_PLAYER_TEAM),
        group: None,
        enabled: Some(true),
        primary_target: None,
        title: None,
        starts_asleep: Some(false),
        dispatch: spawn_dispatch("player"),
        provenance,
    }
}

/// A synthetic stage whose roster holds one awake scripted record with an
/// imported airframe and a measured team, beside the `player` record whose
/// measured team splits the roster.
fn synthetic_restart_stage(scratch: &Path) -> MissionStage {
    let mut stage = synthetic_stage(scratch);
    stage.aircraft = vec![synthetic_player_record(), synthetic_flyer()];
    let mut models = std::collections::BTreeMap::new();
    models.insert("synthetic".to_owned(), synthetic_ai_airframe());
    stage.ai_airframes = MissionAiAirframes {
        models,
        refusals: Vec::new(),
    };
    stage
}

/// **The composed restart respawns the record flying, with the same split,
/// the same behaviour and the same roster relation.**
///
/// The first spawn flies the record (imported airframe, measured team against
/// the stage's player team); the composed entry — which holds the stage
/// resource out of the world for the whole rebuild — then respawns it with
/// `MissionAircraftAi`, a dynamic body and the hostile roster faction, under
/// a freshly allocated session actor. Before #1509's fix the respawn finds no
/// airframe table and the record comes back **held**, so every assertion on
/// the post-restart split fails.
#[test]
fn accept_m01_play_ai_restart_synthetic_composed_restart_respawns_the_record_flying() {
    let scratch = Scratch::new("m01-play-ai-restart");
    let stage = synthetic_restart_stage(scratch.path());
    let mut app = build_headless(&stage).expect("the synthetic stage composes");

    // The spawn-only frame: zero fixed ticks run, so Startup spawns the
    // roster and nothing has stepped it yet.
    *app.world_mut().resource_mut::<TimeUpdateStrategy>() =
        TimeUpdateStrategy::ManualDuration(Duration::ZERO);
    app.update();

    let before = split(&app);
    assert_eq!(
        before.flown,
        [SYNTHETIC_RECORD],
        "the awake record with an imported airframe flies at the first spawn: {before:?}"
    );
    assert!(
        before.held.is_empty(),
        "no record is held at the first spawn: {before:?}"
    );
    assert!(
        app.world()
            .resource::<MissionAiReport>()
            .refusals
            .is_empty(),
        "the first spawn reports no AI refusals: {:?}",
        app.world().resource::<MissionAiReport>().refusals
    );

    let first_entity = entity_named(&mut app, SYNTHETIC_RECORD);
    assert!(
        !is_held(&app, first_entity),
        "the awake record spawns with the law and a dynamic body"
    );
    let first_behaviour = app
        .world()
        .get::<MissionAircraftAi>(first_entity)
        .expect("the spawned record carries its AI")
        .behaviour
        .clone();
    assert_eq!(
        first_behaviour,
        MissionAiBehaviour::Pursue {
            target: TargetRule::Player
        },
        "the measured team against the player's selects the pursuit behaviour"
    );
    let first_actor = spawned(&app, SYNTHETIC_RECORD).actor;

    // The first generation's roster relation: a few committed ticks arm the
    // record through the mission's own sessions, which rosters its faction.
    drive(&mut app, DRIVE_TICKS);
    let hostile = ContentId::from_source(ContentKind::Faction, MISSION_FACTION_HOSTILE)
        .expect("the designed hostile faction id is valid");
    assert_eq!(
        roster_faction(&app, first_actor),
        Some(hostile.clone()),
        "the first generation rosters the opposed record as the hostile faction"
    );

    // The composed restart, exactly as the playtest reset and a terminal's
    // retry reach it: the request consumed by `app.update()`, which takes the
    // host and the stage out of the world for the rebuild.
    app.world_mut().insert_resource(MissionHostRestartRequest);
    app.update();
    assert!(
        app.world().contains_resource::<MissionHostRestartReport>(),
        "the composed restart ran and wrote its report"
    );

    let after = split(&app);
    assert_eq!(
        after.flown, before.flown,
        "the same records fly after the composed restart: {before:?} -> {after:?}"
    );
    assert_eq!(
        after.held, before.held,
        "the same records are held after the composed restart: {before:?} -> {after:?}"
    );
    assert!(
        app.world()
            .resource::<MissionAiReport>()
            .refusals
            .is_empty(),
        "the respawn reports no AI refusals: {:?}",
        app.world().resource::<MissionAiReport>().refusals
    );

    let entity = entity_named(&mut app, SYNTHETIC_RECORD);
    assert_ne!(
        entity, first_entity,
        "the restart despawns the old generation's entities and respawns fresh ones"
    );
    assert!(
        !is_held(&app, entity),
        "the record comes back flying — the law and the dynamic body, never the held body"
    );
    let ai = app
        .world()
        .get::<MissionAircraftAi>(entity)
        .expect("the respawned record carries its AI");
    assert_eq!(
        ai.behaviour, first_behaviour,
        "the respawned record takes the same designed behaviour the first spawn selected"
    );
    assert!(
        ai.opposed(),
        "the respawned record still states the side its measured team selects"
    );
    assert_eq!(
        app.world().get::<RigidBody>(entity),
        Some(&RigidBody::Dynamic),
        "the respawned body is the dynamic one the law's forces move"
    );
    assert!(
        !app.world()
            .get::<PlaytestOriginalFlight>(entity)
            .expect("the respawned record carries the law")
            .is_player,
        "the respawned record flies the law's AI branch, never the player's"
    );
    let fresh_actor = spawned(&app, SYNTHETIC_RECORD).actor;
    assert_ne!(
        fresh_actor, first_actor,
        "the respawned record is rostered under the fresh session's freshly allocated actor"
    );

    // The same roster relation, on the fresh actor: the sessions re-open for
    // the fresh generation and the arming pass rosters the record again.
    drive(&mut app, DRIVE_TICKS);
    assert_eq!(
        roster_faction(&app, fresh_actor),
        Some(hostile),
        "the respawned record rosters under the same hostile faction"
    );
    assert!(
        roster_faction(&app, first_actor).is_none(),
        "the torn-down generation's actor is gone from the fresh store"
    );

    teardown(&mut app);
}

// ---------------------------------------------------------------------------
// M01's own roster, through the installation (retail)
// ---------------------------------------------------------------------------

/// **Against the owner's installation, every M01 record that flew before the
/// composed restart flies after it — freshly allocated, same behaviour — and
/// the measured sleeper stays held on both sides.**
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_play_ai_restart_retail_composed_restart_respawns_every_flown_record() {
    let stage = retail_stage();
    let mut app = build_headless(&stage).expect("M01's stage composes headlessly");

    // The spawn-only frame: zero fixed ticks run, so the roster is exactly
    // what the first spawn made.
    *app.world_mut().resource_mut::<TimeUpdateStrategy>() =
        TimeUpdateStrategy::ManualDuration(Duration::ZERO);
    app.update();

    let before = split(&app);
    assert!(
        !before.flown.is_empty(),
        "M01's awake records fly at the first spawn: {before:?}"
    );
    assert!(
        !before.held.is_empty(),
        "M01's measured `deactivated` sleeper is held at the first spawn: {before:?}"
    );
    assert!(
        app.world()
            .resource::<MissionAiReport>()
            .refusals
            .is_empty(),
        "the first spawn reports no AI refusals: {:?}",
        app.world().resource::<MissionAiReport>().refusals
    );
    let first: Vec<(String, Entity, ActorId, MissionAiBehaviour)> = before
        .flown
        .iter()
        .map(|name| {
            let entity = entity_named(&mut app, name);
            assert!(
                !is_held(&app, entity),
                "{name} spawns flying, exactly as the first spawn reports it"
            );
            let ai = app
                .world()
                .get::<MissionAircraftAi>(entity)
                .unwrap_or_else(|| panic!("{name} spawns with its AI"));
            (
                name.clone(),
                entity,
                spawned(&app, name).actor,
                ai.behaviour.clone(),
            )
        })
        .collect();

    // The composed restart, exactly as the playtest reset and a terminal's
    // retry reach it.
    app.world_mut().insert_resource(MissionHostRestartRequest);
    app.update();
    assert!(
        app.world().contains_resource::<MissionHostRestartReport>(),
        "the composed restart ran and wrote its report"
    );

    let after = split(&app);
    assert_eq!(
        after.flown, before.flown,
        "every record that flew before the restart flies after it: {before:?} -> {after:?}"
    );
    assert_eq!(
        after.held, before.held,
        "the measured sleeper is held on both sides of the restart: {before:?} -> {after:?}"
    );
    assert!(
        app.world()
            .resource::<MissionAiReport>()
            .refusals
            .is_empty(),
        "the respawn reports no AI refusals: {:?}",
        app.world().resource::<MissionAiReport>().refusals
    );

    for (name, first_entity, first_actor, behaviour) in &first {
        let entity = entity_named(&mut app, name);
        assert_ne!(
            entity, *first_entity,
            "{name} respawns as a fresh entity of the fresh generation"
        );
        assert!(
            !is_held(&app, entity),
            "{name} comes back flying, never as the held body"
        );
        let ai = app
            .world()
            .get::<MissionAircraftAi>(entity)
            .unwrap_or_else(|| panic!("{name} comes back with its AI"));
        assert_eq!(
            &ai.behaviour, behaviour,
            "{name} comes back with the same designed behaviour"
        );
        assert_eq!(
            app.world().get::<RigidBody>(entity),
            Some(&RigidBody::Dynamic),
            "{name} comes back as the dynamic body the law moves"
        );
        assert!(
            !app.world()
                .get::<PlaytestOriginalFlight>(entity)
                .unwrap_or_else(|| panic!("{name} comes back with the law"))
                .is_player,
            "{name} comes back on the law's AI branch"
        );
        let fresh_actor = spawned(&app, name).actor;
        assert_ne!(
            fresh_actor, *first_actor,
            "{name} comes back under the fresh session's freshly allocated actor"
        );
    }

    teardown(&mut app);
}
