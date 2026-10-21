//! Acceptance for Rally #1811 `PARITY3-COCKPIT-VIEW.03`: the player's
//! `cockpit1` interior (everything but the gauges) is spawned under the body
//! and is `Visible` in the pilot-eye views, `Hidden` in the chase view
//! (finding `docs/findings/2026-10-21-parity3-cockpit-view-03-interior.md`).
//!
//! The first test drives the production spawn and visibility system on an
//! authored body (CI runs it); the second composes retail M01 and needs
//! `CS_GAME_DIR`.

use std::path::PathBuf;

use avian3d::prelude::{Position, Rotation};
use bevy::app::{App, Update};
use bevy::prelude::{ChildOf, Entity, Handle, Quat, Transform, Vec3, Visibility, With};
use cs_app::mission_launch::plan_mission_launch;
use cs_app::mission_session::{
    CockpitInteriorRoot, MissionCockpitInterior, MissionPlayerBody, MissionViewMode,
    build_headless, drive_cockpit_interior, spawn_cockpit_interior, stage_for,
};
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

fn root_visibility(app: &mut App) -> Visibility {
    let world = app.world_mut();
    *world
        .query_filtered::<&Visibility, With<CockpitInteriorRoot>>()
        .single(world)
        .expect("one interior root")
}

#[test]
fn accept_parity3_cockpit_view_03_interior_shows_in_the_cockpit_and_hides_in_chase() {
    let mut app = App::new();
    app.insert_resource(MissionViewMode::default());
    app.add_systems(Update, drive_cockpit_interior);
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
    app.insert_resource(MissionCockpitInterior::from_parts(
        vec![part(52, "canopy2"), part(54, "inside")],
        Quat::IDENTITY,
    ));
    spawn_cockpit_interior(app.world_mut(), body);

    let world = app.world_mut();
    let parts: Vec<Entity> = world
        .query_filtered::<Entity, With<AircraftPart>>()
        .iter(world)
        .collect();
    assert_eq!(parts.len(), 2, "one entity per interior part");
    let root = world
        .query_filtered::<Entity, With<CockpitInteriorRoot>>()
        .single(world)
        .expect("one root");
    assert_eq!(world.get::<ChildOf>(root).unwrap().parent(), body);
    for part in parts {
        assert_eq!(world.get::<ChildOf>(part).unwrap().parent(), root);
    }

    app.update();
    assert_eq!(root_visibility(&mut app), Visibility::Hidden, "chase");
    for mode in [MissionViewMode::Cockpit, MissionViewMode::SecondEye] {
        *app.world_mut().resource_mut::<MissionViewMode>() = mode;
        app.update();
        assert_eq!(root_visibility(&mut app), Visibility::Visible, "{mode:?}");
    }
    *app.world_mut().resource_mut::<MissionViewMode>() = MissionViewMode::Chase;
    app.update();
    assert_eq!(root_visibility(&mut app), Visibility::Hidden, "chase again");
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_parity3_cockpit_view_03_retail_m01_spawns_the_cockpit1_interior() {
    let root = PathBuf::from(std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR must be set"));
    let label = MissionLabel::new("M01").expect("label");
    let plan = plan_mission_launch(&root, label, "The Lost Treasure").expect("M01 plans");
    let stage = stage_for(&root, &plan).expect("M01's stage reads");
    let mut app = build_headless(&stage).expect("M01 composes");
    app.update();
    app.world_mut()
        .remove_resource::<cs_app::mission_session::LaunchCinematic>();

    let interior = app.world().resource::<MissionCockpitInterior>();
    let report = interior.report();
    let drawn: Vec<&str> = report.drawn.iter().map(|(_, n)| n.as_str()).collect();
    for wanted in [
        "canopy2",
        "g715",
        "inside",
        "outside",
        "l_aileron3",
        "r_aileron3",
    ] {
        assert!(drawn.contains(&wanted), "{wanted} drawn: {drawn:?}");
    }
    // The gauges are the dials task's; the damage decals are unmeasured.
    for left in ["g784", "compass", "pfhorizon", "bul1a", "pcdp4"] {
        assert!(!drawn.contains(&left), "{left} must not be drawn");
        assert!(
            report.undrawn.iter().any(|u| u.node_name == left),
            "{left} is reported undrawn with a reason"
        );
    }
    assert!(report.undrawn.iter().all(|u| !u.reason.is_empty()));
    assert!(
        report
            .glass
            .iter()
            .any(|(node, image)| node == "inside" && image.ends_with(":cockpit6")),
        "the gunsight glass is alpha-blended: {:?}",
        report.glass
    );
    let expected_parts = interior.parts().len();
    assert_eq!(expected_parts, drawn.len());

    let world = app.world_mut();
    let body = world
        .query_filtered::<Entity, With<MissionPlayerBody>>()
        .single(world)
        .expect("one player body");
    let interior_root = world
        .query_filtered::<Entity, With<CockpitInteriorRoot>>()
        .single(world)
        .expect("composing spawns the interior root");
    assert_eq!(world.get::<ChildOf>(interior_root).unwrap().parent(), body);
    let spawned = world
        .query::<(&AircraftPart, &ChildOf)>()
        .iter(world)
        .filter(|(_, child_of)| child_of.parent() == interior_root)
        .count();
    assert_eq!(spawned, expected_parts);

    app.update();
    assert_eq!(root_visibility(&mut app), Visibility::Hidden, "chase");
    *app.world_mut().resource_mut::<MissionViewMode>() = MissionViewMode::Cockpit;
    app.update();
    assert_eq!(root_visibility(&mut app), Visibility::Visible, "cockpit");
    *app.world_mut().resource_mut::<MissionViewMode>() = MissionViewMode::Chase;
    app.update();
    assert_eq!(root_visibility(&mut app), Visibility::Hidden, "chase again");
}
