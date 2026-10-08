//! #727 (`F18-GRID-COLLISION-ORIGIN`): what fed world collision in the original
//! engine, and the six grid-named fog volumes that question lands on.
//!
//! Spec: `specs/F18-world-geometry-terrain-water-and-traversable-interiors.md`
//! (`### F18-B` and `### F18-D`). Shared contract:
//! `docs/contracts/IDENTITY-CONTENT.md`. Task test prefix:
//! `accept_f18_grid_collision_origin_`. The measurement itself, with every
//! address and its evidence class, is
//! `docs/findings/2026-10-07-f18-grid-collision-origin.md`.
//!
//! Two statements about six records disagreed and nothing here decides by
//! assertion which one the 2000 engine meant:
//!
//! * a record the world's partition grid names becomes `Solid` — the designed
//!   rule [`INDEXED_RECORD_IS_STATIC`], which is a claim about *this*
//!   conversion and never about the original;
//! * a record whose name starts with `fvol` is taken by the original's own
//!   fog-volume consumer (the `strncmp` at VA `0x44e087` inside the routine at
//!   VA `0x44d9d0`), and the image shows the grid itself is loaded
//!   (`0x4e3081`–`0x4e3141`) and walked only as a **candidate** set
//!   (`0x4cb579`, filtered by node flags, a zone whitelist and an optional name)
//!   before any box is tested.
//!
//! So the binding this suite pins is: the grid names candidates, it does not
//! declare solidity, and what a grid-named record the original takes as fog
//! resolves is what the walk does with it — task #771 measured the filter at
//! `0x4cb635`: with the record's narrow-phase bit clear the candidate is
//! dropped before any box test, so it resolves `None` under
//! [`FOG_VOLUME_RECORD_NEVER_BLOCKS`] instead of `Solid`, and only a record
//! storing that bit keeps [`GRID_NAMED_FOG_VOLUME_ROLE_UNMEASURED`].
//!
//! The synthetic half needs no original data and runs in CI; the retail half is
//! `#[ignore]`d (`requires CS_GAME_DIR`) and names the six records the original
//! installation actually holds. Nothing here is `verified_original`: static
//! analysis of one executable plus a byte census is `observed_tool`, and no
//! original run happened.

use std::collections::{BTreeMap, BTreeSet};

use cs_app::world::{
    MESH_SETTLE_UPDATES, RetailWorldContainers, SkipReason, read_world_containers, spawn_world,
    world_app,
};
use cs_content::coordinates::{CoordinateSource, SourceAdapter};
use cs_content::textures::WorldTextureLoad;
use cs_content::world::{
    FOG_VOLUME_RECORD_NEVER_BLOCKS, GRID_NAMED_FOG_VOLUME_ROLE_UNMEASURED,
    INDEXED_RECORD_IS_STATIC, UNINDEXED_ROLE_UNMEASURED, WorldCollisionRole, WorldObjectId,
};
use cs_formats::gamez::RawNode;
use cs_types::content::{Origin, Resolved};

use super::import_retail::{
    Fixture, ObjectSpec, SLAB, TILE_A, TILE_B, fixture_meshes, imported, write_container,
};

/// The fixture slot the grid-named fog volume is written to: the first slot
/// after the five records the default fixture already owns.
const FOG: u32 = 6;

/// The mesh slot that record binds, inside the fixture's own mesh table.
const FOG_MESH: i32 = 9;

/// The fixture as this task authors it: the default arrangement plus one more
/// record that the grid names and whose name keys the original's fog consumer.
///
/// The ownership cross-check still has to hold — grid ∪ the stored child list
/// is exactly the set of records naming the world node — so the new record goes
/// into the grid rather than beside it.
fn fog_fixture() -> Fixture {
    let mut fixture = Fixture::default();
    fixture
        .objects
        .push(ObjectSpec::new("fvol_edge", FOG_MESH).extent([-6.0, 0.0, -6.0], [-5.0, 1.0, -5.0]));
    fixture.grid = vec![vec![TILE_A, SLAB, FOG], vec![TILE_B]];
    fixture
}

