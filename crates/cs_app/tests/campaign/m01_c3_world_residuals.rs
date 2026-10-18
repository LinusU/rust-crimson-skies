//! Rally #1578 (`M01-C3-WORLD-RESIDUALS`) retail half: `zbd/c3/gamez.zbd` — the
//! world group M01 is set in since #1565 — leaves no grid record without an
//! answer, and the M01 **label**'s plan is launchable.
//!
//! The synthetic half of this task's suite lives in
//! `crates/cs_app/tests/world/grouping_roles.rs`; the measurement is
//! `docs/findings/2026-10-18-m01-c3-world-residuals.md`. Task test prefix:
//! `accept_m01_c3_world_residuals_`.
//!
//! Before this task `c3` named 47 records in its partition grid that bind no
//! mesh and store a non-empty box only in the third slot, selected by their own
//! stored selector word. They fell into `INDEXED_RECORD_IS_STATIC`'s `Solid` +
//! `FromMesh`, so `spawn_world` reported each of them `unknown_mesh` and
//! `geometry_verdict` read "what the 47 grid records that store no mesh index
//! drew" — and `cs --cs-path $CS_GAME_DIR --mission M01` refused before any
//! window opened. The older retail member of the `world_geometry` verdict suite
//! was re-pinned to the `M06` label (`vs_m01_geometry_verdict.rs`), which is why
//! nothing in the suite caught it; that M06 member stays as it is and this
//! module restores the `M01`-label regression it stopped covering.
//!
//! Both members are `#[ignore = "requires CS_GAME_DIR and CS_ENGINE_IMAGE"]`:
//! CI has no original data. `retail` here is read access to the original's
//! bytes and no original executable ran, so nothing here is `verified_original`.

use std::collections::BTreeSet;
use std::path::PathBuf;

use cs_app::mission_launch::{
    LaunchSurface, SurfaceVerdict, geometry_verdict, plan_mission_launch,
};
use cs_app::world::{
    MESH_SETTLE_UPDATES, RetailWorldContainer, SkipReason, read_world_container, spawn_world,
    world_app,
};
use cs_content::coordinates::{CoordinateSource, SourceAdapter};
use cs_content::textures::WorldTextureLoad;
use cs_content::world::{
    GRID_GROUPING_NODE_NEVER_BLOCKS, GROUPING_BOX_SELECTOR, INTERSECTION_NARROW_PHASE_FLAG,
    OBJECT_STORES_NO_MESH, WorldCollisionRole, WorldObjectId,
};
use cs_formats::gamez::RawNode;
use cs_types::content::{Origin, Resolved};

use crate::common::label;
use crate::vs_m01_rt_window::game_dir;

/// `ZBD/C3/gamez.zbd`, the container M01 is set in since #1565.
const GROUP: &str = "C3";

/// Reads and imports `c3`'s world container through the production path.
fn c3() -> (RetailWorldContainer, cs_content::world::ImportedWorld) {
    let root: PathBuf = game_dir();
    let container = read_world_container(&root, GROUP, &WorldTextureLoad::project_default())
        .expect("c3's world container reads");
    let origin = Origin::Installation {
        source: container.span().clone(),
    };
    let adapter = SourceAdapter::new(CoordinateSource::retail_gamez(container.span().clone()));
    let imported = container
        .definition(origin, &adapter)
        .expect("c3's world container imports");
    (container, imported)
}

/// Whether a stored record binds no mesh and stores its box only in the third
/// slot, the one its own selector word names: re-derived from the record's own
/// bytes rather than read back from the import's counter.
fn stores_grouping_box(record: &RawNode) -> bool {
    let empty = |stored: [[f32; 3]; 2]| (0..3).all(|axis| stored[0][axis] == stored[1][axis]);
    record.mesh_index() < 0
        && record.info.unk112 == GROUPING_BOX_SELECTOR
        && !empty(record.info.unk164)
        && empty(record.info.unk116)
        && empty(record.info.unk140)
}

