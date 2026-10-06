//! #716 (`M01-LC-FVOL-ROLES`): what the `fvol*` volume records are consumed
//! for, and the world axis convention `import_world_container` converts under.
//!
//! Spec: `specs/F18-world-geometry-terrain-water-and-traversable-interiors.md`
//! (`### F18-B` import, `### F18-D` evidence) and the `world_geometry` surface
//! of `VS-M01-RUNTIME` (#359) that #677 left open. Shared contract:
//! `docs/contracts/IDENTITY-CONTENT.md`. Task test prefix:
//! `accept_m01_lc_fvol_roles_`.
//!
//! Two measurements are bound into the production import, and each is pinned
//! here at the strength it was measured at:
//!
//! * **an `fvol*` record is a fog volume** ([`FOG_VOLUME_RECORD_NEVER_BLOCKS`]):
//!   the image's only name-keyed consumer of the four-byte prefix is the fog
//!   routine that pairs those nodes with `fogvol.zrd`'s fog keys, so an
//!   unindexed `fvol*` record resolves to [`WorldCollisionRole::None`] while
//!   its mesh stays a known reference — presented, never blocking. The
//!   discriminator matters: a record that stores geometry but carries no
//!   measured prefix keeps [`UNINDEXED_ROLE_UNMEASURED`], a record that stores
//!   no geometry keeps [`UNINDEXED_RECORD_STORES_NO_GEOMETRY`], and an `fvol*`
//!   record the partition grid *does* name keeps `Solid` under the index rule,
//!   with the disagreement counted instead of settled.
//! * **the axis convention is measured** ([`WORLD_AXIS_CONVENTION_MEASURED`],
//!   task #436's owner note): identity axis map, `+Y` up, right-handed,
//!   radians — code-derived over the decrypted image, so the import reports
//!   `observed_tool` for an installation-backed source that applied exactly
//!   that map, `contradicted` for one that applied anything else, and
//!   `unknown` for a designed source.
//!
//! The retail half (`#[ignore]`d, `requires CS_GAME_DIR`) runs the same
//! assertions over all eight world containers through the production readers
//! and pins the per-group census. Nothing here is `verified_original`: no
//! original executable ran, and every number is a count or a relation read out
//! of the owner's bytes.

use cs_content::coordinates::{
    AngleUnit, CoordinateSource, RotationSense, SourceAdapter, SourceAxis, SourceConvention,
};
use cs_content::textures::WorldTextureLoad;
use cs_content::world::{
    FOG_VOLUME_RECORD_NEVER_BLOCKS, ImportedWorld, UNINDEXED_RECORD_STORES_NO_GEOMETRY,
    UNINDEXED_ROLE_UNMEASURED, WORLD_AXIS_CONVENTION_MEASURED, WorldCollisionRole, WorldId,
    WorldObjectId, import_world_container,
};
use cs_types::asset_id::SourceSpan;
use cs_types::content::{Origin, Resolved};
use cs_types::evidence::{ClaimId, ClaimStatus, ContentHash};
use cs_types::space::Winding;

use super::import_retail::{
    Fixture, MARKER, ObjectSpec, VOLUME, adapter, mesh_slots, provenance, read, write_container,
};

/// The mesh slot the fixture's grid-named `fvol*` record binds.
const MESH_FOG_INDEXED: i32 = 9;
/// The mesh slot the fixture's world-owned, unindexed `fvol*` record binds.
const MESH_FOG_UNINDEXED: i32 = 4;

/// A span that stands for "the container the measurement ran over" in a
/// non-retail test: the shape is exercised, the bytes are not claimed.
fn span() -> SourceSpan {
    SourceSpan::new(
        ContentHash::from_bytes([0; 32]),
        "zbd/c1c/gamez.zbd",
        None,
        0,
        0,
        None,
    )
    .expect("a span for an installation-backed source is valid")
}

/// The conversion one import runs through.
fn imported_with(bytes: &[u8], through: &SourceAdapter) -> ImportedWorld {
    import_world_container(
        WorldId::from_key("fixture").expect("the fixture world key is valid"),
        Origin::SyntheticFixture,
        &read(bytes),
        bytes,
        &mesh_slots(),
        through,
        provenance(),
    )
    .expect("the fixture container imports")
}

