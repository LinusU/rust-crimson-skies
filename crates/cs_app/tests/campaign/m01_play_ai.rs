//! Acceptance suite M01-PLAY-AI (#1436): the designed pursuit-and-fire law
//! that flies M01's scripted aircraft.
//!
//! Three things this suite proves against the retail mission:
//!
//! 1. **Every scripted record gets a law.** `stage_for` imports the
//!    `vehicle.zrd` record each measured name-prefix dispatch resolves, the
//!    spawn attaches the recovered law on its AI seat to every awake record
//!    and leaves the one `starts_asleep` record held at its measured pose,
//!    and the fixed-tick decision writes commands to all of them.
//! 2. **The designed law does what the task asks.** In a headless run the
//!    records of the other team close on the player inside the designed
//!    engagement window, and `wingman_2` (Betty) ends the run inside the
//!    designed station distance of her measured leader — inside the window
//!    `RUN_SECONDS` gives them.
//! 3. **The attackers actually shoot.** Every flown record is armed through
//!    the mission's own weapon session (the designed rig #1437 opened), the
//!    law asks to fire inside the designed window and cone within
//!    `RUN_SECONDS`, and at least one attacker's rounds leave the barrel
//!    through the gun bank's own cadence.
//!
//! Every bound below is a **designed** constant of `crate::campaign::mission_ai`, not
//! an original measurement: the original's AI control law (`0x48c220`) is
//! unrecovered, so this suite judges the designed law against the task's own
//! acceptance wording ("close on the player … within N seconds", "stays
//! within a designed distance"), never against the original.

use avian3d::prelude::{Position, RigidBody};
use bevy::prelude::{App, Entity, Time, Vec3, With};
use bevy::time::TimeUpdateStrategy;

use cs_app::mission_actors::ScriptedAircraftActor;
use cs_app::mission_ai::{
    AI_STARTING_ROUNDS, MissionAiAirframes, MissionAiFireOrders, MissionAiReport, MissionAircraftAi,
};
use cs_app::mission_launch::plan_mission_launch;
use cs_app::mission_session::{
    MISSION_GUN_MOUNT_NOSE, MissionHost, MissionPlayerBody, MissionStage, build_headless,
    mint_host_generation, stage_for, teardown,
};
use cs_app::physics::BASELINE_FIXED_HZ;
use cs_app::playtest::scene::PlaytestOriginalFlight;
use cs_app::weapons::WeaponSession;
use cs_content::damage::DamageNodeKey;

use crate::campaign::common::label;
use crate::campaign::vs_m01_rt_window::{Scratch, game_dir, synthetic_stage};

/// How long the acceptance run drives the fixed loop: **N** in the task's
/// "close on the player … within N seconds".
const RUN_SECONDS: u64 = 45;

/// **Designed**: the engagement window an attacker must reach — the distance
/// at which the pursuit law considers its target engaged.
const ENGAGEMENT_RANGE_M: f32 = 1_000.0;

/// **Designed**: how far an attacker must close on the player at least once
/// during the run, measured from where it spawned.
const MINIMUM_CLOSE_M: f32 = 1_500.0;

/// **Designed**: the station distance Betty must end the run inside, measured
/// from her measured leader `devastator_2`. The station offset itself is
/// ~11 m (`crate::campaign::mission_ai::STATION_OFFSET_M`); the bound is looser than
/// that so a settled station passes, and tighter than the ~270 m the two
/// records spawn apart, so a law that does nothing fails.
const WINGMAN_DISTANCE_M: f32 = 250.0;

/// The production stage for retail M01.
fn retail_stage() -> MissionStage {
    let root = game_dir();
    let plan = plan_mission_launch(&root, label("M06"), "The Red Menace")
        .expect("M01's launch closure plans");
    stage_for(&root, &plan).expect("M01's stage reads through the production readers")
}

/// The live entity of every scripted aircraft, by measured record name.
fn aircraft_by_name(app: &mut App) -> Vec<(String, Entity)> {
    let world = app.world_mut();
    let mut names: Vec<(String, Entity)> = world
        .query::<(&ScriptedAircraftActor, Entity)>()
        .iter(world)
        .map(|(actor, entity)| (actor.record.name.clone(), entity))
        .collect();
    names.sort();
    names
}

/// The player body's entity — addressed through its own component, exactly
/// as the AI's fallback target rule finds it.
fn player_entity(app: &mut App) -> Entity {
    let world = app.world_mut();
    world
        .query_filtered::<Entity, With<MissionPlayerBody>>()
        .iter(world)
        .next()
        .expect("the composition spawns its player body")
}

fn position_of(app: &mut App, entity: Entity) -> Vec3 {
    app.world()
        .get::<Position>(entity)
        .map(|position| position.0)
        .expect("the body still carries a pose")
}

