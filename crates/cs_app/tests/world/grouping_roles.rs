//! Rally #1578 (`M01-C3-WORLD-RESIDUALS`) synthetic half: a grid-named record
//! that binds no mesh and stores a **grouping box** resolves `None`, and one
//! that stores the intersection narrow-phase bit does **not**.
//!
//! Spec: `specs/F18-world-geometry-terrain-water-and-traversable-interiors.md`
//! (`### F18-B`). Shared contract: `docs/contracts/IDENTITY-CONTENT.md`. Task
//! test prefix: `accept_m01_c3_world_residuals_`. The measurement is
//! `docs/findings/2026-10-18-m01-c3-world-residuals.md`; the retail half of this
//! task's suite lives in
//! `crates/cs_app/tests/campaign/m01_c3_world_residuals.rs`.
//!
//! What this member pins, driving the production `import_world_container` and
//! `spawn_world` over authored bytes:
//!
//! * a grid-named record with no mesh and a non-empty box in the third slot,
//!   selected by its own stored selector word, resolves role `None` with its
//!   shape an explicit unknown under [`GRID_GROUPING_NODE_NEVER_BLOCKS`] and its
//!   absent mesh under [`OBJECT_STORES_NO_MESH`];
//! * the same store class **with** [`INTERSECTION_NARROW_PHASE_FLAG`] stored
//!   keeps [`GRID_GROUPING_NODE_ROLE_UNMEASURED`] on role and shape, so the rule
//!   is keyed on the record's own measurement and not on "binds no mesh";
//! * the report's partition identity gains the grouping term, and
//!   `geometry_verdict` stays open while the kept record is;
//! * `spawn_world` presents the answered record and never reports it as a gap,
//!   and reports no `unknown_mesh` at all.
//!
//! Removing the new arm fails this member: the answered record inherits the
//! index's `Solid` + `FromMesh`, the report's grouping counter drops to zero,
//! `objects_unresolved_collision` falls to zero, and the spawn skips it as
//! `unknown_mesh`.
//!
//! Nothing here is `verified_original`: the container is authored bytes and the
//! retail half is read access to the original installation.

use cs_app::mission_launch::{SurfaceVerdict, geometry_verdict};
use cs_app::world::{MESH_SETTLE_UPDATES, SkipReason, spawn_world, world_app};
use cs_content::coordinates::{CoordinateSource, SourceAdapter};
use cs_content::world::{
    GRID_GROUPING_NODE_NEVER_BLOCKS, GRID_GROUPING_NODE_ROLE_UNMEASURED, INDEXED_RECORD_IS_STATIC,
    INTERSECTION_NARROW_PHASE_FLAG, OBJECT_STORES_NO_MESH, WorldCollisionRole, WorldId,
    import_world_container,
};

use cs_types::asset_id::SourceSpan;
use cs_types::content::{Origin, Resolved};
use cs_types::evidence::ContentHash;

use super::import_retail::{
    Fixture, MARKER, ObjectSpec, SLAB, TILE_A, TILE_B, VOLUME, fixture_meshes, mesh_slots,
    provenance, read, write_container,
};

/// The fixture slot of the grouping record whose stored narrow-phase bit is
/// clear: the class `zbd/c3/gamez.zbd` stores 47 of.
///
/// The default fixture owns slots 1–4 once its unindexed `volume` is dropped,
/// so the two records this task adds take 5 and 6.
const GROUPING_DROPPED: u32 = 5;

/// The fixture slot of the grouping record that stores the narrow-phase bit, so
/// the image's walk would copy its box: the class that keeps the explicit
/// unknown.
const GROUPING_KEPT: u32 = 6;

