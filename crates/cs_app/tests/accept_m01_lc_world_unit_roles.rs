//! #677 (`M01-LC-WORLD-UNIT-ROLES`): the stored world-vertex unit is measured,
//! and the unindexed c1c records are classified by what their own bytes store.
//!
//! Spec: `specs/F18-world-geometry-terrain-water-and-traversable-interiors.md`
//! (`### F18-B` import, `### F18-D` evidence) and the first-mission surface of
//! `VS-M01-RUNTIME` (#359), which this measurement unblocks. Shared contract:
//! `docs/contracts/IDENTITY-CONTENT.md`. Task test prefix:
//! `accept_m01_lc_world_unit_roles_`.
//!
//! Two claims are pinned, and they are two different strengths:
//!
//! * **The scale is measured; the convention is not.** The GameZ landmark
//!   census — the `-9.8` gravity word all 61 animation containers store, the
//!   pilot-figure and airframe extents, the LOD switch bands and the world
//!   bounds — pins [`CalibratedQuantity::Scale`] to **one stored unit per
//!   metre** at `observed_tool`. The axis map, handedness and angle unit were
//!   never observed, so the source's whole-calibration status stays `Unknown`
//!   and its gap list names them. The non-retail half of this file asserts both
//!   halves of that sentence, because a source that reported `verified_original`
//!   — or a factor that silently reached a definition — would be a false claim,
//!   not a measurement.
//! * **The 53 unindexed c1c records are two measured classes, not one guess.**
//!   Every record the partition grid omits stores *either* a mesh index and a
//!   non-zero bounding box *or* neither of them, in all eight containers — the
//!   corpus has no third shape. The no-geometry half (the `horizon`, the
//!   `g*` transform groups, the zeppelin anchors) resolves to
//!   [`WorldCollisionRole::None`], because the store gives *this* record
//!   nothing a collider could be built from. The geometry-bearing half — the
//!   `fvol*` volumes — keeps an explicit `Unknown` role, because the container
//!   never says whether the original engine collided with it. The retail half
//!   asserts the split per container, the per-object consequences on the
//!   definition, and the spawn report the split produces.
//!
//! **What this file does not claim.** `retail` is file access: every number
//! here was read out of the owner's bytes by the production readers, and no
//! original executable ran. Nothing here is `verified_original`, and the tests
//! assert that too.

use std::collections::BTreeMap;

use cs_app::world::{
    MESH_SETTLE_UPDATES, RetailWorldContainer, RetailWorldContainers, read_world_containers,
    spawn_world, world_app,
};
use cs_content::coordinates::{
    CalibratedQuantity, CoordinateSource, GAMEZ_VERTEX_UNIT_IS_THE_METRE, SourceAdapter,
};
use cs_content::world::{
    OBJECT_STORES_NO_MESH, UNINDEXED_RECORD_STORES_NO_GEOMETRY, UNINDEXED_ROLE_UNMEASURED,
    WorldCollisionRole,
};
use cs_types::asset_id::SourceSpan;
use cs_types::content::{Origin, Resolved};
use cs_types::evidence::{ClaimStatus, ContentHash};

/// The measured unindexed split of one world container (task #677's census).
///
/// `anchors + volumes == unindexed` in every row, and the census test asserts
/// that relation: the corpus holds no unindexed record carrying only one of a
/// mesh index and a bounding box.
struct Split {
    /// The group's directory spelling, as production discovery spells it.
    group: &'static str,
    /// How many unindexed records store no mesh and no extent: they resolve to
    /// `None`, because the record itself has nothing a collider could be built
    /// from.
    anchors: usize,
    /// How many unindexed records store geometry: they keep the role the
    /// container never states.
    volumes: usize,
    /// `anchors + volumes`, checked against the report's own child-list count.
    unindexed: usize,
}

