//! Acceptance stage PARITY2-M01-HUD-ART (Rally #1726): the mission
//! composition draws the original gauge cluster — the measured
//! `instruments.zrd` placements and the `planes.zbd` `gauges` subtree —
//! instead of only the text/CSS approximation.

use std::path::PathBuf;

use bevy::camera::visibility::RenderLayers;
use bevy::prelude::{Children, Entity, Mesh3d, With};
use cs_app::mission_launch::plan_mission_launch;
use cs_app::mission_session::{
    HUD_CAMERA_LAYER, HudArrow, HudArtCamera, HudArtRoot, HudCompassTape, HudFlag, HudGlyphCell,
    HudNeedle, HudNitroNeedle, HudPip, HudZone, MissionHudArt, NeedleRole, PipGauge,
    RETICLE_TEXTURE, ZonePieceRole, build_headless, gauge_frame, read_instrument_table,
    read_reticle_image, stage_for, teardown, zone_texture_name,
};
use cs_app::playtest_retail::AircraftPart;
use cs_sim::damage::PartState;

use crate::campaign::common::label;

fn game_dir() -> PathBuf {
    PathBuf::from(std::env::var("CS_GAME_DIR").unwrap_or_else(|_| {
        panic!("CS_GAME_DIR is not set: this suite needs the retail capability")
    }))
}

fn count<T: bevy::prelude::Component>(world: &mut bevy::prelude::World) -> usize {
    let mut query = world.query_filtered::<Entity, With<T>>();
    query.iter(world).count()
}

/// The entities the HUD camera pass draws (layer `HUD_CAMERA_LAYER`): the
/// corner cluster. The cockpit panel dials carry the same markers in
/// cockpit space (PARITY3-COCKPIT-VIEW.04, #1812), so the cluster's own
/// counts are measured on this set alone.
fn on_hud_layer(world: &bevy::prelude::World, entity: Entity) -> bool {
    world
        .get::<RenderLayers>(entity)
        .is_some_and(|layers| layers.intersects(&RenderLayers::layer(HUD_CAMERA_LAYER)))
}

/// The entities under `root` — the root's parts and their material-piece
/// children (glyph cells and zone pieces are tagged on the children).
fn cluster_entities(world: &bevy::prelude::World, root: Entity) -> Vec<Entity> {
    let mut members = vec![root];
    let mut cursor = 0;
    while cursor < members.len() {
        let entity = members[cursor];
        cursor += 1;
        if let Some(children) = world.get::<Children>(entity) {
            members.extend(children.iter().copied());
        }
    }
    members
}

/// **The `instruments.zrd` placements decode through the production reader**:
/// the seven measured instruments, at the measured HUD-space positions.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_parity2_m01_hud_art_instrument_table_measures_the_placements() {
    let table = read_instrument_table(&game_dir());
    assert_eq!(table.refusal, None, "{:?}", table.refusal);
    assert_eq!(table.carrier.as_deref(), Some("zbd/zrdr.zbd"));
    let want: [(&str, f32, f32, f32); 7] = [
        ("speedometer", 9.0, -4.05, -17.5),
        ("altimeter", -9.0, -4.05, -17.5),
        ("comp", 0.0, 7.0, -17.5),
        ("damageindicator", -9.0, -6.0, -17.5),
        ("missilegauge", -9.0, -2.1, -17.5),
        ("gungauge", 9.0, -2.1, -17.5),
        ("nitrogauge", 11.5, -10.5, -23.5),
    ];
    for (name, x, y, z) in want {
        let placement = table
            .placements
            .iter()
            .find(|placement| placement.name == name)
            .unwrap_or_else(|| panic!("instruments.zrd must place {name}"));
        assert_eq!(
            placement.position,
            [x, y, z],
            "{name}'s measured position differs"
        );
    }
}

