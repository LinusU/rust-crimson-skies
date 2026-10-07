//! #639 (`F18-WORLD-UNITS-CONTAINERS`): **every** world container of the
//! original installation imports and spawns, and the ones that do not are named
//! with their typed refusal.
//!
//! Spec: `specs/F18-world-geometry-terrain-water-and-traversable-interiors.md`
//! (`### F18-B`, the world-import and static-collision stage, and `### F18-D`'s
//! "visit every discovered world group"). Shared contract:
//! `docs/contracts/IDENTITY-CONTENT.md`. Task test prefix:
//! `accept_f18_world_units_containers_`.
//!
//! Task #629 imported `ZBD/C1C/gamez.zbd` and **measured** the other seven
//! (`docs/findings/2026-10-04-m01-lc-world-import.md`), so a regression in any of
//! them was invisible. This file closes that: the same production path —
//! `cs_app::world::read_world_containers` → [`RetailWorldContainer::definition`]
//! → [`RetailWorldContainer::uploaded_meshes`] → the production `spawn_world` —
//! over **all eight** world groups production discovery finds.
//!
//! Three things are pinned, and they are three different claims:
//!
//! * **The grid is read, per group.** Each container's own `x_count × y_count`,
//!   its value count and its distinct-slot count, so a reader that stopped
//!   reading a partition, or started inventing cells, fails here.
//! * **The three ownership statements agree, per group.** The grid's records plus
//!   the world node's own stored child list is exactly the set of records that
//!   name the world node, so `objects == values + child_list` in every container.
//!   That relation is the import's cross-check restated, and it is what makes a
//!   per-group object count a measurement rather than a number the import chose.
//! * **The gaps are named, per group.** Every unresolved collision role and
//!   every unresolved surface carries its own claim id, the world has no invented
//!   boundary, and the unit factor is reported at its own evidence class.
//!
//! **One discovery, eight containers.** Production discovery hashes the whole
//! installation tree (measured on this host: about half a minute warm), and it is
//! a property of the *installation* rather than of one group. Calling
//! `read_world_container` eight times would pay that eight times for one manifest;
//! [`read_world_containers`] pays it once and hands every group out of it, which
//! is the seam this test exists to exercise.
//!
//! **What this test does not claim.** It runs the production spawn, so anything
//! the physics backend refuses shows up here rather than in flight. It does
//! **not** claim the original engine behaved this way: `retail` is file access
//! and no original run happened, so nothing here is `verified_original`.
//!
//! Since task #656, `c3`'s settle blocker is **resolved, not pinned**: the
//! declared [`SUBNORMAL_POSITION_CLAIM`] canonicalisation flushes a subnormal
//! stored position component to the signed zero of its own sign at the upload
//! boundary, so the parry extent that used to panic the binned BVH builder is
//! exactly zero. `the_declared_flush_reaches_the_collider` reproduces the
//! mechanism on authored bytes (the corpus's own bit patterns) and runs in CI;
//! `the_subnormal_blocker_is_canonicalised` re-measures the real container.
//!
//! The retail half is `#[ignore]`d (`requires CS_GAME_DIR`): CI has no original
//! data. Every number below is a count, a dimension or a relation — no name list,
//! no mesh, no screenshot — so nothing derived from the original bytes is
//! committed.

use std::collections::{BTreeMap, BTreeSet};

use avian3d::prelude::Collider;
use bevy::mesh::{Mesh, VertexAttributeValues};
use cs_app::render::bevy_mesh::SUBNORMAL_POSITION_CLAIM;
use cs_app::world::retail::stored_presentation_unknowns;
use cs_app::world::{
    HARBOR_OBJECT_HANGAR, MESH_SETTLE_UPDATES, RETAIL_WORLD_IMPORT, RetailWorldContainer,
    RetailWorldContainers, SkipReason, harbor_meshes, harbor_world, instance_placement,
    mesh_reference, read_world_containers, spawn_world, world_app,
};
use cs_content::coordinates::{CoordinateSource, SourceAdapter};
use cs_content::mesh::RenderMesh;
use cs_content::textures::WorldTextureLoad;
use cs_content::world::{UNINDEXED_ROLE_UNMEASURED, WORLD_SURFACE_UNMEASURED, WorldPartitionGrid};
use cs_formats::gamez::{PrimitiveKind, RawCorner, RawMesh, RawPolygon};
use cs_types::content::{Origin, Resolved};
use cs_types::evidence::ClaimStatus;

/// The measured shape of one world container, as tabulated by task #629 in
/// `docs/findings/2026-10-04-m01-lc-world-import.md` and re-taken here through
/// the production path.
///
/// `values + child_list == objects` in every row, and the test asserts that
/// relation rather than trusting any single column.
struct Measured {
    /// The group's own directory spelling, as production discovery spells it.
    group: &'static str,
    /// The container's logical key inside the installation.
    container_key: &'static str,
    /// The world record's own stored grid dimensions.
    grid: (u32, u32),
    /// How many records the grid names, distinct.
    values: usize,
    /// How many records the world node's own stored child list names.
    child_list: usize,
    /// How many records name the world node at all: the imported object count.
    objects: usize,
    /// How many of the grid's records bind a mesh.
    values_with_mesh: usize,
    /// How many imported records resolve a mesh reference.
    objects_with_mesh: usize,
    /// How many cells could be given no extent, so their records stay resident.
    cells_without_extent: usize,
    /// How many cells name no record at all.
    empty_cells: usize,
    /// How many records store a matrix their own euler triple disagrees with.
    matrix_disagreements: usize,
    /// How many mesh-binding records the container holds that the world does not own.
    elsewhere: usize,
    /// How many of this container's records end up **resident**: the unindexed ones
    /// plus the indexed ones whose cell could not be given an extent.
    resident: usize,
    /// How many unindexed records resolve to `None` — the anchors, transform
    /// groups and dummies that store no geometry of their own (task #677's
    /// measured split: `anchors + fog + unresolved_roles == child_list`).
    anchors: usize,
    /// How many unindexed records resolve to `None` because their name carries
    /// the measured `fvol` prefix: fog volumes, presented and never blocking
    /// (task #716's measurement).
    fog: usize,
    /// How many unindexed records still carry an explicit unknown — the ones
    /// that store a mesh, an extent or both and whose name carries no
    /// measured prefix.
    unresolved_roles: usize,
    /// How many of this container's records store a transform that is not the
    /// identity, so the affine path is exercised on retail data.
    transformed: usize,
    /// Whether this group's colliders survive the settle on this host.
    ///
    /// `true` for every group since task #656: `C3` used to fail it on a
    /// subnormal stored extent (mesh slot 447) until the declared
    /// [`SUBNORMAL_POSITION_CLAIM`] canonicalisation flushed that extent to
    /// signed zero at the upload boundary — see
    /// [`accept_f18_world_units_containers_the_subnormal_blocker_is_canonicalised`].
    settles: bool,
}

