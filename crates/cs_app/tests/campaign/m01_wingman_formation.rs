//! Acceptance suite M01-WINGMAN-FORMATION (#1550): Betty on the real M01.
//!
//! The real M01 is `ZBD/C3/M01` ("The Lost Treasure", #1565): its `aiv.zrd`
//! carries `wingman_1` — Betty — awake, on the player's team, with the stored
//! primary target `player`
//! (`docs/findings/2026-10-18-m01-wingman-player-leader.md` §1). Two seams
//! stand between that record and the windowed no-teleport smoke's
//! `wingmen_near_player` check:
//!
//! * **Spawn.** `wingman_1` dispatches `SpawnDispatch::WingmanOne`, which the
//!   original resolves through the player airframe row's `w` pointer
//!   (`0x426d60`): the Devastator row M01 flies points at the `wingman`
//!   `vehicle.zrd` record. The stage carries it on
//!   `MissionFlight::wingman_record` and the airframe table imports it, so
//!   Betty spawns flying her measured template instead of being refused.
//! * **Leader.** The follow law resolves a wingman's leader by record name;
//!   the fixed name `player` must bind the player body (the `0x41e78a`
//!   identity test). Once inside the measured engage window — under 700 m,
//!   over 20.576 m/s — she flies the player-leader slot `(6, 0, -18)` of the
//!   leader instead of the other-leader slot, and the run ends with her next
//!   to the player rather than hundreds of metres behind.
//!
//! Both members are retail (`#[ignore]`): the archive records and the
//! vehicle rows are the only source of the values they pin. The headless run
//! drives the same composition and fixed loop the windowed smoke runs, so the
//! distance it ends at is the distance the smoke's t+30 s sample judges.

use avian3d::prelude::{Position, RigidBody};
use bevy::prelude::{App, Entity, Vec3, With};
use bevy::time::{Time, TimeUpdateStrategy};

use cs_app::mission_actors::ScriptedAircraftActor;
use cs_app::mission_ai::{
    FOLLOW_ENGAGE_DIST2, MissionAiBehaviour, MissionAiReport, MissionAircraftAi,
};
use cs_app::mission_launch::plan_mission_launch;
use cs_app::mission_session::{
    MissionPlayerBody, MissionStage, build_headless, stage_for, teardown,
};
use cs_app::physics::BASELINE_FIXED_HZ;
use cs_app::playtest::scene::PlaytestOriginalFlight;

use crate::campaign::common::label;
use crate::campaign::vs_m01_rt_window::game_dir;

/// How long the acceptance run drives the fixed loop: the no-teleport
/// smoke's last wingman sample lands at t+30 s, so the headless run covers
/// it with room to settle.
const RUN_SECONDS: u64 = 45;

/// **Designed** bound mirroring the smoke's `MAX_WINGMAN_DISTANCE_M`
/// (`mission_session::smoke`, 500 m at t+30 s): looser than the ~19 m
/// player-leader slot, tighter than the ~117 m spawn gap — a wingman that
/// never engages fails both sides of it.
const WINGMAN_DISTANCE_M: f32 = 500.0;

/// The production stage for the real M01 — `ZBD/C3/M01`, "The Lost
/// Treasure".
fn retail_stage() -> MissionStage {
    let root = game_dir();
    let plan = plan_mission_launch(&root, label("M01"), "The Lost Treasure")
        .expect("M01's launch closure plans");
    stage_for(&root, &plan).expect("M01's stage reads through the production readers")
}

fn position_of(app: &mut App, entity: Entity) -> Vec3 {
    app.world()
        .get::<Position>(entity)
        .map(|position| position.0)
        .expect("the body still carries a pose")
}

/// **The `wingman_1` special dispatch resolves through the player airframe
/// row's `w` pointer, and Betty spawns flying it on the player's team.**
#[test]
#[ignore = "requires CS_GAME_DIR and CS_ENGINE_IMAGE"]
fn accept_m01_wingman_formation_wingman_1_flies_the_player_rows_wingman_record() {
    let stage = retail_stage();

    // The measured row: `player_pfighter` selects airframe row 5, whose `p`
    // is `pdevastator` and whose `w` is `wingman` (finding §1, `0x426d60`).
    assert_eq!(stage.flight.record, "pdevastator");
    assert_eq!(
        stage.flight.wingman_record, "wingman",
        "the WingmanOne dispatch resolves through the row's w pointer"
    );
    assert!(
        stage.ai_airframes.models.contains_key("wingman"),
        "the stage imports the resolved `wingman` record: {:?}",
        stage.ai_airframes.models.keys().collect::<Vec<_>>()
    );
    assert!(
        stage
            .ai_airframes
            .refusals
            .iter()
            .all(|refusal| !refusal.starts_with("wingman")),
        "no airframe refusal names the wingman: {:?}",
        stage.ai_airframes.refusals
    );

    let mut app = build_headless(&stage).expect("M01 composes headlessly");
    app.update();

    let report = app.world().resource::<MissionAiReport>();
    assert!(
        report.flown.iter().any(|name| name == "wingman_1"),
        "wingman_1 flies the designed law: {:?}",
        report.flown
    );
    assert!(
        report
            .refusals
            .iter()
            .all(|refusal| !refusal.starts_with("wingman_1")),
        "the spawn never refuses the resolved wingman: {:?}",
        report.refusals
    );

    let world = app.world_mut();
    let (actor, ai, flight, body) = world
        .query::<(
            &ScriptedAircraftActor,
            &MissionAircraftAi,
            &PlaytestOriginalFlight,
            &RigidBody,
        )>()
        .iter(world)
        .find(|(actor, ..)| actor.record.name == "wingman_1")
        .expect("wingman_1 spawns with the roster");
    assert_eq!(
        actor.vehicle_record, "wingman",
        "the spawned actor flies the resolved record, not a guessed one"
    );
    assert!(
        !actor.asleep,
        "C3/M01's wingman_1 stores starts_asleep 0 — she spawns awake"
    );
    assert_eq!(
        *body,
        RigidBody::Dynamic,
        "a flown wingman is a dynamic body the law's force moves"
    );
    assert_eq!(
        ai.behaviour,
        MissionAiBehaviour::Wingman {
            leader: "player".to_owned()
        },
        "the stored `player` primary target selects the wingman behaviour"
    );
    assert!(
        !flight.is_player,
        "a scripted wingman flies the law's AI branch, never the player's"
    );
    teardown(&mut app);
}

