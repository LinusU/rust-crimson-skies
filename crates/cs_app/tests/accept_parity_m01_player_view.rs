//! Acceptance PARITY-M01-PLAYER-VIEW (#1644): the three merged slices as one
//! player view on the production M01 composition. Task test prefix:
//! `accept_parity_m01_player_view_` — the integration selection also sweeps
//! up the slices' own `accept_parity_m01_player_view_{a,b,c}_` tests.
//!
//! The slices' tests prove each mechanism on its own. This test composes M01
//! the way the owner-command smoke and the `cs_xtask parity` comparison frame
//! it — the capture-fitted chase camera (#1666), the measured `prop1` blur
//! disc (#1667) and the measured `wing_flare` quads gated on the damage
//! record (#1668), all live on the same body at the same frame — which is
//! what `flight_25s`/`flight_70s` show against `orig25.png`/`orig70.png`.

use std::path::PathBuf;

use avian3d::prelude::{Position, Rotation};
use bevy::prelude::{ChildOf, Entity, Vec3, Visibility, With};
use cs_app::airframe_damage::{
    AirframeDamageRegion, RegionDamageBinding, RegionDamagePresentation, WingFlare,
};
use cs_app::camera::chase_preset::{CHASE_ABOVE_M, CHASE_ASTERN_M};
use cs_app::mission_launch::plan_mission_launch;
use cs_app::mission_session::{
    MISSION_PLAYER_BLUR_NODE_NAME, MISSION_PLAYER_BLUR_NODE_SLOT, MissionPlayerBody,
    MissionPlayerVisual, MissionWeapons, build_headless, stage_for, teardown,
};
use cs_app::playtest::PlaytestCamera;
use cs_app::playtest::propeller::PropellerBlur;
use cs_content::campaign_bindings::MissionLabel;
use cs_sim::damage::PartState;

/// **M01's composed player view is the slices at once.** The chase rig
/// resolves the eye at the capture-fitted offset from the one player body;
/// the `prop1` blur disc hangs off that body and is drawn while its engine
/// runs; both measured `wing_flare` quads bind the player actor's own wing
/// damage nodes and, with no damage graph published yet (#1699), read the
/// honest `Unknown` that keeps them `Hidden` — the intact presentation
/// `orig25.png` shows.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_parity_m01_player_view_retail_m01_composes_the_integrated_player_view() {
    let root = PathBuf::from(std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR must be set"));
    let label = MissionLabel::new("M01").expect("label");
    let plan = plan_mission_launch(&root, label, "The Lost Treasure").expect("M01 plans");
    let stage = stage_for(&root, &plan).expect("M01's stage reads");
    let mut app = build_headless(&stage).expect("M01 composes");
    app.update();

    // The player body and the pose the chase rig resolved against.
    let (body, position, rotation) = {
        let world = app.world_mut();
        world
            .query_filtered::<(Entity, &Position, &Rotation), With<MissionPlayerBody>>()
            .single(world)
            .map(|(entity, position, rotation)| (entity, position.0, rotation.0))
            .expect("one player body")
    };
    let eye = app
        .world()
        .resource::<PlaytestCamera>()
        .frame
        .as_ref()
        .map(|frame| frame.pose.position())
        .expect("the chase rig resolved a frame");

    // The first resolve teleports the rig onto the declared offset, so the
    // eye is the body's own frame applied to (0, up, astern) — no smoothing
    // lag is in play yet.
    let expected =
        position + rotation * Vec3::new(0.0, CHASE_ABOVE_M as f32, CHASE_ASTERN_M as f32);
    let actual = Vec3::new(eye.x() as f32, eye.y() as f32, eye.z() as f32);
    assert!(
        actual.distance(expected) < 0.01,
        "the chase eye sits at the capture-fitted offset: expected {expected}, got {actual}"
    );
    let body_delta = rotation.inverse() * (actual - position);
    assert!(
        body_delta.z > 0.0 && body_delta.y > 0.0,
        "the eye is astern and above, not alongside: {body_delta}"
    );

    // The measured blur disc on the same body, drawn while the engine runs.
    let visual = app
        .world()
        .get_resource::<MissionPlayerVisual>()
        .expect("the player visual");
    let blur = visual.blur().expect("the blur disc is drawn");
    assert_eq!(blur.node_slot, MISSION_PLAYER_BLUR_NODE_SLOT);
    assert_eq!(blur.node_name, MISSION_PLAYER_BLUR_NODE_NAME);
    let world = app.world_mut();
    let discs: Vec<Entity> = world
        .query_filtered::<(Entity, &ChildOf), With<PropellerBlur>>()
        .iter(world)
        .filter(|(_, child)| child.parent() == body)
        .map(|(entity, _)| entity)
        .collect();
    assert_eq!(discs.len(), 1, "exactly one blur disc on the player body");
    let disc = discs[0];
    // The visibility driver has run its first pass by the second frame.
    app.update();
    assert_eq!(
        *app.world().get::<Visibility>(disc).expect("a visibility"),
        Visibility::Inherited,
        "the mission starts with the engine running, so the disc shows"
    );

    // The measured wing fire quads on the same body, bound to the player
    // actor's own damage nodes, quiescent while nothing is damaged.
    let player = app.world().resource::<MissionWeapons>().player();
    let world = app.world_mut();
    let mut flares: Vec<(
        AirframeDamageRegion,
        cs_sim::damage::DamageNodeKey,
        PartState,
        Visibility,
    )> = world
        .query_filtered::<(
            &WingFlare,
            &RegionDamageBinding,
            &RegionDamagePresentation,
            &Visibility,
            &ChildOf,
        ), With<WingFlare>>()
        .iter(world)
        .filter(|(_, binding, _, _, child)| child.parent() == body && binding.actor == player)
        .map(|(flare, binding, record, visibility, _)| {
            (
                flare.region,
                binding.node.clone(),
                record.state,
                *visibility,
            )
        })
        .collect();
    flares.sort_by_key(|flare| flare.0);
    assert_eq!(
        flares.len(),
        2,
        "the two measured fire quads on the player body"
    );
    for (region, node, state, visibility) in &flares {
        assert_eq!(*node, region_key(region), "the {region:?} quad's own node");
        assert_eq!(
            *state,
            PartState::Unknown,
            "no player damage graph yet (#1699): an honest Unknown, never a guessed Intact"
        );
        assert_eq!(
            *visibility,
            Visibility::Hidden,
            "the intact presentation orig25.png shows: no plume"
        );
    }

    teardown(&mut app);
}

fn region_key(region: &AirframeDamageRegion) -> cs_sim::damage::DamageNodeKey {
    cs_sim::damage::DamageNodeKey::new(region.node_name())
        .expect("the measured node names are valid")
}
