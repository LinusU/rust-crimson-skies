//! #771 (`M01-LC-WORLD-RESIDUAL-ROLES`): the world_geometry surface's last
//! unanswered records — a grid-named `fvol*` volume and the grid record that
//! stores no mesh — answered from measured evidence and bound into the import.
//!
//! Spec: `specs/F18-world-geometry-terrain-water-and-traversable-interiors.md`
//! (`### F18-B` and `### F18-D`). Shared contract:
//! `docs/contracts/IDENTITY-CONTENT.md`. Task test prefix:
//! `accept_m01_lc_world_residual_roles_`. The measurement itself, with every
//! address and its evidence class, is
//! `docs/findings/2026-10-08-m01-lc-world-residual-roles.md`.
//!
//! Two statements about the same records were still open when this task
//! started, and each is settled here by something measured rather than by
//! preference:
//!
//! * **A grid-named `fvol*` record.** Task #716 measured that the image's only
//!   name-keyed consumer of the `fvol` prefix is its fog system; task #727
//!   measured that the grid is a broad-phase *candidate* index and left these
//!   records role-unknown because the store states no collision field. What
//!   neither had measured is the candidate's own filter: `cls_di.c`'s walk
//!   reads the record's flags word at `0x4cb635` and reaches the narrow phase
//!   — the only branch that copies a box through `[node+0x70]` — only with
//!   [`INTERSECTION_NARROW_PHASE_FLAG`] set (`0x4cb638`, `0x4cb665`), and
//!   otherwise drops the candidate before any box test (`0x4cb63c`–`0x4cb642`).
//!   A record storing that bit clear resolves [`WorldCollisionRole::None`]
//!   under [`FOG_VOLUME_RECORD_NEVER_BLOCKS`]; one storing it set is a live
//!   candidate and keeps [`GRID_NAMED_FOG_VOLUME_ROLE_UNMEASURED`], which is
//!   what makes this a measurement and not a blanket rule.
//! * **The grid record that stores no mesh.** `mesh_index = -1` is the store's
//!   own "no mesh", and when all three stored boxes are empty too, the
//!   container states no geometry a collider or a drawing could come from —
//!   the same store state task #677 resolved for *unindexed* records, now
//!   reached on a record the grid names ([`GRID_RECORD_STORES_NO_GEOMETRY`]).
//!
//! Both resolutions land in `spawn_world` as answers rather than gaps: the
//! synthetic half drives the production import and spawn over a fixture that
//! carries all three classes (a dropped fog volume, a kept one, an empty
//! record) plus the unindexed records that were already answered; the retail
//! half, `#[ignore]`d (`requires CS_GAME_DIR`), is one discovery over all eight
//! world containers and the spawn over `c1c`'s real geometry — the surface
//! `geometry_verdict` reads.
//!
//! Nothing here is `verified_original`: static analysis of one executable plus
//! a byte census is `observed_tool`, and no original run happened (#358).

use std::collections::BTreeMap;

use cs_app::world::{
    MESH_SETTLE_UPDATES, RetailWorldContainers, SkipReason, read_world_containers, spawn_world,
    world_app,
};
use cs_content::coordinates::{CoordinateSource, SourceAdapter};
use cs_content::textures::WorldTextureLoad;
use cs_content::world::{
    FOG_VOLUME_RECORD_NEVER_BLOCKS, GRID_NAMED_FOG_VOLUME_ROLE_UNMEASURED,
    GRID_RECORD_STORES_NO_GEOMETRY, INDEXED_RECORD_IS_STATIC, INTERSECTION_NARROW_PHASE_FLAG,
    INTERSECTION_QUERY_GAMEPLAY_CONSUMER_UNMEASURED, OBJECT_STORES_NO_MESH,
    UNINDEXED_ROLE_UNMEASURED, WorldCollisionRole, WorldObjectId,
};
use cs_types::content::{Origin, Resolved};
use cs_types::evidence::ClaimId;

use super::import_retail::{
    Fixture, MARKER, ObjectSpec, SLAB, TILE_A, TILE_B, VOLUME, fixture_meshes, imported,
    write_container,
};

/// The fixture slot of the grid-named `fvol*` record whose stored flags drop
/// it before any box test — the class the original installation stores.
const FOG_DROPPED: u32 = 6;

