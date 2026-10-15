//! Acceptance suite M01-PLAY-ACTORS (#1435): the mission composition's
//! scripted actors — M01's `aiv.zrd` aircraft and `zeppelins.zrd`
//! zeppelins — spawned at start through the production readers.
//!
//! M01's `aiv.zrd` spells twelve aircraft records: `player`, the wingmen
//! `wingman_2`/`wingman_3`, the Furies `bsfury_1`/`bsfury_2`, the
//! Devastators `devastator_2`/`devastator_3` and the Russian squadron
//! `rusdevastator_1`..`rusdevastator_5`. The original's spawn loop skips a
//! record whose `enabled` field is `0` — `bsfury_2` and `rusdevastator_3`
//! — and builds `player` through the engine-state path the composition
//! already owns, so a composed run carries exactly the nine enabled
//! non-player records, `bsfury_1` marked asleep, plus one ECS binding per
//! `zeppelins.zrd` actor the world-actor session launched.

use bevy::prelude::{App, Entity, With};
use cs_app::damage::DamageActorBinding;
use cs_app::mission_actors::{
    MissionActorSpawnReport, MissionAircraftRoster, MissionZeppelinActor, ScriptedAircraftActor,
};
use cs_app::mission_launch::plan_mission_launch;
use cs_app::mission_session::{
    MissionHost, MissionPlayerBody, MissionStage, build_headless, mint_host_generation, stage_for,
    teardown,
};
use cs_app::mission_start::{ScriptedAircraft, SpawnDispatch, spawn_dispatch};
use cs_app::weapons::SessionDamageResolver;
use cs_types::content::Provenance;
use cs_types::evidence::ClaimId;

use crate::common::label;
use crate::vs_m01_rt_window::game_dir;

/// The enabled non-player records M01 must carry into the world, sorted
/// the way the collected name lists are.
const EXPECTED_AIRCRAFT: [&str; 9] = [
    "bsfury_1",
    "devastator_2",
    "devastator_3",
    "rusdevastator_1",
    "rusdevastator_2",
    "rusdevastator_4",
    "rusdevastator_5",
    "wingman_2",
    "wingman_3",
];

/// The records the measured `enabled == 0` skip drops.
const DISABLED_AIRCRAFT: [&str; 2] = ["bsfury_2", "rusdevastator_3"];

/// M01's `zeppelins.zrd` records.
const EXPECTED_ZEPPELINS: [&str; 3] = ["piratezep", "workersvoyagezep", "blackswanzep"];

/// The production stage for retail M01.
fn retail_stage() -> MissionStage {
    let root = game_dir();
    let plan = plan_mission_launch(&root, label("M01"), "The Lost Treasure")
        .expect("M01's launch closure plans");
    stage_for(&root, &plan).expect("M01's stage reads through the production readers")
}

/// The record names on the live `ScriptedAircraftActor` entities, sorted.
fn spawned_aircraft_names(app: &mut App) -> Vec<String> {
    let world = app.world_mut();
    let mut names: Vec<String> = world
        .query::<&ScriptedAircraftActor>()
        .iter(world)
        .map(|actor| actor.record.name.clone())
        .collect();
    names.sort();
    names
}

/// **The roster filter alone keeps exactly the enabled non-player records**
/// — the one acceptance seam a synthetic fixture covers without the
/// installation: the measured `enabled == 0` skip and the `player` record's
/// engine-state path are exercised on fabricated records, so CI catches a
/// roster regression without retail data.
#[test]
fn accept_m01_play_actors_the_roster_filter_is_the_measured_enabled_skip() {
    let record = |name: &str, enabled: Option<bool>| ScriptedAircraft {
        name: name.to_owned(),
        index: 0,
        field_count: 81,
        netids: None,
        stored_pose: None,
        team: None,
        group: None,
        enabled,
        primary_target: None,
        title: None,
        starts_asleep: None,
        dispatch: spawn_dispatch(name),
        provenance: Provenance::designed(
            ClaimId::new("test.m01-play-actors.record").expect("the claim id is valid"),
        ),
    };
    let records = vec![
        record("player", Some(true)),
        record("wingman_2", Some(true)),
        record("bsfury_2", Some(false)),
        record("rusdevastator_3", Some(false)),
        record("devastator_2", None),
    ];
    let roster = MissionAircraftRoster::from_records(&records);
    let names: Vec<&str> = roster.0.iter().map(|record| record.name.as_str()).collect();
    assert_eq!(
        names,
        ["wingman_2", "devastator_2"],
        "the filter keeps the enabled non-player records in order: `player` \
         dispatches to the composition's own body, `enabled == Some(false)` is \
         the measured skip, and an unmeasured `enabled` still spawns"
    );
    assert!(
        matches!(records[0].dispatch, SpawnDispatch::Player),
        "spawn_dispatch names `player`'s engine-state path"
    );
}