/// The default fixture plus two `fvol*` records — one the partition grid names,
/// one the world node's stored child list owns — beside the default's
/// geometry-bearing `volume` and its geometry-less `marker`.
///
/// Returns the import and the two new records' node slots.
fn fog_fixture() -> (ImportedWorld, u32, u32) {
    let mut fixture = Fixture::default();
    let indexed_slot = 1 + u32::try_from(fixture.objects.len()).expect("fits a slot");
    fixture.objects.push(
        ObjectSpec::new("fvol3", MESH_FOG_INDEXED).extent([5.0, 970.0, 5.0], [8.0, 1000.0, 8.0]),
    );
    let owned_slot = 1 + u32::try_from(fixture.objects.len()).expect("fits a slot");
    fixture.objects.push(
        ObjectSpec::new("fvol7", MESH_FOG_UNINDEXED)
            .extent([20.0, 971.0, 20.0], [30.0, 1100.0, 30.0]),
    );
    fixture.grid[0].push(indexed_slot);
    fixture.stored_children.push(owned_slot);
    let bytes = write_container(&fixture);
    (imported_with(&bytes, &adapter()), indexed_slot, owned_slot)
}

/// One object of an import, by its node-slot key.
fn object(imported: &ImportedWorld, slot: u32) -> &cs_content::world::WorldObjectInstance {
    let id = WorldObjectId::new(&format!("node-{slot}")).expect("the key is valid");
    imported
        .definition()
        .object(&id)
        .unwrap_or_else(|| panic!("node-{slot} imported"))
}

/// **An unindexed `fvol*` record is a fog volume: presented, never blocking,
/// still drawn — and nothing else changes class.**
#[test]
fn accept_m01_lc_fvol_roles_an_unindexed_fvol_record_is_a_fog_volume_and_never_blocks() {
    let (imported, indexed_slot, owned_slot) = fog_fixture();
    let report = imported.report();

    let fog = object(&imported, owned_slot);
    assert_eq!(
        fog.known_collision(),
        Some(WorldCollisionRole::None),
        "the store draws a fog volume and nothing measured reports a contact for it"
    );
    assert!(
        fog.mesh().is_known(),
        "resolving the role must not drop the mesh: a fog volume is presented, and the \
         box it draws is still drawn"
    );
    let shape = fog.shape();
    let Resolved::Unknown { claim_id, reason } = shape else {
        panic!("a fog volume builds no collider, so its shape stays an unknown: {shape:?}");
    };
    assert_eq!(
        claim_id.as_str(),
        FOG_VOLUME_RECORD_NEVER_BLOCKS,
        "the shape names the measured consumer rather than an absence"
    );
    assert!(
        reason.contains("`fvol`"),
        "the reason names the measured prefix: {reason}"
    );

    // The index rule is untouched: the `fvol*` record the grid names is still
    // the world's static geometry, and the disagreement is counted.
    let grid_fog = object(&imported, indexed_slot);
    assert_eq!(
        grid_fog.known_collision(),
        Some(WorldCollisionRole::Solid),
        "the partition grid still wins for a record it names — no measurement here \
         says how the original collided with a grid-named record"
    );
    assert_eq!(
        grid_fog.known_shape(),
        Some(cs_content::world::WorldCollisionShape::FromMesh),
        "and its collider is still derived from its own mesh"
    );
    assert_eq!(
        report.partition_records_fog_volume(),
        1,
        "the overlap the two rules disagree about is reported, not hidden"
    );

    // The two classes that were already measured keep their own answers, so a
    // one-rule-fits-all implementation fails here.
    let volume = object(&imported, VOLUME);
    let role = volume.collision();
    let Resolved::Unknown { claim_id, reason } = role else {
        panic!("a record with geometry and no measured prefix keeps its unknown: {role:?}");
    };
    assert_eq!(claim_id.as_str(), UNINDEXED_ROLE_UNMEASURED);
    assert!(
        reason.contains("partition grid"),
        "the reason names what is missing: {reason}"
    );
    assert!(!volume.shape().is_known());

    let anchor = object(&imported, MARKER);
    assert_eq!(anchor.known_collision(), Some(WorldCollisionRole::None));
    let shape = anchor.shape();
    let Resolved::Unknown { claim_id, .. } = shape else {
        panic!("a record with no geometry stores no shape: {shape:?}");
    };
    assert_eq!(claim_id.as_str(), UNINDEXED_RECORD_STORES_NO_GEOMETRY);
    assert!(
        !anchor.mesh().is_known(),
        "an anchor binds no mesh, which is what separates it from a fog volume"
    );

    // The report's own census, three ways plus the overlap.
    assert_eq!(report.objects_unindexed_fog(), 1);
    assert_eq!(report.objects_unindexed_unresolved(), 1);
    assert_eq!(report.objects_unindexed_none(), 1);
    assert_eq!(
        report.objects_unindexed_fog()
            + report.objects_unindexed_unresolved()
            + report.objects_unindexed_none(),
        report.stored_child_list(),
        "the three classes still partition the world-owned unindexed records exactly"
    );
    assert_eq!(report.objects_solid(), report.partition_records());
    assert_eq!(report.objects(), 7);
}