/// The fixture slot of the grid-named `fvol*` record that stores the
/// narrow-phase bit, so the walk would copy its box: the class that keeps the
/// explicit unknown.
const FOG_KEPT: u32 = 7;

/// The fixture slot of the grid record that stores no mesh and no box.
const EMPTY: u32 = 8;

/// The mesh slots those two fog records bind: the fixture's last slot, shared
/// so the table never grows.
const MESH_FOG: i32 = 9;

/// The fixture as this task authors it: the default arrangement plus the three
/// records the question is about, all of them **in** the grid — the ownership
/// cross-check (grid ∪ stored child list = the records naming the world node)
/// still has to hold.
fn fixture() -> Fixture {
    let mut fixture = Fixture::default();
    fixture
        .objects
        .push(ObjectSpec::new("fvol_edge", MESH_FOG).extent([-6.0, 0.0, -6.0], [-5.0, 1.0, -5.0]));
    fixture.objects.push(
        ObjectSpec::new("fvol_kept", MESH_FOG)
            .extent([-4.0, 0.0, -4.0], [-3.0, 1.0, -3.0])
            // The installation's fog volumes store `0x0308831c`; this one
            // stores the same word with the narrow-phase bit set, which is
            // exactly the difference between being dropped and being tested.
            .flags(0x0308_831c | INTERSECTION_NARROW_PHASE_FLAG),
    );
    fixture.objects.push(ObjectSpec::new("empty", -1));
    fixture.grid = vec![
        vec![TILE_A, SLAB, FOG_DROPPED, EMPTY],
        vec![TILE_B, FOG_KEPT],
    ];
    fixture
}

/// The object id the fixture's node slot gets.
fn object(slot: u32) -> WorldObjectId {
    WorldObjectId::new(&format!("node-{slot}")).expect("a node slot key is valid")
}

