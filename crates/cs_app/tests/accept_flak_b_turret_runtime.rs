//! Acceptance for Rally #1820 `FLAK-B-TURRET-RUNTIME`, the integration of
//! FLAK-B1 (live byte and wake), B2 (update law) and B3 (fire): in M01 the
//! trucks are silent while asleep, OBJECTIVE6 wakes them, and every one of
//! `t_truck01..03` then shoots the player. Nothing here is `verified_original`.

use std::time::Duration;

use avian3d::prelude::{LinearVelocity, Position};
use bevy::prelude::{App, Entity, Time, Vec3, With};
use bevy::time::TimeUpdateStrategy;
use cs_app::mission_launch::plan_mission_launch;
use cs_app::mission_session::turret_fire::MissionTurretFire;
use cs_app::mission_session::{
    MISSION_TARGET_NODE, MissionHost, MissionPlayerBody, PlayerVictim, build_headless,
    mission_world_observation, stage_for,
};
use cs_app::physics::PhysicsTickLedger;
use cs_app::weapons::SessionDamageResolver;
use cs_content::campaign_bindings::MissionLabel;
use cs_sim::damage::DamageNodeKey;

fn step_host(app: &mut App, at: [f64; 3]) {
    let world = app.world_mut();
    let body = world
        .query_filtered::<Entity, With<MissionPlayerBody>>()
        .single(world)
        .expect("one player body");
    world.get_mut::<Position>(body).expect("a pose").0 =
        Vec3::new(at[0] as f32, at[1] as f32, at[2] as f32);
    mission_world_observation(world);
    world.resource_mut::<PhysicsTickLedger>().ticks += 1;
    let mut host = world
        .remove_resource::<MissionHost>()
        .expect("the composed app carries the host");
    let produced = host
        .step(world, Duration::from_secs(1))
        .expect("the driven step answers");
    host.settle(world, &produced);
    world.insert_resource(host);
}

fn hull(app: &App) -> Option<f64> {
    let victim = app.world().get_resource::<PlayerVictim>()?;
    let key = DamageNodeKey::new(MISSION_TARGET_NODE).unwrap();
    app.world()
        .resource::<SessionDamageResolver>()
        .resolver()
        .remaining_integrity(&victim.actor, &key)
}

#[test]
#[ignore = "requires CS_GAME_DIR and CS_ENGINE_IMAGE"]
fn accept_flak_b_retail_objective6_wakes_every_truck_and_each_one_shoots_the_player() {
    let root = std::path::PathBuf::from(
        std::env::var_os("CS_GAME_DIR").expect("CS_GAME_DIR must be set for the retail leg"),
    );
    let plan = plan_mission_launch(
        &root,
        MissionLabel::new("M01").expect("a mission label"),
        "The Lost Treasure",
    )
    .expect("M01's launch closure plans");
    let stage = stage_for(&root, &plan).expect("M01's stage reads");
    let names = ["t_truck01", "t_truck02", "t_truck03"];
    let positions: Vec<[f64; 3]> = names
        .iter()
        .map(|name| {
            stage
                .host
                .turrets
                .entry(name)
                .and_then(|entry| entry.position_m)
                .unwrap_or_else(|| panic!("{name} has a composed position"))
        })
        .collect();
    for name in names {
        assert!(
            !stage.host.turrets.entry(name).expect("an entry").live,
            "{name} starts asleep"
        );
    }

    let mut app = build_headless(&stage).expect("M01's stage composes headlessly");
    *app.world_mut().resource_mut::<TimeUpdateStrategy>() =
        TimeUpdateStrategy::ManualDuration(Duration::ZERO);
    app.update();

    // Drive the mission along its own targets until the wake lands.
    let mut route: Vec<[f64; 3]> = stage
        .host
        .targets
        .declared_targets()
        .map(|target| target.position_m)
        .collect();
    route.push(positions[0]);
    let mut woke = false;
    for step in 0..600 {
        step_host(&mut app, route[step % route.len()]);
        let ledger = app.world().resource::<MissionHost>().wake_turrets().clone();
        let live: Vec<bool> = names
            .iter()
            .map(|name| ledger.entry(name).is_some_and(|entry| entry.live))
            .collect();
        // They wake together, never one by one.
        assert!(
            live.iter().all(|l| *l) || live.iter().all(|l| !*l),
            "the trucks wake together: {live:?}"
        );
        if live[0] {
            woke = true;
            break;
        }
    }
    assert!(woke, "OBJECTIVE6's WAKEUP_TURRETS wakes the trucks");
    assert_eq!(
        app.world()
            .get_resource::<MissionTurretFire>()
            .map_or(0, |fire| fire.report().cannon_orders),
        0,
        "nothing fired before the player was in range"
    );

    // Park the player 100 m east and 40 m above the middle truck: inside every
    // truck's range and below the 60-degree pitch limit of each (straight
    // overhead is out of the barrel's travel).
    let near = [
        positions[1][0] + 100.0,
        positions[1][1] + 40.0,
        positions[1][2],
    ];
    let timestep = app.world().resource::<Time<bevy::time::Fixed>>().timestep();
    let start = hull(&app).expect("the player's hull has a pool");
    for _ in 0..1800 {
        {
            let world = app.world_mut();
            let body = world
                .query_filtered::<Entity, With<MissionPlayerBody>>()
                .single(world)
                .expect("one player body");
            world.get_mut::<Position>(body).expect("a pose").0 =
                Vec3::new(near[0] as f32, near[1] as f32, near[2] as f32);
            world.get_mut::<LinearVelocity>(body).expect("a velocity").0 = Vec3::ZERO;
        }
        *app.world_mut().resource_mut::<TimeUpdateStrategy>() =
            TimeUpdateStrategy::ManualDuration(timestep);
        app.update();
    }

    let report = app.world().resource::<MissionTurretFire>().report();
    assert!(
        report.cannon_orders > 0,
        "the woken trucks fired: {report:?}"
    );
    for shot in &report.shots {
        assert!(
            shot.weapon == "wep_29" && names.contains(&shot.name.as_str()),
            "only the woken trucks fire, with wep_29: {shot:?}"
        );
    }
    let firing: Vec<&str> = names
        .iter()
        .copied()
        .filter(|name| report.shots.iter().any(|shot| shot.name == *name))
        .collect();
    assert_eq!(firing, names, "each truck shoots: {report:?}");
    assert!(report.sight_queries > 0, "the player ray was asked");
    let end = hull(&app).expect("the hull still has a pool");
    assert!(
        end < start,
        "the rounds landed on the player: {start} -> {end}"
    );
    assert!(report.rocket_bursts.is_empty() && report.rockets_fired == 0);
    // The island guns' own data keeps them asleep, not just out of range.
    let ledger = app.world().resource::<MissionHost>().wake_turrets().clone();
    for name in ["aagun01", "aagun02", "aagun30", "aagun31", "aagun32"] {
        assert!(
            !ledger.entry(name).expect("an entry").live,
            "{name} stays asleep in M01"
        );
    }
}
