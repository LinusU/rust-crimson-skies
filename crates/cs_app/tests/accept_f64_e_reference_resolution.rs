//! F64-E: the inventory's `referenced_by` entries declare where each path
//! resolves (installation-root file vs container member), and the retail
//! reader resolves through that declared kind without guessing.

use crate::f64_d_support as f64_d;

use cs_formats::legacy_profile::{
    BASE_CONTAINER, ContentReference, LEGACY_LAYOUT_INVENTORY, LegacyArtifactClass,
    ReferenceResolution, layout_record,
};
use f64_d::{game_dir, installation, read_reference};

#[test]
fn accept_f64_e_the_plane_construction_reference_is_declared_a_container_member() {
    let requirement = layout_record(LegacyArtifactClass::CustomAircraft).requirement;
    let [reference] = requirement.referenced_by() else {
        panic!("one measured reference expected");
    };
    assert_eq!(reference.path, "ASSETS/SCRIPTS/PLANECONSTRUCTION.SCRIPT");
    assert_eq!(
        reference.resolution,
        ReferenceResolution::ContainerMember {
            container: BASE_CONTAINER
        }
    );
    assert_eq!(
        reference.to_string(),
        "GOSDATA/ASSETS/crimson.rof#ASSETS/SCRIPTS/PLANECONSTRUCTION.SCRIPT"
    );
    // The requirement semantics are unchanged by the typing.
    assert!(requirement.is_required());
    for record in &LEGACY_LAYOUT_INVENTORY {
        assert_eq!(
            record.requirement.is_required(),
            !record.requirement.referenced_by().is_empty()
        );
    }
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f64_e_retail_references_resolve_through_their_declared_kind_only() {
    let root = game_dir();
    let manifest = installation(&root);
    for record in &LEGACY_LAYOUT_INVENTORY {
        for reference in record.requirement.referenced_by() {
            assert!(
                read_reference(&root, &manifest, reference).is_some(),
                "{reference} resolves through its declared kind"
            );
        }
    }
    let member = layout_record(LegacyArtifactClass::CustomAircraft)
        .requirement
        .referenced_by()[0];

    // The same path declared as a root file is not one: no fallback.
    let as_root = ContentReference {
        resolution: ReferenceResolution::InstallationRootFile,
        ..member
    };
    assert!(read_reference(&root, &manifest, &as_root).is_none());

    // A path that resolves as neither kind fails.
    let bogus_member = ContentReference {
        path: "ASSETS/SCRIPTS/NO_SUCH_THING.SCRIPT",
        resolution: member.resolution,
    };
    let bogus_root = ContentReference {
        path: bogus_member.path,
        resolution: ReferenceResolution::InstallationRootFile,
    };
    assert!(read_reference(&root, &manifest, &bogus_member).is_none());
    assert!(read_reference(&root, &manifest, &bogus_root).is_none());
}
