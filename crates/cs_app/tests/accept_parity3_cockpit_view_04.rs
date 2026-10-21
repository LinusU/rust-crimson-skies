//! Acceptance for Rally #1812 `PARITY3-COCKPIT-VIEW.04`: the cockpit panel's
//! `gauges` subtree is spawned under the player body at its stored dash
//! transforms — the six dials (ROCKETS, horizon, GUNS, MPH, damage, ALT),
//! the `pfhorizon` attitude ball and the compass strip — tagged with the
//! same live markers the chase-view cluster carries, so the one
//! `update_hud_art` drive serves both. The corner cluster's camera stands
//! down while a pilot-eye view is up
//! (finding `docs/findings/2026-10-21-parity3-cockpit-view-04-dials.md`).
//!
//! The first test drives the production spawn, visibility and drive systems
//! on authored parts (CI runs it); the second composes retail M01 and needs
//! `CS_GAME_DIR`.

use std::path::PathBuf;

use avian3d::prelude::{Position, Rotation};
use bevy::app::{App, Update};
use bevy::ecs::system::RunSystemOnce;
use bevy::prelude::{
    Assets, Camera, ChildOf, Entity, Handle, Quat, StandardMaterial, Transform, Vec3, Visibility,
    With,
};
use cs_app::mission_launch::plan_mission_launch;
use cs_app::mission_session::{
    CockpitDialsRoot, HudArtCamera, HudAttitudeBall, HudCompassTape, HudNeedle, HudPip,
    MissionCockpitDials, MissionHudArt, MissionPlayerBody, MissionViewMode, NeedleRole, PipGauge,
    build_headless, drive_cockpit_dials, fit_hud_viewport, spawn_cockpit_dials, stage_for,
    update_hud_art,
};
use cs_app::playtest::{PlaytestState, Telemetry};
use cs_app::playtest_retail::{AircraftPart, AircraftPartAsset};
use cs_content::campaign_bindings::MissionLabel;

fn part(slot: u32, name: &str) -> AircraftPartAsset {
    AircraftPartAsset {
        node_slot: slot,
        node_name: name.to_owned(),
        mesh: Handle::default(),
        pieces: Vec::new(),
        local: Transform::from_xyz(0.0, slot as f32, 0.0),
    }
}

fn dials_root_visibility(app: &mut App) -> Visibility {
    let world = app.world_mut();
    *world
        .query_filtered::<&Visibility, With<CockpitDialsRoot>>()
        .single(world)
        .expect("one dials root")
}