/// Every world group the installation declares, with the numbers #629 measured.
///
/// **The table is the assertion.** These are the production readers' own counts
/// over the owner's bytes, so a reader that changed its walk fails here rather
/// than quietly agreeing with a value the import itself computed.
const MEASURED: [Measured; 8] = [
    Measured {
        group: "C1",
        container_key: "zbd/c1/gamez.zbd",
        grid: (12, 12),
        values: 346,
        child_list: 66,
        objects: 412,
        values_with_mesh: 285,
        objects_with_mesh: 295,
        cells_without_extent: 0,
        empty_cells: 0,
        // The one record in the whole corpus whose stored matrix disagrees with
        // its own euler triple, so the import's precedence rule is a measured
        // choice somewhere rather than always being free.
        matrix_disagreements: 1,
        elsewhere: 3671,
        resident: 66,
        anchors: 56,
        fog: 9,
        unresolved_roles: 1,
        transformed: 72,
        settles: true,
    },
    Measured {
        group: "C1B",
        container_key: "zbd/c1b/gamez.zbd",
        grid: (12, 12),
        values: 155,
        child_list: 78,
        objects: 233,
        values_with_mesh: 139,
        objects_with_mesh: 139,
        // Five cells whose members all store an all-zero box, so they get no
        // sector and their records stay resident.
        cells_without_extent: 5,
        empty_cells: 0,
        matrix_disagreements: 0,
        elsewhere: 3346,
        resident: 93,
        anchors: 78,
        fog: 0,
        unresolved_roles: 0,
        transformed: 79,
        settles: true,
    },
    Measured {
        group: "C1C",
        container_key: "zbd/c1c/gamez.zbd",
        grid: (12, 12),
        values: 293,
        child_list: 53,
        objects: 346,
        values_with_mesh: 292,
        objects_with_mesh: 309,
        cells_without_extent: 0,
        empty_cells: 0,
        matrix_disagreements: 0,
        elsewhere: 3045,
        resident: 53,
        anchors: 36,
        fog: 17,
        unresolved_roles: 0,
        transformed: 30,
        settles: true,
    },
    Measured {
        group: "C2",
        container_key: "zbd/c2/gamez.zbd",
        grid: (12, 12),
        values: 258,
        child_list: 24,
        objects: 282,
        values_with_mesh: 186,
        objects_with_mesh: 186,
        cells_without_extent: 0,
        empty_cells: 0,
        matrix_disagreements: 0,
        elsewhere: 2372,
        resident: 24,
        anchors: 24,
        fog: 0,
        unresolved_roles: 0,
        transformed: 30,
        settles: true,
    },
    Measured {
        group: "C2B",
        container_key: "zbd/c2b/gamez.zbd",
        grid: (12, 12),
        values: 290,
        child_list: 48,
        objects: 338,
        values_with_mesh: 289,
        objects_with_mesh: 298,
        cells_without_extent: 0,
        empty_cells: 0,
        matrix_disagreements: 0,
        elsewhere: 2741,
        resident: 48,
        anchors: 39,
        fog: 9,
        unresolved_roles: 0,
        transformed: 34,
        settles: true,
    },
    Measured {
        group: "C3",
        container_key: "zbd/c3/gamez.zbd",
        grid: (16, 16),
        values: 439,
        child_list: 14,
        objects: 453,
        values_with_mesh: 374,
        objects_with_mesh: 374,
        // Three cells whose members all store an all-zero box.
        cells_without_extent: 3,
        empty_cells: 0,
        matrix_disagreements: 0,
        elsewhere: 2494,
        resident: 19,
        anchors: 14,
        fog: 0,
        unresolved_roles: 0,
        // `true` since #656: the subnormal extent of stored mesh slot 447 is
        // canonicalised at the upload boundary, so the settle finishes and all
        // 374 colliders are built.
        transformed: 17,
        settles: true,
    },
    Measured {
        group: "C4",
        container_key: "zbd/c4/gamez.zbd",
        grid: (12, 12),
        values: 350,
        child_list: 51,
        objects: 401,
        values_with_mesh: 303,
        objects_with_mesh: 316,
        cells_without_extent: 0,
        empty_cells: 0,
        matrix_disagreements: 0,
        elsewhere: 4613,
        resident: 51,
        anchors: 38,
        fog: 9,
        unresolved_roles: 4,
        transformed: 57,
        settles: true,
    },
    Measured {
        group: "C5",
        container_key: "zbd/c5/gamez.zbd",
        grid: (16, 16),
        values: 471,
        child_list: 105,
        objects: 576,
        values_with_mesh: 367,
        objects_with_mesh: 452,
        // Four cells whose members all store an all-zero box.
        cells_without_extent: 4,
        // Three of c5's 256 cells name no record at all, which is why its cell
        // count and its sector count differ from the other 16x16 container's.
        empty_cells: 3,
        matrix_disagreements: 0,
        elsewhere: 5551,
        resident: 108,
        anchors: 20,
        fog: 15,
        unresolved_roles: 70,
        // The group whose stored mesh slots include sixteen the store holds no
        // geometry for; see
        // [`accept_f18_world_units_containers_a_mesh_the_store_holds_no_geometry_for_is_a_gap`].
        transformed: 165,
        settles: true,
    },
];

/// The world groups whose colliders the settle cannot finish on this host.
///
/// **Empty since task #656**: `C3` was the one blocker — parry's binned BVH
/// builder could not divide by the subnormal centroid extent its mesh slot 447
/// stores — until the upload boundary's declared
/// [`SUBNORMAL_POSITION_CLAIM`] rule flushed subnormal position components to
/// signed zero. The constant stays so a settle failure is still *measured
/// against the corpus* and reported as a new blocker, not skipped.
const SETTLE_BLOCKERS: [&str; 0] = [];