/// One sampled position by record name.
fn sampled(sample: &[(String, Vec3)], name: &str) -> Vec3 {
    sample
        .iter()
        .find(|(candidate, _)| candidate == name)
        .map(|(_, position)| *position)
        .unwrap_or_else(|| panic!("{name} was sampled with the roster"))
}

/// One sampled scalar by record name.
fn sampled_scalar(sample: &[(String, f32)], name: &str) -> f32 {
    sample
        .iter()
        .find(|(candidate, _)| candidate == name)
        .map_or(f32::INFINITY, |(_, value)| *value)
}

fn smallest(sample: &[(String, f32)]) -> f32 {
    sample
        .iter()
        .map(|(_, value)| *value)
        .fold(f32::INFINITY, f32::min)
}

/// **The stage imports an airframe for every scripted record's measured
/// name prefix, and the spawn attaches the law to every awake record.**
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_play_ai_the_stage_imports_an_airframe_for_every_scripted_record() {
    let stage = retail_stage();
    let airframes: &MissionAiAirframes = &stage.ai_airframes;
    assert!(
        airframes.refusals.is_empty(),
        "every M01 name prefix imports through the production reader: {:?}",
        airframes.refusals
    );
    let prefixes: Vec<&str> = airframes.models.keys().map(String::as_str).collect();
    assert_eq!(
        prefixes,
        ["bsfury", "devastator", "rusdevastator", "wingman"],
        "the four distinct prefixes the twelve records dispatch to, in name order"
    );
    for (prefix, airframe) in &airframes.models {
        assert_eq!(
            airframe.provenance, "OWNER-STATIC-2026-10-08",
            "{prefix} carries the recovered law's own provenance label"
        );
        assert!(
            airframe.model.airframe.veh_weight > 0.0,
            "{prefix} imported a usable weight"
        );
    }

    let mut app = build_headless(&stage).expect("M01's stage composes headlessly");
    app.update();

    let report = app.world().resource::<MissionAiReport>().clone();
    assert!(
        report.refusals.is_empty(),
        "the spawn reported no AI refusals: {:?}",
        report.refusals
    );
    let mut flown = report.flown.clone();
    flown.sort();
    assert_eq!(
        flown,
        [
            "devastator_2",
            "devastator_3",
            "rusdevastator_1",
            "rusdevastator_2",
            "rusdevastator_4",
            "rusdevastator_5",
            "wingman_2",
            "wingman_3",
        ],
        "every awake record with an imported airframe flies the designed law"
    );
    assert_eq!(
        report.held.len(),
        1,
        "exactly one record is held: {:?}",
        report.held
    );
    assert!(
        report.held[0].starts_with("bsfury_1"),
        "the held record is the one whose measured `deactivated` field sleeps: {}",
        report.held[0]
    );

    let world = app.world_mut();
    let mut behaviours: Vec<String> = world
        .query_filtered::<&MissionAircraftAi, With<PlaytestOriginalFlight>>()
        .iter(world)
        .map(|ai| format!("{:?}", ai.behaviour))
        .collect();
    behaviours.sort();
    assert_eq!(
        behaviours.len(),
        8,
        "eight records carry a behaviour beside their law: {behaviours:?}"
    );
    for (record, flight, _) in world
        .query::<(
            &ScriptedAircraftActor,
            &PlaytestOriginalFlight,
            &MissionAircraftAi,
        )>()
        .iter(world)
    {
        assert!(
            !flight.is_player,
            "{} flies the law's AI branch, never the player's",
            record.record.name
        );
        assert!(
            flight.state.fuel.is_finite(),
            "{} carries a finite fuel state",
            record.record.name
        );
    }
    assert!(
        world
            .query_filtered::<&RigidBody, With<PlaytestOriginalFlight>>()
            .iter(world)
            .any(|body| *body == RigidBody::Dynamic),
        "a flown scripted aircraft is a dynamic body the law's force can move"
    );
    assert!(
        !world
            .query_filtered::<&ScriptedAircraftActor, With<MissionAircraftAi>>()
            .iter(world)
            .any(|record| record.asleep),
        "a record the law flies is never one the measured field sleeps"
    );
    teardown(&mut app);
}