/// **The stage carries M01's twelve measured records, and the roster the
/// composition spawns is exactly the nine the original's loop keeps.**
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_play_actors_the_stage_and_roster_carry_the_measured_set() {
    let stage = retail_stage();
    let mut names: Vec<&str> = stage
        .aircraft
        .iter()
        .map(|record| record.name.as_str())
        .collect();
    names.sort_unstable();
    assert_eq!(
        names,
        [
            "bsfury_1",
            "bsfury_2",
            "devastator_2",
            "devastator_3",
            "player",
            "rusdevastator_1",
            "rusdevastator_2",
            "rusdevastator_3",
            "rusdevastator_4",
            "rusdevastator_5",
            "wingman_2",
            "wingman_3",
        ],
        "the stage carries all twelve aiv.zrd records"
    );

    let roster = MissionAircraftRoster::from_records(&stage.aircraft);
    let mut roster_names: Vec<&str> = roster.0.iter().map(|record| record.name.as_str()).collect();
    roster_names.sort_unstable();
    assert_eq!(
        roster_names, EXPECTED_AIRCRAFT,
        "the roster is the enabled non-player set: `player` dispatches to the \
         composition's own body, `bsfury_2`/`rusdevastator_3` are enabled == 0"
    );
    for record in &roster.0 {
        assert!(
            matches!(record.dispatch, SpawnDispatch::VehicleRecord(_)),
            "{} spawns through the measured name-prefix dispatch",
            record.name
        );
        assert!(
            record.stored_pose.is_some(),
            "{} carries the measured stored pose",
            record.name
        );
    }
}

/// **A composed run spawns every enabled scripted aircraft at its measured
/// pose — and nothing else.**
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_play_actors_a_composed_run_spawns_the_enabled_aircraft() {
    let stage = retail_stage();
    let mut app = build_headless(&stage).expect("M01's stage composes headlessly");
    app.update();

    let names = spawned_aircraft_names(&mut app);
    assert_eq!(
        names, EXPECTED_AIRCRAFT,
        "exactly the enabled non-player records spawned"
    );
    for disabled in DISABLED_AIRCRAFT {
        assert!(
            !names.iter().any(|name| name == disabled),
            "{disabled} is enabled == 0: the original's spawn loop skips it"
        );
    }
    assert!(
        !names.iter().any(|name| name == "player"),
        "the player record is the composition's MissionPlayerBody, never a scripted actor"
    );

    // The player is still exactly one body.
    let world = app.world_mut();
    let players = world
        .query_filtered::<Entity, With<MissionPlayerBody>>()
        .iter(world)
        .count();
    assert_eq!(players, 1, "exactly one player body, never duplicated");

    // Each scripted actor holds the measured fields and the designed
    // vehicle-record label, and `bsfury_1` is the one sleeper.
    let mut asleep: Vec<String> = Vec::new();
    for actor in world.query::<&ScriptedAircraftActor>().iter(world) {
        let record = &actor.record;
        assert_eq!(
            record.enabled,
            Some(true),
            "{} spawned enabled",
            record.name
        );
        assert!(
            !actor.vehicle_record.is_empty(),
            "{} carries its measured dispatch prefix as the designed airframe label",
            record.name
        );
        if actor.asleep {
            asleep.push(record.name.clone());
        }
        assert_eq!(
            actor.asleep,
            record.starts_asleep.unwrap_or(false),
            "{}: the binding's asleep flag is the record's measured deactivated field",
            record.name
        );
    }
    assert_eq!(
        asleep,
        ["bsfury_1"],
        "bsfury_1 is M01's only sleeper (#681's wake path)"
    );

    // The spawn report is the composition's own record of the run.
    let report = app.world().resource::<MissionActorSpawnReport>();
    assert_eq!(report.aircraft.len(), 9);
    assert!(
        report.refusals.is_empty(),
        "the spawn reports no refusals: {:?}",
        report.refusals
    );

    teardown(&mut app);
}