/// The retail root, or a loud failure.
///
/// CI has no `CS_GAME_DIR` and the tests that call this are `#[ignore]`d, so the
/// expectation is that the variable is set when they run. A test that skipped
/// itself here would report a pass it never earned.
fn retail_root() -> std::path::PathBuf {
    std::path::PathBuf::from(std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR is set"))
}

/// The conversion every import here is made through: the measured GameZ source
/// (task #677), whose scale landmark census pins one stored unit to the metre
/// at `observed_tool` while the rest of the convention stays uncalibrated.
fn adapter(container: &RetailWorldContainer) -> SourceAdapter {
    SourceAdapter::new(CoordinateSource::retail_gamez(container.span().clone()))
}

/// One production discovery, shared by every group.
fn retail() -> RetailWorldContainers {
    read_world_containers(&retail_root()).expect("the installation is discovered once")
}

/// The origin every container's import carries: the container's own bytes.
fn origin(container: &RetailWorldContainer) -> Origin {
    Origin::Installation {
        source: container.span().clone(),
    }
}

/// The mesh-array slot a definition-side mesh reference names.
///
/// The reference is a **catalog** element (#638): `mesh/<container>.<slot>`, the
/// one id the retail baseline inventory gives that stored mesh. So the slot is
/// the last dot-separated component, and the container part must name **this**
/// group's own container — which the C5 test asserts, because a name from another
/// container would mean the definition and the source had drifted apart.
fn mesh_slot(mesh: &cs_types::content::ContentId) -> Option<u32> {
    mesh.key()
        .rsplit_once('.')
        .and_then(|(_, index)| index.parse::<u32>().ok())
}

/// The catalog id one stored mesh slot of `container` has.
///
/// Built through the **catalog's own** spelling rule
/// ([`cs_content::catalog::baseline::mesh_content_id`], the same one
/// `MeshId::content_id` uses) rather than a second naming of this test's own, so
/// the test cannot disagree with the production id by construction.
fn catalog_mesh_id(container: &RetailWorldContainer, index: u32) -> cs_types::content::ContentId {
    cs_content::catalog::baseline::mesh_content_id(container.container_key(), index)
        .expect("a container's own mesh slot is inside the id grammar")
}

/// **Every world container of the installation is discovered, and every one of
/// them imports with the counts task #629 measured.**
///
/// This is the test #629's own limitations section asked for: it had imported
/// `c1c` only, so the other seven were measured but uncovered. Here the
/// production path runs over all eight, and the per-group grid dimensions, value
/// count, stored child-list length, object count and sector count are pinned.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f18_world_units_containers_every_world_group_imports_with_the_measured_counts() {
    let found = retail();

    // Discovery names the groups as the store spells them, and every reference
    // lead is present, so the loop below really is "every group".
    assert_eq!(
        found.groups(),
        MEASURED
            .iter()
            .map(|measured| measured.group.to_owned())
            .collect::<Vec<_>>(),
        "the discovered world groups are the measured ones, in discovered order"
    );
    assert!(
        found.absent_reference_groups().is_empty(),
        "every reference world-group lead is present: {:?}",
        found.absent_reference_groups()
    );
    assert_eq!(
        found.install_sha256().len(),
        64,
        "the installation fingerprint is a SHA-256 the containers' spans carry"
    );

    let mut imported_groups: Vec<String> = Vec::new();
    for measured in &MEASURED {
        let container = found
            .container(measured.group, &WorldTextureLoad::project_default())
            .unwrap_or_else(|error| panic!("{}: the container reads: {error}", measured.group));
        assert_eq!(
            container.container_key(),
            measured.container_key,
            "{}: the container is the one production discovery spells",
            measured.group
        );
        assert_eq!(
            container.group(),
            measured.group,
            "{}: the container knows which group it is",
            measured.group
        );

        // The grid the sector index is built from, read through the same
        // production reader the import itself calls.
        let grid: WorldPartitionGrid = container
            .partition_grid()
            .unwrap_or_else(|error| panic!("{}: the grid reads: {error}", measured.group));
        assert_eq!(
            (grid.x_count(), grid.y_count()),
            measured.grid,
            "{}: the world record's own grid dimensions",
            measured.group
        );
        assert_eq!(
            grid.value_count(),
            measured.values,
            "{}: one value per record the grid names",
            measured.group
        );
        let indexed = grid.indexed_slots();
        assert_eq!(
            indexed.len(),
            measured.values,
            "{}: every grid value names a distinct record",
            measured.group
        );
        assert_eq!(
            BTreeSet::from_iter(indexed.iter().copied()).len(),
            measured.values,
            "{}: no record is named twice, in one cell or across cells",
            measured.group
        );
        assert_eq!(
            grid.cells().len(),
            usize::try_from(measured.grid.0 * measured.grid.1).expect("a cell count fits"),
            "{}: one cell per grid position",
            measured.group
        );
        assert_eq!(
            grid.world_node(),
            0,
            "{}: and the grid belongs to the world record",
            measured.group
        );

        let imported = container
            .definition(origin(&container), &adapter(&container))
            .unwrap_or_else(|error| panic!("{}: the container imports: {error}", measured.group));
        let report = imported.report();
        let world = imported.definition();

        assert_eq!(
            world.id().key(),
            measured.group.to_ascii_lowercase(),
            "{}: the definition is the world the container is",
            measured.group
        );

        // The three ownership statements, as counts. The last assertion is the
        // relation between them, which is the import's own cross-check restated.
        assert_eq!(
            report.partition_records(),
            measured.values,
            "{}: the grid's record count",
            measured.group
        );
        assert_eq!(
            report.stored_child_list(),
            measured.child_list,
            "{}: the world node's own stored child list",
            measured.group
        );
        assert_eq!(
            report.objects(),
            measured.objects,
            "{}: the records that name the world node",
            measured.group
        );
        assert_eq!(
            measured.values + measured.child_list,
            measured.objects,
            "{}: the grid and the stored child list are disjoint and together are \
             exactly the records that name the world node",
            measured.group
        );
        assert_eq!(
            report.partition_cells(),
            grid.cells().len(),
            "{}: the report and the grid agree on the cell count",
            measured.group
        );
        assert_eq!(
            report.empty_cells(),
            measured.empty_cells,
            "{}: the cells that name no record at all. Measured over the \
             installation, only c5 has any, so a reader that assumed every cell \
             was populated fails here.",
            measured.group
        );
        assert_eq!(
            report.partition_records_with_mesh(),
            measured.values_with_mesh,
            "{}: how many indexed records bind a mesh",
            measured.group
        );
        assert_eq!(
            report.objects_with_mesh(),
            measured.objects_with_mesh,
            "{}: how many imported records resolve a mesh reference",
            measured.group
        );

        // The role follows the spatial index, so the solid count is the grid's.
        assert_eq!(
            report.objects_solid(),
            measured.values,
            "{}: one solid record per indexed record",
            measured.group
        );
        // Residency is a **separate** question from the role, and the two come
        // apart: an indexed record whose cell could not be given an extent has no
        // sector to belong to and therefore stays resident. So the resident count
        // is the stored child list **plus** the records in the cells that lost
        // their extent — which is why `c1b`, `c3` and `c5` (the three containers
        // with such cells) have more resident records than a child-list-only
        // reading would predict. Both halves are asserted, and their sum is the
        // object count, so a reader cannot get one without the other.
        assert_eq!(
            report.objects_resident(),
            measured.resident,
            "{}: the unindexed records plus the indexed ones in cells that could \
             not be given an extent",
            measured.group
        );
        assert_eq!(
            report.objects_in_a_sector() + report.objects_resident(),
            report.objects(),
            "{}: every object either lands in a sector or stays resident",
            measured.group
        );
        assert!(
            report.objects_resident() >= measured.child_list,
            "{}: the unindexed records are always resident, so the resident count \
             is at least the stored child list",
            measured.group
        );
        assert!(
            report.objects_resident() < measured.objects,
            "{}: and residency is a real state, not the whole world",
            measured.group
        );

        // Sectors: one per cell that can be given an extent, so a container whose
        // members all store an all-zero box in some cells declares fewer sectors
        // than cells and says which cells lost one.
        assert_eq!(
            report.sectors(),
            grid.cells().len() - measured.cells_without_extent,
            "{}: a cell whose members store no extent gets no sector",
            measured.group
        );
        assert_eq!(
            report.sectors_without_extent(),
            measured.cells_without_extent,
            "{}: the cells that could not be given an extent",
            measured.group
        );
        assert_eq!(
            world.sectors().len(),
            report.sectors(),
            "{}: and the definition declares exactly those sectors",
            measured.group
        );

        assert_eq!(
            report.matrix_disagreements(),
            measured.matrix_disagreements,
            "{}: how often the import's precedence over the euler triple was a choice",
            measured.group
        );
        assert_eq!(
            report.mesh_binding_records_elsewhere(),
            measured.elsewhere,
            "{}: the container's other mesh-binding records are counted, not imported",
            measured.group
        );

        // Task #677's measured split of the unindexed records: the ones that
        // store no geometry resolve to `None`, the ones that do keep the role
        // the container never states.
        assert_eq!(
            report.objects_unindexed_none(),
            measured.anchors,
            "{}: the unindexed records with no mesh and no extent resolve to `None`",
            measured.group
        );
        assert_eq!(
            report.objects_unindexed_fog(),
            measured.fog,
            "{}: the unindexed records carrying the measured `fvol` prefix are fog \
             volumes, presented and never blocking",
            measured.group
        );
        assert_eq!(
            report.objects_unindexed_unresolved(),
            measured.unresolved_roles,
            "{}: the unindexed records that store geometry and carry no measured \
             prefix keep an unknown role",
            measured.group
        );
        assert_eq!(
            measured.anchors + measured.fog + measured.unresolved_roles,
            measured.child_list,
            "{}: and the three classes partition the unindexed records exactly",
            measured.group
        );

        // Every gap is named, per group, with its own claim id.
        assert_eq!(
            world.unresolved_collision().len(),
            measured.unresolved_roles,
            "{}: exactly the unindexed records that store geometry and carry no \
             measured prefix have no measured role",
            measured.group
        );
        for object in world.unresolved_collision() {
            let Resolved::Unknown { claim_id, .. } = object.collision() else {
                panic!(
                    "{}: an unresolved role must be an explicit unknown",
                    measured.group
                );
            };
            assert_eq!(claim_id.as_str(), UNINDEXED_ROLE_UNMEASURED);
            assert!(
                !object.shape().is_known(),
                "{}: and its shape is unknown for the same reason",
                measured.group
            );
        }
        assert_eq!(
            world.unresolved_surface().len(),
            measured.objects,
            "{}: no gameplay surface is measured for any record",
            measured.group
        );
        for object in world.unresolved_surface() {
            let Resolved::Unknown { claim_id, .. } = object.surface() else {
                panic!(
                    "{}: an unresolved surface must be an explicit unknown",
                    measured.group
                );
            };
            assert_eq!(claim_id.as_str(), WORLD_SURFACE_UNMEASURED);
        }
        assert!(
            world.known_boundary().is_none(),
            "{}: the world's floor, ceiling and lateral rules are unmeasured, so no \
             boundary is invented",
            measured.group
        );

        // The unit is reported, never assumed: one stored unit is the metre at
        // `observed_tool` (task #677's landmark census) — the strongest claim a
        // byte census without an original run can carry — while the rest of the
        // source calibration stays unmeasured.
        assert_eq!(
            report.meters_per_unit(),
            adapter(&container).source().convention().meters_per_unit(),
            "{}: the factor is the caller's, reported",
            measured.group
        );
        assert_eq!(
            report.meters_per_unit(),
            1.0,
            "{}: and it is the metre, measured",
            measured.group
        );
        assert_eq!(
            report.unit_class(),
            ClaimStatus::ObservedTool,
            "{}: and its evidence class is tool-observed, never verified_original",
            measured.group
        );

        // Every retail-derived value points back at the bytes it was read from,
        // at a class that is measured-from-file and never `VerifiedOriginal`.
        for object in world.objects() {
            let provenance = object.provenance();
            assert_eq!(provenance.claim_id.as_str(), RETAIL_WORLD_IMPORT);
            assert_eq!(
                provenance.class,
                ClaimStatus::ObservedTool,
                "{}: reading container bytes is a tool observation, not an original run",
                measured.group
            );
            assert_eq!(
                provenance.source.as_ref(),
                Some(container.span()),
                "{}: and it names the container a reader can go back to",
                measured.group
            );
        }

        imported_groups.push(measured.group.to_owned());
    }

    assert_eq!(
        imported_groups.len(),
        8,
        "every discovered world group imported; none was skipped"
    );
}