/// **The two residual records resolve an answer instead of a gap, and the one
/// record that is still open stays open under its own claim id.**
///
/// Fails when the implementation is removed: without the flags test the
/// dropped fog volume inherits #727's unknown again (report counters and the
/// spawn's skip list move together), and without the store-state arm the empty
/// record inherits the index's `Solid` + `FromMesh` and skips as
/// `unknown_mesh`.
#[test]
fn accept_m01_lc_world_residual_roles_grid_records_resolve_answers_instead_of_gaps() {
    let imported = imported(&write_container(&fixture()));
    let world = imported.definition();
    let report = imported.report();

    // The report partitions the grid exactly: the index's solids, the overlap
    // the fog consumer keys, and the records that store no geometry.
    assert_eq!(
        report.partition_records(),
        6,
        "the grid names six records, the three default ones and this task's three"
    );
    assert_eq!(
        report.partition_records_fog_volume(),
        2,
        "both `fvol*` records are named by the grid, whichever way each resolves"
    );
    assert_eq!(
        report.partition_records_stores_no_geometry(),
        1,
        "and exactly one grid record stores no mesh and no box"
    );
    assert_eq!(
        report.partition_records(),
        report.objects_solid()
            + report.partition_records_fog_volume()
            + report.partition_records_stores_no_geometry(),
        "the three numbers partition the index exactly"
    );
    assert_eq!(
        report.objects_solid(),
        3,
        "the three records with a mesh and no measured exception are still `Solid`"
    );

    // The fog record the walk drops: role `None`, its absent collider explained
    // by the fog claim rather than by the index.
    let dropped = world.object(&object(FOG_DROPPED)).expect("it imported");
    let Resolved::Known(known) = dropped.collision() else {
        panic!(
            "a fog volume the walk drops has a measured role: {:?}",
            dropped.collision()
        );
    };
    assert_eq!(known.value, WorldCollisionRole::None);
    let Resolved::Unknown { claim_id, reason } = dropped.shape() else {
        panic!("its shape stays an explicit unknown, not a guess");
    };
    assert_eq!(claim_id.as_str(), FOG_VOLUME_RECORD_NEVER_BLOCKS);
    assert!(
        reason.contains("0x4cb635") && reason.contains("narrow-phase"),
        "the reason names the measured filter that dropped it: {reason}"
    );

    // The fog record the walk would keep: still an explicit unknown, so the
    // carve-out is keyed on the measurement and not on the name alone.
    let kept = world.object(&object(FOG_KEPT)).expect("it imported");
    let Resolved::Unknown { claim_id, .. } = kept.collision() else {
        panic!(
            "a live candidate's role is still the container's silence: {:?}",
            kept.collision()
        );
    };
    assert_eq!(
        claim_id.as_str(),
        GRID_NAMED_FOG_VOLUME_ROLE_UNMEASURED,
        "the record whose stored flags reach the narrow phase keeps #727's unknown"
    );

    // The empty grid record: no collider because the store states no geometry,
    // and its absent mesh stays readable as its own claim.
    let empty = world.object(&object(EMPTY)).expect("it imported");
    let Resolved::Known(known) = empty.collision() else {
        panic!(
            "the store states no geometry, so nothing blocks: {:?}",
            empty.collision()
        );
    };
    assert_eq!(known.value, WorldCollisionRole::None);
    let Resolved::Unknown { claim_id, .. } = empty.shape() else {
        panic!("and the shape is the store's own silence");
    };
    assert_eq!(claim_id.as_str(), GRID_RECORD_STORES_NO_GEOMETRY);
    let Resolved::Unknown { claim_id, .. } = empty.mesh() else {
        panic!("a record that stores no mesh says so rather than borrowing a reference");
    };
    assert_eq!(claim_id.as_str(), OBJECT_STORES_NO_MESH);

    // The records this task did not touch keep their answers and their gaps.
    for slot in [TILE_A, SLAB, TILE_B] {
        assert_eq!(
            world
                .object(&object(slot))
                .expect("it imported")
                .known_collision(),
            Some(WorldCollisionRole::Solid),
            "{INDEXED_RECORD_IS_STATIC} still resolves the rest of the index"
        );
    }
    let unindexed = world.object(&object(VOLUME)).expect("it imported");
    let Resolved::Unknown { claim_id, .. } = unindexed.collision() else {
        panic!("the unindexed geometry-bearing record keeps the container's silence");
    };
    assert_eq!(claim_id.as_str(), UNINDEXED_ROLE_UNMEASURED);
    assert_eq!(
        report.objects_unindexed_unresolved(),
        1,
        "and it is still counted as unindexed"
    );

    // What is open, in the two numbers a launch verdict reads.
    assert_eq!(
        report.objects_unresolved_collision(),
        2,
        "the kept fog volume and the unindexed volume: nothing else is open"
    );
    assert_eq!(
        report.objects_unresolved_collision(),
        report.objects_unindexed_unresolved() + 1,
        "one of them is grid-named, which the unindexed counter does not count"
    );

    // The effect the task exists for: the spawn reports the two records whose
    // role nothing answered and never reports the three it answered.
    let meshes = fixture_meshes(world);
    let mut app = world_app();
    let spawned = spawn_world(&mut app, world, &meshes).expect("the fixture world spawns");
    for _ in 0..MESH_SETTLE_UPDATES {
        app.update();
    }
    assert_eq!(
        spawned.colliders().len(),
        3,
        "only the three solid tiles collide"
    );
    let reasons: BTreeMap<&str, usize> =
        spawned
            .skipped()
            .iter()
            .fold(BTreeMap::new(), |mut counts, entry| {
                *counts.entry(entry.reason.label()).or_default() += 1;
                counts
            });
    assert_eq!(
        reasons,
        BTreeMap::from([("unknown_collision_role", 2)]),
        "the kept fog volume and the unindexed volume report their gap; the dropped \
         fog volume and the empty record are answers, not gaps"
    );
    let skipped: Vec<SkipReason> = spawned.skipped().iter().map(|entry| entry.reason).collect();
    assert!(
        !skipped.contains(&SkipReason::UnknownMesh),
        "no record skips as `unknown_mesh`: {skipped:?}"
    );
    assert_eq!(
        spawned.non_colliding(),
        vec![object(MARKER), object(FOG_DROPPED), object(EMPTY)],
        "the anchor, the dropped fog volume and the empty record are presented and \
         never block, each by its own answer"
    );
    assert_eq!(
        spawned.presentation_gap_count(),
        0,
        "and no object reports two reasons"
    );

    // The residual claim this task names instead of guessing: which gameplay
    // query consumes the walk needs an original run, and the id is spelled.
    assert!(
        ClaimId::new(INTERSECTION_QUERY_GAMEPLAY_CONSUMER_UNMEASURED).is_ok(),
        "the named residual is a valid claim id a consumer can cite"
    );
}