/// **The composed mission carries the measured cluster**: the placement
/// resource records the seven instruments, the spawned tree holds one root +
/// one dedicated camera on the HUD layer, every mesh node of the placed
/// instruments draws on that layer, and each live element carries its
/// measured marker.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_parity2_m01_hud_art_composition_draws_the_measured_cluster() {
    let root = game_dir();
    let plan = plan_mission_launch(&root, label("M06"), "The Red Menace")
        .expect("M01's launch closure plans");
    assert!(plan.launchable(), "M01's launch surfaces are satisfied");
    let stage = stage_for(&root, &plan).expect("M01's stage reads through the production readers");
    let mut app = build_headless(&stage).expect("M01's stage composes headlessly");
    app.update();

    let (placements, scale, unknowns) = {
        let hud = app
            .world()
            .get_resource::<MissionHudArt>()
            .expect("the composition records the HUD art build");
        (
            hud.placements
                .iter()
                .map(|placement| placement.name.clone())
                .collect::<Vec<String>>(),
            hud.scale,
            hud.unknowns.clone(),
        )
    };
    assert_eq!(
        placements.len(),
        7,
        "all seven measured instruments must be placed; build record: {unknowns:?}"
    );
    assert!(
        scale.is_finite() && scale > 0.0,
        "the authored→HUD scale must be finite and positive"
    );

    let world = app.world_mut();
    assert_eq!(count::<HudArtRoot>(world), 1, "exactly one HUD root");
    assert_eq!(count::<HudArtCamera>(world), 1, "exactly one HUD camera");
    let hud_root = world
        .query_filtered::<Entity, With<HudArtRoot>>()
        .single(world)
        .expect("one HUD root");
    let cluster = cluster_entities(world, hud_root);

    // Every drawn part sits on the HUD layer, never the world pass's.
    let mut layered = world.query_filtered::<(Entity, &AircraftPart), With<Mesh3d>>();
    let parts: Vec<(Entity, u32)> = layered
        .iter(world)
        .filter(|(entity, _)| on_hud_layer(world, *entity))
        .map(|(entity, part)| (entity, part.node_slot))
        .collect();
    assert!(
        parts.len() >= 25,
        "the seven instruments draw {} parts, expected at least the measured ~30",
        parts.len()
    );

    // The measured live elements all carry their marker — counted on the
    // corner cluster alone; the panel dials repeat the same tagging under
    // `CockpitDialsRoot` (PARITY3-COCKPIT-VIEW.04).
    let count_on_cluster =
        |world: &bevy::prelude::World, marker: fn(&bevy::prelude::World, Entity) -> bool| {
            cluster
                .iter()
                .filter(|entity| marker(world, **entity))
                .count()
        };
    let needles: Vec<NeedleRole> = parts
        .iter()
        .filter_map(|(entity, _)| world.get::<HudNeedle>(*entity))
        .map(|needle| needle.role)
        .collect();
    for role in [
        NeedleRole::Speed,
        NeedleRole::AltHundreds,
        NeedleRole::AltThousands,
    ] {
        assert!(needles.contains(&role), "needle {role:?} must be drawn");
    }
    assert_eq!(
        count_on_cluster(world, |world, entity| world
            .get::<HudCompassTape>(entity)
            .is_some()),
        1,
        "one compass tape"
    );
    assert_eq!(
        count_on_cluster(world, |world, entity| world
            .get::<HudZone>(entity)
            .is_some()),
        4,
        "four damage zones (nose/tail/left wing/right wing)"
    );
    {
        let pips: Vec<(PipGauge, usize)> = cluster
            .iter()
            .filter_map(|entity| world.get::<HudPip>(*entity))
            .map(|pip| (pip.gauge, pip.index))
            .collect();
        for index in 0..8 {
            assert!(
                pips.contains(&(PipGauge::Missile, index)),
                "missile pip {index} must be drawn"
            );
        }
        for index in 0..4 {
            assert!(
                pips.contains(&(PipGauge::Gun, index)),
                "gun pip {index} must be drawn"
            );
        }
    }
    assert_eq!(
        count_on_cluster(world, |world, entity| world
            .get::<HudGlyphCell>(entity)
            .is_some()),
        20,
        "4+6 missile glyphs and 4+6 gun glyphs"
    );
    assert_eq!(
        count_on_cluster(world, |world, entity| world
            .get::<HudFlag>(entity)
            .is_some()),
        2,
        "the lowalt and stall flags"
    );
    assert_eq!(
        count_on_cluster(world, |world, entity| world
            .get::<HudArrow>(entity)
            .is_some()),
        2,
        "the mg/gg ordnance arrows"
    );
    assert_eq!(
        count_on_cluster(world, |world, entity| world
            .get::<HudNitroNeedle>(entity)
            .is_some()),
        2,
        "nitro boost + charge needles"
    );

    // Every drawn HUD part keeps its material pieces as children.
    for (entity, slot) in &parts {
        assert!(
            world
                .get::<Children>(*entity)
                .is_some_and(|children| !children.is_empty()),
            "HUD part slot {slot} draws no material piece"
        );
    }

    teardown(&mut app);
}