/// **In a headless run the attackers close on the player inside the designed
/// engagement window within `RUN_SECONDS`, and Betty stays within the
/// designed station distance of her measured leader.**
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_play_ai_hostiles_close_and_betty_holds_station_within_n_seconds() {
    let stage = retail_stage();
    let mut app = build_headless(&stage).expect("M01's stage composes headlessly");
    app.update();

    let player = player_entity(&mut app);
    let roster = aircraft_by_name(&mut app);
    let entity_of = |name: &str| {
        roster
            .iter()
            .find(|(candidate, _)| candidate == name)
            .map(|(_, entity)| *entity)
            .unwrap_or_else(|| panic!("{name} spawns with the roster"))
    };
    let attackers: Vec<(String, Entity)> = roster
        .iter()
        .filter(|(name, _)| name.starts_with("rusdevastator"))
        .cloned()
        .collect();
    assert_eq!(
        attackers.len(),
        4,
        "the four awake records of the other team attack: {attackers:?}"
    );
    let betty = entity_of("wingman_2");
    let leader = entity_of("devastator_2");

    let spawn_player = position_of(&mut app, player);
    let spawn_closest: Vec<(String, f32)> = attackers
        .iter()
        .map(|(name, entity)| {
            (
                name.clone(),
                spawn_player.distance(position_of(&mut app, *entity)),
            )
        })
        .collect();
    let spawn_station = position_of(&mut app, betty).distance(position_of(&mut app, leader));
    let betty_spawn = position_of(&mut app, betty);
    let mut closest = spawn_closest.clone();
    let mut station_min = f32::INFINITY;

    // One committed fixed tick per update, exactly as the radio suite drives
    // the mission host, so `RUN_SECONDS` is a tick count rather than a
    // frame-rate assumption.
    let timestep = app.world().resource::<Time<bevy::time::Fixed>>().timestep();
    let steps = RUN_SECONDS * u64::from(BASELINE_FIXED_HZ);
    let mut ticks = 0_u64;
    while ticks < steps {
        *app.world_mut().resource_mut::<TimeUpdateStrategy>() =
            TimeUpdateStrategy::ManualDuration(timestep);
        app.update();
        ticks += 1;
        if !ticks.is_multiple_of(12) {
            continue;
        }
        let player_position = position_of(&mut app, player);
        for (name, entity) in &attackers {
            let distance = player_position.distance(position_of(&mut app, *entity));
            if let Some(entry) = closest.iter_mut().find(|(candidate, _)| candidate == name) {
                entry.1 = entry.1.min(distance);
            }
        }
        let station = position_of(&mut app, betty).distance(position_of(&mut app, leader));
        station_min = station_min.min(station);
    }

    let report = app.world().resource::<MissionAiReport>().clone();
    assert!(
        report.decisions >= steps,
        "the fixed-tick decision drove every step of the run: {} decisions over {steps} ticks",
        report.decisions
    );
    assert_eq!(
        report.no_target, 0,
        "every attacker and the wingman kept a live target for the whole run"
    );

    // The mission armed every flown record with its own designed rig, and
    // the law asked to fire inside the designed window within N seconds.
    assert_eq!(
        report.armed.len(),
        8,
        "every flown record is armed through the mission's own weapon session: {:?}",
        report.armed
    );
    assert!(
        report.fire_orders > 0,
        "an attacker asks to fire inside the designed window within {RUN_SECONDS} s: {} orders",
        report.fire_orders
    );

    let finals: Vec<(String, Vec3)> = roster
        .iter()
        .map(|(name, entity)| (name.clone(), position_of(&mut app, *entity)))
        .collect();
    let player_final = position_of(&mut app, player);
    for (name, closest_run) in &closest {
        println!(
            "{name}: spawned {:.0} m from the player, closest {closest_run:.0} m over the run, \
             ends {:.0} m away, altitude {:.0} m",
            sampled_scalar(&spawn_closest, name),
            player_final.distance(sampled(&finals, name)),
            sampled(&finals, name).y
        );
    }
    let final_station = sampled(&finals, "wingman_2").distance(sampled(&finals, "devastator_2"));
    println!(
        "betty: spawn gap {spawn_station:.0} m, final {final_station:.0} m, \
         closest over the run {station_min:.0} m"
    );
    println!("player altitude {:.0} m", player_final.y);

    // The attackers closed on the player inside the designed engagement
    // window during the run — "close on the player within N seconds".
    let spawn_reach = smallest(&spawn_closest);
    let closest_approach = smallest(&closest);
    assert!(
        spawn_reach - closest_approach >= MINIMUM_CLOSE_M,
        "an attacker closes on the player within {RUN_SECONDS} s: from {spawn_reach:.0} m to \
         {closest_approach:.0} m (at least {MINIMUM_CLOSE_M} m of closure)"
    );
    assert!(
        closest_approach <= ENGAGEMENT_RANGE_M,
        "an attacker reaches the designed engagement window ({ENGAGEMENT_RANGE_M} m) within \
         {RUN_SECONDS} s: closest {closest_approach:.0} m"
    );
    for (name, closest_run) in &closest {
        let spawned = sampled_scalar(&spawn_closest, name);
        assert!(
            *closest_run < spawned,
            "{name} closes on the player rather than drifting away: {spawned:.0} m -> \
             {closest_run:.0} m"
        );
        let altitude = sampled(&finals, name).y;
        assert!(
            altitude > 50.0,
            "{name} is still airborne at the end of the run: {altitude:.0} m"
        );
    }

    // Betty holds her station: inside the designed distance, and tighter
    // than the gap her own record spawned with.
    assert!(
        final_station <= WINGMAN_DISTANCE_M,
        "Betty stays within {WINGMAN_DISTANCE_M} m of her measured leader: {final_station:.0} m \
         (she spawned {spawn_station:.0} m away, station offset ~89 m)"
    );
    assert!(
        final_station < spawn_station,
        "the station law converges rather than holding a lucky spawn: {spawn_station:.0} m -> \
         {final_station:.0} m"
    );
    assert!(
        sampled(&finals, "wingman_2").distance(betty_spawn) > 100.0,
        "Betty actually flew with her leader rather than staying parked"
    );

    // The ask reached the gun bank: at least one attacker's rounds actually
    // left the barrel through the session's own cadence, not just through
    // the AI's decision.
    let world = app.world();
    let session = world.resource::<WeaponSession>();
    let mount =
        DamageNodeKey::new(MISSION_GUN_MOUNT_NOSE).expect("the designed mount key is valid");
    let mut fired: Vec<String> = Vec::new();
    for (name, entity) in &attackers {
        let Some(script) = world.get::<ScriptedAircraftActor>(*entity) else {
            continue;
        };
        let Some(state) = session.state(&script.damage_actor) else {
            continue;
        };
        if state.ammunition(&mount) < AI_STARTING_ROUNDS {
            fired.push(name.clone());
        }
    }
    assert!(
        !fired.is_empty(),
        "at least one attacker's guns fire through the weapon session's own cadence: {fired:?}"
    );
    teardown(&mut app);
}

