//! M01-WINDOW-FARIMPORT (Rally #1553): the world import owns the world node's
//! `horizon` environment subtree, so M01's far terrain is in its definition.
//!
//! The owner's windowed M01 draw was only the water/cloud tiles and the `fvol*`
//! boxes: the island the mission is set on hung one level below the world node
//! (under the `horizon` child), and a direct-children-only import dropped it.
//! This member drives the production path end to end — `read_world_container` →
//! `RetailWorldContainer::definition` → `import_world_container` → the
//! definition's own accessors — and then re-reads the launch verdict off the
//! same report through [`geometry_verdict`] and [`plan_mission_launch`].
//!
//! What it pins:
//!
//! * M01's imported definition **contains** the `zone1`/`zone2` records and
//!   their far-terrain/sky meshes (node slots 790/795 and their children
//!   791–794, 796), so removing the environment-subtree import fails here;
//! * every one of those records resolves a **known** collision role of `None`
//!   ([`HORIZON_SUBTREE_RECORD_NEVER_BLOCKS`]), so removing the role rule makes
//!   the closure open and fails the launch half;
//! * `world_geometry` stays `Satisfied` and M01's plan stays `launchable()`.
//!
//! It is `#[ignore = "requires CS_GAME_DIR and CS_ENGINE_IMAGE"]`: CI has no
//! original data, so the implementing and reviewing agents run it with
//! `--include-ignored`. Nothing here is `verified_original` — `retail` is
//! read access and no original executable ran.

use std::path::PathBuf;

use cs_app::mission_launch::{
    LaunchSurface, SurfaceVerdict, geometry_verdict, plan_mission_launch,
};
use cs_app::world::{RetailWorldContainer, read_world_container};
use cs_content::coordinates::{CoordinateSource, SourceAdapter};
use cs_content::textures::WorldTextureLoad;
use cs_content::world::{HORIZON_SUBTREE_RECORD_NEVER_BLOCKS, WorldCollisionRole, WorldObjectId};
use cs_types::content::{Origin, Resolved};

use crate::common::label;
use crate::vs_m01_rt_window::game_dir;

/// The world group M01 is set in.
const GROUP: &str = "C1C";

/// The catalog id one stored mesh slot of `container` has, through the
/// catalog's own spelling rule so this test cannot disagree with the
/// production id by construction.
fn catalog_mesh_id(container: &RetailWorldContainer, index: u32) -> cs_types::content::ContentId {
    cs_content::catalog::baseline::mesh_content_id(container.container_key(), index)
        .expect("a container's own mesh slot is inside the id grammar")
}

/// The environment subtree records M01's definition must own: the `horizon`
/// child's descendants (node slot, stored mesh slot, whether it carries a mesh).
///
/// Node slots are the container's own addresses, which is what
/// `OBJECT_ID_IS_THE_NODE_SLOT` makes the imported object's id; the mesh slots
/// are the far-terrain/sky meshes the split measured (`moon` 152, `g1155` 153,
/// `stars` 154, `h_zone2scroll` 155, `g1164` 156).
const ENVIRONMENT: [(u32, Option<u32>); 7] = [
    // zone2#790 and zone1#795 are the two mesh-less transform groups.
    (790, None),
    (791, Some(152)),
    (792, Some(153)),
    (793, Some(154)),
    (794, Some(155)),
    (795, None),
    (796, Some(156)),
];

/// Reads and imports M01's world container through the production path.
fn imported() -> (RetailWorldContainer, cs_content::world::ImportedWorld) {
    let root: PathBuf = game_dir();
    let container = read_world_container(&root, GROUP, &WorldTextureLoad::project_default())
        .expect("M01's world container reads");
    let origin = Origin::Installation {
        source: container.span().clone(),
    };
    let adapter = SourceAdapter::new(CoordinateSource::retail_gamez(container.span().clone()));
    let imported = container
        .definition(origin, &adapter)
        .expect("M01's world container imports");
    (container, imported)
}

/// **M01's definition owns the `horizon` environment subtree, its records never
/// block, and the launch verdict stays satisfied and launchable.**
#[test]
#[ignore = "requires CS_GAME_DIR and CS_ENGINE_IMAGE"]
fn accept_m01_window_world_imports_the_horizon_subtree_and_stays_launchable() {
    let (container, imported) = imported();
    let report = imported.report();
    let world = imported.definition();

    assert_eq!(
        report.environment_records(),
        ENVIRONMENT.len(),
        "the import counted exactly the horizon child's descendants"
    );

    for (slot, mesh) in ENVIRONMENT {
        let id = WorldObjectId::new(&format!("node-{slot}")).expect("a node slot is a valid id");
        let object = world
            .object(&id)
            .unwrap_or_else(|| panic!("node-{slot} is imported into M01's definition"));
        match mesh {
            Some(mesh_slot) => {
                let Resolved::Known(known) = object.mesh() else {
                    panic!("node-{slot} must resolve its far-terrain/sky mesh");
                };
                assert_eq!(
                    known.value,
                    catalog_mesh_id(&container, mesh_slot),
                    "node-{slot} resolves the catalog mesh for stored slot {mesh_slot}"
                );
            }
            None => assert!(
                matches!(object.mesh(), Resolved::Unknown { .. }),
                "node-{slot} is a transform group with no mesh of its own"
            ),
        }
        assert_eq!(
            object.known_collision(),
            Some(WorldCollisionRole::None),
            "node-{slot} is environment presentation and never blocks"
        );
        let Resolved::Unknown { claim_id, .. } = object.shape() else {
            panic!("node-{slot} states no shape this container measured");
        };
        assert_eq!(
            claim_id.as_str(),
            HORIZON_SUBTREE_RECORD_NEVER_BLOCKS,
            "node-{slot}'s collision role is answered by the environment rule"
        );
    }

    // The container's whole surface is answered: no record is left with an
    // `Unknown` collision role, so `world_geometry` is a closed surface.
    assert_eq!(
        report.objects_unresolved_collision(),
        0,
        "the far-terrain import did not open a single collision role"
    );
    let verdict = geometry_verdict(report);
    let SurfaceVerdict::Satisfied { consumer } = &verdict else {
        panic!(
            "the far terrain resolves `None`, so the surface is satisfied: {}",
            verdict.describe()
        );
    };
    assert!(
        consumer.contains("objects with an `Unknown` collision role: 0"),
        "the zero residual is stated, not omitted: {consumer}"
    );

    // And the launch plan, which reads that very verdict, still passes.
    let plan = plan_mission_launch(&game_dir(), label("M01"), "The Lost Treasure")
        .expect("M01's launch closure plans");
    let geometry = plan
        .surface(LaunchSurface::WorldGeometry)
        .expect("the world_geometry surface is reported");
    assert!(
        matches!(geometry.verdict, SurfaceVerdict::Satisfied { .. }),
        "world_geometry is still satisfied: {}",
        geometry.verdict.describe()
    );
    assert!(
        plan.launchable(),
        "M01 still launches with the far terrain imported: {}",
        plan.gaps()
            .map(|gap| format!("{}: {}", gap.surface.label(), gap.verdict.describe()))
            .collect::<Vec<_>>()
            .join("; ")
    );
}
