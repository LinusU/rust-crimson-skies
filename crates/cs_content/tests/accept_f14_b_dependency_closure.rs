//! Acceptance scenario F14-B through the dependency closure and its
//! deterministic report.
//!
//! These tests exercise production code only: `cs_content::catalog::Catalog`
//! rows through `cs_content::catalog::closure::{Closure, CompatibilityOptions}`.
//! Removing dependency propagation, orphan reporting, the ownership-cycle
//! rule or the canonical ordering makes them fail.

use std::collections::BTreeSet;

use cs_content::catalog::Catalog;
use cs_content::catalog::closure::{Closure, ClosureError, CompatibilityOptions};
use cs_types::content::{
    CatalogElement, ConsumerKind, ContentId, ContentKind, Dependency, DependencyKind,
    NormalizeState, Origin, Provenance, Readiness, RuntimeConsumer, UnsupportedReason,
};
use cs_types::evidence::ClaimId;
use cs_types::install::ParseState;

fn claim(id: &str) -> ClaimId {
    ClaimId::new(id).expect("test claim id is valid")
}

fn designed(id: &str) -> Provenance {
    Provenance::designed(claim(id))
}

fn id(kind: ContentKind, key: &str) -> ContentId {
    ContentId::from_source(kind, key).expect("test id is valid")
}

/// A ready synthetic element of `element_id`'s kind, depending on `deps`.
fn ready(element_id: &ContentId, deps: &[(ContentId, DependencyKind)]) -> CatalogElement {
    CatalogElement {
        kind: element_id.kind(),
        id: element_id.clone(),
        display_name: Some(format!("Authored {element_id}")),
        origin: Origin::SyntheticFixture,
        dependencies: deps
            .iter()
            .map(|(target, kind)| Dependency {
                target: target.clone(),
                kind: *kind,
                provenance: designed("f14_b.test.dependency"),
            })
            .collect(),
        parse_state: ParseState::Parsed,
        normalize_state: NormalizeState::Normalized,
        runtime_consumers: vec![RuntimeConsumer {
            kind: ConsumerKind::Gameplay,
            provenance: designed("f14_b.test.consumer"),
        }],
        readiness: Readiness::Ready,
        unsupported_reasons: Vec::new(),
        fingerprint: None,
    }
}

/// A ready mission depending on `deps`.
fn mission(key: &str, deps: &[(ContentId, DependencyKind)]) -> CatalogElement {
    let element_id = id(ContentKind::Mission, key);
    let mut element = ready(&element_id, deps);
    element.display_name = Some(format!("Authored Mission {key}"));
    element
}

/// A mission-to-texture-shaped graph: mission -> airframe -> material -> image.
fn deep_chain_catalog() -> (Catalog, ContentId, ContentId, ContentId, ContentId) {
    let image = id(ContentKind::Image, "wing_tex");
    let material = id(ContentKind::Material, "wing_mat");
    let airframe = id(ContentKind::Airframe, "scout");
    let mission_id = id(ContentKind::Mission, "m01");

    let mut catalog = Catalog::new();
    catalog
        .insert(ready(&material, &[(image.clone(), DependencyKind::Static)]))
        .expect("material inserts");
    catalog
        .insert(ready(
            &airframe,
            &[(material.clone(), DependencyKind::Static)],
        ))
        .expect("airframe inserts");
    catalog
        .insert(mission(
            "m01",
            &[(airframe.clone(), DependencyKind::Static)],
        ))
        .expect("mission inserts");
    catalog.insert(ready(&image, &[])).expect("image inserts");
    catalog
        .declare_launchable(&mission_id)
        .expect("mission is launchable");
    (catalog, mission_id, airframe, material, image)
}