/// **`spawn_world` accepts every world container's definition, and the spawn
/// report names every gap the container's own bytes imply.**
///
/// This is the half of #639 that `c1` and `c5` exist for: both hold grid records
/// that store a **real transform** rather than the identity, so running the
/// production [`spawn_world`] over them exercises the affine path on retail
/// data. Every group spawns into its own app, so one group's geometry cannot
/// reach another's colliders, and the geometry the definition names is uploaded
/// through the same production F17-B adapter the c1c path uses.
///
/// The collider counts are the **spawn's** own, checked against the import's, and
/// the skip report is checked against the two counts that can explain it. Where a
/// group cannot finish the settle, that is a separate measured fact, pinned by
/// [`accept_f18_world_units_containers_the_subnormal_blocker_is_canonicalised`],
/// and it does not weaken anything asserted here: the spawn itself accepted
/// every group.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f18_world_units_containers_every_world_group_spawns_and_reports_its_gaps() {
    let found = retail();

    let mut spawned_groups: Vec<String> = Vec::new();
    for measured in &MEASURED {
        let container = found
            .container(measured.group, &WorldTextureLoad::project_default())
            .unwrap_or_else(|error| panic!("{}: the container reads: {error}", measured.group));
        let imported = container
            .definition(origin(&container), &adapter(&container))
            .unwrap_or_else(|error| panic!("{}: the container imports: {error}", measured.group));
        let world = imported.definition();
        let report = imported.report();
        let meshes = container
            .uploaded_meshes(world)
            .unwrap_or_else(|error| panic!("{}: the geometry uploads: {error}", measured.group));

        let mut app = world_app();
        let spawned = match spawn_world(&mut app, world, &meshes) {
            Ok(spawned) => spawned,
            Err(error) => {
                // A stored matrix with no exact placement is a **typed refusal**
                // naming the object and the reason. It is never a truncation and
                // never a rounded pose, so it is reported here rather than
                // swallowed: a reader must see which group and which record could
                // not be placed.
                panic!(
                    "{}: spawn_world refused a stored transform: {error}",
                    measured.group
                );
            }
        };

        assert_eq!(
            spawned.objects().len(),
            world.objects().len(),
            "{}: every record the world owns is presented",
            measured.group
        );
        assert_eq!(
            spawned.colliders().len(),
            report.partition_records_with_mesh(),
            "{}: every indexed record that binds a mesh collides, from that mesh",
            measured.group
        );
        // Every collider the spawn reports is for a record whose own shape is
        // `FromMesh`, so the collider is derived from the geometry that record
        // draws and no substitute can have entered. This is checked on the
        // **spawn report** rather than on the built `Collider`, because a
        // mesh-derived `Collider` is attached by the physics backend during the
        // settle — measured per group in
        // `accept_f18_world_units_containers_the_subnormal_blocker_is_canonicalised`.
        for spawned_object in spawned.objects() {
            let Some(record) = world.object(&spawned_object.object) else {
                panic!("{}: every spawned object has a record", measured.group);
            };
            match (&spawned_object.collider, record.known_shape()) {
                (None, _) => {}
                (Some(_), Some(cs_content::world::WorldCollisionShape::FromMesh)) => {}
                (Some(_), shape) => panic!(
                    "{}: {} got a collider but its own shape is {shape:?}, so the \
                     collider is not derived from the geometry its record draws",
                    measured.group,
                    record.id().as_str()
                ),
            }
        }

        // And for the groups whose settle finishes, every collider really is a
        // triangle mesh built by the physics backend from that upload. The settle
        // is the shared helper the blocker test uses, so a group that cannot
        // finish it is measured the same way in both tests.
        if settles(&mut app) {
            for object in world.objects() {
                let Some(entity) = spawned.collider_for(object.id()) else {
                    continue;
                };
                let collider = app
                    .world()
                    .get::<Collider>(entity)
                    .expect("a settled mesh collider is a collider");
                assert!(
                    collider.shape().as_trimesh().is_some(),
                    "{}: a FromMesh record is collided by a triangle mesh, not a \
                     substitute shape",
                    measured.group
                );
            }
        } else {
            assert!(
                SETTLE_BLOCKERS.contains(&measured.group),
                "{}: the settle failed but no group is pinned as a blocker, so this \
                 is a new blocker and needs its own finding",
                measured.group
            );
        }

        // The skip report is **exactly** the gaps the container's own bytes
        // imply, with nothing double-reported. An unindexed record that stores
        // geometry reaches the role check first, so the mesh it also lacks —
        // or binds — is never reported twice.
        let mut reasons: BTreeMap<&str, usize> = BTreeMap::new();
        for entry in spawned.skipped() {
            *reasons.entry(entry.reason.label()).or_default() += 1;
        }
        assert_eq!(
            reasons
                .get(SkipReason::UnknownCollisionRole.label())
                .copied()
                .unwrap_or(0),
            measured.unresolved_roles,
            "{}: exactly the unindexed records that store geometry and carry no \
             measured prefix report an unknown role",
            measured.group
        );
        assert_eq!(
            reasons
                .get(SkipReason::UnknownMesh.label())
                .copied()
                .unwrap_or(0),
            measured.values - measured.values_with_mesh,
            "{}: and each indexed record the store binds no mesh for reports that \
             instead, having passed the role check",
            measured.group
        );
        assert_eq!(
            reasons.len(),
            1 + usize::from(measured.unresolved_roles > 0),
            "{}: no other gap reason appears",
            measured.group
        );
        // The anchors (task #677) and the fog volumes (task #716) are a
        // *deliberate* `None`: presented, never blocking, and not in the skip
        // list — a resolved record is not a gap.
        assert_eq!(
            spawned.non_colliding().len(),
            measured.anchors + measured.fog,
            "{}: the records that store no geometry and the `fvol*` records are \
             presented with no collider by their own answer",
            measured.group
        );
        assert_eq!(
            spawned.presentation_gap_count(),
            0,
            "{}: no object reports two reasons",
            measured.group
        );
        assert_eq!(
            spawned.colliders().len() + spawned.skipped_count() + spawned.non_colliding().len(),
            world.objects().len(),
            "{}: every object collides, is reported once, or is deliberately \
             non-colliding",
            measured.group
        );

        spawned_groups.push(measured.group.to_owned());
    }

    assert_eq!(
        spawned_groups.len(),
        8,
        "every discovered world group spawned; none was skipped"
    );
}