/// The authored parts spawn under one root on the body, carry the shared
/// live markers, answer the pilot-eye visibility drive and move with the
/// one `update_hud_art` drive — so removing any of the spawn, the view
/// drive or the shared drive fails this test.
#[test]
fn accept_parity3_cockpit_view_04_dials_reach_the_pilot_eye_and_answer_the_shared_drive() {
    let mut app = App::new();
    app.insert_resource(MissionViewMode::default());
    app.init_resource::<PlaytestState>();
    app.insert_resource(MissionHudArt::default());
    app.world_mut()
        .insert_resource(Assets::<StandardMaterial>::default());
    app.add_systems(Update, (drive_cockpit_dials, update_hud_art));
    let body = app
        .world_mut()
        .spawn((
            MissionPlayerBody,
            Position(Vec3::ZERO),
            Rotation(Quat::IDENTITY),
            // The cockpit camera hides the body in the pilot-eye views.
            Visibility::Hidden,
        ))
        .id();
    app.insert_resource(MissionCockpitDials::from_parts(
        vec![
            // The measured `gauges` slots: the speed needle, the attitude
            // ball, the compass tape, one rocket pip.
            part(75, "speed"),
            part(61, "pfhorizon"),
            part(65, "compass"),
            part(88, "mgindicator0"),
        ],
        Quat::IDENTITY,
    ));
    spawn_cockpit_dials(app.world_mut(), body);

    let world = app.world_mut();
    let parts: Vec<Entity> = world
        .query_filtered::<Entity, With<AircraftPart>>()
        .iter(world)
        .collect();
    assert_eq!(parts.len(), 4, "one entity per dial part");
    let root = world
        .query_filtered::<Entity, With<CockpitDialsRoot>>()
        .single(world)
        .expect("one dials root");
    assert_eq!(world.get::<ChildOf>(root).unwrap().parent(), body);
    for part in &parts {
        assert_eq!(world.get::<ChildOf>(*part).unwrap().parent(), root);
        assert!(
            world.get::<HudNeedle>(*part).is_some()
                || world.get::<HudAttitudeBall>(*part).is_some()
                || world.get::<HudCompassTape>(*part).is_some()
                || world.get::<HudPip>(*part).is_some(),
            "part {part:?} carries no live marker"
        );
    }

    // The view drive: hidden in the chase view, shown at the pilot eye.
    app.update();
    assert_eq!(dials_root_visibility(&mut app), Visibility::Hidden, "chase");
    for mode in [MissionViewMode::Cockpit, MissionViewMode::SecondEye] {
        *app.world_mut().resource_mut::<MissionViewMode>() = mode;
        app.update();
        assert_eq!(
            dials_root_visibility(&mut app),
            Visibility::Visible,
            "{mode:?}"
        );
    }
    *app.world_mut().resource_mut::<MissionViewMode>() = MissionViewMode::Chase;
    app.update();
    assert_eq!(
        dials_root_visibility(&mut app),
        Visibility::Hidden,
        "chase again"
    );

    // The shared drive: the same `update_hud_art` that drives the corner
    // cluster moves the panel's needle, ball and tape from the telemetry.
    app.world_mut().resource_mut::<PlaytestState>().telemetry = Telemetry {
        speed_m_s: 100.0,
        altitude_m: 1_524.0, // 5 000 ft
        heading_deg: 90.0,
        pitch_deg: 20.0,
        roll_deg: -30.0,
        ..Telemetry::default()
    };
    app.update();

    let world = app.world_mut();
    {
        let mut needles = world.query::<(&Transform, &HudNeedle)>();
        let (transform, needle) = needles.single(world).expect("the speed needle");
        assert_eq!(needle.role, NeedleRole::Speed);
        // The engine-measured rate: -0.02811017 rad per m/s.
        let expected = needle.base * Quat::from_rotation_z(100.0 * -0.028_110_17);
        let diff = transform.rotation.dot(expected).abs();
        assert!(
            (diff - 1.0).abs() < 1e-5,
            "needle rotation {:?} vs {expected:?}",
            transform.rotation
        );
    }
    {
        let mut balls = world.query::<(&Transform, &HudAttitudeBall)>();
        let (transform, ball) = balls.single(world).expect("the attitude ball");
        // The designed counter-rotation: -roll about z, -pitch about x.
        let expected = ball.base
            * Quat::from_rotation_z(30_f32.to_radians())
            * Quat::from_rotation_x(-20_f32.to_radians());
        let diff = transform.rotation.dot(expected).abs();
        assert!(
            (diff - 1.0).abs() < 1e-5,
            "ball rotation {:?} vs {expected:?}",
            transform.rotation
        );
    }
    {
        let mut tapes = world.query::<(&Transform, &HudCompassTape)>();
        let (transform, tape) = tapes.single(world).expect("the compass tape");
        // Heading 90 scrolls a quarter of the designed units-per-turn.
        let expected = tape.base_x - 0.25 * tape.period;
        assert!(
            (transform.translation.x - expected).abs() < 1e-5,
            "tape x {} vs {expected}",
            transform.translation.x
        );
    }
}

