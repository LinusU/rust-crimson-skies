//! Task #2004 `BUG-M01-SHIPWRECK-GRAY-TRIANGLES`: six huge flat beige triangles
//! stood over M01's shipwreck. They are the six `cone` nodes (slots 3215, 3218,
//! 3221, 3224, 3227, 3230) under the `shipwreck` subtree of `ZBD/C3`: one
//! triangle each, every material group flat-colour (material 84), stored tilted
//! so no extent axis is zero — which the #795 flat-card rule missed
//! (`docs/findings/2026-10-23-m01-shipwreck-cone-cards.md`).

use std::path::PathBuf;

use cs_app::mission_launch::plan_mission_launch;
use cs_app::mission_session::{MissionWorldParts, build_headless, stage_for};
use cs_content::campaign_bindings::MissionLabel;

/// The shipwreck node's origin in canonical metres
/// (`docs/findings/2026-10-21-m01-objective-target-points.md`).
const SHIPWRECK: [f32; 3] = [-8891.4, 5.0, -4685.5];

/// **Retail:** no flat-colour single-triangle card is drawn within 300 m of the
/// shipwreck, while the wreck's own hull parts still are.
#[test]
#[ignore = "requires CS_GAME_DIR and CS_ENGINE_IMAGE"]
fn accept_m01_shipwreck_gray_triangles_the_cone_cards_are_not_drawn() {
    let root = PathBuf::from(std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR must be set"));
    let label = MissionLabel::new("M01").expect("label");
    let plan = plan_mission_launch(&root, label, "The Lost Treasure").expect("M01 plans");
    let stage = stage_for(&root, &plan).expect("M01's stage reads");
    let mut app = build_headless(&stage).expect("M01 composes");
    app.update();

    let parts = app.world().resource::<MissionWorldParts>();
    let scenery = parts.scenery().expect("the world draw built its scenery");
    let near = |t: bevy::prelude::Vec3| {
        let d = [t.x - SHIPWRECK[0], t.y - SHIPWRECK[1], t.z - SHIPWRECK[2]];
        (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt() < 300.0
    };
    let mut hull_drawn = 0;
    for placement in &scenery.placements {
        if !near(placement.global.translation) {
            continue;
        }
        let pieces = scenery.meshes.get(&placement.mesh).expect("a drawn mesh");
        assert_ne!(
            placement.node_name, "cone",
            "the shipwreck's `cone` card (slot {}) must not be drawn",
            placement.node_slot
        );
        for piece in pieces {
            assert!(
                !(piece.image.is_none() && piece.triangles == 1),
                "a flat-colour single triangle (node {:?} slot {}) is drawn at the shipwreck",
                placement.node_name,
                placement.node_slot
            );
        }
        if placement.node_name.starts_with("hull") {
            hull_drawn += 1;
        }
    }
    assert!(
        hull_drawn >= 2,
        "the wreck's own hull parts must still draw, found {hull_drawn}"
    );
}