/// **A record the partition grid names but the original takes as fog is never
/// resolved `Solid`, and every other grid-named record still is.**
///
/// Fails when the implementation is removed: without the carve-out the fog
/// record inherits `Solid` from the index, so its claim id, the report's fog
/// counter and the spawn's collider list all move at once.
#[test]
fn accept_f18_grid_collision_origin_a_grid_named_fog_volume_is_never_solid_geometry() {
    let bytes = write_container(&fog_fixture());
    let imported = imported(&bytes);
    let world = imported.definition();
    let report = imported.report();

    // The index still sees four records, and the report says how many of them
    // the original takes as fog — a reader can reconcile the two numbers
    // instead of finding a wall where a fog bank was authored.
    assert_eq!(
        report.partition_records(),
        4,
        "the grid names four records, the fog one included"
    );
    assert_eq!(
        report.partition_records_fog_volume(),
        1,
        "and the report names exactly the one the original's fog consumer keys"
    );
    assert_eq!(
        report.objects_solid(),
        3,
        "so three records resolve the index's `Solid`, not four"
    );

    // The fog record's own role and shape are resolved by measured facts, not
    // by the index: task #727 showed the grid is a candidate index, and task
    // #771 measured the candidate's filter — with the stored narrow-phase bit
    // clear (this fixture stores `0x01800000`) the walk drops the record
    // before any box test, so it is presented and never blocks.
    let fog = world
        .object(&WorldObjectId::new(&format!("node-{FOG}")).expect("the key is valid"))
        .expect("the grid-named fog volume imported");
    assert_eq!(
        fog.known_collision(),
        Some(WorldCollisionRole::None),
        "a grid-named fog volume the image's walk drops has a measured role, and it \
         is never the index's `Solid`"
    );
    let shape = fog.shape();
    let Resolved::Unknown { claim_id, reason } = shape else {
        panic!("a fog volume builds no collider, so its shape stays an unknown: {shape:?}");
    };
    assert_eq!(
        claim_id.as_str(),
        FOG_VOLUME_RECORD_NEVER_BLOCKS,
        "the shape names the measured fog consumer, not the index"
    );
    assert!(
        reason.contains("partition grid") && reason.contains("narrow-phase"),
        "the reason names both statements: the grid it is named by and the filter \
         that drops it: {reason}"
    );

    // The carve-out does not spill: the other grid-named records keep the
    // designed rule exactly as it was written.
    for slot in [TILE_A, SLAB, TILE_B] {
        let object = world
            .object(&WorldObjectId::new(&format!("node-{slot}")).expect("the key is valid"))
            .expect("a grid-named record imported");
        assert_eq!(
            object.known_collision(),
            Some(WorldCollisionRole::Solid),
            "{INDEXED_RECORD_IS_STATIC} still resolves the rest of the index"
        );
        assert!(
            object.known_shape().is_some(),
            "and its collider is still the mesh it draws"
        );
    }

    // The unindexed records are untouched by the change: the mesh-bearing one
    // keeps the container's own silence, the anchor keeps its deliberate `None`.
    assert_eq!(
        report.objects_unindexed_unresolved(),
        1,
        "the unindexed volume still resolves the container's own silence"
    );
    let mut by_claim: BTreeMap<&str, usize> = BTreeMap::new();
    for object in world.unresolved_collision() {
        let Resolved::Unknown { claim_id, .. } = object.collision() else {
            panic!("an unresolved role must be an explicit unknown");
        };
        *by_claim.entry(claim_id.as_str()).or_default() += 1;
    }
    assert_eq!(
        by_claim,
        BTreeMap::from([(UNINDEXED_ROLE_UNMEASURED, 1)]),
        "the one gap left is the container's own silence; the grid-named fog volume \
         is answered, so it is no longer a gap"
    );
    assert_eq!(
        report.objects_unresolved_collision(),
        1,
        "and the report counts exactly that one open role"
    );

    // And the effect the task exists for: the fog volume never becomes a
    // collider, while the three index records the original does not take as fog
    // still collide.
    let meshes = fixture_meshes(world);
    let mut app = world_app();
    let spawned = spawn_world(&mut app, world, &meshes).expect("the fixture world spawns");
    for _ in 0..MESH_SETTLE_UPDATES {
        app.update();
    }
    assert_eq!(
        spawned.colliders().len(),
        3,
        "three colliders: one per grid-named record the fog consumer does not take"
    );
    let reasons: BTreeMap<&str, usize> = spawned
        .skipped()
        .iter()
        .map(|entry| entry.reason.label())
        .fold(BTreeMap::new(), |mut counts, label| {
            *counts.entry(label).or_default() += 1;
            counts
        });
    assert_eq!(
        reasons,
        BTreeMap::from([("unknown_collision_role", 1)]),
        "only the unindexed volume reports its gap: the grid-named fog volume and the \
         anchor's `None` are answers, not gaps"
    );
    let skipped: Vec<SkipReason> = spawned.skipped().iter().map(|entry| entry.reason).collect();
    assert!(
        skipped
            .iter()
            .all(|reason| *reason == SkipReason::UnknownCollisionRole),
        "and the one skip is the missing measurement: {skipped:?}"
    );
    assert_eq!(
        spawned.non_colliding(),
        vec![
            WorldObjectId::new("node-4").expect("the key is valid"),
            WorldObjectId::new(&format!("node-{FOG}")).expect("the key is valid")
        ],
        "the anchor is presented and never blocks, and so is the fog volume the \
         image's walk drops"
    );
    assert_eq!(
        spawned.presentation_gap_count(),
        0,
        "no object reports two reasons"
    );
}