/// Retail M01: the composed session draws every mesh of the `gauges`
/// subtree at its stored dash transform under the body, tags the live
/// elements, shows the cluster at the pilot eye only, and stands the
/// corner cluster's camera down there.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_parity3_cockpit_view_04_retail_m01_draws_the_panel_dials() {
    let root = PathBuf::from(std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR must be set"));
    let label = MissionLabel::new("M01").expect("label");
    let plan = plan_mission_launch(&root, label, "The Lost Treasure").expect("M01 plans");
    let stage = stage_for(&root, &plan).expect("M01's stage reads");
    let mut app = build_headless(&stage).expect("M01 composes");
    app.update();
    app.world_mut()
        .remove_resource::<cs_app::mission_session::LaunchCinematic>();

    // The build report: every mesh-bearing node of the subtree drew.
    let (drawn, undrawn, unknowns) = {
        let dials = app.world().resource::<MissionCockpitDials>();
        let report = dials.report();
        (
            report
                .drawn
                .iter()
                .map(|(_, name)| name.clone())
                .collect::<Vec<String>>(),
            report.undrawn.clone(),
            report.unknowns.clone(),
        )
    };
    for wanted in [
        "horiz",
        "pfhorizon",
        "compass",
        "g730",
        "hundreds",
        "thousands",
        "lowalt_on",
        "speed",
        "stallwarning_on",
        "g843",
        "rightwingdamage",
        "leftwingdamage",
        "taildamage",
        "nosedamage",
        "4char_ammo",
        "6char_type",
        "mgarrow",
        "ggarrow",
        "g819",
        "g815",
        "nitro_boost",
        "nitro_backplate",
        "nitro_charge",
    ] {
        assert!(
            drawn.contains(&wanted.to_owned()),
            "{wanted} drawn: {drawn:?}"
        );
    }
    for index in 0..8 {
        assert!(
            drawn.contains(&format!("mgindicator{index}")),
            "mgindicator{index} drawn"
        );
    }
    for index in 0..4 {
        assert!(
            drawn.contains(&format!("ggindicator{index}")),
            "ggindicator{index} drawn"
        );
    }
    assert_eq!(drawn.len(), 39, "every mesh binding drew: {drawn:?}");
    assert!(
        undrawn.is_empty(),
        "no mesh binding may be left undrawn: {undrawn:?}"
    );
    assert!(
        unknowns.iter().any(|unknown| unknown.contains("pfhorizon")),
        "the designed attitude-ball drive is recorded unknown: {unknowns:?}"
    );

    // The spawned tree: the six-dial cluster under one root on the body,
    // each part at its stored dash transform — the `speed` needle's
    // measured (−2.0, −4.931, −10.316) seat is ~10 units aft of the `player`
    // root and on the raked panel (rot ~20 deg about x), not at an
    // `instruments.zrd` HUD placement.
    let world = app.world_mut();
    let body = world
        .query_filtered::<Entity, With<MissionPlayerBody>>()
        .single(world)
        .expect("one player body");
    let dials_root = world
        .query_filtered::<Entity, With<CockpitDialsRoot>>()
        .single(world)
        .expect("composing spawns the dials root");
    assert_eq!(world.get::<ChildOf>(dials_root).unwrap().parent(), body);
    let dial_parts: Vec<(Entity, u32)> = world
        .query::<(Entity, &AircraftPart, &ChildOf)>()
        .iter(world)
        .filter(|(_, _, child_of)| child_of.parent() == dials_root)
        .map(|(entity, part, _)| (entity, part.node_slot))
        .collect();
    assert_eq!(dial_parts.len(), drawn.len(), "one entity per drawn part");
    {
        let (speed, _) = dial_parts
            .iter()
            .find(|(_, slot)| *slot == 75)
            .expect("the speed needle part");
        let transform = world.get::<Transform>(*speed).unwrap();
        let seat = Vec3::new(-2.0, -4.931, -10.316);
        assert!(
            transform.translation.distance(seat) < 0.01,
            "the speed needle seats at the stored dash transform {:?}, not {seat}",
            transform.translation
        );
    }

    // The live markers the shared drive answers hang on the dial parts
    // (distinct entities from the corner cluster's copies).
    let mut needle_roles = Vec::new();
    let (mut balls, mut tapes, mut pips) = (0usize, 0usize, 0usize);
    for (entity, _) in &dial_parts {
        if let Some(needle) = world.get::<HudNeedle>(*entity) {
            needle_roles.push(needle.role);
        }
        balls += usize::from(world.get::<HudAttitudeBall>(*entity).is_some());
        tapes += usize::from(world.get::<HudCompassTape>(*entity).is_some());
        pips += usize::from(world.get::<HudPip>(*entity).is_some());
    }
    for role in [
        NeedleRole::Speed,
        NeedleRole::AltHundreds,
        NeedleRole::AltThousands,
    ] {
        assert!(needle_roles.contains(&role), "panel needle {role:?} tagged");
    }
    assert_eq!(balls, 1, "the pfhorizon attitude ball is tagged");
    assert_eq!(tapes, 1, "the compass tape is tagged");
    assert_eq!(pips, 12, "8 rocket + 4 gun pips are tagged");

    // The view drive the composition installed: the cluster is hidden in
    // the chase view and shown at the pilot eye.
    app.update();
    assert_eq!(dials_root_visibility(&mut app), Visibility::Hidden, "chase");
    *app.world_mut().resource_mut::<MissionViewMode>() = MissionViewMode::Cockpit;
    app.update();
    assert_eq!(
        dials_root_visibility(&mut app),
        Visibility::Visible,
        "cockpit"
    );

    // The corner cluster's camera stands down at the pilot eye and comes
    // back in the chase view (the text layer's SafeArea is windowed-only;
    // the smoke task covers it on screen).
    let world = app.world_mut();
    world
        .run_system_once(fit_hud_viewport)
        .expect("the viewport fit runs");
    {
        let mut cameras = world.query_filtered::<&Camera, With<HudArtCamera>>();
        let camera = cameras.single(world).expect("one HUD camera");
        assert!(
            !camera.is_active,
            "the corner cluster stands down in cockpit"
        );
    }
    *app.world_mut().resource_mut::<MissionViewMode>() = MissionViewMode::Chase;
    let world = app.world_mut();
    world
        .run_system_once(fit_hud_viewport)
        .expect("the viewport fit runs");
    {
        let mut cameras = world.query_filtered::<&Camera, With<HudArtCamera>>();
        let camera = cameras.single(world).expect("one HUD camera");
        assert!(camera.is_active, "the corner cluster returns in chase");
    }

    // The shared drive on the real session authorities: the panel's own
    // needle moves with telemetry and the pips answer the loaded ammo.
    let before: Vec<(u32, Quat)> = {
        let world = app.world_mut();
        dial_parts
            .iter()
            .filter(|(entity, _)| world.get::<HudNeedle>(*entity).is_some())
            .map(|(entity, slot)| (*slot, world.get::<Transform>(*entity).unwrap().rotation))
            .collect()
    };
    app.world_mut().resource_mut::<PlaytestState>().telemetry = Telemetry {
        speed_m_s: 140.0,
        altitude_m: 914.4, // 3 000 ft
        heading_deg: 180.0,
        pitch_deg: 15.0,
        roll_deg: 25.0,
        ..Telemetry::default()
    };
    app.world_mut()
        .run_system_once(update_hud_art)
        .expect("the shared drive runs");
    {
        let world = app.world_mut();
        let after: Vec<(u32, Quat)> = dial_parts
            .iter()
            .filter(|(entity, _)| world.get::<HudNeedle>(*entity).is_some())
            .map(|(entity, _)| {
                (
                    world.get::<AircraftPart>(*entity).unwrap().node_slot,
                    world.get::<Transform>(*entity).unwrap().rotation,
                )
            })
            .collect();
        for (slot, rotation) in &after {
            let base = before
                .iter()
                .find(|(s, _)| s == slot)
                .map(|(_, r)| *r)
                .unwrap_or(Quat::IDENTITY);
            assert_ne!(
                *rotation, base,
                "panel needle slot {slot} did not answer the telemetry drive"
            );
        }
        assert!(!after.is_empty(), "the panel carried needles");
        // M01's player starts armed: the first pip of each gauge is lit
        // (remaining > 0 drives it Visible; Inherited under the cluster).
        let mut lit = (0usize, 0usize);
        let mut pips = world.query::<(&HudPip, &Visibility)>();
        for (pip, visibility) in pips.iter(world) {
            if matches!(visibility, Visibility::Inherited | Visibility::Visible) {
                match pip.gauge {
                    PipGauge::Missile => lit.0 += 1,
                    PipGauge::Gun => lit.1 += 1,
                }
            }
        }
        assert!(
            lit.0 > 0 && lit.1 > 0,
            "M01's loaded ammo lights pips on both gauges: {lit:?}"
        );
    }

    cs_app::mission_session::teardown(&mut app);
}