/// AC02 minimum scenario: the same rows in any input enumeration produce the
/// same ids, the same closure hash and the same serialized JSON — including
/// when each element's own dependency list is reordered.
#[test]
fn accept_f14_b_randomizing_input_enumeration_keeps_ids_and_serialized_order_stable() {
    let a = id(ContentKind::Airframe, "alpha");
    let b = id(ContentKind::Airframe, "bravo");
    let image = id(ContentKind::Image, "shared_tex");
    let mission_id = id(ContentKind::Mission, "m01");

    // A diamond: the mission reaches the image through either airframe, so
    // which predecessor wins depends on the canonical dependency order.
    let graph = |mission_deps: [ContentId; 2]| {
        let mut catalog = Catalog::new();
        catalog.insert(ready(&image, &[])).expect("image inserts");
        catalog
            .insert(ready(&a, &[(image.clone(), DependencyKind::Static)]))
            .expect("alpha inserts");
        catalog
            .insert(ready(&b, &[(image.clone(), DependencyKind::Static)]))
            .expect("bravo inserts");
        catalog
            .insert(mission(
                "m01",
                &[
                    (mission_deps[0].clone(), DependencyKind::Static),
                    (mission_deps[1].clone(), DependencyKind::Static),
                ],
            ))
            .expect("mission inserts");
        catalog
            .declare_launchable(&mission_id)
            .expect("mission is launchable");
        catalog
    };

    let build = |catalog: &Catalog| {
        Closure::compute(
            catalog,
            std::slice::from_ref(&mission_id),
            CompatibilityOptions::default(),
        )
        .expect("closure computes")
    };

    let forward_rows: Vec<CatalogElement> =
        graph([a.clone(), b.clone()]).elements().cloned().collect();
    let swapped_rows: Vec<CatalogElement> =
        graph([b.clone(), a.clone()]).elements().cloned().collect();

    let rebuild = |rows: &[CatalogElement]| {
        let mut catalog = Catalog::new();
        for row in rows {
            catalog.insert(row.clone()).expect("row inserts");
        }
        catalog
            .declare_launchable(&mission_id)
            .expect("mission is launchable");
        build(&catalog)
    };

    let reference = rebuild(&forward_rows);
    let expected_ids: Vec<String> = {
        let mut ids: Vec<String> = reference
            .node_ids()
            .iter()
            .map(|id| id.as_str().to_owned())
            .collect();
        ids.sort_unstable();
        ids
    };

    let permutations: [fn(&mut Vec<CatalogElement>); 3] = [
        |rows| {
            rows.reverse();
        },
        |rows| {
            rows.rotate_left(1);
        },
        |rows| {
            let last = rows.pop().expect("non-empty");
            rows.insert(0, last);
        },
    ];

    for base in [
        swapped_rows.clone(),
        forward_rows.iter().rev().cloned().collect(),
    ] {
        for permute in permutations {
            let mut candidate: Vec<CatalogElement> = base.clone();
            permute(&mut candidate);
            let closure = rebuild(&candidate);

            let ids: Vec<&str> = closure.node_ids().iter().map(|id| id.as_str()).collect();
            let mut sorted = ids.clone();
            sorted.sort_unstable();
            assert_eq!(sorted, expected_ids, "ids do not depend on input order");
            assert_eq!(
                closure.hash(),
                reference.hash(),
                "the closure hash does not depend on input order"
            );
            assert_eq!(
                closure.to_json(),
                reference.to_json(),
                "the serialized report does not depend on input order"
            );
        }
    }

    // The canonical chain picks the lexicographically first predecessor
    // (`airframe/alpha`), not whichever row happened to be listed first.
    assert_eq!(
        reference.chain_to(&image),
        Some(vec![mission_id.clone(), a.clone(), image.clone()])
    );

    // The JSON is a real report: it names the hash and every reached node.
    let json = reference.to_json();
    assert!(json.starts_with("{\"schema\":\"cs-content-closure-v1\",\"hash\":\""));
    assert!(json.contains("\"id\":\"mission/m01\""));
    assert!(json.contains("\"id\":\"image/shared_tex\""));
}

/// AC03 mechanism: deleting a texture several edges deep leaves an explicit
/// chain from the mission to the missing texture and blocks readiness.
#[test]
fn accept_f14_b_deeply_deleted_texture_keeps_its_mission_chain_and_blocks_readiness() {
    // Build the chain without ever inserting the leaf texture row, as if the
    // texture archive entry had been deleted.
    let image = id(ContentKind::Image, "wing_tex");
    let material = id(ContentKind::Material, "wing_mat");
    let airframe = id(ContentKind::Airframe, "scout");
    let mission_id = id(ContentKind::Mission, "m01");

    let mut catalog = Catalog::new();
    catalog
        .insert(ready(&material, &[(image.clone(), DependencyKind::Static)]))
        .expect("material inserts");
    catalog
        .insert(ready(
            &airframe,
            &[(material.clone(), DependencyKind::Static)],
        ))
        .expect("airframe inserts");
    catalog
        .insert(mission(
            "m01",
            &[(airframe.clone(), DependencyKind::Static)],
        ))
        .expect("mission inserts");

    let closure = Closure::compute(
        &catalog,
        std::slice::from_ref(&mission_id),
        CompatibilityOptions::default(),
    )
    .expect("closure computes");

    assert_eq!(closure.unresolved().len(), 1);
    assert_eq!(closure.unresolved()[0].from, material);
    assert_eq!(closure.unresolved()[0].target, image);
    assert!(!closure.is_ready(&mission_id));
    assert!(!closure.is_complete());
    assert_eq!(
        closure.chain_to(&image),
        Some(vec![mission_id.clone(), airframe, material, image]),
        "the mission-to-texture chain survives the missing row"
    );
}

