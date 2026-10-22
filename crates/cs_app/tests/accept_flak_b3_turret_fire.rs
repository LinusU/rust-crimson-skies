//! Acceptance for Rally #1882 `FLAK-B3-TURRET-FIRE`: the woken turrets' rounds
//! in the weapon runtime (`mission_session::turret_fire`), over the FLAK-B1
//! wake and the FLAK-B2 update law. The retail leg composes M01, wakes the
//! trucks through OBJECTIVE6's own `WAKEUP_TURRETS`, parks the player in their
//! range and reads the shots and the hull pool back. Nothing here is
//! `verified_original`; the hull pool and the box it is hit in are designed.

use std::time::Duration;

use avian3d::prelude::{LinearVelocity, Position};
use bevy::prelude::{App, Entity, Time, Vec3, With};
use bevy::time::TimeUpdateStrategy;
use cs_app::mission_launch::plan_mission_launch;
use cs_app::mission_session::turret_fire::{MissionTurretFire, TurretWeaponRecord};
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
fn accept_flak_b3_retail_woken_trucks_shoot_the_player_and_the_island_guns_stay_silent() {
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

    // The record the trucks fire is the retail cannon, with its measured numbers.
    let Some(TurretWeaponRecord::Cannon(cannon)) = stage.host.turrets.weapons.get("wep_29") else {
        panic!("wep_29 reads as a cannon: {:?}", stage.host.turrets.weapons);
    };
    assert!(cannon.gun.velocity_mps > 0.0 && cannon.gun.range_m > 0.0);
    assert!(cannon.gun.health_damage > 0.0 && cannon.ammo_limit > 0);
    let Some(TurretWeaponRecord::Rocket(flak)) = stage.host.turrets.weapons.get("wep_27") else {
        panic!("wep_27 reads as a rocket");
    };
    assert_eq!(flak.velocity_mps, 850.0);
    assert_eq!(flak.detonation_distance_m, 20.0);

    let truck = stage
        .host
        .turrets
        .entry("t_truck01")
        .and_then(|entry| entry.position_m)
        .expect("t_truck01 has a composed position");
    let mut app = build_headless(&stage).expect("M01's stage composes headlessly");
    *app.world_mut().resource_mut::<TimeUpdateStrategy>() =
        TimeUpdateStrategy::ManualDuration(Duration::ZERO);
    app.update();

    // Asleep: nothing is built to fire before the wake, and nothing fires.
    let mut route: Vec<[f64; 3]> = stage
        .host
        .targets
        .declared_targets()
        .map(|target| target.position_m)
        .collect();
    route.push(truck);
    let mut woke = false;
    for step in 0..600 {
        step_host(&mut app, route[step % route.len()]);
        let ledger = app.world().resource::<MissionHost>().wake_turrets().clone();
        if ["t_truck01", "t_truck02", "t_truck03"]
            .iter()
            .all(|name| ledger.entry(name).is_some_and(|entry| entry.live))
        {
            woke = true;
            break;
        }
    }
    assert!(woke, "OBJECTIVE6's WAKEUP_TURRETS wakes the trucks");

    let timestep = app.world().resource::<Time<bevy::time::Fixed>>().timestep();
    let start = hull(&app).expect("the player's hull has a pool");
    // 100 m east and 40 m above the first truck: inside any truck's range.
    let near = [truck[0] + 100.0, truck[1] + 40.0, truck[2]];
    for _ in 0..900 {
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

    let fire = app.world().resource::<MissionTurretFire>();
    let report = fire.report();
    assert!(
        report.cannon_orders > 0 && !report.shots.is_empty(),
        "the woken trucks fired: {report:?}"
    );
    assert!(report.sight_queries > 0, "the player ray was asked");
    for shot in &report.shots {
        assert!(
            shot.name.starts_with("t_truck") && shot.weapon == "wep_29",
            "only the woken trucks fire, with wep_29: {shot:?}"
        );
    }
    let end = hull(&app).expect("the hull still has a pool");
    assert!(
        end < start,
        "a truck's round landed on the player through the damage authority: {start} -> {end}"
    );
    // The island guns stay asleep in M01 and never fire.
    assert!(report.rocket_bursts.is_empty() && report.rockets_fired == 0);
    for name in ["aagun01", "aagun02", "aagun30", "aagun31", "aagun32"] {
        assert!(
            !app.world()
                .resource::<MissionHost>()
                .wake_turrets()
                .entry(name)
                .expect("an entry")
                .live,
            "{name} stays asleep"
        );
    }
}