/// The census, as the production readers measure it over the owner's
/// installation. The table is the assertion: a split that classified by name
/// or by guess could not hold these numbers per group.
const SPLITS: [Split; 8] = [
    Split {
        group: "C1",
        anchors: 56,
        volumes: 10,
        unindexed: 66,
    },
    Split {
        group: "C1B",
        anchors: 78,
        volumes: 0,
        unindexed: 78,
    },
    Split {
        group: "C1C",
        anchors: 36,
        volumes: 17,
        unindexed: 53,
    },
    Split {
        group: "C2",
        anchors: 24,
        volumes: 0,
        unindexed: 24,
    },
    Split {
        group: "C2B",
        anchors: 39,
        volumes: 9,
        unindexed: 48,
    },
    Split {
        group: "C3",
        anchors: 14,
        volumes: 0,
        unindexed: 14,
    },
    Split {
        group: "C4",
        anchors: 38,
        volumes: 13,
        unindexed: 51,
    },
    Split {
        group: "C5",
        anchors: 20,
        volumes: 85,
        unindexed: 105,
    },
];

/// A span that stands for "the container the census ran over" in a non-retail
/// test: the shape is exercised, the bytes are not claimed.
fn synthetic_span() -> SourceSpan {
    SourceSpan::new(
        ContentHash::from_bytes([0; 32]),
        "zbd/c1c/gamez.zbd",
        None,
        0,
        0,
        None,
    )
    .expect("a span for the measured source's declaration is valid")
}

/// The conversion a retail import is made through: the measured GameZ source
/// over this container's own span.
fn retail_adapter(container: &RetailWorldContainer) -> SourceAdapter {
    SourceAdapter::new(CoordinateSource::retail_gamez(container.span().clone()))
}

/// One production discovery pass, shared by every retail test.
fn retail() -> RetailWorldContainers {
    let root = std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR is set");
    read_world_containers(std::path::Path::new(&root)).expect("the installation is discovered once")
}

fn origin(container: &RetailWorldContainer) -> Origin {
    Origin::Installation {
        source: container.span().clone(),
    }
}

/// **The measured source pins the scale at `observed_tool` and reports the
/// rest of the convention as the gaps it still is.**
///
/// The discriminating half: a source that claimed `verified_original` off
/// unfingerprinted tool evidence, or a calibration that reported itself
/// complete after one quantity's census, would both fail here.
#[test]
fn accept_m01_lc_world_unit_roles_the_scale_is_measured_and_the_convention_stays_open() {
    let source = CoordinateSource::retail_gamez(synthetic_span());
    assert_eq!(source.label(), "retail.gamez");
    assert_eq!(
        source.convention().meters_per_unit(),
        1.0,
        "the census pins one stored unit to the metre"
    );
    assert_eq!(
        source.provenance().claim_id.as_str(),
        GAMEZ_VERTEX_UNIT_IS_THE_METRE,
        "and the declaration names the claim it stands on"
    );
    assert_eq!(
        source.provenance().class,
        ClaimStatus::ObservedTool,
        "at the strongest class a byte census without an original run can carry"
    );

    let calibration = source.calibration();
    assert_eq!(
        calibration.quantity_status(CalibratedQuantity::Scale),
        ClaimStatus::ObservedTool,
        "the scale's own evidence class is what the import reports"
    );
    assert!(
        calibration.landmark_count(CalibratedQuantity::Scale) >= 3,
        "the quantity rule's minimum landmark count is met"
    );
    assert!(
        calibration.behavior_landmark_count(CalibratedQuantity::Scale) >= 1,
        "and at least one of them is an observed behaviour, as the rule requires"
    );

    // The other three quantities were never observed: each stays a gap, so the
    // whole-convention answer is honestly `Unknown` — measured is not complete.
    assert!(!calibration.is_complete());
    let gaps: Vec<_> = calibration.gaps().iter().map(|gap| gap.quantity).collect();
    for quantity in [
        CalibratedQuantity::Handedness,
        CalibratedQuantity::AxisOrder,
        CalibratedQuantity::AngleUnit,
    ] {
        assert!(
            gaps.contains(&quantity),
            "{} is unmeasured and the gap list says so",
            quantity.label()
        );
        assert_eq!(
            calibration.quantity_status(quantity),
            ClaimStatus::Unknown,
            "and its per-quantity class is unknown, not borrowed from the scale"
        );
    }
    assert_eq!(
        calibration.claim_status(),
        ClaimStatus::Unknown,
        "an incomplete calibration claims nothing at the convention level"
    );
    assert_ne!(
        calibration.claim_status(),
        ClaimStatus::VerifiedOriginal,
        "tool evidence without an original run never verifies the original"
    );

    // And the adapter built from it converts under the measured factor: a
    // stored position arrives scaled by exactly the metre.
    let adapter = SourceAdapter::new(source);
    assert_eq!(
        adapter.source().convention().meters_per_unit(),
        1.0,
        "the adapter carries the measured factor, not an assumed one"
    );
}