/// **`c3`'s 47 grouping records are answered and the surface is closed.**
#[test]
#[ignore = "requires CS_GAME_DIR and CS_ENGINE_IMAGE"]
fn accept_m01_c3_world_residuals_c3_leaves_no_grouping_record_open() {
    let (container, imported) = c3();
    let report = imported.report();
    let world = imported.definition();

    assert_eq!(
        report.partition_records_grouping_node(),
        47,
        "c3's grid names 47 records that bind no mesh and store a grouping box"
    );
    assert_eq!(
        report.partition_records(),
        report.objects_solid()
            + report.partition_records_fog_volume()
            + report.partition_records_stores_no_geometry()
            + report.partition_records_grouping_node(),
        "the four numbers partition c3's index exactly"
    );

    // The class is the container's own bytes, not the report's counter: walk the
    // grid and check every record the predicate names is answered.
    let indexed: BTreeSet<u32> = container
        .partition_grid()
        .expect("c3's own grid reads")
        .indexed_slots()
        .into_iter()
        .collect();
    let mut answered = 0usize;
    for slot in &indexed {
        let Some(record) = container.nodes().get(*slot) else {
            continue;
        };
        if !stores_grouping_box(record) {
            continue;
        }
        answered += 1;
        let id = WorldObjectId::new(&format!("node-{slot}")).expect("a node slot key is valid");
        let object = world.object(&id).expect("a grid-named record imported");
        assert_eq!(
            object.known_collision(),
            Some(WorldCollisionRole::None),
            "node {slot} stores a grouping box, so the record itself never blocks"
        );
        let Resolved::Unknown { claim_id, .. } = object.shape() else {
            panic!("node {slot} builds no collider, so its shape stays open");
        };
        assert_eq!(
            claim_id.as_str(),
            GRID_GROUPING_NODE_NEVER_BLOCKS,
            "node {slot} is answered by the measured grouping rule"
        );
        let Resolved::Unknown { claim_id, .. } = object.mesh() else {
            panic!("node {slot} binds no mesh and says so");
        };
        assert_eq!(claim_id.as_str(), OBJECT_STORES_NO_MESH);
        // Every one of them groups at least one child — the records whose meshes
        // its box bounds, which is why the image's walk descends instead of
        // testing it — and keeps the narrow-phase bit clear, which is the
        // measurement the rule is keyed on.
        assert!(
            record.info.children_count > 0,
            "node {slot} groups at least one child record"
        );
        assert_eq!(
            record.flags() & INTERSECTION_NARROW_PHASE_FLAG,
            0,
            "node {slot} keeps the narrow-phase bit clear"
        );
    }
    assert_eq!(
        answered, 47,
        "the bytes name exactly the records the report counts"
    );

    assert_eq!(
        report.objects_unresolved_collision(),
        0,
        "no record on c3 is left with an open collision role"
    );
    let verdict = geometry_verdict(report);
    let SurfaceVerdict::Satisfied { consumer } = &verdict else {
        panic!("c3's surface is closed: {}", verdict.describe());
    };
    assert!(
        consumer.contains("objects with an `Unknown` collision role: 0"),
        "the zero residual is stated: {consumer}"
    );
    assert!(
        consumer.contains("grid grouping records: 47"),
        "and the answered class is counted in the consumer's own words: {consumer}"
    );

    // What the spawn reports: the grouping records are presented and never
    // block, and no record reports `unknown_mesh` at all.
    let meshes = container
        .uploaded_meshes(world)
        .expect("c3's own meshes upload");
    let mut app = world_app();
    let spawned = spawn_world(&mut app, world, &meshes).expect("c3's imported world spawns");
    for _ in 0..MESH_SETTLE_UPDATES {
        app.update();
    }
    let skips: BTreeSet<&str> = spawned
        .skipped()
        .iter()
        .map(|entry| entry.reason.label())
        .collect();
    assert!(
        !skips.contains(SkipReason::UnknownMesh.label()),
        "no record reports `unknown_mesh` any more: {skips:?}"
    );
    assert_eq!(
        spawned.colliders().len(),
        report.partition_records_with_mesh() - report.partition_records_fog_volume(),
        "every indexed record that binds a mesh still collides"
    );
    assert!(
        spawned.non_colliding().len() >= 47,
        "the grouping records join the presented-and-never-blocking count: {}",
        spawned.non_colliding().len()
    );
    assert_eq!(spawned.presentation_gap_count(), 0);
    assert_eq!(
        spawned.colliders().len() + spawned.skipped_count() + spawned.non_colliding().len(),
        world.objects().len(),
        "every object collides, is reported once, or is deliberately non-colliding"
    );
}