/// A stored record that binds no mesh **and** stores an empty box in each of
/// its three slots: the store states no geometry for it (task #771).
///
/// Re-derived here from the record's own bytes rather than read back from the
/// import's counter, so the test and the implementation cannot fail together.
fn stores_no_geometry(record: &RawNode) -> bool {
    record.mesh_index() < 0
        && [record.info.unk116, record.info.unk140, record.info.unk164]
            .iter()
            .all(|stored| (0..3).all(|axis| stored[0][axis] == stored[1][axis]))
}

/// **Over the original installation there are exactly six grid-named `fvol*`
/// records, each now resolved as the fog volume the image's own walk drops.**
///
/// One production discovery, all eight world containers: the affected content
/// this task names (`c1c` four, `c5` two, nowhere else), the node slots, the
/// stored names, the records that store no geometry (task #771's second arm),
/// and the fact that every *other* grid-named record in every container still
/// resolves `Solid`.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f18_grid_collision_origin_retail_the_six_grid_named_fog_volumes_are_named() {
    let root = std::path::PathBuf::from(
        std::env::var("CS_GAME_DIR").expect("a retail test needs CS_GAME_DIR to be set"),
    );
    let found: RetailWorldContainers =
        read_world_containers(&root).expect("the installation is discovered once");
    let expected: BTreeMap<&str, (usize, &[u32])> = BTreeMap::from([
        ("C1C", (4usize, &[944u32, 945, 946, 947][..])),
        ("C5", (2usize, &[2304, 2306][..])),
    ]);

    let mut seen_slots: Vec<(String, u32, String)> = Vec::new();
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
        let (fog_count, fog_slots) = expected.get(group.as_str()).copied().unwrap_or((0, &[]));
        assert_eq!(
            report.partition_records_fog_volume(),
            fog_count,
            "{group}: how many grid-named records the original's fog consumer takes"
        );
        assert_eq!(
            report.objects_solid(),
            report.partition_records() - fog_count - report.partition_records_stores_no_geometry(),
            "{group}: every grid-named record but the fog volumes and the records that \
             store no geometry resolves `Solid`"
        );
        assert_eq!(
            report.partition_records_fog_volume()
                + report.partition_records_stores_no_geometry()
                + report.objects_solid(),
            report.partition_records(),
            "{group}: the fog counter, the empty records and the solid count partition \
             the grid exactly"
        );

        // Which records they are: node slot and stored name, read from the same
        // container bytes the import read. Since task #771 each resolves
        // `None` with the fog claim — the image's own walk reads the record's
        // flags word and drops it before any box test, which is the measurement
        // #727 was missing.
        let fog_set: BTreeSet<u32> = fog_slots.iter().copied().collect();
        let mut fog_slots_found: Vec<u32> = Vec::new();
        for slot in &fog_set {
            let object = imported
                .definition()
                .object(&WorldObjectId::new(&format!("node-{slot}")).expect("the key is valid"))
                .expect("a grid-named fog volume imported");
            assert_eq!(
                object.known_collision(),
                Some(WorldCollisionRole::None),
                "{group}: node {slot} is a fog volume the walk drops, so it never blocks"
            );
            let Resolved::Unknown { claim_id, reason } = object.shape() else {
                panic!("{group}: node {slot} builds no collider, so its shape stays open");
            };
            assert_eq!(
                claim_id.as_str(),
                FOG_VOLUME_RECORD_NEVER_BLOCKS,
                "{group}: node {slot} names the measured fog consumer"
            );
            assert!(
                reason.contains("narrow-phase"),
                "{group}: node {slot}'s reason names the filter that drops it: {reason}"
            );
            let record = container
                .nodes()
                .get(*slot)
                .expect("an imported object is a stored record");
            assert!(
                record.name.starts_with("fvol"),
                "{group}: the record the claim names is a stored `fvol*` record: {}",
                record.name
            );
            fog_slots_found.push(*slot);
            seen_slots.push((group.clone(), *slot, record.name.clone()));
        }
        assert_eq!(
            fog_slots_found, fog_slots,
            "{group}: exactly the measured grid-named fog volumes, in stored order"
        );

        // What is left open, and only that: every grid-named record resolves a
        // role (#771), so the unmeasured class is the whole of it — and no
        // object still carries #727's claim, which this installation's
        // narrow-phase bits have retired.
        for object in imported.definition().unresolved_collision() {
            let Resolved::Unknown { claim_id, .. } = object.collision() else {
                panic!("{group}: an unresolved role must be an explicit unknown");
            };
            assert_eq!(
                claim_id.as_str(),
                UNINDEXED_ROLE_UNMEASURED,
                "{group}: the container's other silence keeps its own claim id, and no \
                 grid-named record is open: {} carries {}",
                object.id(),
                claim_id.as_str()
            );
        }
        assert!(
            imported
                .definition()
                .objects()
                .iter()
                .all(|object| !matches!(
                    object.collision(),
                    Resolved::Unknown { claim_id, .. }
                        if claim_id.as_str() == GRID_NAMED_FOG_VOLUME_ROLE_UNMEASURED
                )),
            "{group}: no record carries #727's claim here: every grid-named `fvol*` \
             record stores the narrow-phase bit clear"
        );

        // Every grid-named record that is neither a fog volume nor one the
        // store gives no geometry keeps the designed rule, so the carve-outs
        // are as narrow as the measurement.
        let empty: BTreeSet<u32> = container
            .partition_grid()
            .expect("the container's own grid reads")
            .indexed_slots()
            .into_iter()
            .filter(|slot| container.nodes().get(*slot).is_some_and(stores_no_geometry))
            .collect();
        assert_eq!(
            empty.len(),
            report.partition_records_stores_no_geometry(),
            "{group}: the report's empty records are the container's own bytes"
        );
        let indexed = container
            .partition_grid()
            .expect("the container's own grid reads")
            .indexed_slots();
        assert_eq!(
            indexed.len(),
            report.partition_records(),
            "{group}: the report's grid is the container's grid"
        );
        for slot in indexed {
            if fog_set.contains(&slot) || empty.contains(&slot) {
                continue;
            }
            let object = imported
                .definition()
                .object(&WorldObjectId::new(&format!("node-{slot}")).expect("the key is valid"))
                .expect("a grid-named record imported");
            assert_eq!(
                object.known_collision(),
                Some(WorldCollisionRole::Solid),
                "{group}: node {slot} still resolves {INDEXED_RECORD_IS_STATIC}"
            );
        }
    }

    let mut names: Vec<String> = seen_slots
        .iter()
        .map(|(group, slot, name)| format!("{group}/node-{slot}={name}"))
        .collect();
    names.sort();
    assert_eq!(
        names,
        [
            "C1C/node-944=fvol1".to_owned(),
            "C1C/node-945=fvol2".to_owned(),
            "C1C/node-946=fvol3".to_owned(),
            "C1C/node-947=fvol4".to_owned(),
            "C5/node-2304=fvol1".to_owned(),
            "C5/node-2306=fvol3".to_owned(),
        ],
        "exactly six grid-named fog volumes exist, and these are their slots and names"
    );
}