/// **The classification is the engine's own four-byte prefix, not a substring
/// and not a suffix.**
///
/// The image compares `strncmp(name, "fvol", 4)`, so a record named `fvol`
/// matches, one named `xfvol` or `volume` does not. A `contains`/`ends_with`
/// implementation classifies `volume` (the fixture's non-fog volume) and fails
/// here; classifying nothing fails here too.
#[test]
fn accept_m01_lc_fvol_roles_only_the_four_byte_prefix_classifies_a_fog_volume() {
    let mut fixture = Fixture::default();
    let exact_slot = 1 + u32::try_from(fixture.objects.len()).expect("fits a slot");
    fixture
        .objects
        .push(ObjectSpec::new("fvol", MESH_FOG_INDEXED).extent([0.0, 1.0, 0.0], [4.0, 5.0, 4.0]));
    let suffix_slot = 1 + u32::try_from(fixture.objects.len()).expect("fits a slot");
    fixture.objects.push(
        ObjectSpec::new("xfvol", MESH_FOG_UNINDEXED).extent([8.0, 1.0, 8.0], [12.0, 5.0, 12.0]),
    );
    fixture.stored_children.push(exact_slot);
    fixture.stored_children.push(suffix_slot);
    let bytes = write_container(&fixture);
    let imported = imported_with(&bytes, &adapter());
    let report = imported.report();

    assert_eq!(
        report.objects_unindexed_fog(),
        1,
        "exactly the record whose name starts with the four bytes the image compares"
    );
    assert_eq!(
        report.objects_unindexed_unresolved(),
        2,
        "`volume` and `xfvol` store geometry and carry no measured prefix"
    );
    assert_eq!(
        object(&imported, exact_slot).known_collision(),
        Some(WorldCollisionRole::None)
    );
    let suffix = object(&imported, suffix_slot);
    let Resolved::Unknown { claim_id, .. } = suffix.collision() else {
        panic!("a suffix match is not the engine's rule, so `xfvol` keeps its unknown");
    };
    assert_eq!(claim_id.as_str(), UNINDEXED_ROLE_UNMEASURED);
}