/// **A mesh the store holds no geometry for is a gap, not a refused world.**
/// (retail, `C5`)
///
/// Measured, and this task's production change exists because of it:
/// `ZBD/C5/gamez.zbd` names **16** of its stored mesh slots, and every one of
/// those slots decodes to an **empty polygon list and an empty position list**.
/// Before this task, [`RetailWorldContainer::uploaded_meshes`] propagated the
/// first [`NoGeometry`](cs_app::world::WorldMeshBuildError::NoGeometry) as a
/// refusal, so **one empty slot aborted the whole container**: `c5` produced no
/// world at all, and its other 346 meshes were unreachable. That is the opposite
/// of reporting a gap — it destroys a world over a hole the store itself states.
///
/// What is pinned:
///
/// * the empty slots **exist** in the container's own mesh array as **present**
///   records whose stored `polygon_count` and `vertex_count` are zero, so the
///   store holds a record there and states it has no geometry — rather than the
///   slot being out of range, an all-zero stub, or a reader that walked the
///   wrong offset; and
/// * the world still uploads, registers every mesh that does hold geometry, and
///   spawns with its colliders — with each record that named an empty slot
///   reported rather than silently collided or handed a substitute shape.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f18_world_units_containers_a_mesh_the_store_holds_no_geometry_for_is_a_gap() {
    let found = retail();
    let container = found
        .container("C5", &WorldTextureLoad::project_default())
        .expect("the c5 geometry container reads");
    let imported = container
        .definition(origin(&container), &adapter(&container))
        .expect("the c5 container imports");
    let world = imported.definition();

    // Measured, not assumed: walk the definition and ask the container's own mesh
    // array what each named slot holds.
    let mut empty: BTreeMap<u32, usize> = BTreeMap::new();
    let mut registered_slots: BTreeSet<u32> = BTreeSet::new();
    let mut named = 0usize;
    for object in world.objects() {
        let Resolved::Known(known) = object.mesh() else {
            continue;
        };
        let Some(index) = mesh_slot(&known.value) else {
            continue;
        };
        named += 1;
        let Some(slot) = container.meshes().get(index) else {
            continue;
        };
        // The slot exists in the container's own array — that is what makes this a
        // gap rather than an out-of-range reference — and it stores no geometry,
        // which is what makes it a gap rather than a decode failure.
        assert_eq!(slot.index, index, "the mesh array addresses slot {index}");
        if slot.mesh.polygons.is_empty() && slot.mesh.positions.is_empty() {
            // **The stored record says so.** The empty decode on its own would be
            // consistent with a reader that walked the wrong offset, so the
            // emptiness is pinned against the container's own 100-byte record: a
            // **present** mesh (`parent_count` non-zero, so not an all-zero array
            // stub) whose stored `polygon_count` and `vertex_count` are both zero.
            // That is the store stating it has nothing there, which is the whole
            // reason the upload treats it as a gap rather than a refusal.
            assert_ne!(
                slot.info.parent_count, 0,
                "slot {index} is a present mesh record, not an all-zero stub"
            );
            assert_eq!(
                (slot.info.polygon_count, slot.info.vertex_count),
                (0, 0),
                "slot {index} states zero stored polygon records and zero stored \
                 positions, so the empty decode is what the bytes say"
            );
            *empty.entry(index).or_default() += 1;
        } else if !slot.mesh.polygons.is_empty() {
            // The control: a slot that decodes polygons states a non-zero stored
            // count, so the assertion above discriminates on the store's own
            // record rather than holding for every mesh in the container.
            assert!(
                slot.info.polygon_count > 0,
                "slot {index} decodes polygons, so its own stored record states some"
            );
        }
    }

    assert!(
        named > 0,
        "c5 names mesh slots at all, so the walk above reached records"
    );
    assert_eq!(
        empty.len(),
        16,
        "c5 names exactly 16 mesh slots the store holds no geometry for"
    );
    let affected: usize = empty.values().sum();
    assert_eq!(affected, 61, "and 61 of its records name one of them");

    // The container uploads anyway, and **every distinct mesh the store holds
    // geometry for is registered**. The counts differ from the record counts on
    // purpose: `objects_with_mesh` counts *records*, several records share one
    // stored mesh, and this is the count of distinct *meshes*. Both are asserted,
    // so neither can drift without the other moving.
    let meshes = container
        .uploaded_meshes(world)
        .expect("an empty mesh slot is a gap, not a refusal of the whole container");
    let mut distinct_named = 0usize;
    for object in world.objects() {
        let Resolved::Known(known) = object.mesh() else {
            continue;
        };
        let Some(index) = mesh_slot(&known.value) else {
            continue;
        };
        if !empty.contains_key(&index) && !registered_slots.contains(&index) {
            registered_slots.insert(index);
            distinct_named += 1;
        }
    }
    assert_eq!(
        meshes.len(),
        distinct_named,
        "every distinct mesh the store does hold geometry for is registered, and \
         nothing else is"
    );
    assert!(
        meshes.len() < imported.report().objects_with_mesh(),
        "the mesh count is below the record count because several records name one \
         stored mesh: {} meshes for {} records",
        meshes.len(),
        imported.report().objects_with_mesh()
    );
    for index in empty.keys() {
        let id = catalog_mesh_id(&container, *index);
        assert!(
            !meshes.contains(&id),
            "the empty slot is not registered: nothing is drawn or collided from \
             a mesh the store does not hold"
        );
    }

    // And the spawn reports each affected record. They all name an **unindexed**
    // record, so the role check reaches them first and the reason is the unknown
    // role rather than a missing upload — measured, and the reason matters: it
    // says the record was never measured, not that a download failed.
    let mut app = world_app();
    let spawned = spawn_world(&mut app, world, &meshes).expect("the world spawns");
    let mut reported = 0usize;
    let mut collided = 0usize;
    for object in world.objects() {
        let Resolved::Known(known) = object.mesh() else {
            continue;
        };
        let names_empty = mesh_slot(&known.value).is_some_and(|index| empty.contains_key(&index));
        if !names_empty {
            continue;
        }
        let entry = spawned
            .object(object.id())
            .expect("every object has a spawn row");
        assert_eq!(
            entry.skipped,
            Some(SkipReason::UnknownCollisionRole),
            "{}: a record naming an empty mesh slot is reported, not collided",
            object.id().as_str()
        );
        assert!(
            entry.collider.is_none(),
            "{}: and it is given no collider at all, so no substitute shape stands \
             in for geometry the store does not hold",
            object.id().as_str()
        );
        reported += 1;
        collided += usize::from(entry.collider.is_some());
    }
    assert_eq!(reported, affected, "every affected record is reported once");
    assert_eq!(collided, 0, "and none of them collides");
}