/// Reference cycles are legitimate and stay ready; ownership cycles are
/// invalid and are refused.
#[test]
fn accept_f14_b_reference_cycles_are_allowed_and_ownership_cycles_are_invalid() {
    let a = id(ContentKind::Script, "a");
    let b = id(ContentKind::Script, "b");
    let mission_id = id(ContentKind::Mission, "m01");

    let mut catalog = Catalog::new();
    catalog
        .insert(ready(&a, &[(b.clone(), DependencyKind::Static)]))
        .expect("a inserts");
    catalog
        .insert(ready(&b, &[(a.clone(), DependencyKind::Static)]))
        .expect("b inserts");
    catalog
        .insert(mission("m01", &[(a.clone(), DependencyKind::Static)]))
        .expect("mission inserts");

    let closure = Closure::compute(
        &catalog,
        std::slice::from_ref(&mission_id),
        CompatibilityOptions::default(),
    )
    .expect("a reference cycle is a legitimate graph");
    assert!(closure.is_ready(&mission_id));
    assert!(closure.is_complete());

    let parent = id(ContentKind::World, "c1");
    let child = id(ContentKind::SceneNode, "root_node");
    let mut cyclic = Catalog::new();
    cyclic
        .insert(ready(
            &parent,
            &[(child.clone(), DependencyKind::Ownership)],
        ))
        .expect("parent inserts");
    cyclic
        .insert(ready(
            &child,
            &[(parent.clone(), DependencyKind::Ownership)],
        ))
        .expect("child inserts");
    cyclic
        .insert(mission("m01", &[(parent.clone(), DependencyKind::Static)]))
        .expect("mission inserts");

    let error = Closure::compute(
        &cyclic,
        std::slice::from_ref(&mission_id),
        CompatibilityOptions::default(),
    )
    .expect_err("an ownership cycle is invalid");
    let ClosureError::OwnershipCycle { cycle } = error else {
        panic!("expected an ownership cycle, got {error:?}");
    };
    assert_eq!(cycle.first(), cycle.last(), "the cycle closes on itself");
    let members: BTreeSet<&str> = cycle.iter().map(ContentId::as_str).collect();
    assert_eq!(
        members,
        BTreeSet::from(["world/c1", "scene_node/root_node"])
    );
}

/// An unsupported or unnormalized element is never ready, and the reason is
/// reported with the mission that depends on it.
#[test]
fn accept_f14_b_unnormalized_and_unsupported_nodes_block_their_dependents() {
    let leaf = id(ContentKind::Airframe, "scout");
    let mission_id = id(ContentKind::Mission, "m01");

    let mut unnormalized = ready(&leaf, &[]);
    unnormalized.normalize_state = NormalizeState::NotNormalized;
    let mut catalog = Catalog::new();
    catalog.insert(unnormalized).expect("airframe inserts");
    catalog
        .insert(mission("m01", &[(leaf.clone(), DependencyKind::Static)]))
        .expect("mission inserts");

    let closure = Closure::compute(
        &catalog,
        std::slice::from_ref(&mission_id),
        CompatibilityOptions::default(),
    )
    .expect("closure computes");
    assert!(
        closure
            .reasons(&leaf)
            .iter()
            .any(|reason| reason.code() == "not_normalized"),
        "parsing and normalization are separate states"
    );
    assert!(!closure.is_ready(&leaf));
    assert!(!closure.is_ready(&mission_id));
    assert!(
        closure
            .reasons(&mission_id)
            .contains(&UnsupportedReason::UnsupportedDependency { target: leaf })
    );
}