/// **Every spawned actor registers a session damage actor through the F29
/// consumer path.**
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_play_actors_every_actor_registers_with_the_damage_resolver() {
    let stage = retail_stage();
    let mut app = build_headless(&stage).expect("M01's stage composes headlessly");
    app.update();

    let world = app.world_mut();
    let mut bindings = Vec::new();
    for (entity, actor) in world
        .query::<(Entity, &ScriptedAircraftActor)>()
        .iter(world)
    {
        let binding = world
            .get::<DamageActorBinding>(entity)
            .unwrap_or_else(|| panic!("{} has no damage binding", actor.record.name));
        assert_eq!(
            binding.actor, actor.damage_actor,
            "{}: the binding's actor is the one the resolver registered",
            actor.record.name
        );
        bindings.push((actor.record.name.clone(), actor.damage_actor));
    }
    assert_eq!(bindings.len(), 9);

    let resolver = app
        .world()
        .get_resource::<SessionDamageResolver>()
        .expect("the mission's damage authority is installed");
    let session = resolver.session();
    let host = app.world().resource::<MissionHost>();
    assert_eq!(
        session,
        host.served().get(),
        "the resolver serves the host's session"
    );
    for (name, actor) in &bindings {
        assert_eq!(
            actor.session.get(),
            session,
            "{name}: the damage actor belongs to the served session"
        );
        assert!(
            resolver.resolver().policy(actor).is_some(),
            "{name} is registered with the session's damage resolver"
        );
    }

    teardown(&mut app);
}

/// **The three `zeppelins.zrd` actors bind ECS entities through the
/// world-actor session the host launched — not through a parallel spawn.**
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_play_actors_the_zeppelins_bind_through_the_world_actor_session() {
    let stage = retail_stage();
    let mut app = build_headless(&stage).expect("M01's stage composes headlessly");
    app.update();

    // The production world-actor session inside the launched host is the
    // authority: the bindings must name its actors one for one.
    let host = app.world().resource::<MissionHost>();
    let actors = host
        .world_actors()
        .expect("M01's zeppelin program lowered and launched");
    let mut world_actor_ids: Vec<_> = actors.set().actors().collect();
    world_actor_ids.sort();
    assert_eq!(
        world_actor_ids.len(),
        EXPECTED_ZEPPELINS.len(),
        "the world-actor session launched M01's three zeppelins"
    );

    let world = app.world_mut();
    let mut bound_ids: Vec<_> = world
        .query::<&MissionZeppelinActor>()
        .iter(world)
        .map(|zeppelin| zeppelin.world_actor)
        .collect();
    bound_ids.sort();
    assert_eq!(
        bound_ids, world_actor_ids,
        "one ECS binding per launched world actor, no parallel spawn"
    );

    for (entity, zeppelin) in world.query::<(Entity, &MissionZeppelinActor)>().iter(world) {
        let binding = world
            .get::<DamageActorBinding>(entity)
            .expect("each zeppelin binding registers a damage actor");
        assert_eq!(binding.actor, zeppelin.damage_actor);
    }

    let report = app.world().resource::<MissionActorSpawnReport>();
    assert_eq!(report.zeppelins.len(), 3);
    assert!(
        report.refusals.is_empty(),
        "the zeppelin bindings report no refusals: {:?}",
        report.refusals
    );

    teardown(&mut app);
}

/// **A restart respawns exactly the roster under the fresh generation.**
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_play_actors_a_restart_respawns_exactly_the_roster() {
    let stage = retail_stage();
    let mut app = build_headless(&stage).expect("M01's stage composes headlessly");
    app.update();
    assert_eq!(spawned_aircraft_names(&mut app).len(), 9);

    let mut host = app
        .world_mut()
        .remove_resource::<MissionHost>()
        .expect("the composed app carries the host");
    let generation = mint_host_generation();
    host.restart(app.world_mut(), &stage, generation)
        .expect("the restart rebuilds the authored state");
    let served = host.served();
    app.world_mut().insert_resource(host);

    assert_eq!(
        spawned_aircraft_names(&mut app),
        EXPECTED_AIRCRAFT,
        "the restart respawns exactly the enabled roster, nothing duplicated"
    );
    let zeppelins = {
        let world = app.world_mut();
        world
            .query_filtered::<Entity, With<MissionZeppelinActor>>()
            .iter(world)
            .count()
    };
    assert_eq!(zeppelins, 3, "the restart rebinds the three zeppelins");

    let world = app.world_mut();
    for actor in world.query::<&ScriptedAircraftActor>().iter(world) {
        assert_eq!(
            actor.damage_actor.session, served,
            "{}: the respawned actor belongs to the fresh session",
            actor.record.name
        );
    }

    teardown(&mut app);
}

/// **Remove the stage's aircraft records and nothing spawns** — the tests
/// above exercise the production roster, never a synthetic constant.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_play_actors_an_empty_record_set_spawns_no_scripted_aircraft() {
    let mut stage = retail_stage();
    stage.aircraft.clear();
    let mut app = build_headless(&stage).expect("the stage composes headlessly");
    app.update();
    assert!(
        spawned_aircraft_names(&mut app).is_empty(),
        "no records, no scripted actors"
    );
    let report = app.world().resource::<MissionActorSpawnReport>();
    assert!(report.aircraft.is_empty());
    teardown(&mut app);
}