/// **The axis convention the import applied is reported with the evidence
/// behind it, and the three evidence classes are told apart.**
///
/// The measured answer (task #436's owner note over the decrypted image) is
/// `observed_tool` for an installation-backed source that applied exactly that
/// map; the same map over designed content measures nothing, and an
/// installation-backed source that applied another map disagrees with the
/// measurement and is reported as such.
#[test]
fn accept_m01_lc_fvol_roles_the_axis_convention_is_reported_with_its_evidence_class() {
    let bytes = write_container(&Fixture::default());

    // (a) The measured GameZ source: installation-backed, identity map.
    let measured = SourceAdapter::new(CoordinateSource::retail_gamez(span()));
    let measured_world = imported_with(&bytes, &measured);
    let report = measured_world.report();
    assert_eq!(
        report.axis_map(),
        "identity",
        "the measured map is the identity"
    );
    assert!(
        report.axis_map_preserves_orientation(),
        "the original's frame and the canonical frame are the same handedness"
    );
    assert_eq!(
        report.angle_unit(),
        AngleUnit::Radians,
        "GameZ binaries are radians"
    );
    assert_eq!(
        report.rotation_sense(),
        RotationSense::RightHandRule,
        "the view matrix is a proper rotation, so the frame is right-handed"
    );
    assert_eq!(
        report.axis_class(),
        ClaimStatus::ObservedTool,
        "code-derived evidence over the original image: measured, never a run"
    );
    assert_ne!(
        report.axis_class(),
        ClaimStatus::VerifiedOriginal,
        "static analysis of bytes never reaches verified_original"
    );
    assert_eq!(report.meters_per_unit(), 1.0);
    assert_eq!(report.unit_class(), ClaimStatus::ObservedTool);
    assert_eq!(
        WORLD_AXIS_CONVENTION_MEASURED, "f18-world.world-axis-convention-measured",
        "the claim id the measurement is filed under is stable"
    );

    // (b) The declared canonical source: same map, designed origin, so
    // nothing about the original is measured through it.
    let declared = adapter();
    let declared_world = imported_with(&bytes, &declared);
    let report = declared_world.report();
    assert_eq!(report.axis_map(), "identity");
    assert_eq!(
        report.axis_class(),
        ClaimStatus::Unknown,
        "a designed source measuring the original's convention would be a guess"
    );

    // (c) An installation-backed source that applied another map: the
    // measurement says identity, so the two disagree and say so.
    let other = SourceAdapter::new(
        CoordinateSource::new(
            "installation.fixture.z-up",
            SourceConvention::new(
                [
                    SourceAxis::positive(cs_content::coordinates::Axis::X),
                    SourceAxis::positive(cs_content::coordinates::Axis::Z),
                    SourceAxis::negative(cs_content::coordinates::Axis::Y),
                ],
                cs_content::coordinates::Axis::Y,
                1.0,
                AngleUnit::Degrees,
                RotationSense::RightHandRule,
                Winding::CounterClockwise,
            )
            .expect("the fixture convention is valid"),
            Origin::Installation { source: span() },
            cs_types::content::Provenance::new(
                ClaimId::new("test.installation.non-identity").expect("the claim id is valid"),
                ClaimStatus::ObservedTool,
                Some(span()),
            )
            .expect("an observed_tool declaration with a span is valid"),
        )
        .expect("the source is valid"),
    );
    let other_world = imported_with(&bytes, &other);
    let report = other_world.report();
    assert_eq!(
        report.axis_map(),
        "[+x, +z, -y]",
        "a non-identity map is spelled out per canonical axis instead of summarised"
    );
    assert_eq!(
        report.axis_class(),
        ClaimStatus::Contradicted,
        "the measurement and the applied map disagree, and the report says which"
    );
}