/// Every unready dependency of a node is named, not just the first. Two
/// unsupported leaves of one mission both appear in its reasons, in canonical
/// target order, exactly once each.
#[test]
fn accept_f14_b_reports_every_unsupported_dependency_of_a_node() {
    let texture_alpha = id(ContentKind::Image, "alpha_tex");
    let texture_bravo = id(ContentKind::Image, "bravo_tex");
    let mission_id = id(ContentKind::Mission, "m01");

    let unsupported = |element_id: &ContentId| {
        let mut element = ready(element_id, &[]);
        element.readiness = Readiness::Unavailable;
        element.unsupported_reasons = vec![UnsupportedReason::MissingParser];
        element
    };

    let mut catalog = Catalog::new();
    catalog
        .insert(unsupported(&texture_alpha))
        .expect("alpha texture inserts");
    catalog
        .insert(unsupported(&texture_bravo))
        .expect("bravo texture inserts");
    catalog
        .insert(mission(
            "m01",
            &[
                (texture_bravo.clone(), DependencyKind::Static),
                (texture_alpha.clone(), DependencyKind::Static),
            ],
        ))
        .expect("mission inserts");

    let closure = Closure::compute(
        &catalog,
        std::slice::from_ref(&mission_id),
        CompatibilityOptions::default(),
    )
    .expect("closure computes");

    assert!(!closure.is_ready(&mission_id));
    let reported: Vec<&ContentId> = closure
        .reasons(&mission_id)
        .iter()
        .filter_map(|reason| match reason {
            UnsupportedReason::UnsupportedDependency { target } => Some(target),
            _ => None,
        })
        .collect();
    assert_eq!(
        reported,
        vec![&texture_alpha, &texture_bravo],
        "each unsupported dependency is named once, in canonical target order"
    );
}

/// The declared compatibility options are part of the closure identity, and
/// dynamic candidate edges are only followed when asked.
#[test]
fn accept_f14_b_compatibility_options_change_the_closure_hash_and_dynamic_edges() {
    let (catalog, mission_id, ..) = deep_chain_catalog();
    let strict = Closure::compute(
        &catalog,
        std::slice::from_ref(&mission_id),
        CompatibilityOptions::default(),
    )
    .expect("closure computes");
    let relaxed = Closure::compute(
        &catalog,
        std::slice::from_ref(&mission_id),
        CompatibilityOptions {
            tolerate_unresolved_references: true,
            ..CompatibilityOptions::default()
        },
    )
    .expect("closure computes");
    assert_ne!(
        strict.hash(),
        relaxed.hash(),
        "different options are a different closure"
    );

    let image = id(ContentKind::Image, "wing_tex");
    let airframe = id(ContentKind::Airframe, "scout");
    let candidate_mission = id(ContentKind::Mission, "m02");
    let mut dynamic = Catalog::new();
    dynamic.insert(ready(&image, &[])).expect("image inserts");
    dynamic
        .insert(ready(
            &airframe,
            &[(image.clone(), DependencyKind::DynamicCandidate)],
        ))
        .expect("airframe inserts");
    dynamic
        .insert(mission(
            "m02",
            &[(airframe.clone(), DependencyKind::Static)],
        ))
        .expect("mission inserts");

    let followed = Closure::compute(
        &dynamic,
        std::slice::from_ref(&candidate_mission),
        CompatibilityOptions::default(),
    )
    .expect("closure computes");
    assert!(
        followed.is_reached(&image),
        "dynamic candidates are followed"
    );

    let ignored = Closure::compute(
        &dynamic,
        std::slice::from_ref(&candidate_mission),
        CompatibilityOptions {
            follow_dynamic_candidates: false,
            ..CompatibilityOptions::default()
        },
    )
    .expect("closure computes");
    assert!(
        !ignored.is_reached(&image),
        "the option can suppress a dynamic candidate"
    );
}

/// Roots must be present, launchable elements; an unknown or non-launchable
/// root is a named refusal.
#[test]
fn accept_f14_b_roots_must_be_present_and_launchable() {
    let mut catalog = Catalog::new();
    let world = id(ContentKind::World, "c1");
    catalog.insert(ready(&world, &[])).expect("world inserts");

    assert_eq!(
        Closure::compute(
            &catalog,
            &[id(ContentKind::Mission, "m01")],
            CompatibilityOptions::default()
        ),
        Err(ClosureError::UnknownRoot {
            id: id(ContentKind::Mission, "m01")
        })
    );
    assert_eq!(
        Closure::compute(
            &catalog,
            std::slice::from_ref(&world),
            CompatibilityOptions::default(),
        ),
        Err(ClosureError::NotLaunchableRoot {
            id: world.clone(),
            kind: ContentKind::World
        })
    );
}