/// **M01's plan — the label, not a sibling container — is launchable.**
///
/// This is the read the CLI makes for `cs --cs-path $CS_GAME_DIR --mission M01`:
/// the binding resolves M01 to `ZBD/C3/M01` (task #1565) and the
/// `world_geometry` surface reads `Satisfied` off c3's own counters.
#[test]
#[ignore = "requires CS_GAME_DIR and CS_ENGINE_IMAGE"]
fn accept_m01_c3_world_residuals_m01_s_plan_is_launchable() {
    let plan = plan_mission_launch(&game_dir(), label("M01"), "The Lost Treasure")
        .expect("the installation yields a launch plan for M01");

    assert_eq!(
        plan.group_dir, "zbd/c3",
        "M01 resolves to the world group #1565 bound it to"
    );
    assert_eq!(
        plan.mission_dir, "zbd/c3/m01",
        "and to that group's own m01 archive"
    );

    let geometry = plan
        .surface(LaunchSurface::WorldGeometry)
        .expect("the world_geometry surface is reported");
    let SurfaceVerdict::Satisfied { consumer } = &geometry.verdict else {
        panic!(
            "c3 answers every record, so the surface is satisfied: {}",
            geometry.verdict.describe()
        );
    };
    assert!(
        consumer.contains("objects with an `Unknown` collision role: 0"),
        "the residual is zero and stated: {consumer}"
    );

    let gaps: Vec<String> = plan
        .gaps()
        .map(|report| format!("{}: {}", report.surface.label(), report.verdict.describe()))
        .collect();
    assert!(
        gaps.is_empty(),
        "the plan reports zero gaps — the launch gate names nothing to measure: {gaps:?}"
    );
    assert!(
        plan.launchable(),
        "every surface satisfied means the gate passes: {:?}",
        plan.surfaces
            .iter()
            .map(|report| format!("{}: {}", report.surface.label(), report.verdict.describe()))
            .collect::<Vec<_>>()
    );
}

/// **`c3`'s world node owns a `horizon` environment subtree, as `c1c`'s does.**
///
/// Task #1537 asked this task to record the answer: the far-terrain/sky records
/// task #1553 imports for `c1c` are present in `c3` too, and every one of them
/// is environment presentation.
#[test]
#[ignore = "requires CS_GAME_DIR and CS_ENGINE_IMAGE"]
fn accept_m01_c3_world_residuals_c3_has_a_horizon_environment_subtree() {
    let (container, imported) = c3();
    let report = imported.report();
    let world = imported.definition();

    let horizon = container
        .nodes()
        .nodes
        .iter()
        .find(|node| node.parent == Some(0) && node.name == "horizon")
        .expect("c3's world node owns a child named `horizon`");

    assert!(
        report.environment_records() > 0,
        "the `horizon` child's descendants are imported, not dropped"
    );
    let mut seen = 0usize;
    for object in world.objects() {
        let slot = object
            .id()
            .as_str()
            .strip_prefix("node-")
            .and_then(|slot| slot.parse::<u32>().ok())
            .expect("an object id is its node slot");
        if slot == horizon.index || !is_descendant(&container, horizon.index, slot) {
            continue;
        }
        seen += 1;
        assert_eq!(
            object.known_collision(),
            Some(WorldCollisionRole::None),
            "{} is environment presentation and never blocks",
            object.id().as_str()
        );
    }
    assert_eq!(
        seen,
        report.environment_records(),
        "every environment record the import counted is the `horizon` child's descendant"
    );
}

/// Whether `slot` is a descendant of `root` through the container's own child
/// slots.
fn is_descendant(container: &RetailWorldContainer, root: u32, slot: u32) -> bool {
    let nodes = container.nodes();
    let mut stack = vec![root];
    let mut seen: BTreeSet<u32> = BTreeSet::new();
    while let Some(current) = stack.pop() {
        if !seen.insert(current) {
            continue;
        }
        let Some(node) = nodes.get(current) else {
            continue;
        };
        if node.children.contains(&slot) {
            return true;
        }
        stack.extend(node.children.iter().copied());
    }
    false
}
