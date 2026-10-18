//! PARITY-M01-ATMO-D (Rally #1651): M01's original cloud geometry is drawn.
//!
//! `ZBD/C3/gamez.zbd` hangs the sky under the `horizon` record: `g1155` (the
//! `cumulus1/2` ring and the `cloud1/2` quads), `h_zone2scroll` and `sun`, all
//! under `zone1`. The retail member builds M01's stage through the production
//! path and asserts that those records are drawn with their own art, unlit and
//! unfogged, and that the whole subtree follows the camera as one piece
//! (`CameraSetHorizon horizon`, `docs/findings/2026-10-11-t541-…`).
//!
//! Nothing here is `verified_original`: scroll rates, the zone gating and the
//! lighting of the sky records stay unmeasured
//! (`docs/findings/2026-10-19-parity-m01-atmo-d-cloud-geometry.md`).

use bevy::prelude::{Assets, Camera3d, GlobalTransform, StandardMaterial, Transform, Vec3};
use cs_app::mission_launch::plan_mission_launch;
use cs_app::mission_session::{
    HorizonRest, HorizonSubtree, MissionWorldParts, build_headless, stage_for,
};
use cs_app::world::contacts::WorldObjectBinding;
use cs_content::world::WorldObjectId;
use cs_types::content::Resolved;

use crate::campaign::common::label;
use crate::campaign::vs_m01_rt_window::game_dir;

const HORIZON: u32 = 3234;
const SUN: u32 = 3237;
const G1155: u32 = 3238;
const SCROLL: u32 = 3239;

fn images(
    parts: &MissionWorldParts,
    stage: &cs_app::mission_session::MissionStage,
    slot: u32,
) -> Vec<Option<String>> {
    let id = WorldObjectId::new(&format!("node-{slot}")).expect("a node id");
    let object = stage.world.object(&id).expect("in the definition");
    let Resolved::Known(mesh) = object.mesh() else {
        panic!("node-{slot} names a mesh");
    };
    parts
        .for_mesh(&mesh.value)
        .expect("drawn parts")
        .iter()
        .map(|part| part.image.clone())
        .collect()
}

/// **M01's cumulus ring and cloud quads are drawn from the archive and follow
/// the camera.**
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_parity_m01_clouds_g1155_is_drawn_and_follows_the_camera() {
    let root = game_dir();
    let plan = plan_mission_launch(&root, label("M01"), "The Lost Treasure")
        .expect("M01's launch closure plans");
    let stage = stage_for(&root, &plan).expect("M01's stage reads");
    let mut app = build_headless(&stage).expect("M01's stage composes headlessly");
    app.update();

    let parts = app.world().resource::<MissionWorldParts>().clone();
    let horizon: &HorizonSubtree = parts.horizon().expect("M01 stores a horizon record");
    assert_eq!(horizon.root(), HORIZON);
    for slot in [SUN, G1155, SCROLL] {
        assert!(horizon.contains(slot), "node-{slot} hangs under horizon");
    }

    // The art: the cumulus ring, both cloud quads and the sky cap come from
    // the archive's own images.
    let g1155 = images(&parts, &stage, G1155);
    for art in ["cumulus1", "cumulus2", "sky1"] {
        assert!(
            g1155
                .iter()
                .flatten()
                .any(|image| image.to_ascii_lowercase().ends_with(art)),
            "g1155 draws {art}: {g1155:?}"
        );
    }

    // `cloud1.tif` and `cloud2.tif` are in no ZBD/C3 texture archive: those two
    // quads are recorded as unresolved and not drawn, never given an invented
    // stand-in.
    assert_eq!(
        g1155.iter().filter(|image| image.is_none()).count(),
        1,
        "only the flat vertex-coloured skirt is untextured: {g1155:?}"
    );

    // The sky records are unlit and unfogged copies.
    let materials = app.world().resource::<Assets<StandardMaterial>>();
    for slot in [SUN, G1155, SCROLL] {
        let id = WorldObjectId::new(&format!("node-{slot}")).expect("a node id");
        let Resolved::Known(mesh) = stage.world.object(&id).expect("object").mesh() else {
            panic!("mesh");
        };
        for part in parts.for_mesh(&mesh.value).expect("parts") {
            let material = materials.get(&part.material).expect("material");
            assert!(
                material.unlit && !material.fog_enabled,
                "node-{slot} sky material"
            );
        }
    }

    // The subtree follows the camera as one piece, rest offsets kept.
    let camera = Vec3::new(1234.0, 456.0, -7890.0);
    app.world_mut().spawn((
        Camera3d::default(),
        Transform::from_translation(camera),
        GlobalTransform::from_translation(camera),
    ));
    app.update();
    app.update();
    let world = app.world_mut();
    let mut query = world.query::<(&WorldObjectBinding, &Transform, &HorizonRest)>();
    let mut moved = 0;
    for (binding, transform, rest) in query.iter(world) {
        assert!(
            (transform.translation - (camera + rest.0)).length() < 1e-2,
            "{} is camera + rest: {:?} vs {camera:?} + {:?}",
            binding.object().as_str(),
            transform.translation,
            rest.0
        );
        moved += 1;
    }
    assert!(
        moved >= 4,
        "horizon, zone1, sun, g1155 and the scroll move: {moved}"
    );
    let g1155_id = WorldObjectId::new(&format!("node-{G1155}")).expect("id");
    let (_, transform, _) = query
        .iter(world)
        .find(|(binding, _, _)| binding.object() == &g1155_id)
        .expect("g1155 is anchored");
    assert!(
        (transform.translation - camera).length() < 100.0,
        "g1155's stored offset is small: it is centred on the camera"
    );
}