/// **A stored mesh whose positions carry subnormal components settles into a
/// built collider, because the upload boundary canonicalises exactly those
/// components.** (synthetic — no `CS_GAME_DIR` needed)
///
/// This is the corpus mechanism of `c3` mesh slot 447 reproduced without
/// original bytes: three triangles authored so their leaf-AABB centers span a
/// **subnormal** extent along `y` — the one input parry 0.27's binned BVH
/// builder cannot bin, because it divides by that extent — using the same
/// stored bit patterns the container carries (`0x0000_0003` ≈ `4e-45`,
/// `0x8000_0006` ≈ `-8e-45`). The authored mesh replaces the harbor world's
/// hangar geometry, so the settle builds a real `TrimeshFromMesh` collider
/// from the uploaded buffer.
///
/// Removing [`SUBNORMAL_POSITION_CLAIM`] fails this test twice over: the
/// uploaded positions then keep the subnormal bit patterns, and the settle
/// panics inside `App::update`.
#[test]
fn accept_f18_world_units_containers_the_declared_flush_reaches_the_collider() {
    assert_eq!(
        SUBNORMAL_POSITION_CLAIM, "f17-b.subnormal-position-flushes-to-zero",
        "the canonicalisation rule carries its own claim id"
    );

    // The corpus's own bit patterns on an otherwise flat plane: `+denormal`
    // and `-denormal`, with every other component a stored `±0.0`. Three
    // triangles share the plane patch: parry's `Bvh::from_iter` special-cases
    // one and two leaves, so three is the smallest input that reaches the
    // binned partition. Every leaf's AABB center is `(0, subnormal, 0.5)`, so
    // the centroid extent the builder divides by is subnormal on `y` — the
    // one input it cannot bin.
    let corner = |position: u32| RawCorner {
        position,
        normal: None,
        uv: None,
        color: None,
    };
    let stored = RawMesh {
        positions: vec![
            [0.0, 0.0, 0.0],
            [0.0, f32::from_bits(0x0000_0003), 0.0],
            [0.0, f32::from_bits(0x8000_0006), 0.0],
            [0.0, 0.0, 1.0],
        ],
        normals: Vec::new(),
        polygons: vec![
            RawPolygon {
                kind: PrimitiveKind::Polygon,
                raw_flags: 0,
                material: 0,
                corners: vec![corner(0), corner(1), corner(3)],
            },
            RawPolygon {
                kind: PrimitiveKind::Polygon,
                raw_flags: 0,
                material: 0,
                corners: vec![corner(0), corner(2), corner(3)],
            },
            RawPolygon {
                kind: PrimitiveKind::Polygon,
                raw_flags: 0,
                material: 0,
                corners: vec![corner(1), corner(2), corner(3)],
            },
        ],
    };
    let render = RenderMesh::build(&stored).expect("the authored mesh has a decodable outline");
    let stored_subnormals: Vec<u32> = render
        .vertices()
        .iter()
        .flat_map(|vertex| vertex.position)
        .filter(|component| component.is_subnormal())
        .map(|component| component.to_bits())
        .collect();
    assert_eq!(
        stored_subnormals,
        vec![0x0000_0003, 0x8000_0006],
        "the authored premise: exactly the two corpus bit patterns, subnormal \
         in the IR — the reader is faithful"
    );

    // The production upload path: the harbor world's own mesh source, with the
    // hangar's geometry replaced by the authored mesh under its real id.
    let mut meshes = harbor_meshes();
    let hangar = mesh_reference(HARBOR_OBJECT_HANGAR)
        .known()
        .expect("the fixture hangar reference is known");
    meshes
        .insert_render_mesh(
            hangar.clone(),
            &render,
            &stored_presentation_unknowns(&render),
        )
        .expect("the authored mesh uploads through the production adapter");
    let uploaded = meshes.get(&hangar).expect("the mesh is registered");
    assert_eq!(
        uploaded.subnormal_components(),
        2,
        "the declared rule reports flushing exactly the two stored components"
    );
    let positions = uploaded_positions(uploaded);
    assert_eq!(
        positions
            .iter()
            .flatten()
            .filter(|bits| f32::from_bits(**bits).is_subnormal())
            .count(),
        0,
        "no subnormal reaches the buffer the collider is derived from"
    );
    for bits in [0x0000_0000u32, 0x8000_0000] {
        assert!(
            positions.iter().any(|position| position[1] == bits),
            "the subnormal uploads as the signed zero of its own sign: {bits:#x}"
        );
    }

    // The production spawn and settle: the object draws and collides from one
    // shared handle, so the flushed buffer is what parry bins.
    let world = harbor_world().expect("the synthetic harbor world is well formed");
    let mut app = world_app();
    let spawned = spawn_world(&mut app, &world, &meshes).expect("the world spawns");
    assert!(
        settles(&mut app),
        "the settle builds the subnormal mesh's collider: without the declared \
         flush the binned BVH builder panics inside App::update here"
    );
    let hangar_id = world
        .objects()
        .iter()
        .find(|object| object.id().as_str() == HARBOR_OBJECT_HANGAR)
        .expect("the harbor world has the hangar record")
        .id()
        .clone();
    let entity = spawned
        .collider_for(&hangar_id)
        .expect("the hangar's record collides");
    let collider = app
        .world()
        .get::<Collider>(entity)
        .expect("its collider is built once the settle finishes");
    assert!(
        collider.shape().as_trimesh().is_some(),
        "and it is the triangle mesh itself, not a substitute shape"
    );
}

