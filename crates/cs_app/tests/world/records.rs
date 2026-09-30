//! The record-level contract: identity, sectors, roles, validation and the
//! per-load world instance (F18 non-negotiable behaviors 2, 3 and 5).
//!
//! These call `cs_content::world` production code directly. A synthetic
//! fixture never proves retail compatibility; it proves that the typed
//! contract refuses what it must refuse and states what it must state.

use std::collections::BTreeSet;

use cs_app::world::{
    OBJECT_GROUND, OBJECT_LEG_RIGHT, OBJECT_LINTEL, OBJECT_WATER, SECTOR_ARCH, arch_world,
    object_set,
};
use cs_content::scene::CanonicalTransform;
use cs_content::world::{
    Aabb, AabbError, Sector, SectorId, WorldBoundary, WorldDefinition, WorldError, WorldId,
    WorldInstance, WorldKeyError, WorldObjectId, WorldObjectInstance, WorldPopulation,
};
use cs_types::content::{Known, Origin, Provenance, Resolved};
use cs_types::evidence::ClaimId;

fn provenance(key: &str) -> Provenance {
    Provenance::designed(ClaimId::new(&format!("test.{key}")).expect("the claim id is valid"))
}

fn known<T>(value: T, key: &str) -> Resolved<T> {
    Resolved::Known(Known::new(value, provenance(key)))
}

fn unknown<T>(key: &str) -> Resolved<T> {
    Resolved::unknown(
        ClaimId::new(&format!("test.{key}")).expect("the claim id is valid"),
        "the test record deliberately leaves this unmeasured",
    )
    .expect("the reason is non-empty")
}

fn identity() -> CanonicalTransform {
    CanonicalTransform::IDENTITY
}

fn object(key: &str) -> WorldObjectId {
    WorldObjectId::new(key).expect("the test object key is valid")
}

fn sector(key: &str) -> SectorId {
    SectorId::new(key).expect("the test sector key is valid")
}

/// A definition with one sector and one object, used to build invalid
/// variants without duplicating the whole fixture.
fn one_object_definition(
    id_key: &str,
    sectors: Vec<Sector>,
    objects: Vec<WorldObjectInstance>,
) -> Result<WorldDefinition, WorldError> {
    WorldDefinition::try_new(
        WorldId::from_key(id_key).expect("the world key is valid"),
        Origin::SyntheticFixture,
        known(
            WorldBoundary::try_new(Some(0.0), Some(100.0), None).expect("the boundary is valid"),
            "boundary",
        ),
        sectors,
        objects,
        provenance("definition"),
    )
}

fn plain_object(key: &str, sectors: Vec<SectorId>) -> WorldObjectInstance {
    WorldObjectInstance::try_new(
        object(key),
        known(
            cs_types::content::ContentId::from_source(
                cs_types::content::ContentKind::Mesh,
                &format!("synthetic.{key}"),
            )
            .expect("the mesh id is valid"),
            "mesh",
        ),
        identity(),
        known(cs_content::world::WorldCollisionRole::Solid, "collision"),
        known(
            cs_content::world::WorldCollisionShape::cuboid([1.0, 1.0, 1.0])
                .expect("the box is valid"),
            "shape",
        ),
        known(cs_content::world::SurfaceRole::Ground, "surface"),
        sectors,
        provenance("object"),
    )
    .expect("the sector list has no duplicates")
}

fn one_sector() -> Sector {
    Sector::new(
        sector("only"),
        Aabb::try_new([0.0, 0.0, 0.0], [10.0, 10.0, 10.0]).expect("the bounds are valid"),
    )
}