/// The measured census of all eight containers, and the axis report the
/// retail conversion produces.** (retail)
///
/// The per-group numbers are what the production readers measure over the
/// owner's installation: the `fvol*` half of #677's unindexed split has left
/// [`UNINDEXED_ROLE_UNMEASURED`] for the fog-volume measurement, and what
/// stays unknown there is every mesh-bearing record no measured prefix names.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_lc_fvol_roles_every_container_fog_split_is_measured() {
    let root = std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR is set");
    let found = cs_app::world::read_world_containers(std::path::Path::new(&root))
        .expect("the installation is discovered once");

    for split in &SPLITS {
        let container = found
            .container(split.group, &WorldTextureLoad::project_default())
            .unwrap_or_else(|error| panic!("{}: the container reads: {error}", split.group));
        let through = SourceAdapter::new(CoordinateSource::retail_gamez(container.span().clone()));
        let imported = container
            .definition(
                Origin::Installation {
                    source: container.span().clone(),
                },
                &through,
            )
            .unwrap_or_else(|error| panic!("{}: the container imports: {error}", split.group));
        let report = imported.report();
        let world = imported.definition();

        assert_eq!(
            report.objects_unindexed_fog(),
            split.fog,
            "{}: the unindexed records carrying the measured `fvol` prefix",
            split.group
        );
        assert_eq!(
            report.objects_unindexed_unresolved(),
            split.unresolved,
            "{}: the unindexed records that store geometry and carry no measured prefix",
            split.group
        );
        assert_eq!(
            report.objects_unindexed_none(),
            split.anchors,
            "{}: the unindexed records that store no geometry",
            split.group
        );
        assert_eq!(
            report.partition_records_fog_volume(),
            split.indexed_fog,
            "{}: the `fvol*` records the partition grid also names",
            split.group
        );
        assert_eq!(
            report.objects_unindexed_fog()
                + report.objects_unindexed_unresolved()
                + report.objects_unindexed_none(),
            split.unindexed,
            "{}: the three classes partition the unindexed records exactly",
            split.group
        );
        assert_eq!(
            report.objects_solid(),
            report.partition_records(),
            "{}: the index rule is untouched by the fog measurement",
            split.group
        );

        // Object level: every fog volume is presented and keeps its mesh, and
        // every record still unknown carries the unknown's own claim.
        let fog_objects = world
            .objects()
            .iter()
            .filter(|object| {
                object.known_collision() == Some(WorldCollisionRole::None)
                    && matches!(
                        object.shape(),
                        Resolved::Unknown { claim_id, .. }
                            if claim_id.as_str() == FOG_VOLUME_RECORD_NEVER_BLOCKS
                    )
            })
            .count();
        assert_eq!(
            fog_objects, split.fog,
            "{}: a fog volume is recognised by its resolved role and its shape claim",
            split.group
        );
        for object in world.unresolved_collision() {
            let Resolved::Unknown { claim_id, .. } = object.collision() else {
                panic!("an unresolved role must be an explicit unknown");
            };
            assert_eq!(
                claim_id.as_str(),
                UNINDEXED_ROLE_UNMEASURED,
                "{}: what is left unknown is the unmeasured class",
                split.group
            );
            assert!(
                object.mesh().is_known(),
                "{}: the unmeasured class stores geometry",
                split.group
            );
        }
        assert_eq!(
            world.unresolved_collision().len(),
            split.unresolved,
            "{}: the report's unresolved count is the definition's",
            split.group
        );

        // The axis convention the retail conversion applied, with its evidence.
        assert_eq!(report.axis_map(), "identity", "{}", split.group);
        assert!(report.axis_map_preserves_orientation(), "{}", split.group);
        assert_eq!(report.angle_unit(), AngleUnit::Radians, "{}", split.group);
        assert_eq!(
            report.rotation_sense(),
            RotationSense::RightHandRule,
            "{}",
            split.group
        );
        assert_eq!(
            report.axis_class(),
            ClaimStatus::ObservedTool,
            "{}: the map over original bytes is code-derived, never a run",
            split.group
        );
        assert_eq!(report.meters_per_unit(), 1.0, "{}", split.group);
        assert_eq!(
            report.unit_class(),
            ClaimStatus::ObservedTool,
            "{}",
            split.group
        );
    }
}

/// One row of the measured census: what the production readers report per
/// world container after #716.
struct Split {
    /// The group's directory spelling, as production discovery spells it.
    group: &'static str,
    /// Unindexed records that store no geometry (the anchor class).
    anchors: usize,
    /// Unindexed records whose name carries the `fvol` prefix (fog volumes).
    fog: usize,
    /// Unindexed records that store geometry and carry no measured prefix.
    unresolved: usize,
    /// `anchors + fog + unresolved`, checked against the child-list count.
    unindexed: usize,
    /// Records the partition grid names whose name carries the `fvol` prefix.
    indexed_fog: usize,
}

/// The census, as the production readers measure it over the owner's
/// installation.
const SPLITS: [Split; 8] = [
    Split {
        group: "C1",
        anchors: 56,
        fog: 9,
        unresolved: 1,
        unindexed: 66,
        indexed_fog: 0,
    },
    Split {
        group: "C1B",
        anchors: 78,
        fog: 0,
        unresolved: 0,
        unindexed: 78,
        indexed_fog: 0,
    },
    Split {
        group: "C1C",
        anchors: 36,
        fog: 17,
        unresolved: 0,
        unindexed: 53,
        indexed_fog: 4,
    },
    Split {
        group: "C2",
        anchors: 24,
        fog: 0,
        unresolved: 0,
        unindexed: 24,
        indexed_fog: 0,
    },
    Split {
        group: "C2B",
        anchors: 39,
        fog: 9,
        unresolved: 0,
        unindexed: 48,
        indexed_fog: 0,
    },
    Split {
        group: "C3",
        anchors: 14,
        fog: 0,
        unresolved: 0,
        unindexed: 14,
        indexed_fog: 0,
    },
    Split {
        group: "C4",
        anchors: 38,
        fog: 9,
        unresolved: 4,
        unindexed: 51,
        indexed_fog: 0,
    },
    Split {
        group: "C5",
        anchors: 20,
        fog: 15,
        unresolved: 70,
        unindexed: 105,
        indexed_fog: 2,
    },
];