/// The uploaded position buffer of one world mesh, as bit patterns, so no
/// comparison depends on float equality.
fn uploaded_positions(mesh: &cs_app::world::WorldMesh) -> Vec<[u32; 3]> {
    match mesh.mesh().attribute(Mesh::ATTRIBUTE_POSITION) {
        Some(VertexAttributeValues::Float32x3(values)) => {
            values.iter().map(|value| value.map(f32::to_bits)).collect()
        }
        other => panic!("positions are Float32x3, got {other:?}"),
    }
}

/// **The `c3` settle blocker is resolved by a declared canonicalisation at the
/// upload boundary, and the stored bytes — not a substitute — prove it.**
/// (retail)
///
/// #639 measured the blocker: `c3`'s mesh slot 447 stores two position `y`
/// components as subnormals (`0x0000_0003`, `0x8000_0006`) on an otherwise
/// exactly-`y = 0` plane; parry's binned BVH builder divides by the leaf-center
/// extent, the subnormal extent overflows the `f32` division, the bin index
/// saturates to `usize::MAX`, and the 8-entry bin array is indexed out of
/// bounds — a panic inside `App::update` that left 33 of c3's 374 colliders
/// unbuilt.
///
/// #656 resolves it with [`SUBNORMAL_POSITION_CLAIM`] — a stored position
/// component that is subnormal uploads as the signed zero of its own sign, and
/// nothing else changes — so the builder's extent becomes exactly `0`, the
/// input that always binned rather than panicked. This test asserts the
/// resolution rather than trusting it:
///
/// * the **stored** bytes still carry the two subnormals — the premise is
///   intact, the reader is still faithful;
/// * the production upload reports exactly those two components flushed,
///   hands Bevy a buffer with no subnormal left and no other value changed;
/// * **every** group's settle finishes, and every collider a group reports is
///   a built triangle mesh — for `c3` that is all **374**, the count the panic
///   kept this suite from reaching.
///
/// Removing the rule fails this test where #639 measured the failure: the
/// settle panics, `c3` goes back on the blocker list, and the
/// `subnormal_components` report loses its two.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f18_world_units_containers_the_subnormal_blocker_is_canonicalised() {
    let found = retail();

    // The slot is a constant because it is a property of the corpus, like every
    // other number here.
    const C3_SUBNORMAL_SLOT: u32 = 447;
    let container = found
        .container("C3", &WorldTextureLoad::project_default())
        .expect("the c3 geometry container reads");
    let slot = container
        .meshes()
        .get(C3_SUBNORMAL_SLOT)
        .expect("c3 holds the blocking mesh slot");

    // The premise is intact: the stored bytes still carry exactly the two
    // subnormal components the blocker was measured on, decoded verbatim.
    let subnormal_bits: BTreeSet<u32> = slot
        .mesh
        .positions
        .iter()
        .flatten()
        .filter(|component| component.is_subnormal())
        .map(|component| component.to_bits())
        .collect();
    assert_eq!(
        subnormal_bits,
        BTreeSet::from([0x0000_0003, 0x8000_0006]),
        "the stored bytes are unchanged, so what unblocks the settle is the \
         declared canonicalisation, not a changed corpus"
    );
    // The canonicalisation, stated: each subnormal component becomes the signed
    // zero of its own sign, every other bit pattern is kept. Building the
    // expected set from the rule rather than from the upload keeps the
    // comparison honest.
    let canonical: BTreeSet<[u32; 3]> = slot
        .mesh
        .positions
        .iter()
        .map(|position| {
            position.map(|component| {
                if component.is_subnormal() {
                    0.0f32.copysign(component)
                } else {
                    component
                }
            })
        })
        .map(|position| position.map(f32::to_bits))
        .collect();
    let flushed: BTreeSet<[u32; 3]> = slot
        .mesh
        .positions
        .iter()
        .filter(|position| position.iter().any(|component| component.is_subnormal()))
        .map(|position| {
            position.map(|component| {
                if component.is_subnormal() {
                    0.0f32.copysign(component)
                } else {
                    component
                }
            })
        })
        .map(|position| position.map(f32::to_bits))
        .collect();

    // Per group: does the settle finish? The answer is a property of the corpus
    // on this host, and it is asserted rather than assumed.
    let mut blockers: Vec<String> = Vec::new();
    for measured in &MEASURED {
        let group_container = found
            .container(measured.group, &WorldTextureLoad::project_default())
            .unwrap_or_else(|error| panic!("{}: the container reads: {error}", measured.group));
        let imported = group_container
            .definition(origin(&group_container), &adapter(&group_container))
            .unwrap_or_else(|error| panic!("{}: the container imports: {error}", measured.group));
        let world = imported.definition();
        let meshes = group_container
            .uploaded_meshes(world)
            .unwrap_or_else(|error| panic!("{}: the geometry uploads: {error}", measured.group));
        let mut app = world_app();
        let spawned = spawn_world(&mut app, world, &meshes)
            .unwrap_or_else(|error| panic!("{}: the world spawns: {error}", measured.group));
        // The spawn's own acceptance is asserted, not assumed: this test is about
        // the settle that *follows* it, and its collider count is the number the
        // settle is trying to realise.
        assert_eq!(
            spawned.colliders().len(),
            imported.report().partition_records_with_mesh(),
            "{}: the spawn reports every indexed record that binds a mesh, and \
             those are the colliders the settle has to build",
            measured.group
        );

        // On the group the blocker came from, the canonicalisation is checked
        // on the uploaded mesh itself — before the settle — so "the rule ran"
        // is measured, not inferred from the panic being gone.
        if measured.group == "C3" {
            let uploaded = meshes
                .get(&catalog_mesh_id(&group_container, C3_SUBNORMAL_SLOT))
                .expect("the once-blocking mesh is registered");
            assert_eq!(
                uploaded.subnormal_components(),
                2,
                "C3: the declared rule reports flushing exactly the two stored \
                 subnormal components"
            );
            let uploaded_positions = uploaded_positions(uploaded);
            assert!(
                uploaded_positions
                    .iter()
                    .flatten()
                    .all(|bits| !f32::from_bits(*bits).is_subnormal()),
                "C3: no subnormal component reaches the buffer the collider is \
                 derived from"
            );
            for position in &uploaded_positions {
                assert!(
                    canonical.contains(position),
                    "C3: every uploaded position is a stored position or its \
                     signed-zero canonicalisation, so nothing else changed: \
                     {position:?}"
                );
            }
            for position in &flushed {
                assert!(
                    uploaded_positions.contains(position),
                    "C3: the stored subnormal vertices upload as their signed \
                     zeros, not some other value: {position:?}"
                );
            }
        }

        let settled = settles(&mut app);
        if settled != measured.settles {
            panic!(
                "{}: the settle outcome flipped against the measured table \
                 (settles = {}): a new blocker is named by failing here, not \
                 skipped",
                measured.group, measured.settles
            );
        }
        if !settled {
            blockers.push(measured.group.to_owned());
            continue;
        }
        // Every collider the spawn reports is actually built and is the record's
        // own triangle mesh: for `c3` that is all 374, not the 341 the panic
        // left behind.
        let mut built = 0usize;
        for object in world.objects() {
            let Some(entity) = spawned.collider_for(object.id()) else {
                continue;
            };
            let collider = app
                .world()
                .get::<Collider>(entity)
                .expect("a settled mesh collider is a collider");
            assert!(
                collider.shape().as_trimesh().is_some(),
                "{}: a FromMesh record is collided by a triangle mesh, not a \
                 substitute shape",
                measured.group
            );
            built += 1;
        }
        assert_eq!(
            built,
            spawned.colliders().len(),
            "{}: every collider the spawn reports is built after the settle",
            measured.group
        );
        // And the records that name the once-blocking mesh specifically:
        // their colliders are built, from that mesh.
        if measured.group == "C3" {
            let mut consumers = 0usize;
            for object in world.objects() {
                let Resolved::Known(mesh) = object.mesh() else {
                    continue;
                };
                if mesh_slot(&mesh.value) != Some(C3_SUBNORMAL_SLOT) {
                    continue;
                }
                consumers += 1;
                let entity = spawned
                    .collider_for(object.id())
                    .expect("a record naming the mesh has a collider");
                assert!(
                    app.world().get::<Collider>(entity).is_some(),
                    "C3: the collider on slot {C3_SUBNORMAL_SLOT}'s geometry is \
                     built, for {}",
                    object.id().as_str()
                );
            }
            assert!(
                consumers > 0,
                "C3: at least one record names the once-blocking mesh slot"
            );
        }
    }

    assert_eq!(
        blockers, SETTLE_BLOCKERS,
        "no group is a settle blocker on this host anymore: every group's \
         colliders finish building"
    );
}