/// The definition refuses duplicate identities, dangling sector references
/// and a malformed boundary — each by name, so an authoring mistake is
/// reported instead of becoming a runtime surprise.
///
/// Observable failure if validation is removed: the duplicate definition
/// builds and the two records silently compete for one id.
#[test]
fn accept_f18_a_world_definition_refuses_duplicate_ids_and_dangling_sector_refs() {
    let duplicate_sector = one_object_definition(
        "test.duplicate_sector",
        vec![one_sector(), one_sector()],
        vec![plain_object("a", vec![sector("only")])],
    );
    assert!(
        matches!(&duplicate_sector, Err(WorldError::DuplicateSector { .. })),
        "two sectors with one id must be refused, got {duplicate_sector:?}"
    );

    let duplicate_object = one_object_definition(
        "test.duplicate_object",
        vec![one_sector()],
        vec![
            plain_object("a", vec![sector("only")]),
            plain_object("a", vec![sector("only")]),
        ],
    );
    assert!(
        matches!(&duplicate_object, Err(WorldError::DuplicateObject { .. })),
        "two objects with one id must be refused, got {duplicate_object:?}"
    );

    let dangling = one_object_definition(
        "test.dangling",
        vec![one_sector()],
        vec![plain_object("a", vec![sector("nowhere")])],
    );
    assert!(
        matches!(&dangling, Err(WorldError::DanglingSectorRef { .. })),
        "a reference to an undeclared sector must be refused, got {dangling:?}"
    );

    // The id grammar itself: an id is not a path.
    assert_eq!(
        SectorId::new("nested/sector"),
        Err(WorldKeyError::BadCharacter { ch: '/' }),
        "a sector id must not smuggle a path separator"
    );

    // Bounds and boundaries are validated where they are built.
    assert!(
        matches!(
            Aabb::try_new([0.0, 1.0, 0.0], [1.0, 0.0, 1.0]),
            Err(AabbError::Inverted { axis: "y", .. })
        ),
        "an inverted extent must name its axis"
    );
    assert!(
        WorldBoundary::try_new(Some(100.0), Some(10.0), None).is_err(),
        "a floor above its ceiling must be refused"
    );
    assert!(
        WorldBoundary::default().is_absent(),
        "a boundary with no rules must say so rather than imply a wall"
    );

    // The real fixture builds, so the refusals above are not a fixture that
    // can never succeed.
    let fixture = arch_world();
    assert!(
        fixture.is_ok(),
        "the synthetic arch world must build, got {fixture:?}"
    );
}

/// Sector membership and the resident set are explicit: an object names its
/// sectors, an object that names none is resident, and neither is inferred
/// from a bounding box (F18 non-negotiable behavior 3's precondition).
#[test]
fn accept_f18_a_sector_membership_and_residency_are_explicit_records() {
    let definition = arch_world().expect("the synthetic arch world is valid");

    let arch = definition
        .sector(&sector(SECTOR_ARCH))
        .expect("the arch sector exists");
    assert_eq!(arch.bounds().min(), [-1.0, -2.0, -6.0]);
    assert_eq!(arch.bounds().max(), [1.0, 10.0, 6.0]);

    let in_arch: Vec<&WorldObjectId> = definition
        .objects_in_sector(&sector(SECTOR_ARCH))
        .iter()
        .map(|object| object.id())
        .collect();
    assert_eq!(
        in_arch,
        vec![
            &object("arch.leg_left"),
            &object("arch.leg_right"),
            &object("arch.lintel"),
        ],
        "the arch sector holds exactly the three parts that build the opening"
    );

    let resident: Vec<&WorldObjectId> = definition
        .resident_objects()
        .iter()
        .map(|object| object.id())
        .collect();
    assert_eq!(
        resident,
        vec![&object(OBJECT_WATER)],
        "the water patch declares no sector, so it is resident"
    );

    // Identity survives: the same key resolves to the same record, whether
    // it is reached through the object list or a sector query.
    let by_id = definition
        .object(&object(OBJECT_WATER))
        .expect("the water patch exists");
    assert!(
        !definition
            .objects_in_sector(&sector("approach"))
            .iter()
            .any(|candidate| candidate.id() == by_id.id()),
        "a resident object is in no sector"
    );

    // Sector bounds are validated up front, so `contains` can be trusted.
    let arch_bounds = arch.bounds();
    assert!(arch_bounds.contains([-1.0, 0.0, 0.0]));
    assert!(!arch_bounds.contains([-2.0, 0.0, 0.0]));
}