/// **Over the original installation the surface is closed: `c1c`'s spawn
/// reports no `unknown_collision_role` and no `unknown_mesh`, and every
/// container's open-role count is the count its own bytes imply.**
///
/// One production discovery, all eight world containers, plus the spawn over
/// `c1c`'s real geometry through the production upload.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_lc_world_residual_roles_the_installation_leaves_nothing_unanswered_in_c1c() {
    let root = std::path::PathBuf::from(
        std::env::var("CS_GAME_DIR").expect("a retail test needs CS_GAME_DIR to be set"),
    );
    let found: RetailWorldContainers =
        read_world_containers(&root).expect("the installation is discovered once");
    // Measured through the production readers, once, by this task's census.
    let expected_empty: BTreeMap<&str, usize> = BTreeMap::from([
        ("C1", 13),
        ("C1B", 8),
        ("C1C", 1),
        ("C2", 23),
        ("C2B", 1),
        ("C3", 18),
        ("C4", 4),
        ("C5", 80),
    ]);
    let mut c1c = None;
    for group in found.groups() {
        let container = found
            .container(&group, &WorldTextureLoad::project_default())
            .unwrap_or_else(|error| panic!("{group}: the container reads: {error}"));
        let origin = Origin::Installation {
            source: container.span().clone(),
        };
        let adapter = SourceAdapter::new(CoordinateSource::retail_gamez(container.span().clone()));
        let imported = container
            .definition(origin, &adapter)
            .unwrap_or_else(|error| panic!("{group}: the container imports: {error}"));
        let report = imported.report();

        assert_eq!(
            report.partition_records_stores_no_geometry(),
            expected_empty[group.as_str()],
            "{group}: the grid records that store no mesh and no box, as measured"
        );
        assert_eq!(
            report.objects_unresolved_collision(),
            report.objects_unindexed_unresolved(),
            "{group}: every grid-named record resolves a role, so the open roles are \
             exactly the unindexed records the container says nothing about"
        );
        assert_eq!(
            report.partition_records(),
            report.objects_solid()
                + report.partition_records_fog_volume()
                + report.partition_records_stores_no_geometry(),
            "{group}: the index partitions into solids, fog volumes and empty records"
        );

        // The six grid-named `fvol*` records resolve the fog claim as role
        // `None`, each with the measured filter in its reason.
        for object in imported.definition().unresolved_collision() {
            let Resolved::Unknown { claim_id, .. } = object.collision() else {
                panic!("{group}: an unresolved role is an explicit unknown");
            };
            assert_eq!(
                claim_id.as_str(),
                UNINDEXED_ROLE_UNMEASURED,
                "{group}: the only open roles left are unindexed records; {} carries {}",
                object.id(),
                claim_id.as_str()
            );
        }
        if group == "C1C" {
            c1c = Some((container, imported));
        }
    }

    let (container, imported) = c1c.expect("the installation holds c1c");
    let report = imported.report();
    let world = imported.definition();
    assert_eq!(
        report.objects_unresolved_collision(),
        0,
        "c1c's surface is closed: nothing unanswered"
    );
    assert_eq!(
        report.partition_records_fog_volume(),
        4,
        "the four grid-named fog volumes are still counted, now as answers"
    );
    assert_eq!(
        report.partition_records_stores_no_geometry(),
        1,
        "and the one grid record that stores no geometry is answered too"
    );

    // The spawn over the real container's geometry: the verdict's own numbers.
    let meshes = container
        .uploaded_meshes(world)
        .expect("c1c's own meshes upload");
    let mut app = world_app();
    let spawned = spawn_world(&mut app, world, &meshes).expect("the imported world spawns");
    for _ in 0..MESH_SETTLE_UPDATES {
        app.update();
    }
    assert_eq!(
        spawned.skipped(),
        Vec::new(),
        "c1c's spawn reports zero skips: no `unknown_collision_role`, no `unknown_mesh`"
    );
    assert_eq!(
        spawned.colliders().len(),
        288,
        "every indexed record that binds a mesh collides, and the five records with \
         no measured role to build one from stay out of the collider list by answer"
    );
    assert_eq!(
        spawned.non_colliding().len(),
        58,
        "36 anchors, 17 unindexed fog volumes, the four grid-named fog volumes and \
         the one empty grid record: presented and never blocking, each by its own answer"
    );
    assert_eq!(
        spawned.presentation_gap_count(),
        0,
        "and no object reports two reasons"
    );
}