/// **The flight reticle is the gunsight, not the map circle.** The original
/// owner frames show a small ring with cross ticks at screen centre; in
/// `ZBD/rimage.zbd` that is `impact_point` (32×32). `rcircle` (32×36) is the
/// hand-drawn felt-marker circle and must not be the HUD reticle.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_bug_m01_crosshair_is_impact_point_not_rcircle() {
    assert_eq!(RETICLE_TEXTURE, "impact_point");
    let mut images = bevy::prelude::Assets::<bevy::prelude::Image>::default();
    let handle =
        read_reticle_image(&game_dir(), &mut images).expect("the reticle texture must decode");
    let image = images.get(&handle).expect("the handle holds an image");
    let extent = image.texture_descriptor.size;
    assert_eq!((extent.width, extent.height), (32, 32));
    // The gunsight is a ring with cross ticks: the centre column and row carry
    // visible texels while the corners are fully transparent.
    let data = image.data.as_ref().expect("the image has texel data");
    let alpha = |x: usize, y: usize| data[(y * 32 + x) * 4 + 3];
    assert_eq!(alpha(0, 0), 0, "the corner is transparent");
    assert_eq!(alpha(31, 31), 0, "the corner is transparent");
    assert!(
        (0..32).any(|y| alpha(16, y) > 0) && (0..32).any(|x| alpha(x, 16) > 0),
        "the cross ticks are drawn"
    );
}

/// **The gauge frame follows the measured ladder** (0.72 / 0.46 / 0.2 over
/// the 0-based `[green, yellow, orange, red]` cycle): orange is reachable and
/// `Destroyed` is the last frame.
#[test]
fn accept_parity_m01_player_damage_gauge_frame_follows_the_measured_ladder() {
    let damaged = |fraction| gauge_frame(Some(fraction), PartState::Damaged);
    assert_eq!(gauge_frame(Some(1.0), PartState::Intact), 0);
    assert_eq!(gauge_frame(None, PartState::Unknown), 0);
    assert_eq!(damaged(0.95), 0, "damaged but above the first level");
    assert_eq!(damaged(0.73), 0);
    assert_eq!(damaged(0.72), 1);
    assert_eq!(damaged(0.60), 1);
    assert_eq!(damaged(0.46), 2);
    assert_eq!(damaged(0.30), 2);
    assert_eq!(damaged(0.20), 3);
    assert_eq!(damaged(0.05), 3);
    assert_eq!(gauge_frame(Some(0.0), PartState::Destroyed), 3);
    assert_eq!(gauge_frame(None, PartState::Destroyed), 3);
    assert_eq!(zone_texture_name(ZonePieceRole::Hilite, 2), "orangehilite");
    assert_eq!(zone_texture_name(ZonePieceRole::Hatch, 2), "orng_hatchptrn");
    assert_eq!(zone_texture_name(ZonePieceRole::Hatch, 3), "red_hatchptrn");
}

/// **Every gauge frame's retail texture decodes** through the production
/// composition: the yellow, orange and red `*hilite` and `*_hatchptrn`
/// images are preloaded for the zone drive.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_parity_m01_player_damage_gauge_composition_preloads_every_frame() {
    let root = game_dir();
    let plan = plan_mission_launch(&root, label("M06"), "The Red Menace")
        .expect("M01's launch closure plans");
    let stage = stage_for(&root, &plan).expect("M01's stage reads through the production readers");
    let mut app = build_headless(&stage).expect("M01's stage composes headlessly");
    app.update();
    {
        let hud = app
            .world()
            .get_resource::<MissionHudArt>()
            .expect("the composition records the HUD art build");
        for role in [ZonePieceRole::Hilite, ZonePieceRole::Hatch] {
            for frame in 1..=3u8 {
                let name = zone_texture_name(role, frame);
                assert!(
                    hud.images.contains_key(name),
                    "gauge frame {name} must preload; build record: {:?}",
                    hud.unknowns
                );
            }
        }
    }
    teardown(&mut app);
}