/// The panic hook is **process-global**, so the silence window below is taken
/// under this lock.
///
/// Without it two of these tests, which libtest runs on parallel threads, would
/// interleave their save/restore and the last one out would install the *other*
/// test's no-op hook as the process's permanent one — silently swallowing the
/// backtrace of every later failure in this binary. One settle at a time keeps
/// the swap strictly nested, and the hook a test finds is always the one it left.
static SETTLE_HOOK_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Runs the settle updates, reporting whether they all completed.
///
/// The physics backend builds mesh-derived colliders inside Bevy systems, so a
/// refusal surfaces as a panic escaping `App::update`. Catching it here is what
/// turns "the run crashed" into "this group is a named blocker", which is the
/// difference between a measurement and an unexplained failure. The panic hook is
/// silenced for the duration so a known blocker does not print a backtrace that
/// reads like a test failure, and the loop **stops at the first** failure: a
/// backend that panicked mid-frame is not in a state where running more frames
/// measures anything.
pub(crate) fn settles(app: &mut bevy::prelude::App) -> bool {
    // A panic while the lock is held would poison it; the measurement itself is
    // what failed, and the other test still needs its own settle window.
    let _guard = SETTLE_HOOK_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let mut settled = true;
    for _ in 0..MESH_SETTLE_UPDATES {
        if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| app.update())).is_err() {
            settled = false;
            break;
        }
    }
    std::panic::set_hook(hook);
    settled
}

/// **Every stored transform in every world container places exactly.** (retail)
///
/// `c1` and `c5` hold grid records that store a real transform rather than the
/// identity, and the task named them as the reason the affine path is exercised
/// on retail data. This asserts the two halves of that: **how many** records per
/// group store one (a measured corpus fact, so a reader that started dropping
/// transforms is caught), and that
/// [`instance_placement`](cs_app::world::instance_placement) — the production
/// classifier the spawn runs over every instance **before** it spawns anything —
/// places **every** record of **every** group exactly.
///
/// An exactly-placeable assertion rather than a "nothing refused" one: a stored
/// matrix with no exact placement would be a typed refusal, and a rounded pose
/// standing in for it would be F18 non-negotiable behavior 1 violated.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f18_world_units_containers_every_stored_transform_places_exactly() {
    /// The identity, as an imported transform spells it.
    const IDENTITY_LINEAR: [[f64; 3]; 3] = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];

    let found = retail();
    let mut classified = 0usize;
    let mut with_transform = 0usize;
    for measured in &MEASURED {
        let container = found
            .container(measured.group, &WorldTextureLoad::project_default())
            .unwrap_or_else(|error| panic!("{}: the container reads: {error}", measured.group));
        let imported = container
            .definition(origin(&container), &adapter(&container))
            .unwrap_or_else(|error| panic!("{}: the container imports: {error}", measured.group));
        let world = imported.definition();

        let transformed = world
            .objects()
            .iter()
            .filter(|object| {
                object.transform().linear() != IDENTITY_LINEAR
                    || object.transform().translation() != [0.0, 0.0, 0.0]
            })
            .count();
        assert_eq!(
            transformed, measured.transformed,
            "{}: how many records store a real transform",
            measured.group
        );
        with_transform += transformed;

        for object in world.objects() {
            let outcome = instance_placement(object);
            assert!(
                outcome.is_ok(),
                "{}: {} stores a transform with no exact placement: {outcome:?}",
                measured.group,
                object.id().as_str()
            );
            classified += 1;
        }
    }

    assert!(
        with_transform > 0,
        "at least one record stores a transform, so the affine path is exercised \
         on retail data rather than only on a fixture"
    );
    assert_eq!(
        classified,
        412 + 233 + 346 + 282 + 338 + 453 + 401 + 576,
        "every record of every world group was classified; none was skipped"
    );
}