/// **A declared source still reports the unit unmeasured — the measurement is
/// the landmark census, not the number.** A conversion that declared `1.0` and
/// stopped there must read differently from one that measured it.
#[test]
fn accept_m01_lc_world_unit_roles_a_declared_source_stays_unmeasured() {
    let canonical = SourceAdapter::declared()
        .into_iter()
        .find(|adapter| adapter.source().label() == "canonical")
        .expect("the F16-A registry declares the canonical source");
    let calibration = canonical.source().calibration();
    assert_eq!(
        canonical.source().convention().meters_per_unit(),
        1.0,
        "the declared factor is also one — the class is what differs"
    );
    assert_eq!(
        calibration.quantity_status(CalibratedQuantity::Scale),
        ClaimStatus::Unknown,
        "declared is not measured: the same number without landmarks is unknown"
    );
    assert_eq!(
        calibration.claim_status(),
        ClaimStatus::Unknown,
        "and the declared convention makes no calibration claim at all"
    );
}

/// **All eight containers show the same split: every unindexed record either
/// stores no geometry (resolved `None`) or stores some (measured unknown).**
///
/// The per-group numbers are the census, and the object-level cross-check is
/// the discriminator: a `None` role on a mesh-bearing record — or a `Solid`
/// guessed onto an `fvol` — fails here, as does any unindexed record that is
/// neither.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_lc_world_unit_roles_every_container_unindexed_split_is_measured() {
    let found = retail();

    for split in &SPLITS {
        let container = found
            .container(split.group)
            .unwrap_or_else(|error| panic!("{}: the container reads: {error}", split.group));
        let imported = container
            .definition(origin(&container), &retail_adapter(&container))
            .unwrap_or_else(|error| panic!("{}: the container imports: {error}", split.group));
        let report = imported.report();
        let world = imported.definition();

        // The report's own census fields, against the measured table.
        assert_eq!(
            report.stored_child_list(),
            split.unindexed,
            "{}: the world node's own stored child list",
            split.group
        );
        assert_eq!(
            report.objects_unindexed_none(),
            split.anchors,
            "{}: the unindexed records with no mesh and no extent resolve to `None`",
            split.group
        );
        assert_eq!(
            report.objects_unindexed_unresolved(),
            split.volumes,
            "{}: the unindexed records that store geometry stay unknown",
            split.group
        );
        assert_eq!(
            split.anchors + split.volumes,
            split.unindexed,
            "{}: the two classes partition the unindexed records exactly — no \
             third shape exists in the corpus",
            split.group
        );

        // The object-level consequence, checked per record rather than as a
        // count: `None` is answered only by a record whose own store holds no
        // geometry, and the role the container owes is asked only of one that
        // does.
        for object in world.objects() {
            match object.known_collision() {
                Some(WorldCollisionRole::None) => {
                    assert!(
                        !object.mesh().is_known(),
                        "{}: {} resolved to `None` but names a mesh — the store \
                         holds geometry the role denies",
                        split.group,
                        object.id().as_str()
                    );
                    let Resolved::Unknown { claim_id, .. } = object.shape() else {
                        panic!(
                            "{}: {} has no geometry to take a shape from",
                            split.group,
                            object.id().as_str()
                        );
                    };
                    assert_eq!(claim_id.as_str(), UNINDEXED_RECORD_STORES_NO_GEOMETRY);
                    let Resolved::Unknown { claim_id, .. } = object.mesh() else {
                        panic!(
                            "{}: {} binds no mesh, and says so",
                            split.group,
                            object.id().as_str()
                        );
                    };
                    assert_eq!(claim_id.as_str(), OBJECT_STORES_NO_MESH);
                }
                Some(WorldCollisionRole::Solid) => {
                    assert_eq!(
                        object.known_shape(),
                        Some(cs_content::world::WorldCollisionShape::FromMesh),
                        "{}: {} resolved solid, so its collider is derived from \
                         its own mesh — the indexed rule keeps the pairing",
                        split.group,
                        object.id().as_str()
                    );
                }
                Some(WorldCollisionRole::Sensor) => {
                    panic!(
                        "{}: {} is a sensor — nothing in the container measured \
                         that, so a sensor role is a guess",
                        split.group,
                        object.id().as_str()
                    );
                }
                None => {
                    let Resolved::Unknown { claim_id, .. } = object.collision() else {
                        unreachable!("checked above")
                    };
                    assert_eq!(
                        claim_id.as_str(),
                        UNINDEXED_ROLE_UNMEASURED,
                        "{}: {} keeps the unknown the container owes it",
                        split.group,
                        object.id().as_str()
                    );
                    assert!(
                        object.mesh().is_known(),
                        "{}: {} asks the role question of a record that stores \
                         geometry — an anchor would have resolved `None`",
                        split.group,
                        object.id().as_str()
                    );
                }
            }
        }

        // And the unit the import ran under: the measured metre, at its own
        // evidence class, on the definition the runtime consumes.
        assert_eq!(
            report.meters_per_unit(),
            1.0,
            "{}: one stored unit is the metre",
            split.group
        );
        assert_eq!(
            report.unit_class(),
            ClaimStatus::ObservedTool,
            "{}: the factor's evidence is the census, not a guess and not an \
             original run",
            split.group
        );
    }
}