/// A stand-in actor id for the pending-order fixtures: the identity a fire
/// intent names, hand-built so the fixture never waits on a spawn.
fn pending_shooter() -> cs_sim::damage::ActorId {
    cs_sim::damage::ActorId {
        session: cs_types::net::SessionId::new(9).expect("session 9 is a valid id"),
        serial: 41,
    }
}

/// **Teardown leaves no pending AI fire orders behind.** The plugin
/// initialises [`MissionAiFireOrders`] only when it is missing, so a run
/// torn down between a decision and its publication must drop its asks
/// itself — a stale list would ride into the next composition's first tick
/// under the wrong session, naming actors that no longer exist. The
/// synthetic stage composes without an installation, so this runs in CI.
#[test]
fn accept_m01_play_ai_fire_orders_teardown_drops_the_pending_intents() {
    let scratch = Scratch::new("ai_orders");
    let stage = synthetic_stage(scratch.path());
    let mut app = build_headless(&stage).expect("the synthetic stage composes");
    app.update();
    {
        let world = app.world_mut();
        let mut orders = world.resource_mut::<MissionAiFireOrders>();
        orders.pending.push(pending_shooter());
        orders.sequence = 3;
    }

    teardown(&mut app);
    assert!(
        !app.world().contains_resource::<MissionAiFireOrders>(),
        "teardown must drop the pending fire orders with the run that decided them"
    );
}

/// **A restart resets the pending fire orders to the generation it
/// spawns.** The respawn replaces the AI report per generation; the asks go
/// with it, because the previous generation's actors no longer exist in the
/// fresh session and their intents would resolve to nothing. The synthetic
/// stage composes without an installation, so this runs in CI.
#[test]
fn accept_m01_play_ai_fire_orders_a_restart_resets_the_pending_intents() {
    let scratch = Scratch::new("ai_orders_restart");
    let stage = synthetic_stage(scratch.path());
    let mut app = build_headless(&stage).expect("the synthetic stage composes");
    app.update();
    {
        let world = app.world_mut();
        let mut orders = world.resource_mut::<MissionAiFireOrders>();
        orders.pending.push(pending_shooter());
        orders.sequence = 3;
    }

    let mut host = app
        .world_mut()
        .remove_resource::<MissionHost>()
        .expect("the composed app carries the host");
    let generation = mint_host_generation();
    host.restart(app.world_mut(), &stage, generation)
        .expect("the restart rebuilds the authored state");
    app.world_mut().insert_resource(host);

    let orders = app.world().resource::<MissionAiFireOrders>();
    assert!(
        orders.pending.is_empty(),
        "the respawned generation starts with no asks of the previous one: {:?}",
        orders.pending
    );
    assert_eq!(
        orders.sequence, 0,
        "the intent sequence restarts with the generation that spawns it"
    );
    teardown(&mut app);
}