/// A load of a world states its own variant, population and initial damage,
/// and is validated against the definition it reads from
/// (F18 non-negotiable behavior 5).
///
/// Observable failure if a load could inherit state: the second instance
/// would come back carrying the first one's population or damage, and
/// `validate_against` would accept ids the definition never declared.
#[test]
fn accept_f18_a_each_world_instance_states_its_variant_population_and_damage() {
    let definition = arch_world().expect("the synthetic arch world is valid");
    let variant = known(
        WorldId::from_key("synthetic.arch_world.mission_02").expect("the variant key is valid"),
        "variant",
    );

    let first = WorldInstance::try_new(
        definition.id().clone(),
        variant.clone(),
        WorldPopulation::Only(object_set(&[OBJECT_LEG_RIGHT, OBJECT_LINTEL])),
        BTreeSet::from([object(OBJECT_LINTEL)]),
        provenance("first"),
    )
    .expect("a non-empty population is valid");
    first
        .validate_against(&definition)
        .expect("the population and damage name real objects");

    assert!(first.activates(&object(OBJECT_LINTEL)));
    assert!(!first.activates(&object(OBJECT_GROUND)));
    assert_eq!(
        first.initially_damaged().collect::<Vec<_>>(),
        vec![&object(OBJECT_LINTEL)],
        "the load carries exactly its own initial damage"
    );

    // A second load of the same world, for another mission: its own
    // population, no damage, and the first one is untouched.
    let second = WorldInstance::try_new(
        definition.id().clone(),
        unknown("variant"),
        WorldPopulation::AllAuthored,
        BTreeSet::new(),
        provenance("second"),
    )
    .expect("all-authored is a valid population");
    second
        .validate_against(&definition)
        .expect("an all-authored population names only declared objects");
    assert!(second.activates(&object(OBJECT_GROUND)));
    assert_eq!(
        second.initially_damaged().count(),
        0,
        "the second load starts clean"
    );
    assert_eq!(
        first.initially_damaged().count(),
        1,
        "a later load must not reach back and change an earlier one"
    );
    assert!(
        matches!(second.variant(), Resolved::Unknown { .. }),
        "an unmeasured variant stays an explicit unknown"
    );

    // Refusals: an empty explicit population, an object the definition does
    // not have, and checking against the wrong definition.
    let empty = WorldInstance::try_new(
        definition.id().clone(),
        variant.clone(),
        WorldPopulation::Only(BTreeSet::new()),
        BTreeSet::new(),
        provenance("empty"),
    );
    assert!(
        matches!(&empty, Err(WorldError::EmptyPopulation)),
        "an empty population must be refused, got {empty:?}"
    );

    let phantom = WorldInstance::try_new(
        definition.id().clone(),
        variant,
        WorldPopulation::Only(object_set(&["nope.missing"])),
        BTreeSet::new(),
        provenance("phantom"),
    )
    .expect("the record itself is well formed");
    assert!(
        matches!(
            phantom.validate_against(&definition),
            Err(WorldError::UnknownInstanceObject { .. })
        ),
        "an object the definition never declared must be refused by name"
    );

    let other = one_object_definition(
        "synthetic.other_world",
        vec![one_sector()],
        vec![plain_object("a", vec![sector("only")])],
    )
    .expect("the other world is valid");
    assert!(
        matches!(
            first.validate_against(&other),
            Err(WorldError::DefinitionMismatch { .. })
        ),
        "checking a load against another world must be refused"
    );
}

/// The spawn reports every gap it cannot fill, so a consumer of the records
/// can see unresolved surface roles (F18 non-negotiable behavior 2) instead
/// of a default nobody chose.
#[test]
fn accept_f18_a_unresolved_surface_roles_are_listed_not_defaulted() {
    let definition = arch_world().expect("the synthetic arch world is valid");

    let unresolved: Vec<&WorldObjectId> = definition
        .unresolved_surface()
        .iter()
        .map(|object| object.id())
        .collect();
    assert_eq!(
        unresolved,
        vec![&object("sign.unevidenced_role")],
        "exactly the unevidenced instance is unresolved"
    );

    let water = definition
        .object(&object(OBJECT_WATER))
        .expect("the water patch exists");
    assert!(
        matches!(water.surface(), Resolved::Known(known) if known.value.label() == "water"),
        "water carries the water role, not a generic ground role"
    );
}