/// **A grouping grid record resolves `None`; one that stores the narrow-phase
/// bit keeps its own unknown.**
#[test]
fn accept_m01_c3_world_residuals_a_grouping_grid_record_resolves_and_a_kept_one_does_not() {
    let bytes = write_container(&grouping_fixture());
    let imported = import_world_container(
        WorldId::from_key("fixture").expect("the fixture world key is valid"),
        Origin::SyntheticFixture,
        &read(&bytes),
        &bytes,
        &mesh_slots(),
        &retail_adapter(),
        provenance(),
    )
    .expect("the fixture container imports");
    let report = imported.report();
    let world = imported.definition();

    // The report counts the store class the way the verdict reads it.
    assert_eq!(report.partition_records(), 5, "the grid names five records");
    assert_eq!(
        report.partition_records_grouping_node(),
        2,
        "two of them bind no mesh and store a grouping box"
    );
    assert_eq!(
        report.partition_records_stores_no_geometry(),
        0,
        "and none of them stores no box at all"
    );
    assert_eq!(
        report.partition_records(),
        report.objects_solid()
            + report.partition_records_fog_volume()
            + report.partition_records_stores_no_geometry()
            + report.partition_records_grouping_node(),
        "the four numbers partition the index exactly"
    );
    assert_eq!(
        report.objects_solid(),
        3,
        "the three records with a mesh keep {INDEXED_RECORD_IS_STATIC}"
    );

    // The grouping record the walk descends past: role `None`, its absent
    // geometry explained by the store's own state rather than by the index.
    let dropped = world
        .object(&object(GROUPING_DROPPED))
        .expect("the grouping record imported");
    assert_eq!(
        dropped.known_collision(),
        Some(WorldCollisionRole::None),
        "the record itself never blocks: the walk descends instead of testing its box"
    );
    let Resolved::Unknown { claim_id, reason } = dropped.shape() else {
        panic!("its shape stays an explicit unknown, not a guess");
    };
    assert_eq!(claim_id.as_str(), GRID_GROUPING_NODE_NEVER_BLOCKS);
    assert!(
        reason.contains("0x4cb635") && reason.contains("offset 164"),
        "the reason names the measured filter and the slot the box lives in: {reason}"
    );
    let Resolved::Unknown { claim_id, .. } = dropped.mesh() else {
        panic!("a record that binds no mesh says so rather than borrowing a reference");
    };
    assert_eq!(
        claim_id.as_str(),
        OBJECT_STORES_NO_MESH,
        "and its absent mesh keeps its own claim"
    );

    // The record that stores the narrow-phase bit: the walk would copy its box,
    // so nothing measured answers it and both halves stay open.
    let kept = world
        .object(&object(GROUPING_KEPT))
        .expect("the kept grouping record imported");
    let Resolved::Unknown { claim_id, .. } = kept.collision() else {
        panic!(
            "a grouping record whose box the walk would test keeps the container's silence: {:?}",
            kept.collision()
        );
    };
    assert_eq!(claim_id.as_str(), GRID_GROUPING_NODE_ROLE_UNMEASURED);
    let Resolved::Unknown { claim_id, .. } = kept.shape() else {
        panic!("and its shape is unknown for the same reason");
    };
    assert_eq!(claim_id.as_str(), GRID_GROUPING_NODE_ROLE_UNMEASURED);
    assert_eq!(
        report.objects_unresolved_collision(),
        1,
        "exactly the kept grouping record is left open"
    );

    // The verdict is satisfied only when nothing is open: with the kept record
    // present the surface stays open under the grouping claim, and the answered
    // one is never named as the residual.
    let verdict = geometry_verdict(report);
    let SurfaceVerdict::Unknown { detail } = &verdict else {
        panic!(
            "an open grouping record keeps the surface open: {}",
            verdict.describe()
        );
    };
    assert!(
        detail.contains(GRID_GROUPING_NODE_ROLE_UNMEASURED),
        "the residual names the claim it is read off: {detail}"
    );
    assert!(
        !detail.contains("no mesh index drew"),
        "and the answered grouping record is never named as open: {detail}"
    );

    // What the spawn reports: the answered grouping record is presented and
    // never blocks, the open one is the only skip, and no record reports
    // `unknown_mesh` at all.
    let mut app = world_app();
    let spawned =
        spawn_world(&mut app, world, &fixture_meshes(world)).expect("the fixture world spawns");
    for _ in 0..MESH_SETTLE_UPDATES {
        app.update();
    }
    let skipped: Vec<SkipReason> = spawned.skipped().iter().map(|entry| entry.reason).collect();
    assert_eq!(
        skipped,
        vec![SkipReason::UnknownCollisionRole],
        "only the record whose box the walk would test is a gap"
    );
    assert_eq!(
        spawned.non_colliding(),
        vec![object(MARKER), object(GROUPING_DROPPED)],
        "the anchor (whose store also says `no geometry`) and the answered grouping \
         record are presented and never block"
    );
    assert_eq!(
        spawned.presentation_gap_count(),
        0,
        "and no object reports two reasons"
    );
}

/// The fixture as this task authors it: the default arrangement plus the two
/// grouping records the question is about, both **in** the grid, so the
/// ownership cross-check (grid ∪ stored child list = the records naming the
/// world node) still holds.
///
/// The default fixture's `volume` is dropped: it is the unindexed
/// geometry-bearing class, which is not this task's question and would
/// otherwise add an unrelated open role.
fn grouping_fixture() -> Fixture {
    let mut fixture = Fixture::default();
    fixture.objects.retain(|object| object.name != "volume");
    fixture.stored_children.retain(|slot| *slot != VOLUME);
    fixture.objects.push(
        ObjectSpec::new("grouping_edge", -1).grouping_box([-6.0, 0.0, -6.0], [-5.0, 1.0, -5.0]),
    );
    fixture.objects.push(
        ObjectSpec::new("grouping_kept", -1)
            .grouping_box([-4.0, 0.0, -4.0], [-3.0, 1.0, -3.0])
            // The records the original installation stores keep the narrow-phase
            // bit clear; this one stores it set, which is exactly the difference
            // between the walk descending past the box and testing it.
            .flags(0x0308_851c | INTERSECTION_NARROW_PHASE_FLAG),
    );
    fixture.grid = vec![
        vec![TILE_A, SLAB, GROUPING_DROPPED],
        vec![TILE_B, GROUPING_KEPT],
    ];
    fixture
}

/// The object id one fixture node slot gets.
fn object(slot: u32) -> cs_content::world::WorldObjectId {
    cs_content::world::WorldObjectId::new(&format!("node-{slot}"))
        .expect("a node slot key is valid")
}

/// The conversion the fixture is imported through: the measured GameZ source
/// over a stand-in span, so `axis_class()` reports `ObservedTool` and the
/// verdict is testable without original data. The bytes are authored here; the
/// span is a key, not a claim.
fn retail_adapter() -> SourceAdapter {
    let span = SourceSpan::new(
        ContentHash::from_bytes([0; 32]),
        "zbd/c3/gamez.zbd",
        None,
        0,
        0,
        None,
    )
    .expect("a span for an installation-backed source is valid");
    SourceAdapter::new(CoordinateSource::retail_gamez(span))
}