/// **On c1c, the 53 records the task asked about land as 36 presented anchors
/// and 17 still-unresolved `fvol` volumes, and the spawn says so.** (retail)
///
/// This is the number the `world_geometry` blocker was waiting on: the
/// definition imported from `ZBD/C1C/gamez.zbd` carries no invented roles, and
/// the spawn report shows exactly which records block and which never could.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_lc_world_unit_roles_the_c1c_spawn_reports_the_measured_split() {
    let found = retail();
    let container = found
        .container("C1C")
        .expect("the c1c geometry container reads");
    let imported = container
        .definition(origin(&container), &retail_adapter(&container))
        .expect("the c1c container imports");
    let world = imported.definition();
    let meshes = container
        .uploaded_meshes(world)
        .expect("the container's own meshes upload");

    let mut app = world_app();
    let spawned = spawn_world(&mut app, world, &meshes).expect("the world spawns");
    for _ in 0..MESH_SETTLE_UPDATES {
        app.update();
    }

    assert_eq!(spawned.objects().len(), world.objects().len());
    assert_eq!(
        spawned.colliders().len(),
        imported.report().partition_records_with_mesh(),
        "every indexed record that binds a mesh collides, from that mesh"
    );
    let mut reasons: BTreeMap<&str, usize> = BTreeMap::new();
    for entry in spawned.skipped() {
        *reasons.entry(entry.reason.label()).or_default() += 1;
    }
    assert_eq!(
        reasons,
        BTreeMap::from([("unknown_collision_role", 17), ("unknown_mesh", 1),]),
        "exactly the 17 geometry-bearing unindexed records report the missing \
         role, and the one mesh-less indexed record reports its own gap"
    );
    assert_eq!(
        spawned.non_colliding().len(),
        36,
        "the 36 anchors are presented and deliberately never block: a resolved \
         `None` is not a skip"
    );
    assert_eq!(spawned.presentation_gap_count(), 0);
}