/// **In a headless run Betty resolves the player body as her leader, latches
/// the engage once she is moving, and ends the run beside the player —
/// inside the distance the no-teleport smoke judges.**
#[test]
#[ignore = "requires CS_GAME_DIR and CS_ENGINE_IMAGE"]
fn accept_m01_wingman_formation_betty_engages_and_closes_on_the_player() {
    let stage = retail_stage();
    let mut app = build_headless(&stage).expect("M01 composes headlessly");
    app.update();

    let player = {
        let world = app.world_mut();
        world
            .query_filtered::<Entity, With<MissionPlayerBody>>()
            .iter(world)
            .next()
            .expect("the composition spawns its player body")
    };
    let betty = {
        let world = app.world_mut();
        world
            .query::<(Entity, &ScriptedAircraftActor)>()
            .iter(world)
            .find(|(_, actor)| actor.record.name == "wingman_1")
            .map(|(entity, _)| entity)
            .expect("wingman_1 spawns with the roster")
    };

    let spawn_gap = position_of(&mut app, betty).distance(position_of(&mut app, player));
    let betty_spawn = position_of(&mut app, betty);
    assert!(
        spawn_gap * spawn_gap < FOLLOW_ENGAGE_DIST2,
        "Betty's measured spawn sits inside the 700 m engage window: {spawn_gap:.0} m"
    );

    // One committed fixed tick per update, exactly as the M01-PLAY-AI suite
    // drives the same composition.
    let timestep = app.world().resource::<Time<bevy::time::Fixed>>().timestep();
    let steps = RUN_SECONDS * u64::from(BASELINE_FIXED_HZ);
    let mut engaged_at = None;
    let mut ticks = 0_u64;
    while ticks < steps {
        *app.world_mut().resource_mut::<TimeUpdateStrategy>() =
            TimeUpdateStrategy::ManualDuration(timestep);
        app.update();
        ticks += 1;
        if engaged_at.is_none()
            && app
                .world()
                .get::<MissionAircraftAi>(betty)
                .is_some_and(|ai| ai.follow_engaged)
        {
            engaged_at = Some(ticks);
        }
    }

    let report = app.world().resource::<MissionAiReport>();
    assert!(
        report.decisions >= steps,
        "the fixed-tick decision drove every step of the run: {} over {steps}",
        report.decisions
    );

    let ai = app
        .world()
        .get::<MissionAircraftAi>(betty)
        .expect("Betty's law");
    assert_eq!(
        ai.leader,
        Some(player),
        "the stored `player` target resolves to the player body"
    );
    let engaged_at = engaged_at
        .unwrap_or_else(|| panic!("Betty engages once inside the window and moving (>20.576 m/s)"));
    println!("betty: engaged at tick {engaged_at}");

    let player_final = position_of(&mut app, player);
    let betty_final = position_of(&mut app, betty);
    let final_gap = betty_final.distance(player_final);
    println!(
        "betty: spawn gap {spawn_gap:.0} m, final {final_gap:.0} m, \
         travelled {:.0} m",
        betty_final.distance(betty_spawn)
    );
    assert!(
        final_gap <= WINGMAN_DISTANCE_M,
        "Betty ends inside the smoke's {WINGMAN_DISTANCE_M:.0} m threshold: {final_gap:.0} m \
         (spawned {spawn_gap:.0} m out)"
    );
    assert!(
        final_gap < spawn_gap,
        "the follow law converges rather than holding the spawn: {spawn_gap:.0} -> \
         {final_gap:.0} m"
    );
    assert!(
        betty_final.distance(betty_spawn) > 100.0,
        "Betty actually flew with the player rather than staying parked"
    );
    teardown(&mut app);
}
